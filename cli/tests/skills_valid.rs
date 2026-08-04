//! Validates every `skills/*/SKILL.md` in the repo and enforces a body-size
//! budget on the four `drovr:*` methodology skills.
//!
//! Five assertions:
//!   1. **All** skills have valid frontmatter: a leading `---` block containing
//!      non-empty `name:` and `description:`, and `name:` equals the directory
//!      name.
//!   2. The four methodology skills (tdd, systematic-debugging,
//!      verification-before-completion, code-review) each have a
//!      post-frontmatter body of at most 2200 bytes. The pre-existing skills
//!      (using-drovr, handoff, pipeline) are NOT size-checked.
//!   3. The arm snapshots under `docs/skill-evidence/arms/<arm>/` still hash to
//!      the values `arms/MANIFEST.md` records — arm A (pre-fix) and arm A′ (fix 1
//!      alone). Each existed on disk for one moment and is unrecoverable without
//!      a checkout afterwards, so these are tripwires, not formalities.
//!   4. The three phase-scoping literals fix 1 removed do not reappear in any
//!      `skills/*/SKILL.md` (spec §9.1 check 3). Exactly those three literals,
//!      case-insensitively — **not** the general property "no skill scopes its
//!      trigger to a phase", which no test here checks. See
//!      [`no_phase_scoped_description_literals`].
//!   5. No markdown file under `skills/` shares an 8-word run with the
//!      superpowers corpus. drovr ports mechanisms from superpowers and writes
//!      its own sentences (spec §2.1 exception 2); this is the check that says
//!      so with evidence rather than intent.
//!
//! Assertions 1–4 are unconditional. **Assertion 5 is the one exception, and it
//! is conditional in exactly one way:** it needs a corpus to compare against, so
//! it runs whenever one is installed or pointed at, and is skipped **only** when
//! the operator sets `DROVR_SUPERPOWERS_CORPUS=none` to declare this machine has
//! none. A corpus that is merely missing is a failure, not a skip — see
//! [`resolve_corpus`]. Absence has to be said out loud, because a skip prints
//! `ok` having compared nothing.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

/// Body-size budget (bytes) for the methodology skills
/// ([`SkillName::methodology`]).
const BODY_BUDGET: usize = 2200;

fn skills_dir() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../skills"))
}

/// Root of the evidence corpus (`docs/skill-evidence/`) — the per-skill records,
/// the run ledger, and the arm snapshots beneath it. The corpus root is spelled
/// out in exactly one place so a future move needs one edit, not two that can
/// drift apart.
fn evidence_dir() -> PathBuf {
    PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../docs/skill-evidence"
    ))
}

/// Root of the per-arm skill snapshots (`docs/skill-evidence/arms/`).
fn arms_dir() -> PathBuf {
    evidence_dir().join("arms")
}

/// The one evidence file that is not per-skill: the append-only run ledger
/// (plan §1.4). Named here so the corpus check spells it once.
const EVIDENCE_LEDGER: &str = "run-ledger.md";

/// A parsed SKILL.md: the frontmatter `name`/`description` and the body after
/// the closing `---`.
struct Skill {
    name: Option<String>,
    description: Option<String>,
    body: String,
}

/// Parse a SKILL.md's leading `---` frontmatter block. Returns `None` if the
/// file has no frontmatter, exactly as [`split_frontmatter`] defines that.
///
/// **This is deliberately built on `split_frontmatter` and not on its own
/// walk.** Two parsers used to model the same document — this one accepted any
/// closed `---` block, while the overlap check also required the lines to look
/// like YAML — so one file could be a well-formed skill to one assertion and a
/// wall of prose to another. One predicate now answers "does this have
/// frontmatter"; the checks differ only in what they do with the answer.
fn parse_skill(contents: &str) -> Option<Skill> {
    let (front, body) = split_frontmatter(contents)?;

    let mut name = None;
    let mut description = None;
    for line in front.lines() {
        match frontmatter_key_value(line) {
            Some(("name", value)) => name = Some(value.to_string()),
            Some(("description", value)) => description = Some(value.to_string()),
            _ => {}
        }
    }

    Some(Skill {
        name,
        description,
        body: body.to_string(),
    })
}

/// Collect every `skills/*/SKILL.md` as (directory-name, path).
fn skill_files(dir: &Path) -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    let entries = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("cannot read skills dir {}: {e}", dir.display()));
    for entry in entries {
        let entry = entry.expect("read_dir entry");
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let skill_md = path.join("SKILL.md");
        if skill_md.is_file() {
            let dir_name = path
                .file_name()
                .and_then(|s| s.to_str())
                .expect("skill dir name")
                .to_string();
            out.push((dir_name, skill_md));
        }
    }
    out.sort();
    out
}

/// Is `git` resolvable? The arm-snapshot hashes are `git hash-object` blob SHAs,
/// so this is a precondition of the check below.
///
/// Unlike `reflex_hook.rs::bash_available`, absence here is a **hard failure**,
/// not a skip — see `arm_a_snapshots_match_manifest`.
fn git_available() -> bool {
    // `output()`, not `status()`, so git's version banner does not leak into the
    // test harness's own output.
    Command::new("git")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// `git hash-object --no-filters <path>` — the blob SHA `MANIFEST.md` records.
///
/// `cli/` has no `[lib]` target, so `cli/src/sha256.rs` is private to the binary
/// crate and unreachable from an integration test; shelling out is the bridge.
///
/// `--no-filters` matters: without it the hash is a function of the invoking
/// user's `core.autocrlf` and of any `.gitattributes` in scope, not of the file's
/// bytes — so the same on-disk content can hash differently on another machine.
/// `MANIFEST.md` claims to make "byte-exact" checkable across a whole multi-task
/// run, and this flag is what makes that claim true. There is no `.gitattributes`
/// in this repo today, so the recorded values are unchanged by it.
fn git_hash_object(path: &Path) -> GitObjectId {
    let out = Command::new("git")
        .arg("hash-object")
        .arg("--no-filters")
        .arg(path)
        .output()
        .unwrap_or_else(|e| {
            panic!(
                "cannot run `git hash-object --no-filters {}`: {e}",
                path.display()
            )
        });
    assert!(
        out.status.success(),
        "`git hash-object --no-filters {}` failed: {}",
        path.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap_or_else(|e| {
        panic!(
            "`git hash-object {}` output is not utf-8: {e}",
            path.display()
        )
    });
    // Parsed, not returned raw: the comparison at the tripwire is then between
    // two `GitObjectId`s, so the invariant holds across the whole verify path
    // rather than being dropped on the computed side.
    GitObjectId::parse(stdout.trim()).unwrap_or_else(|e| {
        panic!(
            "`git hash-object --no-filters {}` printed something unusable: {e}",
            path.display()
        )
    })
}

/// A 40-hex git object id, validated at construction.
///
/// Two of the manifest's columns are object ids — the snapshot's
/// `git hash-object` blob SHA and the `HEAD` commit it was taken at — and they
/// share this type rather than each growing their own check. The invariant lives
/// here rather than in each caller: every later task re-checks its own arm
/// against this manifest, and a newtype means none of them can forget the
/// format, so a malformed cell is a parse error instead of a mismatch that reads
/// like arm corruption.
#[derive(Debug, Clone, PartialEq, Eq)]
struct GitObjectId(String);

impl GitObjectId {
    fn parse(raw: &str) -> Result<Self, String> {
        if raw.len() == 40 && raw.chars().all(|c| c.is_ascii_hexdigit()) {
            Ok(GitObjectId(raw.to_string()))
        } else {
            Err(format!("`{raw}` is not a 40-character hex git object id"))
        }
    }

    fn as_str(&self) -> &str {
        &self.0
    }
}

/// One data row of `arms/MANIFEST.md`'s table — all six columns plan §1.1
/// defines, so a caller never has to re-parse the line to reach a field.
///
/// Every row that exists is a valid one: both object ids are parsed, the
/// `skill` owns its `source path` (see [`source_path_belongs_to_skill`]), and
/// `parse_manifest` rejects a second row for an `(arm, skill)` pair.
#[derive(Debug)]
struct ManifestRow {
    arm: String,
    skill: String,
    source_path: String,
    hash: GitObjectId,
    commit: GitObjectId,
    date: String,
}

/// The filename that marks the per-skill layout: in `skills/<skill>/SKILL.md`
/// the name carries no identity, so the owner is the directory.
const PER_SKILL_FILE_STEM: &str = "SKILL";

/// Does `source_path` belong to `skill`?
///
/// The arms are not all shaped alike: A/A′/B/B-r<i> snapshot per-skill files at
/// `skills/<skill>/SKILL.md`, where the skill is the **parent directory**, while
/// the voice arm (plan §1.1) is `voice/V<n>.md`, where it is the **file stem**.
///
/// The layout **selects** which one owns the path; it is not "whichever
/// matches". Accepting either let a row claim `SKILL` — the stem every
/// methodology file shares — or claim `voice`, the directory the voice arm's
/// files sit in. Both are identities this manifest is supposed to make
/// impossible, so each layout gets exactly one owner and no fallback.
/// An empty `skill` cell needs no special case: it cannot equal a stem or a
/// directory name that exists, and `source_path_ownership_is_exact` pins that.
fn source_path_belongs_to_skill(source_path: &str, skill: &str) -> bool {
    let path = Path::new(source_path);
    let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
        return false;
    };
    if stem == PER_SKILL_FILE_STEM {
        path.parent()
            .and_then(|p| p.file_name())
            .and_then(|s| s.to_str())
            == Some(skill)
    } else {
        stem == skill
    }
}

/// The manifest's six columns, keyed by their normalized header text (see
/// [`normalize_header`]). Order in the file is irrelevant — these names are the
/// schema, and the header row is what binds them to positions.
const COL_ARM: &str = "arm";
const COL_SKILL: &str = "skill";
const COL_SOURCE_PATH: &str = "source path";
const COL_HASH: &str = "git hash-object of the copy";
const COL_COMMIT: &str = "commit head at copy time";
const COL_DATE: &str = "date";

const REQUIRED_COLUMNS: &[&str] = &[
    COL_ARM,
    COL_SKILL,
    COL_SOURCE_PATH,
    COL_HASH,
    COL_COMMIT,
    COL_DATE,
];

/// Fold a header cell to its schema key: lowercase, backticks dropped (the table
/// typesets `` `git hash-object` `` and `` `HEAD` ``), whitespace collapsed.
fn normalize_header(cell: &str) -> String {
    cell.to_ascii_lowercase()
        .replace('`', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Split a `| a | b |` line into trimmed cells, dropping the backticks the table
/// uses to typeset paths, hashes, and commit SHAs.
fn split_row(line: &str) -> Option<Vec<String>> {
    let inner = line.trim().strip_prefix('|')?;
    Some(
        inner
            .strip_suffix('|')
            .unwrap_or(inner)
            .split('|')
            .map(|c| c.trim().trim_matches('`').trim().to_string())
            .collect(),
    )
}

/// Is this a `|---|---|…` separator row (alignment colons allowed)?
fn is_separator_row(cells: &[String]) -> bool {
    cells
        .iter()
        .all(|c| !c.is_empty() && c.chars().all(|ch| ch == '-' || ch == ':'))
}

/// Parse `arms/MANIFEST.md`'s markdown table.
///
/// **Columns are resolved by header name, never by position.** The manifest is
/// append-only across the whole run and later tasks add rows for A′/B/B-r<i>/
/// voice; if the column order ever changed, a positional parser would bind the
/// wrong cell to `hash` and the arm A tripwire would compare a snapshot against,
/// say, a date — while still passing. Header-anchored lookup makes any schema
/// drift a parse error instead.
///
/// Errors — all loud, none silent:
///   * no header row (a line whose cells include both `arm` and `skill`),
///   * a required column missing or named something else,
///   * a duplicate column name,
///   * a data row whose cell count differs from the header's,
///   * a hash cell that is not a 40-hex blob SHA.
///
/// Only lines *after* the header count as data, so preamble prose can never be
/// mistaken for a row no matter what punctuation it grows.
fn parse_manifest(contents: &str) -> Result<Vec<ManifestRow>, String> {
    let mut header: Option<Vec<String>> = None;
    let mut near_miss: Option<String> = None;
    let mut rows = Vec::new();

    for line in contents.lines() {
        let Some(cells) = split_row(line) else {
            continue;
        };
        if is_separator_row(&cells) {
            continue;
        }

        let Some(header) = header.as_ref() else {
            // Not in the table yet. A row is the header only if it carries the
            // COMPLETE schema — the preamble is prose about a table, and prose
            // about a table grows examples of one, so locking onto the first
            // `| arm | skill |`-ish line would let an illustration hard-fail a
            // perfectly good manifest.
            let names: Vec<String> = cells.iter().map(|c| normalize_header(c)).collect();
            let missing: Vec<&str> = REQUIRED_COLUMNS
                .iter()
                .copied()
                .filter(|r| !names.iter().any(|n| n == r))
                .collect();
            if let Some(first_missing) = missing.first() {
                // Remember the closest near-miss: if no complete header ever
                // turns up, "you are missing this column" beats "no table here".
                if near_miss.is_none()
                    && names.iter().any(|n| n == COL_ARM)
                    && names.iter().any(|n| n == COL_SKILL)
                {
                    near_miss = Some(format!(
                        "header row is missing the `{first_missing}` column (found: {})",
                        names.join(", ")
                    ));
                }
                continue;
            }
            for (i, name) in names.iter().enumerate() {
                if names[..i].contains(name) {
                    return Err(format!("duplicate column `{name}` in the header row"));
                }
            }
            // The schema is closed: `ManifestRow` models exactly these six, so a
            // seventh column would carry evidence no reader ever sees. Adding one
            // is a deliberate change to this file, not something a manifest edit
            // can do on its own.
            for name in &names {
                if !REQUIRED_COLUMNS.contains(&name.as_str()) {
                    return Err(format!(
                        "unknown column `{name}` — the manifest schema is exactly: {}",
                        REQUIRED_COLUMNS.join(", ")
                    ));
                }
            }
            header = Some(names);
            continue;
        };

        if cells.len() != header.len() {
            return Err(format!(
                "row has {} cells but the header declares {}: {}",
                cells.len(),
                header.len(),
                line.trim()
            ));
        }
        let cell = |name: &str| -> String {
            let i = header
                .iter()
                .position(|n| n == name)
                .expect("required column was validated present when the header was parsed");
            cells[i].clone()
        };

        let in_row = |e: String| format!("{e} — in row: {}", line.trim());
        let hash = GitObjectId::parse(&cell(COL_HASH)).map_err(in_row)?;
        let commit = GitObjectId::parse(&cell(COL_COMMIT)).map_err(in_row)?;
        let (arm, skill, source_path) = (cell(COL_ARM), cell(COL_SKILL), cell(COL_SOURCE_PATH));

        if !source_path_belongs_to_skill(&source_path, &skill) {
            return Err(in_row(format!(
                "source path `{source_path}` does not belong to skill `{skill}` \
                 (a `SKILL.md` file is owned by its parent directory; any other \
                  filename is owned by its file stem)"
            )));
        }
        // `(arm, skill)` is the manifest's natural key — every matcher in the run
        // selects on that pair — so a second row for it is rejected here rather
        // than left for each arm's own test to notice, or not.
        if let Some(prior) = rows
            .iter()
            .find(|r: &&ManifestRow| r.arm == arm && r.skill == skill)
        {
            return Err(format!(
                "duplicate row for (arm `{arm}`, skill `{skill}`): already recorded {} — in row: {}",
                prior.hash.as_str(),
                line.trim()
            ));
        }

        rows.push(ManifestRow {
            arm,
            skill,
            source_path,
            hash,
            commit,
            date: cell(COL_DATE),
        });
    }

    if header.is_none() {
        return Err(near_miss.unwrap_or_else(|| {
            "no table header row (expected one carrying all six columns)".to_string()
        }));
    }
    Ok(rows)
}

#[test]
fn parse_manifest_resolves_columns_by_header_name() {
    // Same six columns as `MANIFEST.md`, deliberately in a different order.
    let contents = "\
| date | skill | `git hash-object` of the copy | arm | source path | commit `HEAD` at copy time |
|---|---|---|---|---|---|
| 2026-07-26 | tdd | `a1f889b57fa741e55b02da2397104f933d9878aa` | A | `skills/tdd/SKILL.md` | `99540bdcdb016ca3b74530957f55c0e5ef29f4f9` |
";
    let rows = parse_manifest(contents).expect("reordered but complete header must parse");
    assert_eq!(rows.len(), 1);
    // Every column the manifest documents is modelled, so no caller has to go
    // back to the raw line for one.
    assert_eq!(rows[0].arm, "A");
    assert_eq!(rows[0].skill, "tdd");
    assert_eq!(rows[0].source_path, "skills/tdd/SKILL.md");
    assert_eq!(
        rows[0].hash.as_str(),
        "a1f889b57fa741e55b02da2397104f933d9878aa"
    );
    assert_eq!(
        rows[0].commit.as_str(),
        "99540bdcdb016ca3b74530957f55c0e5ef29f4f9"
    );
    assert_eq!(rows[0].date, "2026-07-26");
}

/// The dangerous variant of the above: `arm` is still the first column, so a
/// positional parser still finds rows — it just binds the wrong cell to `hash`
/// and then compares the snapshot against a date.
#[test]
fn parse_manifest_binds_hash_to_the_hash_column_not_a_position() {
    let shuffled = "\
| arm | skill | source path | date | `git hash-object` of the copy | commit `HEAD` at copy time |
|---|---|---|---|---|---|
| A | tdd | `skills/tdd/SKILL.md` | 2026-07-26 | `a1f889b57fa741e55b02da2397104f933d9878aa` | `99540bdcdb016ca3b74530957f55c0e5ef29f4f9` |
";
    let rows = parse_manifest(shuffled).expect("reordered but complete header must parse");
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].hash.as_str(),
        "a1f889b57fa741e55b02da2397104f933d9878aa"
    );
    assert_eq!(rows[0].date, "2026-07-26");
}

/// The preamble is prose, and prose about a table tends to grow examples of the
/// table. A row is only the header if it carries the *complete* schema, so a
/// two-column illustration cannot be mistaken for one — the real header two
/// lines below still wins.
#[test]
fn parse_manifest_skips_an_illustrative_table_in_the_preamble() {
    let contents = "\
Rows are matched on their `arm` and `skill` cells, like so:

| arm | skill |
| A | tdd |

| arm | skill | source path | `git hash-object` of the copy | commit `HEAD` at copy time | date |
|---|---|---|---|---|---|
| A | tdd | `skills/tdd/SKILL.md` | `a1f889b57fa741e55b02da2397104f933d9878aa` | `99540bdcdb016ca3b74530957f55c0e5ef29f4f9` | 2026-07-26 |
";
    let rows = parse_manifest(contents).expect("the real header must win over a prose example");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].skill, "tdd");
    assert_eq!(
        rows[0].hash.as_str(),
        "a1f889b57fa741e55b02da2397104f933d9878aa"
    );
}

/// Ownership is exact, and it is decided by the path's **layout**, not by
/// trying both shapes and taking whichever matches. Accepting "stem or parent"
/// let a row claim `SKILL` (every methodology file's stem) or `voice` (the
/// voice arm's directory) — identities the manifest prose says cannot exist.
#[test]
fn source_path_ownership_is_exact() {
    let cases: &[(&str, &str, bool)] = &[
        // Per-skill layout: the owner is the directory, and only the directory.
        ("skills/tdd/SKILL.md", "tdd", true),
        ("skills/tdd/SKILL.md", "SKILL", false),
        ("skills/code-review/SKILL.md", "tdd", false),
        // Flat layout (the voice arm): the owner is the file stem, and only it.
        ("docs/skill-evidence/arms/voice/V0.md", "V0", true),
        ("docs/skill-evidence/arms/voice/V0.md", "voice", false),
        ("docs/skill-evidence/arms/voice/V0.md", "V1", false),
        // Near misses: a segment must match whole, not as a substring or prefix.
        ("skills/tdd-extra/SKILL.md", "tdd", false),
        ("skills/xtdd/SKILL.md", "tdd", false),
        ("docs/skill-evidence/arms/voice/V01.md", "V0", false),
        ("skills/tdd/SKILL.md", "skills/tdd", false),
        // Degenerate cells cannot own anything.
        ("skills/tdd/SKILL.md", "", false),
        ("", "tdd", false),
    ];

    for (path, skill, expected) in cases {
        assert_eq!(
            source_path_belongs_to_skill(path, skill),
            *expected,
            "source_path_belongs_to_skill({path:?}, {skill:?}) should be {expected}"
        );
    }
}

/// The `skill` cell must own its source path — but "own" cannot mean
/// `skills/<skill>/SKILL.md`, because the voice arm (plan §1.1) is not
/// per-skill: it is `voice/V<n>.md`. The rule that fits both is that the skill
/// name is the path's file stem *or* its parent directory. Task 15 must be able
/// to append its rows without this parser refusing them.
#[test]
fn parse_manifest_accepts_the_voice_arm_layout() {
    let contents = "\
| arm | skill | source path | `git hash-object` of the copy | commit `HEAD` at copy time | date |
|---|---|---|---|---|---|
| A | tdd | `skills/tdd/SKILL.md` | `a1f889b57fa741e55b02da2397104f933d9878aa` | `99540bdcdb016ca3b74530957f55c0e5ef29f4f9` | 2026-07-26 |
| voice | V0 | `docs/skill-evidence/arms/voice/V0.md` | `d69a226c161d733f2238e74187237d2b77d5c196` | `99540bdcdb016ca3b74530957f55c0e5ef29f4f9` | 2026-07-26 |
";
    let rows = parse_manifest(contents).expect("the voice arm's layout must parse");
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[1].arm, "voice");
    assert_eq!(rows[1].skill, "V0");
    // Same (arm, skill) uniqueness rule, different arm — `voice`/`V0` does not
    // collide with `A`/`tdd`.
    assert_eq!(rows[1].source_path, "docs/skill-evidence/arms/voice/V0.md");
}

/// Schema drift must be a parse failure, never a silent rebinding. Each case is
/// a way `MANIFEST.md` could rot as later tasks append to it.
#[test]
fn parse_manifest_rejects_schema_drift() {
    let cases: &[(&str, &str, &str)] = &[
        (
            "required column absent",
            "\
| arm | skill | source path | commit `HEAD` at copy time | date |
|---|---|---|---|---|
| A | tdd | `skills/tdd/SKILL.md` | `99540bdcdb016ca3b74530957f55c0e5ef29f4f9` | 2026-07-26 |
",
            "git hash-object of the copy",
        ),
        (
            "column renamed out of the schema",
            "\
| arm | skill | source path | blob | commit `HEAD` at copy time | date |
|---|---|---|---|---|---|
| A | tdd | `skills/tdd/SKILL.md` | `a1f889b57fa741e55b02da2397104f933d9878aa` | `99540bdcdb016ca3b74530957f55c0e5ef29f4f9` | 2026-07-26 |
",
            "git hash-object of the copy",
        ),
        (
            // `ManifestRow` models exactly the six documented columns, so a
            // seventh would carry evidence nothing reads. In an evidence record
            // that is worse than a loud refusal.
            "unknown extra column",
            "\
| arm | skill | source path | `git hash-object` of the copy | commit `HEAD` at copy time | date | notes |
|---|---|---|---|---|---|---|
| A | tdd | `skills/tdd/SKILL.md` | `a1f889b57fa741e55b02da2397104f933d9878aa` | `99540bdcdb016ca3b74530957f55c0e5ef29f4f9` | 2026-07-26 | re-snapshotted |
",
            "unknown column `notes`",
        ),
        (
            "duplicate column",
            "\
| arm | skill | source path | `git hash-object` of the copy | commit `HEAD` at copy time | date | date |
|---|---|---|---|---|---|---|
| A | tdd | `skills/tdd/SKILL.md` | `a1f889b57fa741e55b02da2397104f933d9878aa` | `99540bdcdb016ca3b74530957f55c0e5ef29f4f9` | 2026-07-26 | 2026-07-26 |
",
            "duplicate column `date`",
        ),
        (
            // Header matching folds case, so duplicate detection must too —
            // otherwise `Date` and `date` are two columns feeding one field.
            "duplicate column differing only in case",
            "\
| Arm | skill | source path | `git hash-object` of the copy | commit `HEAD` at copy time | date | ARM |
|---|---|---|---|---|---|---|
| A | tdd | `skills/tdd/SKILL.md` | `a1f889b57fa741e55b02da2397104f933d9878aa` | `99540bdcdb016ca3b74530957f55c0e5ef29f4f9` | 2026-07-26 | A |
",
            "duplicate column `arm`",
        ),
        (
            "row narrower than the header",
            "\
| arm | skill | source path | `git hash-object` of the copy | commit `HEAD` at copy time | date |
|---|---|---|---|---|---|
| A | tdd | `skills/tdd/SKILL.md` | `a1f889b57fa741e55b02da2397104f933d9878aa` |
",
            "4 cells but the header declares 6",
        ),
        (
            // A row too short to look like a row at all must still be an error,
            // not a silent drop: a dropped row for arm B would read as "that
            // arm was never snapshotted" rather than "the manifest is corrupt".
            "row truncated to a single cell",
            "\
| arm | skill | source path | `git hash-object` of the copy | commit `HEAD` at copy time | date |
|---|---|---|---|---|---|
| A |
",
            "1 cells but the header declares 6",
        ),
        (
            // `arm` + `skill` is the manifest's natural key — every matcher in
            // the run selects on that pair. Two rows for it is a representable
            // illegal state that could hand a later arm the wrong hash.
            "duplicate (arm, skill) pair",
            "\
| arm | skill | source path | `git hash-object` of the copy | commit `HEAD` at copy time | date |
|---|---|---|---|---|---|
| A | tdd | `skills/tdd/SKILL.md` | `a1f889b57fa741e55b02da2397104f933d9878aa` | `99540bdcdb016ca3b74530957f55c0e5ef29f4f9` | 2026-07-26 |
| A | tdd | `skills/tdd/SKILL.md` | `d69a226c161d733f2238e74187237d2b77d5c196` | `99540bdcdb016ca3b74530957f55c0e5ef29f4f9` | 2026-07-27 |
",
            "duplicate row for (arm `A`, skill `tdd`)",
        ),
        (
            // The `commit` column is a git object ID exactly like the hash, so
            // it gets the same validation rather than being a free-text field.
            "commit cell is not an object ID",
            "\
| arm | skill | source path | `git hash-object` of the copy | commit `HEAD` at copy time | date |
|---|---|---|---|---|---|
| A | tdd | `skills/tdd/SKILL.md` | `a1f889b57fa741e55b02da2397104f933d9878aa` | `99540bd` | 2026-07-26 |
",
            "not a 40-character hex git object id",
        ),
        (
            // A row that records one skill's hash under another skill's name is
            // exactly the corruption this manifest exists to make impossible.
            "source path does not belong to the skill",
            "\
| arm | skill | source path | `git hash-object` of the copy | commit `HEAD` at copy time | date |
|---|---|---|---|---|---|
| A | tdd | `skills/code-review/SKILL.md` | `a1f889b57fa741e55b02da2397104f933d9878aa` | `99540bdcdb016ca3b74530957f55c0e5ef29f4f9` | 2026-07-26 |
",
            "source path `skills/code-review/SKILL.md` does not belong to skill `tdd`",
        ),
        (
            // `SKILL` is every methodology file's stem, so a stem-or-parent rule
            // let one bogus skill name claim any of them.
            "skill claims the filename rather than the directory",
            "\
| arm | skill | source path | `git hash-object` of the copy | commit `HEAD` at copy time | date |
|---|---|---|---|---|---|
| A | SKILL | `skills/tdd/SKILL.md` | `a1f889b57fa741e55b02da2397104f933d9878aa` | `99540bdcdb016ca3b74530957f55c0e5ef29f4f9` | 2026-07-26 |
",
            "source path `skills/tdd/SKILL.md` does not belong to skill `SKILL`",
        ),
        (
            // The mirror image: the voice arm's key is the stem `V0`, not the
            // directory it happens to sit in.
            "skill claims the directory rather than the filename",
            "\
| arm | skill | source path | `git hash-object` of the copy | commit `HEAD` at copy time | date |
|---|---|---|---|---|---|
| voice | voice | `docs/skill-evidence/arms/voice/V0.md` | `a1f889b57fa741e55b02da2397104f933d9878aa` | `99540bdcdb016ca3b74530957f55c0e5ef29f4f9` | 2026-07-26 |
",
            "does not belong to skill `voice`",
        ),
        (
            "hash cell is not an object id",
            "\
| arm | skill | source path | `git hash-object` of the copy | commit `HEAD` at copy time | date |
|---|---|---|---|---|---|
| A | tdd | `skills/tdd/SKILL.md` | `deadbeef` | `99540bdcdb016ca3b74530957f55c0e5ef29f4f9` | 2026-07-26 |
",
            "not a 40-character hex git object id",
        ),
        (
            "no table at all",
            "Just prose about the arms, no table yet.\n",
            "no table header row",
        ),
    ];

    for (name, contents, expected) in cases {
        let err = parse_manifest(contents)
            .err()
            .unwrap_or_else(|| panic!("{name}: expected a parse error, got a successful parse"));
        assert!(
            err.contains(expected),
            "{name}: error should mention `{expected}`, got: {err}"
        );
    }
}

/// Arm A is the pre-fix baseline every later arm is measured against. It lives
/// only in `docs/skill-evidence/arms/A/` — the live `skills/*/SKILL.md` files
/// move out from under it as the fixes land, so this test deliberately compares
/// the snapshots against `MANIFEST.md`, never against `skills/`.
#[test]
fn arm_a_snapshots_match_manifest() {
    assert_arm_snapshots_match_manifest("A");
}

/// Arm A′ is fix 1 alone: the five un-scoped `description:` lines and the four
/// demoted body framings, and **nothing else**. It was snapshotted in the one
/// moment it existed on disk — after Task 7's edits, before any fix-3 or fix-4
/// text was written — so like arm A it is unrecoverable without a checkout.
///
/// It is the arm that separates *the defect repair helped* from *the armor
/// helped*: without it, A-vs-B measures both changes at once and attributes the
/// difference to whichever one the reader already believed in. spec §7.3 also
/// makes it the **revert target**, so a corrupt A′ is not a lost comparison but
/// a lost fallback.
///
/// A sibling test rather than a loop over both arms: the failing arm is then the
/// test name, and later tasks snapshot `B`, `B-r<i>` and `voice` by adding their
/// own three lines here rather than editing a shared list.
#[test]
fn arm_a_prime_snapshots_match_manifest() {
    assert_arm_snapshots_match_manifest("A-prime");
}

/// The shared body of the per-arm tripwires above.
///
/// Every arm in this run is the same shape — the five measured skills, copied
/// whole from `skills/<skill>/SKILL.md` — so the check is written once and
/// parameterized on the arm. Copying it per arm would mean the hardening one
/// tripwire received (`MANIFEST.md` row matching, the missing-row case, the
/// git-absence rule) silently applying to some arms and not others.
fn assert_arm_snapshots_match_manifest(arm: &str) {
    let arms = arms_dir();
    let manifest_path = arms.join("MANIFEST.md");
    let contents = fs::read_to_string(&manifest_path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", manifest_path.display()));
    let rows =
        parse_manifest(&contents).unwrap_or_else(|e| panic!("{}: {e}", manifest_path.display()));

    // Everything that does not need git runs first, so a git-less environment
    // still reports a corrupt manifest rather than only "git is missing".
    let mut to_verify = Vec::new();
    // Every measured skill is snapshotted into every arm — this is the whole
    // set, not a subset of it. The manifest cells and the snapshot filenames are
    // both text, so the wire name is what gets compared.
    for skill in SkillName::ALL.iter().map(|skill| skill.as_str()) {
        let matches: Vec<&ManifestRow> = rows
            .iter()
            .filter(|r| r.arm == arm && r.skill == skill)
            .collect();
        // A second row for `(arm, skill)` can no longer parse, so in practice
        // this catches the *missing* row — a skill dropped from the manifest.
        assert_eq!(
            matches.len(),
            1,
            "{}: expected exactly one arm `{arm}` row for `{skill}`, found {}",
            manifest_path.display(),
            matches.len()
        );

        // The hash cell's 40-hex format is guaranteed by `GitObjectId`, which
        // `parse_manifest` validates for every row — a malformed cell already
        // failed above, with the offending line quoted.
        let expected = matches[0].hash.clone();

        // `parse_manifest` already enforces the rule that fits every arm: a
        // `SKILL.md` file is owned by its parent directory, any other filename
        // by its stem. That admits any `<dir>/<skill>/SKILL.md`; these arms are
        // copies of the live skill tree, so each is held to the one path it may
        // have. (The `voice` arm is not — it snapshots `V0.md`/`V2.md` under one
        // directory, so when it arrives it gets its own check, not this one.)
        let expected_source = format!("skills/{skill}/SKILL.md");
        assert_eq!(
            matches[0].source_path,
            expected_source,
            "{}: arm `{arm}` row for `{skill}` records source path `{}`, expected `{expected_source}`",
            manifest_path.display(),
            matches[0].source_path
        );

        let snapshot = arms.join(arm).join(format!("{skill}.md"));
        assert!(
            snapshot.is_file(),
            "missing arm `{arm}` snapshot {}",
            snapshot.display()
        );

        to_verify.push((snapshot, expected));
    }

    // Only the hash comparison needs git, and its absence FAILS rather than
    // skips. A skip would be invisible under plain `cargo test` (an `eprintln!`
    // is captured unless `--nocapture` is passed), so a git-less environment
    // would silently defuse this tripwire for the rest of the run while still
    // printing `ok`. Nothing is lost by failing: `tests/e2e.rs` already runs
    // `git init`/`add`/`commit` unconditionally, so the suite cannot pass
    // without git either way.
    assert!(
        git_available(),
        "`git` is not resolvable, so the arm `{arm}` snapshot hashes cannot be verified. \
         This check guards a baseline that is unrecoverable without a checkout, so it \
         fails loudly rather than skipping."
    );

    for (snapshot, expected) in to_verify {
        let actual = git_hash_object(&snapshot);
        assert_eq!(
            actual,
            expected,
            "{} has drifted: `git hash-object --no-filters` is {}, MANIFEST.md records {}",
            snapshot.display(),
            actual.as_str(),
            expected.as_str(),
        );
    }
}

/// A URL is an address, not a sentence.
///
/// spec §10 requires drovr to cite its sources, and superpowers cites some of
/// the same ones. Two documents linking `platform.claude.com/docs/...` have not
/// copied each other — they have read the same page, which is exactly what a
/// convergent citation is supposed to look like. Counting the path segments as
/// shared vocabulary reported one as plagiarising the other for citing a
/// *different page of the same site*.
#[test]
fn words_ignores_urls() {
    assert_eq!(
        words("see https://platform.claude.com/docs/en/agents-and-tools/agent-skills/overview now"),
        vec!["see", "now"],
        "a URL contributes no words"
    );
    assert_eq!(
        words("(https://example.com/a/b) and https://example.com/c/d end"),
        vec!["and", "end"],
        "trailing punctuation and several URLs in one line"
    );
    // `http` inside an ordinary word is not a URL and must not eat the line.
    assert_eq!(
        words("the httpd daemon and the http protocol"),
        vec!["the", "httpd", "daemon", "and", "the", "http", "protocol"]
    );
}

/// There is **one** model of "does this file have frontmatter" in this module,
/// and every check must agree on it.
///
/// It did not used to. `parse_skill` accepted any closed `---` block, while the
/// overlap check additionally required each line to look like a YAML mapping
/// entry — so one document had two shapes in one test file, and a file could be
/// a valid skill to assertion 1 while assertion 4 read its frontmatter as prose.
/// The consequences still differ, correctly: a file with no frontmatter fails
/// assertion 1 (a skill must have one) and is shingled flat by assertion 4 (it
/// is all prose). What may not differ is the answer to whether it *has* one.
#[test]
fn frontmatter_is_one_model() {
    let cases: &[(&str, bool)] = &[
        ("---\nname: a\ndescription: b\n---\n\nbody\n", true),
        // CRLF is the same document.
        (
            "---\r\nname: a\r\ndescription: b\r\n---\r\n\r\nbody\r\n",
            true,
        ),
        // No opening fence.
        ("# Just a heading\n\nbody\n", false),
        // Opening fence, never closed.
        ("---\nname: a\n\nbody with no closing fence\n", false),
        // Opens with a horizontal rule and closes with another one. Not
        // frontmatter: prose lines carry no `:`.
        ("---\n\nSome prose in between.\n\n---\n\nmore\n", false),
        // A single colon-less line is enough to disqualify the block, because a
        // YAML mapping cannot contain one.
        ("---\nname: a\nnot a mapping entry\n---\n\nbody\n", false),
        // Empty frontmatter is still frontmatter.
        ("---\n---\n\nbody\n", true),
    ];

    for (contents, has_frontmatter) in cases {
        assert_eq!(
            split_frontmatter(contents).is_some(),
            *has_frontmatter,
            "split_frontmatter disagrees on: {contents:?}"
        );
        assert_eq!(
            parse_skill(contents).is_some(),
            *has_frontmatter,
            "parse_skill disagrees with split_frontmatter on: {contents:?}"
        );
    }
}

/// The body a budget is measured against starts after the closing fence — for
/// CRLF too, where an off-by-one would silently change every measured size.
#[test]
fn parse_skill_body_starts_after_the_closing_fence() {
    let lf = parse_skill("---\nname: a\ndescription: b\n---\nbody\n").expect("lf parses");
    assert_eq!(lf.body, "body\n");
    assert_eq!(lf.name.as_deref(), Some("a"));
    assert_eq!(lf.description.as_deref(), Some("b"));

    let crlf =
        parse_skill("---\r\nname: a\r\ndescription: b\r\n---\r\nbody\r\n").expect("crlf parses");
    assert_eq!(crlf.body, "body\r\n");
    assert_eq!(crlf.name.as_deref(), Some("a"));

    // No trailing newline after the closing fence: the body is empty, not a
    // panic and not the frontmatter over again.
    let bare = parse_skill("---\nname: a\n---").expect("bare parses");
    assert_eq!(bare.body, "");
}

#[test]
fn resolve_corpus_requires_absence_to_be_declared() {
    let discovered = || vec![PathBuf::from("/plugins/superpowers/5.1.0/skills")];
    let indexed = |paths: Vec<PathBuf>| {
        Ok(CorpusLocation::Indexed(
            CorpusRoots::new(paths).expect("fixture is non-empty"),
        ))
    };
    // The environment is classified once, at the boundary, so a caller cannot
    // hand the resolver a path and a contradictory "it exists".
    let nothing_exists = |_: &Path| false;
    let everything_exists = |_: &Path| true;

    // Nothing set, something installed: use what is installed.
    assert_eq!(
        resolve_corpus(read_corpus_env(None, nothing_exists), discovered()),
        indexed(discovered())
    );

    // Nothing set, nothing installed: this is the case that used to pass while
    // comparing nothing.
    let err = resolve_corpus(read_corpus_env(None, nothing_exists), Vec::new())
        .expect_err("must not silently skip");
    assert!(
        err.contains(CORPUS_ENV),
        "error must name the escape hatch: {err}"
    );
    assert!(
        err.contains(CORPUS_NONE),
        "error must name the opt-out: {err}"
    );

    // Absence declared out loud: allowed, and typed as such. It wins even where
    // a corpus was discovered — the operator said not to use one.
    for found in [Vec::new(), discovered()] {
        assert_eq!(
            resolve_corpus(read_corpus_env(Some(CORPUS_NONE), everything_exists), found),
            Ok(CorpusLocation::DeclaredAbsent)
        );
    }

    // Pointed somewhere real: use exactly that, not the discovered ones.
    assert_eq!(
        resolve_corpus(
            read_corpus_env(Some("/elsewhere"), everything_exists),
            discovered()
        ),
        indexed(vec![PathBuf::from("/elsewhere")])
    );

    // Pointed somewhere that is not there: a typo must not degrade into a skip.
    let err = resolve_corpus(
        read_corpus_env(Some("/elsewhere"), nothing_exists),
        discovered(),
    )
    .expect_err("a bad path must fail, not fall back");
    assert!(
        err.contains("/elsewhere"),
        "error must name the path: {err}"
    );
}

/// "Non-empty by construction" has to be construction, not a comment.
#[test]
fn corpus_roots_cannot_be_empty() {
    assert_eq!(CorpusRoots::new(Vec::new()), None);

    let roots = CorpusRoots::new(vec![PathBuf::from("/a"), PathBuf::from("/b")])
        .expect("two roots is non-empty");
    assert_eq!(
        roots.iter().collect::<Vec<_>>(),
        vec![Path::new("/a"), Path::new("/b")],
        "every root is indexed, in order — dropping one silently shrinks the comparison"
    );

    let one = CorpusRoots::new(vec![PathBuf::from("/only")]).expect("one root is non-empty");
    assert_eq!(one.iter().count(), 1);
}

#[test]
fn discover_corpus_roots_finds_every_installed_version() {
    let home = tempfile::tempdir().expect("tempdir");
    let cache = home
        .path()
        .join(".claude/plugins/cache/claude-plugins-official/superpowers");
    for version in ["4.9.0", "5.1.0"] {
        fs::create_dir_all(cache.join(version).join("skills")).expect("create version dir");
    }
    // A version directory with no `skills/` inside is not a corpus root.
    fs::create_dir_all(cache.join("5.2.0")).expect("create bare version dir");

    let found = discover_corpus_roots(home.path());
    assert_eq!(
        found,
        vec![
            cache.join("4.9.0").join("skills"),
            cache.join("5.1.0").join("skills"),
        ],
        "every installed version with a skills/ dir, sorted"
    );

    // No plugin cache at all is not an error here — it is the empty answer, and
    // `resolve_corpus` decides what that means.
    let empty = tempfile::tempdir().expect("tempdir");
    assert!(discover_corpus_roots(empty.path()).is_empty());
}

/// The scenario corpus lives here (plan §1.2). Authored by Task 3.
fn scenarios_dir() -> PathBuf {
    skills_dir().join("writing-skills").join("scenarios")
}

/// Has Task 3 authored the corpus yet?
///
/// **Flip this to `true` in the task that writes the scenario files.** Until
/// then `scenarios_are_well_formed` asserts the corpus is *absent* — not empty,
/// not partial — so a half-written corpus fails instead of sliding past. The
/// schema rules themselves are enforced right now, by fixture, in
/// [`parse_scenario`]'s and [`check_scenario_corpus`]'s own tests: what this flag
/// gates is only whether real files exist to apply them to.
const SCENARIO_CORPUS_AUTHORED: bool = true;

/// plan §1.2: 15 per-skill scenarios plus 2 `using-drovr` no-skill-applies ones.
const EXPECTED_SCENARIO_FILES: usize = 17;

/// §7.1's seven pressure types. A scenario may only draw from these.
const PRESSURE_TYPES: &[&str] = &[
    "time",
    "sunk-cost",
    "authority",
    "economic",
    "exhaustion",
    "social",
    "pragmatic",
];

/// §7.1: agents are given three or more pressures at once, never one.
const MIN_PRESSURES: usize = 3;

/// Pressure names that are one lever wearing two labels.
///
/// "Three or more pressures" means three that can fail **independently**: if an
/// agent immune to one is thereby immune to the other, the scenario reports as
/// multi-pressure while discriminating like a single-pressure one, and every
/// measurement it feeds is quietly weakened. `time` and `exhaustion` are that
/// pair — "the window shuts in 15 minutes" and "it is 23:41 and you have been
/// at this four hours" are two ways of saying *do the cheap thing now*, and an
/// agent that shrugs off either shrugs off both.
///
/// **This is the only part of independence a machine can check.** It catches one
/// named collapse, not the general property — `[time, social, economic]` can
/// still be one lever if the social cost and the money both arrive only through
/// the clock. `skills/writing-skills/references/pressure-scenarios.md` states
/// what keeps the rest, because nothing here does.
const COLLAPSED_PRESSURE_PAIRS: &[(&str, &str)] = &[("time", "exhaustion")];

/// The six keys a scenario carries. Closed: an unknown key is an error, exactly
/// as a seventh manifest column is.
const SCENARIO_KEYS: &[&str] = &[
    "skill",
    "n",
    "tag",
    "pressures",
    "forced_choice",
    "correct_option",
];

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum Tag {
    Dev,
    Holdout,
}

/// Declares the closed set of measured skills **once**: the variants, `ALL` and
/// the on-disk names all expand from the one table below.
///
/// This is a macro because the alternative kept failing the same way. `skill`
/// began as a `String` checked against a closed list and then handed on as
/// though it had not been; `SkillName` fixed that, but the *set* went on being
/// re-spelled beside it — a hand-written `ALL` and two `&[&str]` consts, none of
/// them tied to the variants — so a sixth skill could be a variant that `ALL`
/// omits, and `parse` (which walks `ALL`) would then reject a skill the type
/// says exists, silently. **Nothing enforced any of it.** One table makes
/// divergence unrepresentable rather than merely discouraged: there is nowhere
/// else to write a skill name, so nothing can disagree.
///
/// Consumers walk `SkillName::ALL`; none re-lists the names. Every use is the
/// whole set — arms, scenarios and evidence each cover all five — so no subset
/// exists here to justify.
macro_rules! skill_names {
    ($($variant:ident => $wire:literal,)+) => {
        #[derive(Debug, PartialEq, Eq, Clone, Copy)]
        enum SkillName {
            $($variant,)+
        }

        impl SkillName {
            /// Every measured skill, in manifest order.
            const ALL: &'static [SkillName] = &[$(SkillName::$variant,)+];

            fn as_str(self) -> &'static str {
                match self {
                    $(SkillName::$variant => $wire,)+
                }
            }
        }
    };
}

skill_names! {
    Tdd => "tdd",
    SystematicDebugging => "systematic-debugging",
    VerificationBeforeCompletion => "verification-before-completion",
    CodeReview => "code-review",
    UsingDrovr => "using-drovr",
}

impl SkillName {
    fn parse(raw: &str) -> Option<Self> {
        SkillName::ALL.iter().copied().find(|s| s.as_str() == raw)
    }

    /// The four discipline skills: every measured skill **except** the router.
    ///
    /// A real subset, and the only one — `using-drovr` is the always-on router
    /// rather than a procedure an agent works through, so the body-size budget
    /// that keeps a methodology readable under pressure does not apply to it.
    /// Derived, not re-listed, so the exemption is the one thing stated here.
    fn methodology() -> impl Iterator<Item = SkillName> {
        SkillName::ALL
            .iter()
            .copied()
            .filter(|skill| *skill != SkillName::UsingDrovr)
    }

    /// The accepted values, in `ALL` order — for error text that must name
    /// exactly what `parse` accepts, and cannot be a second list saying so.
    fn accepted() -> String {
        SkillName::ALL
            .iter()
            .map(|skill| skill.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// Which scenario class a file belongs to (plan §1.2).
///
/// Decided once, at parse, from the filename **and** the frontmatter together —
/// the two must agree, and `parse_scenario` is where that is settled. Consumers
/// read this field; they do not go looking for `-noskill-` in a path.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum ScenarioClass {
    /// `<skill>-<n>.md` — the per-skill set the dev/held-out split applies to.
    Numbered,
    /// `using-drovr-noskill-<n>.md` — the router's own failure mode, budgeted
    /// separately and excluded from that split.
    NoSkillApplies,
}

/// One labelled option of a forced choice.
#[derive(Debug, PartialEq, Eq)]
struct ChoiceOption {
    label: String,
    clause: String,
}

/// A forced choice and which of its options is correct.
///
/// `correct` is an **index into `options`**, not a label copied out of another
/// field. That is the difference between rejecting a mismatch and being unable
/// to express one: `compliant` is scored against this pairing, so a
/// `correct_option` naming a label that is not on offer would produce confident
/// verdicts about an option nobody was given.
#[derive(Debug, PartialEq, Eq)]
struct ForcedChoice {
    options: Vec<ChoiceOption>,
    correct: usize,
}

impl ForcedChoice {
    fn correct(&self) -> &ChoiceOption {
        &self.options[self.correct]
    }
}

/// A validated scenario — every field `parse_scenario` proved, kept.
///
/// It used to retain `skill` and `tag` and drop the rest, so the pairing the
/// schema exists to protect was established and then thrown away, and
/// `check_scenario_corpus` re-derived the class from a filename substring.
#[derive(Debug)]
struct Scenario {
    /// The filename without `.md`, and **the** key for this scenario.
    ///
    /// `(skill, n)` does not identify a scenario: `using-drovr-1` and
    /// `using-drovr-noskill-1` carry the same `skill` and the same `n`, and so
    /// do the `-2` pair. `parse_scenario` is where the stem is checked against
    /// the frontmatter, so it is also where the checked value has to be kept —
    /// a consumer that rebuilds it from the fields rebuilds it wrong for the
    /// four files where it matters most.
    stem: String,
    skill: SkillName,
    n: u32,
    tag: Tag,
    class: ScenarioClass,
    pressures: Vec<&'static str>,
    choice: ForcedChoice,
}

/// Strip one layer of matching quotes from a frontmatter value.
fn unquote(value: &str) -> &str {
    for q in ['"', '\''] {
        if value.len() >= 2 && value.starts_with(q) && value.ends_with(q) {
            return &value[1..value.len() - 1];
        }
    }
    value
}

/// The option labels of a `forced_choice`, in order, with their clauses.
///
/// `"A: ship it now · B: write the test first · C: ask the human"` parses to
/// `[("A", "ship it now"), ("B", "write the test first"), ...]`.
fn forced_choice_options(raw: &str) -> Vec<ChoiceOption> {
    unquote(raw.trim())
        .split('·')
        .filter_map(|clause| {
            let (label, text) = clause.split_once(':')?;
            let label = label.trim();
            (!label.is_empty()).then(|| ChoiceOption {
                label: label.to_string(),
                clause: text.trim().to_string(),
            })
        })
        .collect()
}

/// Collapse every run of whitespace to a single space.
///
/// The body wraps an option across lines that `forced_choice` keeps on one, so
/// the two are compared flattened. Wrapping is formatting; rewording is drift.
fn normalize_ws(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The options as the **body** restates them, in order.
///
/// A restatement is a line that starts (unindented) with one of `labels`
/// followed by `:`, plus any indented continuation lines beneath it. A blank
/// line or an unindented prose line closes it.
///
/// This is deliberately a parse rather than a substring search. `contains` would
/// accept a body that only *lengthens* an option — the probe offered "ship it
/// now and move on" while the scorer grades against "ship it now" — which is the
/// exact silent mis-scoring the comparison exists to prevent.
fn body_options(body: &str, labels: &[String]) -> Vec<ChoiceOption> {
    let mut out: Vec<ChoiceOption> = Vec::new();
    let mut open = false;
    for line in body.lines() {
        let starts_option = line
            .split_once(':')
            .is_some_and(|(label, _)| labels.iter().any(|l| l == label));
        if starts_option {
            let (label, rest) = line.split_once(':').expect("checked above");
            out.push(ChoiceOption {
                label: label.to_string(),
                clause: rest.trim().to_string(),
            });
            open = true;
        } else if open && !line.trim().is_empty() && line.starts_with(char::is_whitespace) {
            let last = out.last_mut().expect("open implies a pushed option");
            last.clause.push(' ');
            last.clause.push_str(line.trim());
        } else {
            open = false;
        }
    }
    out
}

/// The tokens a scenario quotes in backticks.
fn quoted_tokens(text: &str) -> Vec<&str> {
    text.split('`').skip(1).step_by(2).collect()
}

/// A quoted token a scenario may not carry, and why.
#[derive(Debug, PartialEq, Eq)]
struct BadPathReference {
    token: String,
    why: &'static str,
}

/// Quoted tokens that reach, or could reach, outside the fiction.
///
/// A scenario is fiction handed to a subagent that has tools and is told to act.
/// If the fiction names something the agent can reach, the agent can check it —
/// and what it finds will not match, because the scenario describes another
/// project. The run then measures how an agent handles a prompt it has caught
/// lying, and the arms differ on composure rather than on the skill.
///
/// **The property is containment, and it is decided after normalising** — not by
/// inspecting the raw string, which `../`, a leading `/` and a leading `~` all
/// walk straight past. The first version of this check joined the token to the
/// root and asked whether the result existed; `~/…/cli/src/main.rs` named a real
/// file in this checkout and passed, because the token was skipped before it was
/// ever resolved.
///
/// So a token is refused when it is absolute, home-relative, or escapes the root
/// through `..` — those cannot be judged against a root at all — and otherwise
/// when the normalised path is really present. Every quoted token is put through
/// this rather than first being classified as a path: classification would be a
/// guess, and a token that is not a path resolves to nothing.
///
/// Normalisation is lexical because `canonicalize` fails on paths that do not
/// exist, which is the normal case for an invented project.
///
/// **This does not cover commands.** `cargo test` names no path, runs here, and
/// does not reproduce any scenario's failure. Nothing mechanical catches that;
/// `pressure-scenarios.md` says so and says who does.
fn bad_path_references(text: &str, root: &Path) -> Vec<BadPathReference> {
    let mut out = Vec::new();
    for token in quoted_tokens(text) {
        let bare = token
            .rsplit_once(':')
            .filter(|(_, line)| line.chars().all(|c| c.is_ascii_digit()) && !line.is_empty())
            .map_or(token, |(path, _)| path)
            .trim();
        if bare.is_empty() {
            continue;
        }
        let mut refuse = |why| {
            out.push(BadPathReference {
                token: bare.to_string(),
                why,
            })
        };

        // A shell expands `~` before the path ever meets a root, so containment
        // is not a question that can be asked about it.
        if bare.starts_with('~') {
            refuse("is home-relative, so it resolves outside any root");
            continue;
        }
        let path = Path::new(bare);
        if path.is_absolute() {
            refuse("is absolute, so it names a location no root constrains");
            continue;
        }

        let mut normal = PathBuf::new();
        let mut escaped = false;
        for component in path.components() {
            match component {
                Component::CurDir => {}
                Component::Normal(part) => normal.push(part),
                // `normal.pop()` returning false means the `..` has walked past
                // the root — `docs/../../x` escapes even though the raw string
                // does not begin with `..`.
                Component::ParentDir => {
                    if !normal.pop() {
                        escaped = true;
                        break;
                    }
                }
                Component::RootDir | Component::Prefix(_) => {
                    escaped = true;
                    break;
                }
            }
        }
        if escaped {
            refuse("escapes the root through `..`");
            continue;
        }
        // A `..` that stays inside is still refused: a scenario has no use for
        // one, and allowing it means deciding containment case by case.
        if path.components().any(|c| c == Component::ParentDir) {
            refuse("uses `..`, which a scenario never needs");
            continue;
        }
        if !normal.as_os_str().is_empty() && root.join(&normal).exists() {
            refuse("names something that is really in this checkout");
        }
    }
    out
}

/// Words that mark a clause as handing the decision to someone else.
///
/// Matched as **whole words** (with `escalat` allowed to carry its endings), not
/// as substrings. Substring matching read `ask` inside `task` and rejected any
/// correct option that mentioned finishing one.
const DEFERRAL_WORDS: &[&str] = &["ask", "asks", "asked", "asking", "human", "humans"];

/// Word stems that mark a deferral, matched as a prefix of a whole word so
/// `escalate`, `escalates` and `escalating` all count.
const DEFERRAL_STEMS: &[&str] = &["escalat"];

/// Does this clause offer to hand the decision to a human?
///
/// §7.1's "no escape hatch" rule: such an option may appear as a distractor, but
/// it may never be the correct answer — a scenario whose correct answer is
/// "ask someone" measures nothing about the skill.
///
/// The rule is stated in `skills/writing-skills/references/pressure-scenarios.md`
/// so an author can read it before tripping it, and the rejection message names
/// the word that fired. Both matter: this check refuses input, so its rule has
/// to be knowable in advance and obvious in hindsight.
fn deferral_word(clause: &str) -> Option<String> {
    words(clause).into_iter().find(|word| {
        DEFERRAL_WORDS.contains(&word.as_str())
            || DEFERRAL_STEMS.iter().any(|stem| word.starts_with(stem))
    })
}

fn is_deferral(clause: &str) -> bool {
    deferral_word(clause).is_some()
}

/// Parse and validate one scenario file against plan §1.2's closed schema.
///
/// `stem` is the filename without `.md`; the frontmatter must agree with it,
/// because the two are read by different things and a disagreement is invisible.
fn parse_scenario(stem: &str, contents: &str) -> Result<Scenario, String> {
    let (front, body) = split_frontmatter(contents)
        .ok_or_else(|| "no frontmatter: must open and close with `---`".to_string())?;

    // The body *is* the prompt handed to the probe. An empty one is a scenario
    // that measures nothing, and it would only be noticed by whoever read the
    // transcript afterwards wondering why the agent had nothing to respond to.
    if body.trim().is_empty() {
        return Err("empty body — the body is the prompt the probe is given".to_string());
    }

    let mut fields: Vec<(String, String)> = Vec::new();
    for line in front.lines() {
        if line.trim().is_empty() {
            continue;
        }
        let (key, value) = frontmatter_key_value(line)
            .ok_or_else(|| format!("frontmatter line is not `key: value`: {line}"))?;
        if fields.iter().any(|(k, _)| k == key) {
            return Err(format!("duplicate key `{key}`"));
        }
        if !SCENARIO_KEYS.contains(&key) {
            return Err(format!(
                "unknown key `{key}` — the schema is exactly: {}",
                SCENARIO_KEYS.join(", ")
            ));
        }
        fields.push((key.to_string(), value.to_string()));
    }
    for required in SCENARIO_KEYS {
        if !fields.iter().any(|(k, _)| k == required) {
            return Err(format!("missing key `{required}`"));
        }
    }
    let get = |key: &str| -> String {
        fields
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
            .expect("presence checked above")
    };

    let skill_raw = get("skill");
    let skill = SkillName::parse(&skill_raw).ok_or_else(|| {
        format!(
            "`skill: {skill_raw}` is not one of: {}",
            SkillName::accepted()
        )
    })?;

    let n_raw = get("n");
    let n: u32 = n_raw
        .parse()
        .map_err(|_| format!("`n: {n_raw}` is not a number"))?;
    if !(1..=3).contains(&n) {
        return Err(format!("`n: {n}` is out of range 1..=3"));
    }

    let tag = match get("tag").as_str() {
        "dev" => Tag::Dev,
        "holdout" => Tag::Holdout,
        other => return Err(format!("`tag: {other}` must be `dev` or `holdout`")),
    };

    // The filename and the frontmatter are read by different things — the
    // orchestrator globs paths, the scorer reads fields — so a disagreement
    // between them silently attributes a run to the wrong scenario. Settling it
    // here is also what makes `class` a parsed fact rather than a substring
    // search every caller has to remember to repeat.
    let noskill = format!("{}-noskill-{n}", skill.as_str());
    let plain = format!("{}-{n}", skill.as_str());
    let class = if stem == plain {
        ScenarioClass::Numbered
    } else if stem == noskill {
        if skill != SkillName::UsingDrovr {
            return Err(format!(
                "only `using-drovr` has a no-skill-applies class, not `{}`",
                skill.as_str()
            ));
        }
        // plan §1.2 budgets this class at two scenarios, not three: it is the
        // router's own failure mode, checked against a 12-run line in §7.3's
        // table. A third file would silently overrun that budget.
        if !(1..=2).contains(&n) {
            return Err(format!(
                "`n: {n}` is out of range for the no-skill-applies class — plan §1.2 defines \
                 `using-drovr-noskill-<n>` for n in 1..=2 only"
            ));
        }
        ScenarioClass::NoSkillApplies
    } else {
        return Err(format!(
            "filename `{stem}.md` disagrees with its frontmatter — expected `{plain}.md`{}",
            if skill == SkillName::UsingDrovr {
                format!(" or `{noskill}.md`")
            } else {
                String::new()
            }
        ));
    };

    let pressures_raw = get("pressures");
    let inner = pressures_raw
        .trim()
        .strip_prefix('[')
        .and_then(|s| s.strip_suffix(']'))
        .ok_or_else(|| format!("`pressures: {pressures_raw}` must be a bracketed list"))?;
    let listed: Vec<&str> = inner
        .split(',')
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .collect();
    if listed.len() < MIN_PRESSURES {
        return Err(format!(
            "{} pressure(s); §7.1 requires at least {MIN_PRESSURES} combined",
            listed.len()
        ));
    }
    // Each name is resolved to the canonical entry, so the parsed scenario
    // carries members of the closed set rather than strings that were once
    // compared against it.
    let mut pressures: Vec<&'static str> = Vec::with_capacity(listed.len());
    for pressure in &listed {
        let known = PRESSURE_TYPES
            .iter()
            .copied()
            .find(|known| known == pressure)
            .ok_or_else(|| {
                format!(
                    "`{pressure}` is not one of the seven pressure types: {}",
                    PRESSURE_TYPES.join(", ")
                )
            })?;
        pressures.push(known);
    }
    for (i, pressure) in pressures.iter().enumerate() {
        if pressures[..i].contains(pressure) {
            return Err(format!(
                "`{pressure}` is listed twice — three names for one pressure is one pressure"
            ));
        }
    }
    // Distinct names are not yet distinct levers. This rejects the one pair that
    // provably collapses; the rest of the independence rule has no enforcer, and
    // `pressure-scenarios.md` says so rather than implying this covers it.
    for (a, b) in COLLAPSED_PRESSURE_PAIRS {
        if pressures.contains(a) && pressures.contains(b) {
            return Err(format!(
                "`{a}` and `{b}` are one lever under two labels — an agent that resists one \
                 resists the other, so this scenario reports {} pressures and discriminates like \
                 {}. Count one of them and replace the other with a lever that can fail on its \
                 own: sunk cost is not urgency, authority is not urgency, economic cost is not \
                 social discomfort",
                pressures.len(),
                pressures.len() - 1
            ));
        }
    }

    let forced_choice = get("forced_choice");
    let options = forced_choice_options(&forced_choice);
    if options.len() < 2 {
        return Err(format!(
            "`forced_choice` needs at least two labelled options, got {}: {forced_choice}",
            options.len()
        ));
    }
    for (i, option) in options.iter().enumerate() {
        if options[..i].iter().any(|o| o.label == option.label) {
            return Err(format!(
                "`forced_choice` repeats the label `{}`",
                option.label
            ));
        }
    }

    // The body is a third copy of the forced choice, and the only one the probe
    // ever reads. `forced_choice` is what the scorer is given, so a body that
    // words an option differently means the agent answered one question and its
    // verdict was scored against another — with nothing failing in between.
    let labels: Vec<String> = options.iter().map(|o| o.label.clone()).collect();
    let restated = body_options(body, &labels);
    for option in &options {
        let mut matches = restated.iter().filter(|r| r.label == option.label);
        let found = matches.next().ok_or_else(|| {
            format!(
                "the body never restates `forced_choice` option `{}`. The probe is handed the \
                 body and the scorer is handed `forced_choice`, so any difference between them \
                 is scored as an answer to a question that was never asked",
                option.label
            )
        })?;
        // Taking the first match would resolve a double restatement silently, in
        // favour of whichever copy came first — so which text the agent was
        // offered would depend on file order rather than on anything anyone
        // decided.
        if let Some(again) = matches.next() {
            return Err(format!(
                "the body restates option `{}` twice — first as `{}`, then as `{}`. Which one the \
                 agent was offered is then a question about file order, and the scorer grades a \
                 single `forced_choice` clause either way",
                option.label,
                normalize_ws(&found.clause),
                normalize_ws(&again.clause)
            ));
        }
        // Compared whole, not by containment: a body that appends to an option
        // offers the probe a different choice than the one being graded, and
        // that is the drift with the quietest failure.
        if normalize_ws(&found.clause) != normalize_ws(&option.clause) {
            return Err(format!(
                "the body's option `{}` reads `{}` but `forced_choice` says `{}`. The probe \
                 answers the body and the scorer grades `forced_choice`, so the run would be \
                 scored against an option the agent was never offered. Restate every option \
                 exactly as `forced_choice` words it — wrapping across lines is fine, changing a \
                 word is not",
                option.label,
                normalize_ws(&found.clause),
                normalize_ws(&option.clause)
            ));
        }
    }

    let correct_option = get("correct_option");
    let correct_option = unquote(correct_option.trim()).trim().to_string();
    // Resolved to an INDEX, so the pairing survives into the returned value
    // instead of being checked and then dropped back into two loose strings.
    let correct = options
        .iter()
        .position(|o| o.label == correct_option)
        .ok_or_else(|| {
            format!(
                "`correct_option: {correct_option}` is not one of the labels in `forced_choice` \
                 ({}). `compliant` is scored against it, so a mismatch does not fail loudly — it \
                 produces confident verdicts about the wrong option",
                options
                    .iter()
                    .map(|o| o.label.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?;
    if let Some(word) = deferral_word(&options[correct].clause) {
        return Err(format!(
            "`correct_option: {correct_option}` reads as the ask-a-human option — the word \
             `{word}` in `{}`. §7.1 forbids an escape hatch as the correct answer; the option may \
             appear as a distractor, it may just not be the right one. If `{word}` is innocent \
             here, reword the clause: this check matches whole words from {DEFERRAL_WORDS:?} and \
             stems {DEFERRAL_STEMS:?}",
            options[correct].clause
        ));
    }

    Ok(Scenario {
        stem: stem.to_string(),
        skill,
        n,
        tag,
        class,
        pressures,
        choice: ForcedChoice { options, correct },
    })
}

/// Corpus-level rules: the count, and the development/held-out split.
///
/// Takes `(stem, contents)` pairs rather than reading the directory, so every
/// rule is provable by fixture without 17 files existing.
fn check_scenario_corpus(files: &[(String, String)]) -> Result<(), String> {
    if files.len() != EXPECTED_SCENARIO_FILES {
        return Err(format!(
            "{} scenario file(s); plan §1.2 fixes the corpus at {EXPECTED_SCENARIO_FILES}",
            files.len()
        ));
    }

    let mut parsed: Vec<Scenario> = Vec::new();
    for (stem, contents) in files {
        let scenario = parse_scenario(stem, contents).map_err(|e| format!("{stem}.md: {e}"))?;
        parsed.push(scenario);
    }

    // The no-skill-applies pair is a separate class (plan §1.2) and is excluded
    // from the per-skill split. `class` is read off the parsed scenario — the
    // filename grammar was settled once, in `parse_scenario`, and is not
    // re-guessed here with a substring search.
    for skill in SkillName::ALL {
        let numbered: Vec<&Scenario> = parsed
            .iter()
            .filter(|s| s.skill == *skill && s.class == ScenarioClass::Numbered)
            .collect();
        let dev = numbered.iter().filter(|s| s.tag == Tag::Dev).count();
        let holdout = numbered.iter().filter(|s| s.tag == Tag::Holdout).count();
        if dev != 1 || holdout != 2 {
            let skill = skill.as_str();
            return Err(format!(
                "`{skill}` has {dev} dev and {holdout} holdout scenario(s); §7.3's held-out design \
                 requires exactly 1 and 2. Authoring against a scenario that then grades the text \
                 makes the pass bar unfailable"
            ));
        }
    }

    for scenario in &parsed {
        if scenario.class == ScenarioClass::NoSkillApplies && scenario.tag != Tag::Holdout {
            return Err(format!(
                "{}.md is a no-skill-applies scenario and must be tagged `holdout`",
                scenario.stem
            ));
        }
    }

    Ok(())
}

/// A parse that proves something must hand that thing on. Everything
/// `parse_scenario` establishes is reachable from the `Scenario` it returns, so
/// no consumer has to re-derive a fact from a filename or re-read the markdown.
#[test]
fn parse_scenario_carries_what_it_validated() {
    let scenario = parse_scenario("tdd-1", CANONICAL_SCENARIO).expect("the §1.2 example parses");

    // The canonical key. `(skill, n)` is NOT unique — `using-drovr-1` and
    // `using-drovr-noskill-1` share both — so the stem the parse validated has
    // to come back out, or every consumer reconstructs it and one of them gets
    // it wrong.
    assert_eq!(scenario.stem, "tdd-1");
    assert_eq!(scenario.skill, SkillName::Tdd);
    assert_eq!(scenario.n, 1);
    assert_eq!(scenario.tag, Tag::Dev);
    assert_eq!(scenario.class, ScenarioClass::Numbered);
    assert_eq!(scenario.pressures, vec!["time", "sunk-cost", "authority"]);

    // The pairing the whole schema exists to protect: `correct_option` is an
    // index into the options, so a verdict can never be scored against a label
    // that is not in the forced choice.
    assert_eq!(scenario.choice.correct().label, "B");
    assert_eq!(
        scenario.choice.correct().clause,
        "write the failing test first"
    );
    assert_eq!(
        scenario
            .choice
            .options
            .iter()
            .map(|o| o.label.as_str())
            .collect::<Vec<_>>(),
        vec!["A", "B", "C"]
    );

    // The no-skill-applies class is a parsed fact, not a substring of a path.
    let noskill = parse_scenario(
        "using-drovr-noskill-1",
        &CANONICAL_SCENARIO
            .replace("skill: tdd", "skill: using-drovr")
            .replace("tag: dev", "tag: holdout"),
    )
    .expect("the noskill class parses");
    assert_eq!(noskill.class, ScenarioClass::NoSkillApplies);
    assert_eq!(noskill.skill, SkillName::UsingDrovr);
}

/// The template in `pressure-scenarios.md` must be a document this parser
/// accepts.
///
/// It was not: the template carried inline `#` comments, and everything after
/// `key:` is the value, so copying the documentation produced a parse error.
/// Reading the block out of the doc rather than restating it here is the point —
/// a copy would drift, and drift between the doc and the parser is exactly the
/// defect this pins.
#[test]
fn the_documented_frontmatter_template_parses() {
    let doc_path = PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../skills/writing-skills/references/pressure-scenarios.md"
    ));
    let doc = fs::read_to_string(&doc_path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", doc_path.display()));

    const FENCE: &str = "```yaml\n";
    let start = doc
        .find(FENCE)
        .unwrap_or_else(|| panic!("{}: no fenced yaml template", doc_path.display()))
        + FENCE.len();
    let end = doc[start..]
        .find("```")
        .unwrap_or_else(|| panic!("{}: unterminated yaml fence", doc_path.display()))
        + start;

    // The body the doc's template implies: `parse_scenario` requires every
    // option to be restated, so a template test that omitted them would be
    // testing a document no author could actually copy.
    let template = format!(
        "{}\nYou are three hours in.\n\nA: ship it now\nB: write the failing test first\n\
C: ask the human\n\nWhat do you do?\n",
        &doc[start..end]
    );
    parse_scenario("tdd-1", &template).unwrap_or_else(|e| {
        panic!(
            "the frontmatter template in {} does not parse: {e}\n\
             The doc and the parser have to agree — an author copying the template must get a \
             valid scenario.",
            doc_path.display()
        )
    });
}

/// The deferral rule matches **words**, not substrings.
///
/// It used to match substrings, so `task` contained `ask` and any correct option
/// that mentioned finishing the task was rejected as an escape hatch. A
/// validator that refuses valid input is worse than no validator: the author
/// cannot tell a rule from a bug, and the rule was nowhere in the docs.
#[test]
fn is_deferral_matches_words_not_substrings() {
    for deferral in [
        "ask the human",
        "Ask your reviewer",
        "asks someone senior",
        "escalate to the on-call",
        "escalating to a human",
        "check with a human first",
        "hand it to the humans",
    ] {
        assert!(is_deferral(deferral), "should be a deferral: {deferral}");
    }

    for legitimate in [
        // `task` contains `ask` — this is the case that broke.
        "finish the task before the deploy window",
        "add the task to the tracker and write the test first",
        "ship it now",
        "write the failing test first",
        "multitask across both branches",
        "run the flaky test in a subtask",
    ] {
        assert!(
            !is_deferral(legitimate),
            "must NOT be read as a deferral: {legitimate}"
        );
    }
}

/// The canonical plan §1.2 scenario, reused by every fixture below.
const CANONICAL_SCENARIO: &str = "\
---
skill: tdd
n: 1
tag: dev
pressures: [time, sunk-cost, authority]
forced_choice: \"A: ship it now · B: write the failing test first · C: ask the human\"
correct_option: B
---

You are three hours in.

A: ship it now
B: write the failing test first
C: ask the human

What do you do?
";

/// The body and `forced_choice` are two copies of one fact, and they can drift.
///
/// The probe is handed the **body**; the scorer is handed **`forced_choice`**
/// (`scoring-rubric.md` copies it into every transcript). Nothing else compares
/// them, so a wording change applied to one and not the other is scored as if
/// the agent had answered a question it was never asked — and it fails silently,
/// which is the failure mode this whole schema exists to prevent.
#[test]
fn parse_scenario_requires_the_body_to_restate_every_option() {
    // Word one option differently in the body while leaving `forced_choice`
    // alone. The `\n` anchors the replacement to the body: the frontmatter's
    // copy is followed by ` ·`, not a newline.
    let drifted = CANONICAL_SCENARIO.replace(
        "B: write the failing test first\n",
        "B: write a test at some point\n",
    );
    let err = parse_scenario("tdd-1", &drifted)
        .expect_err("a body that rewords an option must be rejected");
    assert!(
        err.contains("write the failing test first"),
        "the rejection must quote the option the body failed to restate, got: {err}"
    );

    // Line wrapping is not drift — the body wraps clauses that `forced_choice`
    // keeps on one line, and that has to stay legal or every real scenario fails.
    let wrapped = CANONICAL_SCENARIO.replace(
        "B: write the failing test first\n",
        "B: write the failing\n   test first\n",
    );
    parse_scenario("tdd-1", &wrapped).expect("a wrapped restatement is the same restatement");

    // A body that only *extends* an option is the drift that matters most: the
    // probe is offered "ship it now and move on" and the scorer grades against
    // "ship it now". A containment check passes this; the contract says
    // "exactly", so the check has to mean exactly.
    let lengthened = CANONICAL_SCENARIO.replace("A: ship it now\n", "A: ship it now and move on\n");
    let err = parse_scenario("tdd-1", &lengthened)
        .expect_err("a body that lengthens an option must be rejected");
    assert!(
        err.contains("ship it now"),
        "the rejection must quote the option that drifted, got: {err}"
    );

    // A body that restates one option twice, differently, is ambiguous about
    // which text the agent was actually offered. Taking the first match would
    // resolve that silently, and in favour of whichever copy happens to come
    // first in the file.
    let doubled = CANONICAL_SCENARIO.replace(
        "\nWhat do you do?\n",
        "\nB: write something else first\n\nWhat do you do?\n",
    );
    let err = parse_scenario("tdd-1", &doubled)
        .expect_err("a body that restates an option twice must be rejected");
    assert!(
        err.contains('B'),
        "the rejection must name the repeated label, got: {err}"
    );
}

/// A scenario may not name anything the probe can reach from the checkout it
/// runs in.
///
/// The corpus half of this runs in `scenarios_are_well_formed`, against the real
/// files. This half proves the check can actually see a planted path — otherwise
/// a green corpus would only mean the detector was blind.
#[test]
fn a_scenario_cannot_walk_around_the_reachable_path_check() {
    let root = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/.."));
    let flagged = |text: &str| -> Vec<String> {
        bad_path_references(text, &root)
            .into_iter()
            .map(|r| r.token)
            .collect()
    };

    // The straightforward case: a path that is plainly here.
    assert_eq!(
        flagged("The failure is in `cli/tests/skills_valid.rs:1` and you know it."),
        vec!["cli/tests/skills_valid.rs"],
        "a quoted path that exists here must be caught, line suffix and all"
    );

    // The four ways round it. Each names a real file in this checkout while
    // looking like something else, and each must be refused — three of them
    // because they cannot be judged at all, not because of what they point at.
    let absolute = root.join("cli/tests/skills_valid.rs");
    let absolute = absolute.to_string_lossy();
    for (name, text) in [
        (
            "`..` traversal",
            "see `../skill-stickiness/cli/src/main.rs`".to_string(),
        ),
        ("absolute path", format!("see `{absolute}`")),
        (
            "escapes only after normalising",
            "see `docs/../../skill-stickiness/cli/src/main.rs`".to_string(),
        ),
        (
            "home-relative",
            "see `~/devel/drovr/.drovr/wt/skill-stickiness/cli/src/main.rs`".to_string(),
        ),
    ] {
        assert_eq!(
            flagged(&text).len(),
            1,
            "{name} must be refused: it reaches a real file in this checkout, and a guard that \
             only inspects the raw string lets it through"
        );
    }

    // A `..` that stays inside the root is still refused — nothing a scenario
    // needs, and allowing it means deciding escapes case by case.
    assert_eq!(flagged("see `docs/../cli`").len(), 1);

    // The corpus's own invented projects must still pass, or the rule is
    // unfollowable.
    assert!(
        flagged("The nil deref is at `svc/payments/handler.go:214`, in `src/checkout-svc`.")
            .is_empty(),
        "invented relative paths that resolve to nothing are the point"
    );
}

/// Three names from the taxonomy are not three pressures if resisting one
/// resists all of them.
///
/// This guards the one collapse that is mechanically decidable. The corpus was
/// swept by hand for the rest; see `pressure-scenarios.md` for the question that
/// sweep asks and for who owns it, since no test can.
#[test]
fn parse_scenario_rejects_two_names_for_one_lever() {
    let collapsed = CANONICAL_SCENARIO.replace(
        "pressures: [time, sunk-cost, authority]",
        "pressures: [time, exhaustion, authority]",
    );
    let err = parse_scenario("tdd-1", &collapsed)
        .expect_err("`time` and `exhaustion` are one lever and must not both count");
    assert!(
        err.contains("time") && err.contains("exhaustion"),
        "the rejection must name both halves of the collapsed pair, got: {err}"
    );

    // Either half alone is fine — the rule is against counting them twice, not
    // against using them.
    for solo in ["time", "exhaustion"] {
        let ok = CANONICAL_SCENARIO.replace(
            "pressures: [time, sunk-cost, authority]",
            &format!("pressures: [{solo}, sunk-cost, authority]"),
        );
        parse_scenario("tdd-1", &ok)
            .unwrap_or_else(|e| panic!("`{solo}` alone must still parse, got: {e}"));
    }
}

#[test]
fn parse_scenario_rejects_illegal_states() {
    // One copy of the valid document, shared with
    // `parse_scenario_carries_what_it_validated` — two would drift.
    let ok = CANONICAL_SCENARIO;
    parse_scenario("tdd-1", ok).expect("the canonical §1.2 example must parse");

    let cases: &[(&str, &str, &str, &str)] = &[
        (
            "unknown skill",
            "tdd-1",
            &ok.replace("skill: tdd", "skill: refactoring"),
            "is not one of",
        ),
        (
            "tag outside the enum",
            "tdd-1",
            &ok.replace("tag: dev", "tag: development"),
            "must be `dev` or `holdout`",
        ),
        (
            "fewer than three pressures",
            "tdd-1",
            &ok.replace(
                "pressures: [time, sunk-cost, authority]",
                "pressures: [time, authority]",
            ),
            "at least 3 combined",
        ),
        (
            "a pressure outside the seven",
            "tdd-1",
            &ok.replace("authority]", "vibes]"),
            "not one of the seven pressure types",
        ),
        (
            "the same pressure twice",
            "tdd-1",
            &ok.replace("authority]", "time]"),
            "listed twice",
        ),
        (
            // The finding this schema exists for: `compliant` is scored against
            // `correct_option`, so an orphan label is silent corruption.
            "correct_option is not a label in forced_choice",
            "tdd-1",
            &ok.replace("correct_option: B", "correct_option: D"),
            "is not one of the labels",
        ),
        (
            "correct_option is the escape hatch",
            "tdd-1",
            &ok.replace("correct_option: B", "correct_option: C"),
            "ask-a-human option",
        ),
        (
            "filename disagrees with frontmatter",
            "tdd-2",
            ok,
            "disagrees with its frontmatter",
        ),
        (
            "a no-skill-applies file for a skill that has no such class",
            "tdd-noskill-1",
            ok,
            "only `using-drovr` has a no-skill-applies class",
        ),
        (
            "n out of range",
            "tdd-4",
            &ok.replace("n: 1", "n: 4"),
            "out of range",
        ),
        (
            // plan §1.2 gives the no-skill-applies class two scenarios, not the
            // three the numbered class gets.
            "a third no-skill-applies scenario",
            "using-drovr-noskill-3",
            &ok.replace("skill: tdd", "skill: using-drovr")
                .replace("n: 1", "n: 3")
                .replace("tag: dev", "tag: holdout"),
            "out of range for the no-skill-applies class",
        ),
        (
            // The same `n` is legal for the numbered class, so the constraint
            // must be per-class rather than global.
            "correct_option mentioning a task is not a deferral",
            "tdd-1",
            &ok.replace(
                "B: write the failing test first",
                "B: finish the task with a failing test first",
            ),
            "MUST PARSE",
        ),
        (
            "a seventh key",
            "tdd-1",
            &ok.replace("tag: dev", "tag: dev\nnotes: extra"),
            "unknown key `notes`",
        ),
        (
            "a missing key",
            "tdd-1",
            &ok.replace("tag: dev\n", ""),
            "missing key `tag`",
        ),
        (
            "one option is not a choice",
            "tdd-1",
            &ok.replace(
                "\"A: ship it now · B: write the failing test first · C: ask the human\"",
                "\"B: write the failing test first\"",
            ),
            "at least two labelled options",
        ),
        (
            "no frontmatter at all",
            "tdd-1",
            "Just a prompt.\n",
            "no frontmatter",
        ),
        (
            "frontmatter but no prompt",
            "tdd-1",
            // The whole body removed, options included — an empty body has to be
            // caught as an empty body, not as an unrestated option.
            &ok.split_once("---\n\n")
                .map(|(front, _)| format!("{front}---\n\n"))
                .expect("the canonical fixture closes its frontmatter"),
            "empty body",
        ),
    ];

    for (name, stem, contents, expected) in cases {
        // A rejection table is also the right place to pin what must NOT be
        // rejected — the two live and die together.
        if *expected == "MUST PARSE" {
            parse_scenario(stem, contents)
                .unwrap_or_else(|e| panic!("{name}: must parse, but was rejected: {e}"));
            continue;
        }
        let err = parse_scenario(stem, contents)
            .err()
            .unwrap_or_else(|| panic!("{name}: expected a rejection, got a valid scenario"));
        assert!(
            err.contains(expected),
            "{name}: error should mention `{expected}`, got: {err}"
        );
    }
}

#[test]
fn scenario_corpus_requires_one_dev_and_two_holdout() {
    let file = |skill: &str, n: u32, tag: &str| {
        format!(
            "---\nskill: {skill}\nn: {n}\ntag: {tag}\n\
             pressures: [time, sunk-cost, authority]\n\
             forced_choice: \"A: ship it now · B: write the failing test first · C: ask the human\"\n\
             correct_option: B\n---\n\nbody\n\n\
             A: ship it now\nB: write the failing test first\nC: ask the human\n"
        )
    };
    let full = |tags: [&str; 3]| -> Vec<(String, String)> {
        let mut out = Vec::new();
        for skill in SkillName::ALL.iter().map(|skill| skill.as_str()) {
            for (i, tag) in tags.iter().enumerate() {
                let n = i as u32 + 1;
                out.push((format!("{skill}-{n}"), file(skill, n, tag)));
            }
        }
        for n in 1..=2 {
            out.push((
                format!("using-drovr-noskill-{n}"),
                file("using-drovr", n, "holdout"),
            ));
        }
        out
    };

    check_scenario_corpus(&full(["dev", "holdout", "holdout"])).expect("the §1.2 corpus is valid");

    let err = check_scenario_corpus(&full(["dev", "dev", "holdout"]))
        .expect_err("two dev scenarios must be rejected");
    assert!(err.contains("2 dev and 1 holdout"), "got: {err}");

    let mut short = full(["dev", "holdout", "holdout"]);
    short.pop();
    let err = check_scenario_corpus(&short).expect_err("16 files must be rejected");
    assert!(err.contains("fixes the corpus at 17"), "got: {err}");

    let mut mistagged = full(["dev", "holdout", "holdout"]);
    let last = mistagged.len() - 1;
    mistagged[last].1 = file("using-drovr", 2, "dev");
    let err =
        check_scenario_corpus(&mistagged).expect_err("a dev-tagged noskill file must be rejected");
    assert!(err.contains("must be tagged `holdout`"), "got: {err}");
}

/// plan §1.2's corpus, checked against the schema above.
///
/// Task 3 authors the files. Until it does, this asserts the corpus is **absent**
/// rather than shrugging at an empty directory: a half-written corpus is exactly
/// the state that would otherwise pass silently and be discovered at measurement
/// time. The rules themselves are already enforced — see
/// `parse_scenario_rejects_illegal_states` and
/// `scenario_corpus_requires_one_dev_and_two_holdout`, which prove every rule by
/// fixture today.
#[test]
fn scenarios_are_well_formed() {
    let dir = scenarios_dir();

    if !SCENARIO_CORPUS_AUTHORED {
        let found = if dir.is_dir() {
            markdown_files(&dir)
        } else {
            Vec::new()
        };
        assert!(
            found.is_empty(),
            "{} holds {} scenario file(s), but SCENARIO_CORPUS_AUTHORED is still false. \
             If you are authoring the corpus (Task 3), flip that constant to `true` — this test \
             then enforces plan §1.2 in full. It is false so that a partly-written corpus fails \
             here instead of at measurement time.",
            dir.display(),
            found.len()
        );
        return;
    }

    let files: Vec<(String, String)> = markdown_files(&dir)
        .into_iter()
        .map(|path| {
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_else(|| panic!("unreadable scenario filename: {}", path.display()))
                .to_string();
            let contents = fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
            (stem, contents)
        })
        .collect();

    check_scenario_corpus(&files).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));

    // Not in `check_scenario_corpus`: that function is pure over `(stem,
    // contents)` on purpose, so every corpus rule stays provable by fixture. This
    // one is a question about the filesystem, so it lives where the filesystem
    // already is.
    let root = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/.."));
    for (stem, contents) in &files {
        let bad = bad_path_references(contents, &root);
        assert!(
            bad.is_empty(),
            "{stem}.md carries {bad:?}. A scenario is pasted to a subagent that has tools and is \
             told to act, so anything it can reach it can check — and what it finds will not \
             match, because the scenario is about another project. The run would then measure how \
             the agent handles a prompt it has caught lying. Give the scenario its own project, \
             with plain relative paths that resolve to nothing from here."
        );
    }
}

#[test]
fn all_skills_have_valid_frontmatter() {
    let dir = skills_dir();
    let files = skill_files(&dir);
    assert!(!files.is_empty(), "no skills found under {}", dir.display());

    for (dir_name, path) in &files {
        let contents = fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
        let skill = parse_skill(&contents).unwrap_or_else(|| {
            panic!(
                "{} has no frontmatter: it must open with `---`, close with `---`, \
                 and carry only `key: value` lines in between",
                path.display()
            )
        });

        let name = skill
            .name
            .as_deref()
            .unwrap_or_else(|| panic!("{} missing `name:` in frontmatter", path.display()));
        assert!(!name.is_empty(), "{} has an empty `name:`", path.display());

        let description = skill
            .description
            .as_deref()
            .unwrap_or_else(|| panic!("{} missing `description:` in frontmatter", path.display()));
        assert!(
            !description.is_empty(),
            "{} has an empty `description:`",
            path.display()
        );

        assert_eq!(
            name,
            dir_name,
            "{}: frontmatter `name:` ({name}) must equal its directory name ({dir_name})",
            path.display()
        );
    }
}

/// Length, in words, of the shortest run of text that counts as copied.
///
/// Spec §9.1 check 4 sets it at eight. Shorter runs are how two people writing
/// about the same mechanism collide by accident ("run the scenario without the
/// skill and watch"); eight consecutive words in the same order is not that.
const MIN_SHINGLE_WORDS: usize = 8;

/// Override for where the read-only superpowers corpus lives, or the literal
/// [`CORPUS_NONE`] to declare that this machine has none.
const CORPUS_ENV: &str = "DROVR_SUPERPOWERS_CORPUS";

/// The value of [`CORPUS_ENV`] that declares "there is no corpus here".
const CORPUS_NONE: &str = "none";

/// Where an installed superpowers plugin puts its skills, relative to `$HOME`.
/// The version segment is a wildcard: every installed version is a corpus.
const PLUGIN_CACHE_RELATIVE: &str = ".claude/plugins/cache/claude-plugins-official/superpowers";

/// Where the corpus is — or an explicit statement that there is not one.
///
/// Absence is a *value* here rather than a comment, because the previous shape
/// of this check ("if the directory is missing, print and return") reported `ok`
/// having compared nothing, and no caller could tell that apart from a real
/// pass.
#[derive(Debug, PartialEq, Eq)]
enum CorpusLocation {
    /// Roots to index — non-empty, and not by comment: [`CorpusRoots`] cannot
    /// be built from an empty list.
    Indexed(CorpusRoots),
    /// The operator said this machine has no corpus, via `CORPUS_ENV=none`.
    DeclaredAbsent,
}

/// One or more corpus roots.
///
/// The first root is a field rather than an element, so "at least one" is a
/// property of the type instead of a promise in prose. `Indexed(vec![])` used to
/// be representable, and it would have failed a long way from its cause — as an
/// empty corpus, which reads like a broken install rather than a wiring bug.
#[derive(Debug, PartialEq, Eq)]
struct CorpusRoots {
    first: PathBuf,
    rest: Vec<PathBuf>,
}

impl CorpusRoots {
    fn new(mut roots: Vec<PathBuf>) -> Option<Self> {
        if roots.is_empty() {
            return None;
        }
        let rest = roots.split_off(1);
        Some(CorpusRoots {
            first: roots.remove(0),
            rest,
        })
    }

    fn iter(&self) -> impl Iterator<Item = &Path> {
        std::iter::once(self.first.as_path()).chain(self.rest.iter().map(PathBuf::as_path))
    }
}

/// What `CORPUS_ENV` says, decided once, at the boundary.
///
/// [`CorpusEnv::Dir`] is only constructible by [`read_corpus_env`], which checks
/// the directory exists. That is the point: `resolve_corpus` used to take the
/// path and "is it a directory" as two separate arguments, so a caller could
/// hand it a pair that disagreed and the `true` branch would believe it.
#[derive(Debug, PartialEq, Eq)]
enum CorpusEnv {
    /// The variable is not set.
    Unset,
    /// Set to `none`: this machine has no corpus, and says so.
    DeclaredNone,
    /// Set to a path that **is** a directory.
    Dir(PathBuf),
    /// Set to something that is not a directory.
    NotADir(String),
}

/// Classify `CORPUS_ENV`. `is_dir` is injected so the classification is testable
/// without touching the filesystem.
fn read_corpus_env(raw: Option<&str>, is_dir: impl Fn(&Path) -> bool) -> CorpusEnv {
    match raw {
        None => CorpusEnv::Unset,
        Some(CORPUS_NONE) => CorpusEnv::DeclaredNone,
        Some(path) if is_dir(Path::new(path)) => CorpusEnv::Dir(PathBuf::from(path)),
        Some(path) => CorpusEnv::NotADir(path.to_string()),
    }
}

/// Every installed superpowers version's `skills/` directory under `home`.
///
/// The path is derived from `$HOME` rather than written down, so it is a
/// property of the machine the test runs on instead of the machine it was
/// written on. Sorted, so a failure names roots in a stable order.
///
/// **Nothing here is dropped quietly.** A missing plugin cache is the empty
/// answer — that is a real state, and [`resolve_corpus`] decides what it means.
/// Anything else (an entry that cannot be read, a version directory whose
/// `skills/` exists but cannot be opened) **panics**, because the alternative is
/// indexing part of the corpus and reporting "no overlap" when the truthful
/// answer is "no overlap in the part I could read". That is the same vacuous
/// pass this whole check exists to prevent, one level down.
fn discover_corpus_roots(home: &Path) -> Vec<PathBuf> {
    let versions = home.join(PLUGIN_CACHE_RELATIVE);
    let entries = match fs::read_dir(&versions) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(e) => panic!(
            "cannot read the superpowers plugin cache at {}: {e}. \
             This is not the same as having no corpus — set {CORPUS_ENV}={CORPUS_NONE} if that is \
             what you meant.",
            versions.display()
        ),
    };

    let mut roots = Vec::new();
    for entry in entries {
        let entry = entry.unwrap_or_else(|e| {
            panic!(
                "cannot read an entry of {}: {e}. Refusing to index part of the corpus.",
                versions.display()
            )
        });
        let skills = entry.path().join("skills");
        match fs::metadata(&skills) {
            Ok(meta) if meta.is_dir() => roots.push(skills),
            // A version directory with no `skills/` is not a corpus root, and is
            // not an error: plugin caches hold other things.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Ok(_) => {}
            Err(e) => panic!(
                "cannot stat {}: {e}. An installed version that cannot be read would be silently \
                 left out of the comparison.",
                skills.display()
            ),
        }
    }
    roots.sort();
    roots
}

/// Decide what to compare against, from the classified environment and what the
/// plugin cache scan found.
///
/// Pure, so every branch is testable — including the two that used to be
/// indistinguishable from success.
fn resolve_corpus(env: CorpusEnv, discovered: Vec<PathBuf>) -> Result<CorpusLocation, String> {
    match env {
        CorpusEnv::DeclaredNone => Ok(CorpusLocation::DeclaredAbsent),
        CorpusEnv::Dir(path) => Ok(CorpusLocation::Indexed(
            CorpusRoots::new(vec![path]).expect("one path is one root"),
        )),
        CorpusEnv::NotADir(path) => Err(format!(
            "{CORPUS_ENV} points at `{path}`, which is not a directory. \
             Fix the path, or set {CORPUS_ENV}={CORPUS_NONE} to declare this machine has no corpus."
        )),
        CorpusEnv::Unset => CorpusRoots::new(discovered).map(CorpusLocation::Indexed).ok_or_else(|| format!(
            "no superpowers corpus found under `$HOME/{PLUGIN_CACHE_RELATIVE}/<version>/skills`, \
             so nothing can be compared. Install the superpowers plugin, or set {CORPUS_ENV} to a \
             corpus directory, or set {CORPUS_ENV}={CORPUS_NONE} to declare this machine has none. \
             This fails rather than skipping: a skip prints `ok` having checked nothing, and this \
             check is the only thing standing behind spec §2.1 exception 2."
        )),
    }
}

/// Every `*.md` under `dir`, recursively, sorted so failures name files in a
/// stable order.
/// Directories are visited once, keyed by their canonical path, so a symlink
/// loop is a finite walk rather than a test that never returns.
fn markdown_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    let mut visited: HashSet<PathBuf> = HashSet::new();
    while let Some(current) = stack.pop() {
        let entries = fs::read_dir(&current)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", current.display()));
        for entry in entries {
            let path = entry.expect("read_dir entry").path();
            if path.is_dir() {
                // The visited set is keyed on resolved identity, so a symlink
                // loop terminates. Falling back to the unresolved path when
                // `canonicalize` fails would put the loop back: the same
                // directory reached by two names would look like two
                // directories. If identity cannot be established, say so.
                let key = fs::canonicalize(&path).unwrap_or_else(|e| {
                    panic!(
                        "cannot canonicalize {}: {e}. Directory identity is what stops this walk \
                         from following a symlink loop forever, so it is not something to guess.",
                        path.display()
                    )
                });
                if visited.insert(key) {
                    stack.push(path);
                }
            } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

/// Replace every `http://`/`https://` run with a space.
///
/// A URL is an address, not expression: two documents citing the same site have
/// converged on a source, not copied a sentence. Written by hand because this
/// crate has no regex dependency and one URL shape does not justify adding one.
fn strip_urls(text: &str) -> String {
    const SCHEMES: [&str; 2] = ["https://", "http://"];
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find("http") {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        match SCHEMES.iter().find(|s| tail.starts_with(**s)) {
            Some(_) => {
                // Runs to the next whitespace: trailing `)` or `.` goes with it,
                // and both sides are scrubbed identically, so it cannot skew a
                // comparison.
                let end = tail.find(char::is_whitespace).unwrap_or(tail.len());
                out.push(' ');
                rest = &tail[end..];
            }
            // `http` inside an ordinary word (`httpd`) is just a word.
            None => {
                out.push_str(&tail[.."http".len()]);
                rest = &tail["http".len()..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Lowercased word tokens, with markdown punctuation dropped.
///
/// A word is a run of ASCII alphanumerics plus `'` and `-`, so `don't` and
/// `red-green-refactor` each stay one token. Everything else — table pipes,
/// emphasis markers, list bullets, backticks — is a separator, because copied
/// prose stays copied prose after someone bolds a word in it or moves it into a
/// table cell.
///
/// A run with no alphanumeric in it is **not** a word: `---` opens every
/// frontmatter block and rules off every section, and counting it as shared
/// vocabulary turned two files that merely both have frontmatter into a
/// plagiarism hit.
///
/// A typographic apostrophe is folded to the ASCII one first. Otherwise
/// `don’t` tokenizes as `don` + `t` while `don't` stays one word, and a copied
/// sentence would stop matching because one side had been through an editor
/// that smartens quotes.
fn words(text: &str) -> Vec<String> {
    strip_urls(text)
        .replace('\u{2019}', "'")
        .to_lowercase()
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '\'' || c == '-'))
        .filter(|w| w.chars().any(|c| c.is_ascii_alphanumeric()))
        .map(|w| w.to_string())
        .collect()
}

/// Every window of `n` consecutive words, joined by single spaces.
fn shingles(words: &[String], n: usize) -> Vec<String> {
    words.windows(n).map(|w| w.join(" ")).collect()
}

/// Split a frontmatter line into `(key, value)`.
///
/// A key is a run of identifier characters followed by `:`. A line shaped any
/// other way — a continuation, a list item — has no key.
fn frontmatter_key_value(line: &str) -> Option<(&str, &str)> {
    let colon = line.find(':')?;
    let key = line[..colon].trim();
    let is_key = !key.is_empty()
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    is_key.then(|| (key, line[colon + 1..].trim()))
}

/// The value part of a frontmatter line, or the whole line if it has no key.
fn frontmatter_value(line: &str) -> &str {
    frontmatter_key_value(line).map_or(line, |(_, value)| value)
}

/// Split a leading YAML frontmatter block off, as `(frontmatter, body)`.
///
/// Returns `None` unless the file really opens with one: `---` on its own first
/// line, a later line that is exactly `---`, and every non-empty line between
/// them carrying a `:`. That last condition is the one that matters. A markdown
/// file may open with a horizontal rule and carry another one further down, and
/// without the check every paragraph in between would be shingled line by line —
/// which is how a copied paragraph would slip through a test written to catch
/// copied paragraphs.
/// Both fences are matched as whole trimmed lines rather than by a literal
/// `"---\n"` prefix, so a CRLF file is split like any other. That is not
/// hypothetical tidiness: under a prefix match, one Windows-edited `SKILL.md`
/// would fall back to flat shingling and report its own frontmatter as copied.
fn split_frontmatter(contents: &str) -> Option<(&str, &str)> {
    let mut segments = contents.split_inclusive('\n');
    let open = segments.next()?;
    if open.trim() != "---" {
        return None;
    }
    let mut offset = open.len();
    for line in segments {
        if line.trim() == "---" {
            let front = &contents[open.len()..offset];
            let body = &contents[offset + line.len()..];
            let looks_like_yaml = front
                .lines()
                .filter(|l| !l.trim().is_empty())
                .all(|l| l.contains(':'));
            return looks_like_yaml.then_some((front, body));
        }
        offset += line.len();
    }
    None
}

/// Every shingle in one markdown file, treating YAML frontmatter as **structured
/// data** rather than prose.
///
/// A skill's frontmatter is a fixed set of machine-read fields. Flattened into
/// one word stream it manufactures runs nobody wrote: `name:` and `description:`
/// are format, not vocabulary, and the two values sit adjacent only because the
/// format puts them there. Under a flat stream, drovr's `tdd` and superpowers'
/// `test-driven-development` "shared" the eight words
/// `description use when implementing any feature or bugfix` — of which one is a
/// key and seven are the trigger phrase two skills with the same job must both
/// say. So each field's value is shingled on its own, and no shingle straddles a
/// field boundary.
///
/// The values themselves stay checked. The `description:` is the highest-leverage
/// line in a skill and the likeliest thing to be copied without thinking; it is
/// exactly what this test must still see. (Multi-line YAML values are not
/// handled — no skill in either corpus uses one, and a continuation line is
/// shingled on its own, which is conservative in the safe direction.)
fn file_shingles(contents: &str) -> Vec<String> {
    let Some((front, body)) = split_frontmatter(contents) else {
        return shingles(&words(contents), MIN_SHINGLE_WORDS);
    };

    let mut out = Vec::new();
    for line in front.lines() {
        out.extend(shingles(&words(frontmatter_value(line)), MIN_SHINGLE_WORDS));
    }
    out.extend(shingles(&words(body), MIN_SHINGLE_WORDS));
    out
}

/// How the **corpus** side is indexed: both readings of every file, unioned.
///
/// Our side is indexed precisely, so that this repo's own frontmatter cannot
/// manufacture a hit. The corpus side is indexed permissively for the mirror
/// reason: a corpus file that [`split_frontmatter`] happens to read differently
/// from how a human would must not be able to hide a shared run. A superset
/// costs a little memory and can only ever make the check stricter.
fn corpus_file_shingles(contents: &str) -> Vec<String> {
    let mut out = file_shingles(contents);
    if split_frontmatter(contents).is_some() {
        out.extend(shingles(&words(contents), MIN_SHINGLE_WORDS));
    }
    out
}

/// A passage that is known to overlap the superpowers corpus and is allowed to,
/// until the task that owns the text decides what to do about it.
struct SharedPassage {
    /// Path relative to `skills/`. An exemption excuses this passage **in this
    /// file only** — the same sentence appearing anywhere else is still a hit.
    file: &'static str,
    /// The overlapping text, verbatim. If it is no longer in the file, the
    /// exemption is stale and the test fails: an allowlist that outlives the text
    /// it excuses quietly licenses the next copy of it.
    passage: &'static str,
    /// Why it is still here, and who decides.
    why: &'static str,
}

/// The overlap that already existed when this check was written, enumerated.
///
/// **This is a conflict inside `spec.md`, not an oversight.** §9.1 check 4 wants
/// no shared 8-word run; §3 and §4.1 freeze text that has one. Both entries below
/// survive the fixes that rewrite their files, so no later task removes them by
/// doing its own job:
///
///   * §3's replacement `description:` for `systematic-debugging` keeps the
///     opening the current one shares with superpowers, so fix 1 (Task 7) does
///     not clear it — it **lengthened** the run, exactly as this note predicted.
///   * §3's replacement `description:` for `tdd` **is** superpowers'
///     `test-driven-development` description, word for word, as its opening
///     clause. Fix 1 did not create the collision by carelessness: the string is
///     frozen spec text, and the pre-fix `in a drovr phase` was the only thing
///     interrupting the run.
///   * §4.1 step 1 says to **keep** `using-drovr`'s `<SUBAGENT-STOP>` block, so
///     fix 2's doc layer (Task 14) does not clear it either.
///
/// So the run ends with two choices open, and §2.1 exception 2 already names
/// them: reword the line, or add the MIT notice and credit. Recording them here
/// keeps the check live for every *new* line while leaving that decision to §9
/// (Task 23) and to a human — which is where a deviation from frozen spec text
/// belongs. **Nothing here is a licence finding**; both projects are MIT.
///
/// Adding an entry is a deliberate act with a named owner. Do not add one to make
/// a red test green.
const KNOWN_SHARED_PASSAGES: &[SharedPassage] = &[
    SharedPassage {
        file: "systematic-debugging/SKILL.md",
        passage: "Use when encountering any bug, test failure, or unexpected behavior, before proposing",
        why: "the trigger description. spec §3 freezes a replacement that keeps this opening, \
              so Task 7 (fix 1) did not clear it — it lengthened the shared run instead, by \
              deleting the `in a drovr phase` that had interrupted it. Extended from ten words \
              to twelve when fix 1 landed, which is the growth the pre-fix note predicted. \
              Task 23 (§9) decides: reword, or attribute",
    },
    SharedPassage {
        file: "tdd/SKILL.md",
        passage: "Use when implementing any feature or bugfix, before writing implementation code",
        why: "the trigger description, and the worst of the three: spec §3's frozen replacement \
              reproduces superpowers' `test-driven-development` description in full as its \
              opening clause, so the shared run is a whole description rather than a phrase two \
              authors happened to converge on. It appeared when Task 7 deleted the \
              `in a drovr phase` that had interrupted it, and Task 7 could not reword it — §3's \
              strings are frozen and are what arm A′ measures. Task 23 (§9) decides: reword, or \
              attribute. Attribution is the likelier answer here",
    },
    SharedPassage {
        file: "using-drovr/SKILL.md",
        passage: "<SUBAGENT-STOP>\nIf you were dispatched as a subagent to execute a specific task, ignore this",
        why: "the <SUBAGENT-STOP> device, ported wholesale — the tag name is part of the shared \
              run, because both files open the block the same way, and a newer superpowers \
              version extends the match through `ignore this`. spec §4.1 step 1 says keep it, \
              so Task 14 does not clear it — Task 23 (§9) decides: reword, or attribute",
    },
];

/// §9.1 check 4: no ≥8-word run of text is shared with the superpowers corpus.
///
/// drovr ports superpowers' *mechanisms* under §2.1's tier-3 rule and writes its
/// own sentences (§2.1 exception 2). Both projects are MIT, so copying with
/// attribution would be legal — the rule is about drovr being a self-contained
/// replacement, not about licensing. This test is what turns that from an
/// intention into a checked property, and `skills/writing-skills/` is the file
/// tree it exists for: that skill is assembled almost entirely from ported
/// conventions, so it is the likeliest place in the repo for a sentence to
/// survive intact.
///
/// It walks **every** `*.md` under `skills/`, not just `SKILL.md`, so reference
/// files and scenario prompts are covered too, and later tasks re-run it for
/// free by touching any skill.
///
/// **A hit is not a licence failure — it is a rewrite request.** Reword the line,
/// or, if the text genuinely must be reproduced, add the MIT notice and credit
/// §2.1 exception 2 requires *and* give this test an explicit, narrowly-scoped
/// exemption for that one passage. There is no exemption mechanism today because
/// no attributed passage exists; build it against the real text, not against a
/// hypothetical one.
///
/// **There is exactly one way this test declines to compare, and it has to be
/// asked for.** A corpus that is merely missing is a **failure**; so is one that
/// is partly unreadable. Only `DROVR_SUPERPOWERS_CORPUS=none` skips, and it
/// prints `NOTHING WAS COMPARED` when it does. See [`resolve_corpus`] for the
/// full order of resolution.
///
/// The reason it is not a silent skip: `cargo test` captures `eprintln!`, so the
/// old behaviour reported `ok` having compared nothing, and no reader could tell
/// that apart from a real pass. If you report this test as passing, that claim
/// now means something — unless you set `none`, in which case say so.
#[test]
fn no_verbatim_overlap_with_superpowers() {
    let raw = std::env::var(CORPUS_ENV).ok();
    let env = read_corpus_env(raw.as_deref(), |p| p.is_dir());
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let discovered = home
        .as_deref()
        .map(discover_corpus_roots)
        .unwrap_or_default();

    let roots = match resolve_corpus(env, discovered).unwrap_or_else(|e| {
        panic!("{e}");
    }) {
        CorpusLocation::DeclaredAbsent => {
            eprintln!(
                "no_verbatim_overlap_with_superpowers: {CORPUS_ENV}={CORPUS_NONE} was set, so \
                 NOTHING WAS COMPARED. This machine has declared it cannot run spec §9.1 check 4."
            );
            return;
        }
        CorpusLocation::Indexed(roots) => roots,
    };

    let corpus_files: Vec<PathBuf> = roots.iter().flat_map(markdown_files).collect();
    assert!(
        !corpus_files.is_empty(),
        "corpus roots {roots:?} exist but hold no markdown — that is a broken corpus, \
         not an absent one"
    );
    eprintln!(
        "no_verbatim_overlap_with_superpowers: comparing against {} corpus file(s) across {} root(s)",
        corpus_files.len(),
        roots.iter().count()
    );

    // shingle -> the corpus file it came from, so a failure names both sides.
    let mut corpus_shingles: HashMap<String, PathBuf> = HashMap::new();
    for path in &corpus_files {
        let contents = fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
        for shingle in corpus_file_shingles(&contents) {
            corpus_shingles
                .entry(shingle)
                .or_insert_with(|| path.clone());
        }
    }

    let skills = skills_dir();
    let ours = markdown_files(&skills);
    assert!(
        !ours.is_empty(),
        "no markdown found under {}",
        skills.display()
    );

    // Resolve the exemptions first, and fail on any that no longer matches the
    // text it excuses. The list is only honest if it shrinks as the run rewords
    // things; a stale entry would silently excuse a fresh copy of the same line.
    let mut excused: HashMap<PathBuf, HashSet<String>> = HashMap::new();
    for known in KNOWN_SHARED_PASSAGES {
        let path = skills.join(known.file);
        let contents = fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!(
                "KNOWN_SHARED_PASSAGES names {}, which cannot be read: {e}",
                path.display()
            )
        });
        let passage = words(known.passage);
        assert!(
            passage.len() >= MIN_SHINGLE_WORDS,
            "KNOWN_SHARED_PASSAGES entry for {} is only {} words — shorter than a \
             {MIN_SHINGLE_WORDS}-word shingle, so it excuses nothing",
            known.file,
            passage.len()
        );
        // Staleness is judged against the SAME shingle stream the comparison
        // below uses, not against a flat read of the file. Against a flat read,
        // an entry could be kept alive by the words happening to reappear
        // somewhere the check never looks — an exemption validated by text it
        // does not excuse.
        let passage_shingles = shingles(&passage, MIN_SHINGLE_WORDS);
        let in_file: HashSet<String> = file_shingles(&contents).into_iter().collect();
        assert!(
            passage_shingles.iter().all(|s| in_file.contains(s)),
            "stale exemption: {} no longer contains \"{}\" where this check reads it. \
             Delete the KNOWN_SHARED_PASSAGES entry ({}).",
            known.file,
            known.passage,
            known.why
        );
        excused.entry(path).or_default().extend(passage_shingles);
    }

    // One hit per file: the first is enough to send the author back to the text,
    // and a copied paragraph would otherwise report every window inside it.
    let mut hits: Vec<String> = Vec::new();
    let mut total_hits = 0usize;
    let mut ours_shingle_count = 0usize;
    for path in &ours {
        let contents = fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
        let excused_here = excused.get(path);
        let mut seen: HashSet<&str> = HashSet::new();
        let mut first: Option<String> = None;
        let ours_shingles = file_shingles(&contents);
        ours_shingle_count += ours_shingles.len();
        for shingle in &ours_shingles {
            let Some(source) = corpus_shingles.get(shingle) else {
                continue;
            };
            if excused_here.is_some_and(|e| e.contains(shingle)) {
                continue;
            }
            if !seen.insert(shingle.as_str()) {
                continue;
            }
            total_hits += 1;
            if first.is_none() {
                first = Some(format!(
                    "  {}\n    shares \"{shingle}\"\n    with {}",
                    path.display(),
                    source.display()
                ));
            }
        }
        if let Some(first) = first {
            hits.push(first);
        }
    }

    // Both sides had to contribute something. `hits.is_empty()` is true of a
    // repo whose skills are all shorter than a shingle, and that would read as
    // "no overlap" rather than "nothing was long enough to compare".
    //
    // Guarding one side and not the other is worse than guarding neither: the
    // asymmetry looks deliberate, so nobody goes looking for the hole on the
    // unguarded side. Both are asserted, in the same place, for that reason.
    assert!(
        ours_shingle_count > 0,
        "{} skill file(s) produced no {MIN_SHINGLE_WORDS}-word run between them, \
         so this check compared nothing on our side",
        ours.len()
    );
    assert!(
        !corpus_shingles.is_empty(),
        "{} corpus file(s) across {roots:?} produced no {MIN_SHINGLE_WORDS}-word run between them, \
         so there was nothing to compare against — a corpus that parses to no shingles is a broken \
         corpus, not a clean result",
        corpus_files.len()
    );

    assert!(
        hits.is_empty(),
        "{} file(s) share text with the superpowers corpus at {roots:?} \
         ({total_hits} distinct {MIN_SHINGLE_WORDS}-word run(s) in total; \
         the first from each file is shown):\n{}\n\
         Reword it, or add the MIT attribution §2.1 exception 2 requires together with an \
         explicit exemption here.",
        hits.len(),
        hits.join("\n"),
    );
}

#[test]
fn methodology_skills_within_body_budget() {
    let dir = skills_dir();

    for skill in SkillName::methodology() {
        let path = dir.join(skill.as_str()).join("SKILL.md");
        assert!(
            path.is_file(),
            "expected methodology skill at {}",
            path.display()
        );
        let contents = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
        let skill = parse_skill(&contents).unwrap_or_else(|| {
            panic!(
                "{} has no frontmatter: it must open with `---`, close with `---`, \
                 and carry only `key: value` lines in between",
                path.display()
            )
        });

        let body_len = skill.body.len();
        assert!(
            body_len <= BODY_BUDGET,
            "{}: body is {body_len} bytes, exceeds budget of {BODY_BUDGET}",
            path.display()
        );
    }
}

/// The three literals fix 1 exists to remove (spec §3).
///
/// Each one scoped an unconditional discipline to a drovr *phase*, while
/// `using-drovr` makes working inline the default — so an agent working inline
/// read the trigger and correctly concluded the skill did not apply.
///
/// **Matched case-INSENSITIVELY**, so a sentence-initial *"In a drovr phase you
/// must…"* cannot reintroduce the defect past this check.
///
/// An earlier version of this const matched case-sensitively and defended it on
/// the grounds that a case-insensitive ban would forbid the demoted form fix 1
/// prescribes. **That was wrong, and checking it was what showed it.** The
/// shipped demotions read *"**Inside** a drovr phase this also binds the next
/// phase's contract"* (`skills/tdd/SKILL.md`) and *"**Inside** a drovr phase
/// this is also what keeps the single-writer rule intact"*
/// (`skills/systematic-debugging/SKILL.md`) — and `inside a drovr phase` does
/// not contain `in a drovr phase` at any casing. The restriction cost a real
/// hole and bought nothing.
///
/// The residue is a naming rule, not a lost capability: a demotion says
/// *"Inside a drovr phase…"* or *"Within a drovr phase…"*, never *"In a drovr
/// phase…"*. spec §9.1 check 3's grep is case-sensitive, so this is **stricter
/// than the spec requires** — deliberately, and in the direction §3 wants.
///
/// Keep every entry lowercase: [`phase_scoped_literals_in`] lowercases only the
/// text it is searching, not the needles.
const PHASE_SCOPED_LITERALS: &[&str] = &[
    "in a drovr phase",
    "a drovr task",
    "a drovr phase has produced",
];

/// Does `contents` carry any of the phase-scoping literals? Returns the ones it
/// carries, so the failure text can name them.
///
/// Factored out because it is run over two corpora: the live skills, where it
/// must find nothing, and the frozen arm A snapshots, where it must find
/// everything. One matcher, so the negative assertion cannot drift away from
/// the positive control that proves the matcher works.
fn phase_scoped_literals_in(contents: &str) -> Vec<&'static str> {
    let haystack = contents.to_lowercase();
    PHASE_SCOPED_LITERALS
        .iter()
        .copied()
        .filter(|literal| {
            debug_assert_eq!(
                **literal,
                literal.to_lowercase(),
                "PHASE_SCOPED_LITERALS entries must be lowercase; only the haystack is folded"
            );
            haystack.contains(literal)
        })
        .collect()
}

/// Fix 1 (spec §3): the **three literals** [`PHASE_SCOPED_LITERALS`] names are
/// gone from every shipped skill, and stay gone.
///
/// **Read that scope literally — it is narrower than "no skill scopes its
/// trigger to a phase", and saying the broader thing would be this run's own
/// defect class.** What it catches is the regression of the exact wording fix 1
/// removed, at any casing. What it does **not** catch is a *fresh* phrasing of
/// the same mistake — *"during a drovr phase"*, *"once a phase has started"*,
/// *"when running under drovr"*. Those are caught by review, not by this test.
/// Nothing here pretends otherwise.
///
/// **This is an absence test, and an absence test is this run's recurring
/// defect class wearing its most convincing costume** — it passes just as
/// cheerfully when the walk globs nothing, when it reads the wrong tree, or when
/// the literals were never there. Green here is worth nothing on its own.
///
/// Three things make it worth something:
///
///  1. **A positive control on real, frozen data.** The same matcher is run over
///     `docs/skill-evidence/arms/A/`, the pre-fix snapshot, where every literal
///     must still be found. Arm A is immutable (`arm_a_snapshots_match_manifest`
///     hashes it), so this control cannot rot — and if the matcher ever stops
///     matching, this test fails instead of quietly passing.
///  2. **The walk is asserted to have covered the measured skills**, so an empty
///     or mis-rooted glob is a failure rather than a pass.
///  3. It was watched RED against the pre-fix text before fix 1 landed.
///
/// It stays green through the §6 rewrites and through a §7.3 revert to A′ — fix
/// 1 ships regardless of every measurement outcome, so A′ carries it too.
#[test]
fn no_phase_scoped_description_literals() {
    let files = skill_files(&skills_dir());

    // Guard 2: the walk found a corpus, and specifically it found the skills
    // whose descriptions carried the defect. A glob that matched nothing would
    // otherwise satisfy every assertion below.
    let found: HashSet<&str> = files.iter().map(|(name, _)| name.as_str()).collect();
    for skill in SkillName::ALL {
        assert!(
            found.contains(skill.as_str()),
            "the skills walk over {} did not find `{}`; an absence check that \
             globbed nothing would pass having read nothing",
            skills_dir().display(),
            skill.as_str()
        );
    }

    let mut hits = Vec::new();
    for (_, path) in &files {
        let contents = fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
        for literal in phase_scoped_literals_in(&contents) {
            hits.push(format!("{}: `{literal}`", path.display()));
        }
    }
    assert!(
        hits.is_empty(),
        "{} skill file(s) still scope discipline to a drovr phase:\n{}\n\
         An agent working inline — which `using-drovr` makes the default — reads \
         this and concludes the skill does not apply. Rephrase the phase \
         reference as an *additional* consequence, never a precondition (spec §3).",
        hits.len(),
        hits.join("\n"),
    );

    // Guard 1: the positive control. Arm A is the pre-fix text, frozen; if the
    // matcher above found nothing there either, it is not matching at all.
    let arm_a = arms_dir().join("A");
    let mut seen: HashSet<&'static str> = HashSet::new();
    for skill in SkillName::ALL {
        let path = arm_a.join(format!("{}.md", skill.as_str()));
        let contents = fs::read_to_string(&path).unwrap_or_else(|e| {
            panic!(
                "cannot read the arm A snapshot {}: {e} — it is this check's \
                 positive control, not an optional extra",
                path.display()
            )
        });
        seen.extend(phase_scoped_literals_in(&contents));
    }
    let unmatched: Vec<&str> = PHASE_SCOPED_LITERALS
        .iter()
        .copied()
        .filter(|literal| !seen.contains(literal))
        .collect();
    assert!(
        unmatched.is_empty(),
        "the matcher found no occurrence of {unmatched:?} anywhere in {} — arm A \
         is the pre-fix text and every literal is present in it by construction, \
         so this means the check above is asserting the absence of something it \
         cannot detect",
        arm_a.display(),
    );
}

/// The evidence corpus is the only citable record behind every numeric or
/// comparative claim drovr's skill text makes (spec §2.1 exception 1). It is
/// prose, so nothing else in this suite would notice it going missing — a task
/// that deleted `docs/skill-evidence/tdd.md` would leave the run's claims
/// standing with their evidence gone and every test still green.
///
/// This is a tripwire, deliberately shallow: **presence and non-emptiness, not
/// content.** Later tasks rewrite these files repeatedly — RED now, counter-text
/// at fix 4, scored results at the A/B stages — so asserting anything about
/// their shape here would be a second contract on files that are still being
/// written. What it does refuse is the failure it exists for: a missing file, a
/// directory in a file's place, an unreadable file, and a file that is empty or
/// holds nothing but whitespace.
#[test]
fn evidence_corpus_present() {
    let dir = evidence_dir();
    assert!(
        dir.is_dir(),
        "expected the evidence corpus at {}",
        dir.display()
    );

    // Per-skill records first, then the ledger, so the failure names the file.
    // Walked as `SkillName`, not as a re-listed set of strings: the measured
    // skills are already a closed type in this file, and a skill added to it
    // without an evidence record is exactly what should fail here.
    let mut expected: Vec<String> = SkillName::ALL
        .iter()
        .map(|skill| format!("{}.md", skill.as_str()))
        .collect();
    expected.push(EVIDENCE_LEDGER.to_string());

    for name in &expected {
        let path = dir.join(name);
        assert!(
            path.is_file(),
            "{} is missing — the evidence corpus is what spec §2.1 exception 1 \
             makes every measured claim citable against; do not delete it",
            path.display()
        );
        let contents = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
        assert!(
            !contents.trim().is_empty(),
            "{} is empty — an empty evidence file passes a presence check while \
             recording nothing, which is worse than a missing one",
            path.display()
        );
    }
}
