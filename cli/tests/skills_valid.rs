//! Validates every `skills/*/SKILL.md` in the repo and enforces a per-skill
//! body-size budget on the five measured `drovr:*` skills.
//!
//! Six assertions:
//!   1. **All** skills have valid frontmatter: a leading `---` block containing
//!      non-empty `name:` and `description:`, and `name:` equals the directory
//!      name.
//!   2. Every skill is budgeted or declared unchecked, and no checked skill
//!      exceeds its own cap (spec §2.4). The four discipline skills get 12000
//!      bytes of post-frontmatter body; `using-drovr` gets 9000, because it is
//!      injected in full at every `SessionStart`. `handoff`, `pipeline`,
//!      `worktrees` and `writing-skills` are recorded [`UNCHECKED_SKILLS`] with
//!      the reason — an exemption, never an omission. See [`BodyBudget`].
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
//!   6. Every skill declares whether it carries spec §6's fix-4 armor, and the
//!      ones that do carry §6's REQUIRED sections in §6's order — plus exactly
//!      the CONDITIONAL sections §6 names them for, and no others. See
//!      [`SKILL_ARMOR_STATES`].
//!
//! Assertions 1–4 and 6 are unconditional. **Assertion 5 is the one exception, and it
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

/// Arm B is fix 3 + fix 4: the armored discipline skills and the router's
/// per-turn gate. Filled in one skill at a time by Tasks 10–14.
///
/// **This test could not exist until now, and that is why it is here.**
/// [`assert_arm_snapshots_match_manifest`] requires a row for every
/// [`SkillName::ALL`] entry, so a partially-filled arm fails it — which is why
/// `MANIFEST.md` reserves this test for the task that lands arm B's fifth
/// skill. Task 14 is that task.
///
/// **What it closes:** until now nothing hashed `docs/skill-evidence/arms/B/`,
/// so a byte appended to any arm B snapshot — or a live `SKILL.md` edited after
/// its arm was frozen — passed the entire suite in silence.
/// [`manifest_commits_contain_their_snapshots`] covered arm B's *provenance*
/// (does history hold the recorded text) but not its *drift* (does the file on
/// disk still hash to what the manifest says), and those failed independently
/// once already: an arm B row matched its snapshot perfectly while naming a
/// commit where the source path still held arm A′.
#[test]
fn arm_b_snapshots_match_manifest() {
    assert_arm_snapshots_match_manifest("B");
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

/// The arm whose rows are the voice probe's four register variants (§7.4).
const VOICE_ARM: &str = "voice";

/// The variant every other one is diffed against. It is not a [`VoiceVariant`]
/// row because it is the left-hand side of every comparison, not a case of one.
const VOICE_BASELINE: &str = "V0";

/// §6's section 2, spelled once — `V1`'s and `V3`'s device site.
///
/// Centralized for the same reason as [`VOICE_IRON_LAW_SECTION`]: two table
/// rows naming the same section independently can be renamed apart, leaving one
/// variant's separability check watching a section the other's does not.
const VOICE_OVERVIEW_SECTION: &str = "Overview";

/// §6's section 4, spelled once.
///
/// Two checks depend on this exact heading and they must not drift apart: it is
/// `V2`'s declared [`VoiceVariant::device_site`] — the one section `V2` may
/// differ from `V0` in — and it is where
/// [`every_voice_variant_keeps_the_baselines_iron_law_line`] looks for the
/// fenced line that all four share. Spelled twice, a rename could leave the
/// separability check watching one section while the fence-identity check
/// watched another, and both would still pass.
const VOICE_IRON_LAW_SECTION: &str = "The Iron Law";

/// One register variant of the voice probe (§7.4, plan Task 15), named with the
/// single section its device is allowed to live in.
///
/// **The site is data, not something the test discovers.** A check that looked
/// for whichever section happened to differ would pass for a variant that
/// differed *somewhere* — which is exactly the failure this table exists to
/// catch, because a device that leaked into a second section still leaves "one
/// section differs" true once you go looking for it afterwards. Naming the site
/// up front turns a leak into a named mismatch.
struct VoiceVariant {
    /// File stem under `arms/voice/`, and the variant's `skill` cell in
    /// `MANIFEST.md`.
    name: &'static str,
    /// The one `##` heading whose body may differ from [`VOICE_BASELINE`]'s.
    /// Every other section, and the frontmatter, must be byte-identical.
    device_site: &'static str,
    /// A phrase carried by **this variant and no other**, which is what makes
    /// the row a claim about *which* device the variant adds rather than only
    /// about where it differs.
    ///
    /// **Without it, `V1` and `V3` are swappable.** They declare the same
    /// [`Self::device_site`], so "exactly one section differs, and it is the
    /// Overview" stays true if their two Overview paragraphs trade places — and
    /// Task 21 would then run the moral arm under the unity label, score it, and
    /// apply §7.4's rule 4 (escalate if unity loses) to the wrong arm entirely.
    /// The identity of an arm cannot rest on its filename alone.
    device_marker: &'static str,
}

/// §7.4's three non-baseline variants and where each one's single device sits.
///
/// - `V1` adds the **unity** line to the Overview.
/// - `V2` adds the **authority** register — `MUST`/`NEVER` prose and the
///   absolutist "No exceptions:" framing — to the Iron Law. It does **not** add
///   the fenced all-caps line: `V0` already carries that, per plan Task 15's
///   ruling, so the `V0`→`V2` diff is two named devices and not §7.4's three.
///   [`every_voice_variant_keeps_the_baselines_iron_law_line`] is what holds
///   that ruling in place.
/// - `V3` adds **moral** framing to the Overview.
///
/// `V1` and `V3` deliberately share a site: two devices in the same slot make
/// their diffs against `V0` the same shape, so a reader comparing them is
/// comparing registers rather than positions.
const VOICE_VARIANTS: &[VoiceVariant] = &[
    VoiceVariant {
        name: "V1",
        device_site: VOICE_OVERVIEW_SECTION,
        // The unity line itself, which §6 section 3 and §2.3 both name.
        device_marker: UNITY_LINE,
    },
    VoiceVariant {
        name: "V2",
        device_site: VOICE_IRON_LAW_SECTION,
        // `MUST` in the *Fresh* definition. The bullets' `NEVER` would serve
        // equally; this one is picked because it is the device at its least
        // ambiguous — a modal verb in prose, not a shouted list item.
        device_marker: "both halves MUST hold",
    },
    VoiceVariant {
        name: "V3",
        device_site: VOICE_OVERVIEW_SECTION,
        // The moral characterisation of the violation, which is the device:
        // superpowers' register is *dishonesty*, not *cost*.
        device_marker: "is a lie, not a shortcut",
    },
];

/// One heading-delimited section of a markdown body.
struct MdSection {
    /// The heading text, or empty for the run of lines before the first
    /// heading.
    heading: String,
    /// Everything under that heading up to the next one, verbatim.
    body: String,
}

/// Split a body into its heading-delimited sections, in document order.
///
/// Built on [`headings`], so it is fence-aware: a `##` line inside a fenced
/// block does not open a section.
///
/// **The text before the first heading is returned as a section too**, with an
/// empty heading. It is usually the blank line after the frontmatter and easy to
/// dismiss, but dropping it would let a variant grow a paragraph above its H1
/// that no comparison ever looked at — a register device hiding in the one place
/// the checker was not.
fn md_sections(body: &str) -> Vec<MdSection> {
    let lines: Vec<&str> = body.lines().collect();
    let heads = headings(body);
    let first = heads.first().map_or(lines.len(), |(line, _)| *line);

    let mut out = vec![MdSection {
        heading: String::new(),
        body: lines[..first].join("\n"),
    }];
    for (idx, (line, text)) in heads.iter().enumerate() {
        let end = heads.get(idx + 1).map_or(lines.len(), |(l, _)| *l);
        out.push(MdSection {
            heading: text.clone(),
            body: lines[line + 1..end].join("\n"),
        });
    }
    out
}

/// Read one voice variant, parsed as the skill document it is pasted into a
/// probe run as.
///
/// **Fence termination is checked here, before any caller splits the body into
/// sections.** [`headings`] treats an unterminated fence as swallowing the rest
/// of the file, so a missing closing ``` would reach the separability check as
/// *"different section set"* or *"must differ in Overview"* — a true statement
/// about a document nobody meant to write, and one that sends the reader to
/// audit prose instead of to the fence. Checking once at the read point names
/// the real fault and names it for every voice test at the same time.
fn read_voice_variant(name: &str) -> Skill {
    let path = arms_dir().join(VOICE_ARM).join(format!("{name}.md"));
    let contents = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read voice variant {}: {e}", path.display()));
    let skill = parse_skill(&contents)
        .unwrap_or_else(|| panic!("{}: no frontmatter — a variant is a skill document, and its `description:` is part of what the probe measures", path.display()));
    if let Err(line) = fenced_blocks(&skill.body) {
        panic!(
            "{}: the fence opened at body line {line} never closes, so every section below it \
             reads as code. Fix the fence; the section checks cannot say anything useful until \
             you do.",
            path.display()
        );
    }
    skill
}

/// §7.4's separability invariant: each variant differs from `V0` in **exactly
/// one** section, and the section set and order are identical across all four.
///
/// This is the whole probe. Four variants that differ in more than their one
/// register device do not measure that device at n=6 — they measure the sum of
/// whatever else drifted, and no amount of care in Task 21's scoring recovers
/// the difference. The plan asks for the four diffs by hand; this is the same
/// question asked on every `cargo test`, so a later edit to one variant cannot
/// quietly desynchronise the set.
///
/// **The differing section is asserted present, not merely bounded.** A variant
/// byte-identical to `V0` would satisfy "at most one section differs" while
/// being a silently null arm: six runs of the baseline reported as six runs of
/// the device.
#[test]
fn voice_variants_differ_from_the_baseline_in_exactly_one_section() {
    let baseline = read_voice_variant(VOICE_BASELINE);
    let baseline_sections = md_sections(&baseline.body);
    let baseline_headings: Vec<&str> = baseline_sections
        .iter()
        .map(|s| s.heading.as_str())
        .collect();

    for variant in VOICE_VARIANTS {
        let file = read_voice_variant(variant.name);
        let sections = md_sections(&file.body);
        let variant_headings: Vec<&str> = sections.iter().map(|s| s.heading.as_str()).collect();

        // Structure before content: a differing section *set* is reported as
        // itself, rather than as a pile of differing bodies that sends a reader
        // hunting for prose changes when a whole section moved.
        assert_eq!(
            variant_headings, baseline_headings,
            "voice variant `{}` has a different section set or order than `{VOICE_BASELINE}` — \
             §7.4 requires the four variants to be identical in structure",
            variant.name
        );

        let differing: Vec<&str> = baseline_sections
            .iter()
            .zip(sections.iter())
            .filter(|(base, var)| base.body != var.body)
            .map(|(_, var)| var.heading.as_str())
            .collect();
        assert_eq!(
            differing,
            vec![variant.device_site],
            "voice variant `{}` must differ from `{VOICE_BASELINE}` in `{}` and nowhere else",
            variant.name,
            variant.device_site
        );
    }
}

/// The Iron Law's fenced all-caps line is byte-identical in all four variants —
/// including `V0`.
///
/// **This is the trap plan Task 15 disarmed, held open.** §7.4 lists "all-caps
/// Iron Law" among `V2`'s added devices, while §6 makes the fenced all-caps line
/// unconditional structure; the plan ruled that the *format* is §6 structure and
/// survives every §7.4 outcome, and that only the surrounding `MUST`/`NEVER`
/// prose is the register device. If `V0` ever loses the fenced line, the
/// measured baseline stops matching the text that actually ships under §7.4
/// outcome 2 and the probe's result no longer transfers to the documents it
/// governs — a failure that is invisible in the scores and fatal to their
/// meaning.
#[test]
fn every_voice_variant_keeps_the_baselines_iron_law_line() {
    let iron_law = |name: &str| -> String {
        let file = read_voice_variant(name);
        let section = md_sections(&file.body)
            .into_iter()
            .find(|s| s.heading == VOICE_IRON_LAW_SECTION)
            .unwrap_or_else(|| {
                panic!("voice variant `{name}` has no `## {VOICE_IRON_LAW_SECTION}` section")
            });
        let blocks = fenced_blocks(&section.body).unwrap_or_else(|line| {
            panic!("voice variant `{name}`: unterminated fence in the Iron Law, body line {line}")
        });
        let block = blocks.first().unwrap_or_else(|| {
            panic!(
                "voice variant `{name}` has no fenced block in `## {VOICE_IRON_LAW_SECTION}` — §6 makes the \
                 fenced all-caps line unconditional structure, and plan Task 15 rules that it \
                 survives every §7.4 outcome, `V0` included"
            )
        });
        block.body.clone()
    };

    let baseline = iron_law(VOICE_BASELINE);
    assert_eq!(
        baseline,
        baseline.to_uppercase(),
        "`{VOICE_BASELINE}`'s Iron Law line must stay all-caps: it is §6 structure, not the \
         register device §7.4 attributes to `V2`"
    );
    for variant in VOICE_VARIANTS {
        assert_eq!(
            iron_law(variant.name),
            baseline,
            "voice variant `{}` states a different Iron Law than `{VOICE_BASELINE}` — §7.4 \
             requires the same Iron Law across all four",
            variant.name
        );
    }
}

/// Each variant carries **its own** device, and carries no other variant's.
///
/// [`voice_variants_differ_from_the_baseline_in_exactly_one_section`] pins
/// *where* each variant differs from `V0`; this pins *what* it differs by, which
/// is a separate claim and the one the labels rest on. `V1` and `V3` declare the
/// same [`VoiceVariant::device_site`], so nothing else in this file notices if
/// their two Overview paragraphs trade places — and Task 21 reads the arm labels
/// off these filenames when it scores 24 runs and applies §7.4's decision rule.
/// A silently transposed pair produces a confident, wrong conclusion about which
/// register binds.
///
/// **The absence half is the load-bearing one.** Asserting only that `V1` holds
/// the unity line would pass for a `V1` that held the moral framing too — which
/// is a two-device variant, and not separable at n=6.
#[test]
fn each_voice_variant_carries_its_own_device_and_no_others() {
    let baseline = normalize_ws(&read_voice_variant(VOICE_BASELINE).body);
    // Folded before matching: these files are hard-wrapped, and every marker
    // spans a line break in at least one of them. An unfolded substring search
    // reports "the phrase is not in the file at all" for a paragraph that
    // merely re-wrapped.
    let bodies: Vec<(&str, String)> = VOICE_VARIANTS
        .iter()
        .map(|v| (v.name, normalize_ws(&read_voice_variant(v.name).body)))
        .collect();

    for variant in VOICE_VARIANTS {
        let marker = normalize_ws(variant.device_marker);
        assert!(
            !baseline.contains(&marker),
            "`{VOICE_BASELINE}` carries `{}`'s device marker \"{}\" — the baseline is defined by \
             lacking it, so the arm measures nothing",
            variant.name,
            variant.device_marker
        );
        for (name, body) in &bodies {
            let expected = *name == variant.name;
            assert_eq!(
                body.contains(&marker),
                expected,
                "voice variant `{name}` {} `{}`'s device marker \"{}\"",
                if expected { "is missing" } else { "carries" },
                variant.name,
                variant.device_marker
            );
        }
    }
}

/// Every variant carries the same frontmatter as `V0`.
///
/// The `description:` is the trigger and is itself under test elsewhere in this
/// run, so a variant that reworded it would be running a second experiment
/// inside the first. The `name:` matters for a different reason: it is what the
/// Announce sentence and the cross-refs spell, so four different names would
/// make four different documents rather than one document in four registers.
#[test]
fn voice_variants_share_one_frontmatter() {
    let baseline = read_voice_variant(VOICE_BASELINE);
    for variant in VOICE_VARIANTS {
        let file = read_voice_variant(variant.name);
        assert_eq!(
            file.name, baseline.name,
            "voice variant `{}` renames the skill",
            variant.name
        );
        assert_eq!(
            file.description, baseline.description,
            "voice variant `{}` rewords the `description:` — that is the §3 trigger, and varying \
             it would run a second experiment inside the voice probe",
            variant.name
        );
    }
}

/// The voice arm's drift tripwire — the counterpart of
/// [`arm_b_snapshots_match_manifest`], written for the one arm whose layout is
/// not per-skill.
///
/// [`assert_arm_snapshots_match_manifest`] cannot serve here: it iterates
/// [`SkillName::ALL`] and holds every row to `skills/<skill>/SKILL.md`, and the
/// voice arm has neither shape. **Its `source path` is the snapshot's own
/// path**, because a variant has no live source elsewhere in the tree — it is
/// authored as the measurement artifact it is, and never becomes a `skills/`
/// file. `manifest_commits_contain_their_snapshots` then reads that cell as
/// provenance exactly as it does for every other arm.
///
/// **The four names are a closed set, and the arm is checked in both directions
/// against it** — every expected row present, no `voice` row that is not one of
/// them, and no `.md` in the directory without a row. Walking the expected names
/// alone is what [`SkillName`] exists to prevent for the per-skill arms: a fifth
/// `voice` row, or a fifth variant file, would otherwise sit in the tree
/// unhashed and unnoticed, and Task 21 pastes whatever it finds in this
/// directory into a probe run.
#[test]
fn voice_snapshots_match_manifest() {
    let arms = arms_dir();
    let voice = arms.join(VOICE_ARM);
    let manifest_path = arms.join("MANIFEST.md");
    let contents = fs::read_to_string(&manifest_path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", manifest_path.display()));
    let rows =
        parse_manifest(&contents).unwrap_or_else(|e| panic!("{}: {e}", manifest_path.display()));

    let names: Vec<&str> = std::iter::once(VOICE_BASELINE)
        .chain(VOICE_VARIANTS.iter().map(|v| v.name))
        .collect();

    // Direction 1: the manifest holds these rows and no other `voice` row.
    // Sorted rather than compared in document order — `MANIFEST.md`'s key is
    // `(arm, skill)`, not position, and a re-snapshotted row that moved would
    // otherwise read as a corrupt arm.
    let mut recorded: Vec<&str> = rows
        .iter()
        .filter(|r| r.arm == VOICE_ARM)
        .map(|r| r.skill.as_str())
        .collect();
    recorded.sort_unstable();
    let mut expected = names.clone();
    expected.sort_unstable();
    assert_eq!(
        recorded,
        expected,
        "{}: the arm `{VOICE_ARM}` rows are not the four registered variants",
        manifest_path.display()
    );

    // Direction 2: the directory holds these files and no other markdown. An
    // unregistered variant is the dangerous one — it is indistinguishable from
    // a registered one at the point Task 21 reads the directory.
    let mut present: Vec<String> = markdown_files(&voice)
        .iter()
        .filter_map(|p| p.file_stem().and_then(|s| s.to_str()).map(str::to_string))
        .collect();
    present.sort();
    assert_eq!(
        present,
        expected,
        "{} holds markdown that is not one of the four registered variants",
        voice.display()
    );

    // Everything that does not need git runs first, so a git-less environment
    // still reports a corrupt manifest rather than only "git is missing".
    let mut to_verify = Vec::new();
    for name in names {
        let matches: Vec<&ManifestRow> = rows
            .iter()
            .filter(|r| r.arm == VOICE_ARM && r.skill == name)
            .collect();
        assert_eq!(
            matches.len(),
            1,
            "{}: expected exactly one arm `{VOICE_ARM}` row for `{name}`, found {}",
            manifest_path.display(),
            matches.len()
        );

        let snapshot = arms.join(VOICE_ARM).join(format!("{name}.md"));
        // Spelled from the repo root, which is what a manifest row records and
        // what `git rev-parse <commit>:<path>` takes.
        let expected_source = format!("docs/skill-evidence/arms/{VOICE_ARM}/{name}.md");
        assert_eq!(
            matches[0].source_path,
            expected_source,
            "{}: arm `{VOICE_ARM}` row for `{name}` records source path `{}`, expected \
             `{expected_source}` — a voice variant is its own source",
            manifest_path.display(),
            matches[0].source_path
        );
        assert!(
            snapshot.is_file(),
            "missing arm `{VOICE_ARM}` snapshot {}",
            snapshot.display()
        );
        to_verify.push((snapshot, matches[0].hash.clone()));
    }

    // Same rule as every other arm: git's absence FAILS rather than skips,
    // because a skip prints `ok` having verified nothing.
    assert!(
        git_available(),
        "`git` is not resolvable, so the arm `{VOICE_ARM}` snapshot hashes cannot be verified"
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

/// What this repository can say about one manifest row's `<commit>:<path>`.
///
/// **`Undetermined` is a separate answer from the two absences, and that is the
/// whole point of the type.** The predecessor of this enum folded "git could not
/// be run" into "the commit is not here", so a spawn failure — an `EMFILE`
/// partway through the row loop, git missing, a broken `PATH` — was reported as
/// *"commit … is not present in this repository"*: a confident claim about
/// history, produced by a check that had not looked. That is the fail-open shape
/// this run keeps meeting, and it does not belong in the check that guards arm
/// integrity, which every measurement rests on.
///
/// The precedent is `arm_a_snapshots_match_manifest`, which hard-fails when git
/// is absent rather than skipping green. **A provenance check that cannot run
/// must refuse, not conclude.**
enum Provenance {
    /// The commit holds exactly this blob at that path.
    Blob(GitObjectId),
    /// git answered, and the commit does not carry that path.
    PathAbsent { git_says: String },
    /// git answered, and the commit itself is not here.
    CommitAbsent { git_says: String },
    /// git could not be asked, or answered something unusable. Not an absence —
    /// an absence of evidence.
    Undetermined { how: String },
}

/// Run a `git` subcommand, keeping the failure to launch it distinct from the
/// failure it reports.
///
/// `Err` is *"I could not ask"*; `Ok` is git's own answer, whatever it was.
fn git_output(args: &[String]) -> Result<std::process::Output, String> {
    Command::new("git")
        .args(args)
        .output()
        .map_err(|e| format!("cannot run `git {}`: {e}", args.join(" ")))
}

/// git's stderr, trimmed — so a failure quotes git's own words instead of
/// paraphrasing them.
fn git_stderr(out: &std::process::Output) -> String {
    String::from_utf8_lossy(&out.stderr).trim().to_string()
}

/// Resolve one manifest row against the repository.
///
/// `git rev-parse <commit>:<path>` exits 128 whether the commit is missing or
/// the path is, so a second question is asked to tell those apart — otherwise
/// "the path does not exist" sends a reader to audit a path when the real answer
/// is that the history was rewritten out from under the row. Note that
/// `cat-file -e` also exits 128 for both "no such object" and "not a commit", so
/// the two absences are distinguished by *which question git refused*, never by
/// an exit code carrying more meaning than it has.
fn resolve_provenance(commit: &str, path: &str) -> Provenance {
    let rev_parse = ["rev-parse".to_string(), format!("{commit}:{path}")];
    let out = match git_output(&rev_parse) {
        Ok(out) => out,
        Err(how) => return Provenance::Undetermined { how },
    };

    if out.status.success() {
        let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
        // A zero exit that prints something other than an object id is git
        // answering in a language this check does not speak. Reporting it as a
        // missing path would blame the manifest for a broken toolchain.
        return match GitObjectId::parse(&stdout) {
            Ok(id) => Provenance::Blob(id),
            Err(e) => Provenance::Undetermined {
                how: format!("`git {}` succeeded but printed {e}", rev_parse.join(" ")),
            },
        };
    }
    let rev_parse_says = git_stderr(&out);

    let cat_file = [
        "cat-file".to_string(),
        "-e".to_string(),
        format!("{commit}^{{commit}}"),
    ];
    match git_output(&cat_file) {
        Err(how) => Provenance::Undetermined { how },
        Ok(out) if out.status.success() => Provenance::PathAbsent {
            git_says: rev_parse_says,
        },
        Ok(out) => Provenance::CommitAbsent {
            git_says: git_stderr(&out),
        },
    }
}

/// Every row's commit **contains** the blob the row records, at the path the row
/// records.
///
/// This is what makes the commit cell provenance rather than a timestamp: a
/// reader runs `git show <commit>:<source path>` and gets the exact text that
/// arm measured. Without it the column degrades into "roughly when", and an arm
/// whose text cannot be recovered from history is an arm nobody can audit.
///
/// **It is a separate test from [`assert_arm_snapshots_match_manifest`] on
/// purpose, and the difference is the point.** That one asks *does the file on
/// disk still hash to what the manifest says* — a drift tripwire on
/// `arms/<arm>/<skill>.md`. This one asks *does the recorded history actually
/// hold that text*, which is a claim about the repository and not about the
/// snapshot. They failed independently: arm B's row matched its snapshot
/// perfectly while naming a commit where `skills/tdd/SKILL.md` was still arm A′.
///
/// It walks **rows**, not `SkillName::ALL`, so it reaches arms that are still
/// being filled in one skill at a time — arm `B` across Tasks 10–14 — which the
/// per-arm tripwires cannot check until their last skill lands. **That is
/// provenance only, not drift.** Nothing yet hashes
/// `docs/skill-evidence/arms/B/<skill>.md`, so a byte appended to an arm B
/// snapshot still passes the whole suite; closing that needs
/// `arm_b_snapshots_match_manifest`, which `MANIFEST.md` defers to the task that
/// lands arm B's fifth skill.
///
/// **Consequence worth knowing before you rebase:** every commit a row names is
/// now load-bearing. Amending or rebasing one, or dropping the branch that
/// carries it, turns this check red — and a shallow clone (`fetch-depth: 1`)
/// has none of them. That is the cost of the column meaning something.
#[test]
fn manifest_commits_contain_their_snapshots() {
    let manifest_path = arms_dir().join("MANIFEST.md");
    let contents = fs::read_to_string(&manifest_path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", manifest_path.display()));
    let rows =
        parse_manifest(&contents).unwrap_or_else(|e| panic!("{}: {e}", manifest_path.display()));
    assert!(
        !rows.is_empty(),
        "{}: no rows, so this check compared nothing",
        manifest_path.display()
    );

    // Same rule as the snapshot tripwires: git's absence FAILS rather than
    // skips, because a skip prints `ok` having verified nothing.
    assert!(
        git_available(),
        "`git` is not resolvable, so no manifest row's provenance can be verified"
    );

    // Two buckets, never merged: rows this check REFUTED, and rows it could not
    // read. Reporting the second as the first would be the check inventing
    // history it failed to consult.
    let mut wrong = Vec::new();
    let mut unreadable = Vec::new();
    for row in &rows {
        let at = format!("arm `{}` / `{}`", row.arm, row.skill);
        match resolve_provenance(row.commit.as_str(), &row.source_path) {
            Provenance::Blob(found) if found == row.hash => {}
            Provenance::Blob(found) => wrong.push(format!(
                "  {at}: commit {} holds {} at `{}`, but the row records {}",
                row.commit.as_str(),
                found.as_str(),
                row.source_path,
                row.hash.as_str(),
            )),
            Provenance::CommitAbsent { git_says } => wrong.push(format!(
                "  {at}: commit {} is not present in this repository ({git_says})",
                row.commit.as_str(),
            )),
            Provenance::PathAbsent { git_says } => wrong.push(format!(
                "  {at}: commit {} exists but has no `{}` ({git_says})",
                row.commit.as_str(),
                row.source_path,
            )),
            Provenance::Undetermined { how } => unreadable.push(format!("  {at}: {how}")),
        }
    }

    // Reported before the refutations, because it changes what they are worth:
    // if any row could not be read, this run did not verify the manifest, and
    // saying which rows failed would imply the rest passed.
    assert!(
        unreadable.is_empty(),
        "{} manifest row(s) could not be checked at all:\n{}\n\n\
         This is NOT a finding about the manifest — it is this check reporting that it \
         could not run. `git` could not be asked, or answered something unusable. \
         A provenance check that cannot run must refuse rather than conclude, so it \
         fails here instead of calling an unreadable row absent. Fix the environment \
         and re-run; do not read the result below as a verdict.",
        unreadable.len(),
        unreadable.join("\n"),
    );

    assert!(
        wrong.is_empty(),
        "{} manifest row(s) name a commit that does not carry the text they record:\n{}\n\n\
         The commit cell must be a commit CONTAINING the snapshot, not whatever `HEAD` \
         happened to be while you were editing. Snapshot and append your row in a \
         follow-up commit, after the one carrying the rewrite — recording `HEAD` while \
         the rewrite is still uncommitted names a commit where the source path still \
         holds the previous arm's text.",
        wrong.len(),
        wrong.join("\n"),
    );
}

/// Repository root — one directory above `cli/`.
///
/// Spelled once so the git invocations below all ask their questions from the
/// same place. `git -C` is used rather than the test harness's cwd because a
/// test's cwd is the package directory, not the repo, and a pathspec resolved
/// against the wrong root silently matches nothing — which for an ordering
/// check would read as "no arms to verify".
fn repo_root() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/.."))
}

/// The spec-length A/B's candidate arms
/// (`docs/skill-evidence/arms/spec-length/`).
fn spec_length_arms_dir() -> PathBuf {
    arms_dir().join("spec-length")
}

/// The spec-length A/B's freeze record
/// (`docs/skill-evidence/spec-length/FREEZE.md`).
fn spec_length_freeze_path() -> PathBuf {
    evidence_dir().join("spec-length").join("FREEZE.md")
}

/// What this repository can say about the commit that **introduced** a path.
///
/// The same three-way shape as [`Provenance`], for the same reason: "git says
/// nothing added this path" and "git could not be asked" are different facts,
/// and collapsing them lets a broken toolchain read as a clean history. The
/// question differs — [`Provenance`] asks *what does this commit hold*, this
/// asks *which commit first held it* — so it is a separate type rather than a
/// variant bolted onto that one.
enum Introduced {
    /// `git log --diff-filter=A` named the commit that added the path.
    At(GitObjectId),
    /// git answered, and no commit adds this path. The file may be on disk;
    /// history does not know about it yet.
    NotCommitted,
    /// git could not be asked, or answered something unusable. Not an absence —
    /// an absence of evidence.
    Undetermined { how: String },
}

/// The **earliest** commit that added `path`, as `git log --diff-filter=A`
/// reports it.
///
/// An untracked path is not an error to git: it prints nothing and exits 0, so
/// "no output" is the [`Introduced::NotCommitted`] answer and a non-zero exit is
/// a real failure to ask.
///
/// **Two flags carry this, and both were added because an arm slipped past
/// without them.** Each closes a way to make text authored before the freeze
/// look like it arrived after:
///
/// - **No `-1`.** `git log` prints newest-first, so `--diff-filter=A -1` returns
///   the most RECENT add, not the first — and a path can be added twice, because
///   a `git rm` plus a fresh commit of the same bytes is a second `A` event. An
///   arm could be laundered by deleting and re-committing it after the freeze.
///   The **last** line of the default (newest-first) output is the earliest add.
/// - **`--follow`.** Without it the search is anchored to the final filename, so
///   drafting an arm as `draft.md` before the freeze and `git mv`-ing it to
///   `S1.md` afterwards reports the *rename* as the introduction. `--follow`
///   walks through the rename to the original add.
///
/// **`--reverse` must not come back.** It looks like the natural way to ask for
/// the oldest commit and it is how this was first written, but combined with
/// `--follow` git prints **nothing at all** — which this function would read as
/// [`Introduced::NotCommitted`], failing every arm rather than passing them, so
/// at least it fails loudly. Take the last line instead.
///
/// **The honest limit:** `--follow` is rename *detection*, a similarity
/// heuristic. A file moved to `S<n>.md` and substantially rewritten in the same
/// commit is not recognised as a rename, and resolves to that commit. That is
/// defensible — text rewritten wholesale is new text — but it is a heuristic
/// boundary, not a proof, and this check should not be described as one.
fn introducing_commit_in(repo: &Path, path: &Path) -> Introduced {
    let shown = format!("git log --follow --diff-filter=A -- {}", path.display());
    let args = [
        "-C".to_string(),
        repo.display().to_string(),
        "log".to_string(),
        "--follow".to_string(),
        "--diff-filter=A".to_string(),
        "--format=%H".to_string(),
        "--".to_string(),
        path.display().to_string(),
    ];
    let out = match git_output(&args) {
        Ok(out) => out,
        Err(how) => return Introduced::Undetermined { how },
    };
    if !out.status.success() {
        return Introduced::Undetermined {
            how: format!("`{shown}` failed: {}", git_stderr(&out)),
        };
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    // Last non-empty line: git printed newest-first, so this is the earliest add.
    let Some(earliest) = stdout.lines().map(str::trim).rfind(|l| !l.is_empty()) else {
        return Introduced::NotCommitted;
    };
    match GitObjectId::parse(earliest) {
        Ok(id) => Introduced::At(id),
        Err(e) => Introduced::Undetermined {
            how: format!("`{shown}` printed {e}"),
        },
    }
}

/// Does `descendant` have `ancestor` in its history?
///
/// Three-way for the same reason as [`Introduced`]: `git merge-base
/// --is-ancestor` exits 0 for yes and 1 for no, and **anything else is git
/// refusing the question**, not a no.
enum Descent {
    Yes,
    No,
    Undetermined { how: String },
}

/// `git merge-base --is-ancestor <ancestor> <descendant>`.
///
/// Reflexive: a commit is its own ancestor, so an arm introduced by the very
/// commit that introduced the freeze record passes. That is the right answer —
/// such an arm did not exist before the freeze either.
fn descends_from_in(repo: &Path, ancestor: &GitObjectId, descendant: &GitObjectId) -> Descent {
    let args = [
        "-C".to_string(),
        repo.display().to_string(),
        "merge-base".to_string(),
        "--is-ancestor".to_string(),
        ancestor.as_str().to_string(),
        descendant.as_str().to_string(),
    ];
    let out = match git_output(&args) {
        Ok(out) => out,
        Err(how) => return Descent::Undetermined { how },
    };
    match out.status.code() {
        Some(0) => Descent::Yes,
        Some(1) => Descent::No,
        other => Descent::Undetermined {
            how: format!(
                "`git merge-base --is-ancestor {} {}` exited {} ({})",
                ancestor.as_str(),
                descendant.as_str(),
                other.map_or_else(|| "on a signal".to_string(), |c| c.to_string()),
                git_stderr(&out),
            ),
        },
    }
}

/// What `docs/skill-evidence/arms/spec-length/` holds.
///
/// Both fields matter to [`freeze_precedes_every_candidate_arm`], and the second
/// exists because the first is not enough. A scan that only collected the files
/// it recognised would let a **misnamed** arm — `s1.md`, `S1.markdown`,
/// `S1-draft.md`, or one tucked into a subdirectory — fall through every failure
/// bucket at once: not a violation, not uncommitted, not unreadable, simply
/// invisible. Silence is the one answer an ordering gate must never give, so
/// everything the naming rule does not recognise is collected and reported.
#[derive(Default)]
struct SpecLengthArms {
    /// `S<n>.md` with `n >= 1`, sorted by `n`. These are ordered against the
    /// freeze.
    ///
    /// `S0.md` is excluded by the `n >= 1` rule, not by name: it is the
    /// **control**, the text that was already in `brainstorm.md` before the
    /// experiment, so it necessarily predates the freeze and has nothing to be
    /// ordered against.
    candidates: Vec<(u32, PathBuf)>,
    /// Everything else in the directory. The naming convention is the one
    /// `FREEZE.md` and `MANIFEST.md` both record, and this directory holds
    /// arms and nothing else — so a stray file is a mistake to surface, not a
    /// README to tolerate.
    strays: Vec<PathBuf>,
}

/// Scan `docs/skill-evidence/arms/spec-length/`.
///
/// **A missing directory is the empty answer; any other `read_dir` failure
/// panics.** Those are different facts, and collapsing them is the fail-open
/// shape the rest of this file keeps refusing: zero arms is a legitimate state
/// (it is the state at T1), but a directory that exists and cannot be
/// enumerated means candidate arms may be sitting there unchecked while the
/// gate reports `ok`. Same rule as [`discover_corpus_roots`].
fn spec_length_arms() -> SpecLengthArms {
    let dir = spec_length_arms_dir();
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return SpecLengthArms::default(),
        Err(e) => panic!(
            "cannot read the spec-length arms directory {}: {e}. This is NOT the same as \
             holding no arms — refusing to report an ordering verdict on a directory this \
             check could not enumerate.",
            dir.display()
        ),
    };

    let mut out = SpecLengthArms::default();
    for entry in entries {
        let entry = entry.unwrap_or_else(|e| {
            panic!(
                "cannot read an entry of {}: {e}. Refusing to order only the arms that \
                 happened to be readable.",
                dir.display()
            )
        });
        let path = entry.path();
        // A subdirectory is a stray too: an arm placed one level down is exactly
        // as invisible to the gate as a misspelled one.
        let recognised =
            if path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("md") {
                path.file_stem()
                    .and_then(|s| s.to_str())
                    .and_then(|stem| stem.strip_prefix('S'))
                    .and_then(|digits| {
                        let n: u32 = digits.parse().ok()?;
                        // The name must be the CANONICAL spelling of its number.
                        // `S00.md` and `S01.md` parse fine but are not `S0.md`
                        // and `S1.md`; treating them as recognised would let a
                        // mistyped candidate arm sit in the directory being
                        // read as the control (n == 0) or as a duplicate of a
                        // real arm — invisible to the ordering loop either way,
                        // which is the blind spot the stray bucket exists for.
                        (digits == n.to_string()).then_some(n)
                    })
            } else {
                None
            };
        match recognised {
            Some(n) if n >= 1 => out.candidates.push((n, path)),
            // `S0.md` — recognised, and deliberately not ordered.
            Some(_) => {}
            None => out.strays.push(path),
        }
    }
    out.candidates.sort();
    out.strays.sort();
    out
}

/// Every candidate arm of the spec-length A/B was introduced **after** the
/// key-point ledger was frozen.
///
/// This is the ordering gate the whole experiment rests on. The ledger under
/// `docs/skill-evidence/spec-length/ledger/` is derived from the *control*
/// specs and hashed in `FREEZE.md` before any arm text exists; an arm written
/// first — or a ledger revised once an arm's weaknesses were visible — is the
/// experiment grading itself, and the result is void. Prose in the plan cannot
/// enforce that. Git history can: an arm's introducing commit must descend from
/// `FREEZE.md`'s.
///
/// **Four outcomes, not two**, extending the distinction [`Provenance`] draws.
/// Three of them fail, and they fail with *different* messages because they call
/// for different fixes:
///   - *pass* — the arm's commit descends from the freeze commit;
///   - *ordering violation* — git answered, and it does not. Both commits are
///     named, because the fix depends on which one moved;
///   - *not committed* — the arm is on disk with no introducing commit. `git
///     log` prints nothing, and feeding that to `merge-base` would be a usage
///     error dressed as a verdict. It fails, so a draft cannot sit indefinitely
///     in a state this check cannot speak to;
///   - *unreadable* — git could not be asked at all. Reported first and on its
///     own, because it is not a verdict about any arm: if one arm could not be
///     resolved, this run did not verify the ordering, and listing the others
///     would imply they were the only problems.
///
/// A fifth state is refused before the loop rather than bucketed: a **stray**
/// file in the arms directory. See [`SpecLengthArms`] — an arm the naming rule
/// does not recognise reaches none of the four outcomes above, and invisibility
/// is the one answer this gate must never give.
///
/// **What this cannot see, so nobody mistakes it for a proof.** It is a
/// reachability check, and three routes are outside what reachability can
/// answer: a **cherry-picked** pre-freeze arm commit (the cherry-pick has no
/// ancestry link to the original, so the original is not reachable from `HEAD`
/// at all — reproduced, and it passes); a rename into an arm's filename combined
/// with a wholesale rewrite in one commit (`--follow` is a similarity
/// heuristic); and simply retyping text composed earlier, which is
/// indistinguishable in principle from authoring it fresh.
///
/// Those are documented rather than patched, and deliberately so: no commit-graph
/// check closes the last one, and bolting a patch-id heuristic beside this gate
/// would buy the look of coverage without the fact of it. The load-bearing
/// guarantee is elsewhere and it is airtight — the *ledger* cannot move
/// ([`freeze_rows_still_hash_to_their_files`]), so a candidate arm is always
/// graded against a rubric it could not have influenced, whenever its text was
/// composed. `docs/skill-evidence/spec-length/FREEZE.md` states the full
/// boundary under "What the freeze proves, and what it does not".
///
/// **Zero arms on disk is a correct state and must not fail.** That is a
/// deliberate divergence from [`manifest_commits_contain_their_snapshots`],
/// which hard-fails on `rows.is_empty()` because an empty MANIFEST is never
/// right. Here the freeze lands one task before the first arm is written, so a
/// hard-fail on an empty directory would be red for a whole task's worth of
/// legitimate state — and a test that is red while nothing is wrong teaches
/// people to ignore it. Do not "fix" this into hard-failing.
///
/// **It is still never vacuous**, because the first assertion does not depend on
/// any arm existing: `FREEZE.md` must itself resolve to an introducing commit.
/// That is a real fact about a real file, checked from the freeze task onward,
/// and it is exactly the failure that would otherwise pass unnoticed — an
/// uncommitted `FREEZE.md` makes every descent check below it unanswerable and
/// the whole scheme silently inert. The anti-vacuity has to live in an
/// assertion rather than a message because an `assert!` message is never
/// printed on success.
#[test]
fn freeze_precedes_every_candidate_arm() {
    // Same rule as every other check in this file that consults history: git's
    // absence FAILS rather than skips, because a skip prints `ok` having
    // verified nothing.
    assert!(
        git_available(),
        "`git` is not resolvable, so no arm's position relative to the freeze can be verified"
    );

    let repo = repo_root();
    let arms = spec_length_arms();
    // Before any ordering verdict: a file this scan does not recognise as an arm
    // is one the ordering loop below will never see. Refused here so it cannot
    // be mistaken for "no arms to check".
    assert!(
        arms.strays.is_empty(),
        "{} holds {} file(s) that are not named `S<n>.md`:\n{}\n\n\
         Every file in this directory is an arm, and only `S<n>.md` is ordered against \
         the freeze. A misnamed or misplaced one reaches none of this check's outcomes — \
         not a violation, not uncommitted, not unreadable — so it would evade the ordering \
         gate silently rather than fail loudly. Rename it, move it out, or teach this \
         check about it deliberately.",
        spec_length_arms_dir().display(),
        arms.strays.len(),
        arms.strays
            .iter()
            .map(|p| format!("  {}", p.display()))
            .collect::<Vec<_>>()
            .join("\n"),
    );

    let freeze = spec_length_freeze_path();
    assert!(
        freeze.is_file(),
        "{} does not exist. It is the freeze record every candidate arm is ordered \
         against; without it this check has nothing to compare to.",
        freeze.display()
    );
    let freeze_commit = match introducing_commit_in(&repo, &freeze) {
        Introduced::At(commit) => commit,
        Introduced::NotCommitted => panic!(
            "{} exists on disk but no commit introduces it.\n\n\
             Every candidate arm is measured as a descendant of the freeze record's \
             introducing commit, so an uncommitted freeze makes this check — and the \
             ordering guarantee the whole spec-length A/B rests on — silently inert. \
             Commit it before writing any arm text.",
            freeze.display()
        ),
        Introduced::Undetermined { how } => panic!(
            "cannot determine when {} was introduced: {how}\n\n\
             This is NOT a finding about the freeze — it is this check reporting that it \
             could not run. A check that cannot run must refuse rather than conclude.",
            freeze.display()
        ),
    };

    let mut violations = Vec::new();
    let mut uncommitted = Vec::new();
    let mut unreadable = Vec::new();
    for (n, path) in arms.candidates {
        let at = format!("arm `S{n}` (`{}`)", path.display());
        let arm_commit = match introducing_commit_in(&repo, &path) {
            Introduced::At(commit) => commit,
            Introduced::NotCommitted => {
                uncommitted.push(format!("  {at}: not yet committed — not verifiable"));
                continue;
            }
            Introduced::Undetermined { how } => {
                unreadable.push(format!("  {at}: {how}"));
                continue;
            }
        };
        match descends_from_in(&repo, &freeze_commit, &arm_commit) {
            Descent::Yes => {}
            Descent::No => violations.push(format!(
                "  {at}: introduced by {}, which does NOT descend from the freeze commit {}",
                arm_commit.as_str(),
                freeze_commit.as_str(),
            )),
            Descent::Undetermined { how } => unreadable.push(format!("  {at}: {how}")),
        }
    }

    // Reported before the other two, because it changes what they are worth: if
    // any arm could not be read, this run did not verify the ordering, and
    // listing the rest would imply they were the only problems.
    assert!(
        unreadable.is_empty(),
        "{} candidate arm(s) could not be checked at all:\n{}\n\n\
         This is NOT a finding about the arms — it is this check reporting that it could \
         not run. Fix the environment and re-run; do not read a green elsewhere as a \
         verdict on these.",
        unreadable.len(),
        unreadable.join("\n"),
    );

    assert!(
        uncommitted.is_empty(),
        "{} candidate arm(s) are on disk with no introducing commit:\n{}\n\n\
         An arm that is not in history cannot be shown to postdate the freeze, so this \
         check can say nothing about it — which is not the same as it being fine. Commit \
         the arm (or delete the draft); do not leave it parked here.",
        uncommitted.len(),
        uncommitted.join("\n"),
    );

    assert!(
        violations.is_empty(),
        "{} candidate arm(s) predate the freeze record {}:\n{}\n\n\
         The key-point ledger is derived from the CONTROL specs and frozen before any arm \
         text exists. An arm written first — or a ledger revised once an arm's weaknesses \
         were visible — is the experiment grading itself, and every measurement taken \
         against it is void.",
        violations.len(),
        spec_length_freeze_path().display(),
        violations.join("\n"),
    );
}

/// One data row of `spec-length/FREEZE.md`'s hash table.
///
/// **`frozen_at_head` is NOT [`ManifestRow`]'s `commit`, and the name is
/// different so the two cannot be confused.** They are both a 40-hex commit id
/// in a four-or-six column evidence table, which is exactly why the distinction
/// needs to survive a careless reader: `ManifestRow::commit` is a *containment*
/// claim — [`resolve_provenance`] requires that commit to hold the recorded blob
/// at the recorded path, and `manifest_commits_contain_their_snapshots` enforces
/// it. This field records what `HEAD` was when the freeze was *taken*, which for
/// the fixtures and ledgers is a commit at which those paths did not yet exist.
/// Running manifest provenance logic over it would answer a question this column
/// never asked, and fail by design.
///
/// It is carried, not checked — parsed only so a malformed SHA is a parse error
/// rather than a cell nobody reads. Same for `date`.
#[derive(Debug)]
struct FreezeRow {
    path: String,
    hash: GitObjectId,
    #[allow(dead_code)]
    frozen_at_head: GitObjectId,
    #[allow(dead_code)]
    date: String,
}

const FREEZE_COL_PATH: &str = "path";
const FREEZE_COL_HASH: &str = "git hash-object --no-filters";
const FREEZE_COL_COMMIT: &str = "frozen at commit";
const FREEZE_COL_DATE: &str = "date";

/// `FREEZE.md`'s four columns, keyed by normalized header text — the same
/// header-anchored scheme [`parse_manifest`] uses, and for the same reason: a
/// positional parser would silently rebind `hash` to `date` if the columns were
/// ever reordered, and compare a blob SHA against a date while still passing.
const FREEZE_REQUIRED_COLUMNS: &[&str] = &[
    FREEZE_COL_PATH,
    FREEZE_COL_HASH,
    FREEZE_COL_COMMIT,
    FREEZE_COL_DATE,
];

/// Parse `spec-length/FREEZE.md`'s hash table.
///
/// **Unlike [`parse_manifest`], this stops at the first non-table line after the
/// header.** `MANIFEST.md` forbids any later line from beginning with `|`, so it
/// can treat everything after the header as data; `FREEZE.md` carries a second,
/// two-column table (who appends which rows, and when) below the prose, which a
/// read-to-end parser would try to read as short hash rows. Consuming only the
/// contiguous block keeps that table documentation instead of making it an
/// error.
fn parse_freeze(contents: &str) -> Result<Vec<FreezeRow>, String> {
    let mut header: Option<Vec<String>> = None;
    let mut rows = Vec::new();

    for line in contents.lines() {
        let Some(cells) = split_row(line) else {
            // A non-table line ends the block. Before the header it is prose; after
            // it, it is the end of the data.
            if header.is_some() {
                break;
            }
            continue;
        };
        let normalized: Vec<String> = cells.iter().map(|c| normalize_header(c)).collect();

        let Some(header) = header.as_ref() else {
            // A row is the header only if it carries the COMPLETE schema. The
            // preamble is prose about a table and grows illustrations of one, so
            // locking onto the first `| path | … |`-ish line would let an example
            // hard-fail a perfectly good freeze record.
            if !FREEZE_REQUIRED_COLUMNS
                .iter()
                .all(|want| normalized.iter().any(|got| got == want))
            {
                continue;
            }
            for (i, name) in normalized.iter().enumerate() {
                if normalized[..i].contains(name) {
                    return Err(format!("duplicate column `{name}` in the header row"));
                }
            }
            // The schema is CLOSED, exactly as `MANIFEST.md`'s is. `FreezeRow`
            // models these four and nothing else, so a fifth column would carry
            // evidence no reader ever sees — and this file's whole point is that
            // a frozen artifact's record is checkable rather than decorative.
            // Adding a column is a deliberate edit here, not something a
            // `FREEZE.md` edit can do on its own.
            for name in &normalized {
                if !FREEZE_REQUIRED_COLUMNS.contains(&name.as_str()) {
                    return Err(format!(
                        "unknown column `{name}` — the freeze schema is exactly: {}",
                        FREEZE_REQUIRED_COLUMNS.join(", ")
                    ));
                }
            }
            header = Some(normalized);
            continue;
        };

        if is_separator_row(&cells) {
            continue;
        }
        if cells.len() != header.len() {
            return Err(format!(
                "row has {} cells, header has {}: {line}",
                cells.len(),
                header.len()
            ));
        }
        let cell = |want: &str| -> String {
            let idx = header
                .iter()
                .position(|h| h == want)
                .expect("column present");
            cells[idx].clone()
        };
        let path = cell(FREEZE_COL_PATH);
        if path.is_empty() {
            return Err(format!("row has an empty `path` cell: {line}"));
        }
        rows.push(FreezeRow {
            hash: GitObjectId::parse(&cell(FREEZE_COL_HASH))
                .map_err(|e| format!("`{path}`: hash cell {e}"))?,
            frozen_at_head: GitObjectId::parse(&cell(FREEZE_COL_COMMIT))
                .map_err(|e| format!("`{path}`: `frozen at commit` cell {e}"))?,
            date: cell(FREEZE_COL_DATE),
            path,
        });
    }

    if header.is_none() {
        return Err(format!(
            "no header row carrying all of {FREEZE_REQUIRED_COLUMNS:?}"
        ));
    }
    Ok(rows)
}

/// Every frozen artifact still hashes to what `FREEZE.md` records.
///
/// **This is what makes the freeze a freeze.** Without it the whole scheme rests
/// on prose: `freeze_precedes_every_candidate_arm` checks *ordering* and says
/// nothing about content, and `manifest_commits_contain_their_snapshots` asks
/// whether a recorded commit holds a recorded blob — neither notices a fixture
/// spec or a key-point ledger being edited on disk after the fact. A ledger
/// quietly revised once a candidate arm's weaknesses were visible is the exact
/// contamination the spec-length A/B is built to prevent, and it would have
/// passed the entire suite.
///
/// **The `frozen at commit` cell is deliberately not checked here, and that is
/// not an oversight.** `FREEZE.md`'s commit column records what `HEAD` was when
/// the freeze was *taken* — for the fixtures and ledgers, a commit at which
/// those paths did not yet exist. It is not `MANIFEST.md`'s
/// contains-the-blob column, and asserting containment on it would fail by
/// design. `FREEZE.md` says so in prose; this comment says so where someone
/// would otherwise "fix" it.
///
/// An empty table fails, unlike the zero-arms case in
/// [`freeze_precedes_every_candidate_arm`]: a freeze record with no rows has
/// never been a correct state.
#[test]
fn freeze_rows_still_hash_to_their_files() {
    assert!(
        git_available(),
        "`git` is not resolvable, and these rows are `git hash-object` blob SHAs. \
         Skipping would turn this into a check that passes when it cannot run"
    );

    let path = spec_length_freeze_path();
    let contents =
        fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    let rows = parse_freeze(&contents).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    assert!(
        !rows.is_empty(),
        "{}: no rows, so this check compared nothing",
        path.display()
    );

    let repo = repo_root();
    let mut wrong = Vec::new();
    for row in &rows {
        let file = repo.join(&row.path);
        if !file.is_file() {
            wrong.push(format!(
                "  `{}`: recorded in the freeze but not on disk",
                row.path
            ));
            continue;
        }
        let found = git_hash_object(&file);
        if found != row.hash {
            wrong.push(format!(
                "  `{}`: hashes to {}, but the freeze records {}",
                row.path,
                found.as_str(),
                row.hash.as_str(),
            ));
        }
    }

    assert!(
        wrong.is_empty(),
        "{} frozen artifact(s) no longer match {}:\n{}\n\n\
         These files are the measurement basis of the spec-length A/B: the control specs, \
         the key-point ledgers derived from them, and the arms graded against those \
         ledgers. A change here is not a diff to review — it invalidates every measurement \
         taken against the old bytes. If the change was deliberate, it is a new row for a \
         new artifact, not an edit to this one.",
        wrong.len(),
        path.display(),
        wrong.join("\n"),
    );
}

/// Split a key-point ledger row, honouring `\|` as an escaped pipe.
///
/// [`split_row`] cannot be reused: `MANIFEST.md` forbids a literal `|` in any
/// cell and has no escape handling, so it splits on every one. A ledger row
/// quotes real text out of a frozen spec, and some of that text is shell
/// alternations (`startup|clear|compact`, a `grep -E` pattern) — the ledgers
/// escape those as `\|`, which markdown renders as a literal pipe inside the
/// cell. Splitting on them anyway would report the row as having extra columns.
fn split_ledger_row(line: &str) -> Option<Vec<String>> {
    let trimmed = line.trim();
    let inner = trimmed.strip_prefix('|')?;
    let inner = inner.strip_suffix('|').unwrap_or(inner);

    let mut cells = Vec::new();
    let mut cur = String::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                // `\|` is one literal pipe, not a separator.
                Some('|') => cur.push('|'),
                Some(other) => {
                    cur.push('\\');
                    cur.push(other);
                }
                None => cur.push('\\'),
            },
            '|' => cells.push(std::mem::take(&mut cur)),
            other => cur.push(other),
        }
    }
    cells.push(cur);
    Some(cells.into_iter().map(|c| c.trim().to_string()).collect())
}

/// The four values a ledger row's `kind` cell may take.
const LEDGER_KINDS: &[&str] = &["decision", "interface", "constraint", "scope"];

/// Each key-point ledger is the closed list it claims to be.
///
/// The ledgers are the rubric every candidate arm is graded against: a row
/// dropped from a candidate spec eliminates that arm. So the shape of the rubric
/// has to be checkable, not merely asserted in a preamble — a row whose `kind`
/// is a typo, an id sequence with a gap, or a "Closed list: N rows" header that
/// no longer matches the table beneath it would all corrupt the grading while
/// looking fine.
///
/// **The ledgers are discovered from `FREEZE.md`, not from a list repeated
/// here.** A second list would be a second place to forget a fixture; the freeze
/// is already the authority on which files are frozen.
#[test]
fn spec_length_ledgers_are_the_closed_lists_they_claim() {
    let freeze_path = spec_length_freeze_path();
    let contents = fs::read_to_string(&freeze_path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", freeze_path.display()));
    let rows = parse_freeze(&contents).unwrap_or_else(|e| panic!("{}: {e}", freeze_path.display()));

    let repo = repo_root();
    let ledgers: Vec<&FreezeRow> = rows
        .iter()
        .filter(|r| {
            r.path
                .starts_with("docs/skill-evidence/spec-length/ledger/")
        })
        .collect();
    assert!(
        !ledgers.is_empty(),
        "{} freezes no ledger, so this check compared nothing",
        freeze_path.display()
    );

    for row in ledgers {
        let path = repo.join(&row.path);
        let text = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
        let stem = Path::new(&row.path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_else(|| panic!("{}: no file stem", row.path));

        let declared: usize = text
            .lines()
            .find_map(|l| {
                l.split("Closed list:")
                    .nth(1)?
                    .split_whitespace()
                    .next()?
                    .parse()
                    .ok()
            })
            .unwrap_or_else(|| {
                panic!(
                    "{}: no `Closed list: N rows` declaration. The count is what makes the \
                     list closed rather than open-ended.",
                    path.display()
                )
            });

        let mut seen = 0usize;
        let mut in_table = false;
        for line in text.lines() {
            let Some(cells) = split_ledger_row(line) else {
                continue;
            };
            if !in_table {
                if cells.len() == 3 && cells[0] == "id" && cells[1] == "kind" && cells[2] == "item"
                {
                    in_table = true;
                }
                continue;
            }
            if is_separator_row(&cells) {
                continue;
            }
            assert_eq!(
                cells.len(),
                3,
                "{}: row has {} cells, expected 3 (`| id | kind | item |`). An unescaped \
                 `|` in the text is the usual cause — escape it as `\\|`.\n  {line}",
                path.display(),
                cells.len(),
            );
            seen += 1;
            assert_eq!(
                cells[0],
                format!("{stem}-{seen:02}"),
                "{}: ids must run `{stem}-01`, `{stem}-02`, … with no gaps, renumbering or \
                 duplicates — every id is cited by an elimination and must stay stable \
                 forever",
                path.display(),
            );
            assert!(
                LEDGER_KINDS.contains(&cells[1].as_str()),
                "{}: row {} has kind `{}`, which is not one of {LEDGER_KINDS:?}",
                path.display(),
                cells[0],
                cells[1],
            );
            assert!(
                !cells[2].is_empty(),
                "{}: row {} has an empty item",
                path.display(),
                cells[0],
            );
        }

        assert!(
            in_table,
            "{}: no `| id | kind | item |` header row",
            path.display()
        );
        assert_eq!(
            seen,
            declared,
            "{}: declares {declared} rows but carries {seen}. The declared count is the \
             closed-list claim; a table that has drifted from it means either the ledger \
             grew after the freeze or the count is stale, and both invalidate grading.",
            path.display(),
        );
    }
}

/// Build a throwaway repository so the git helpers above can be exercised
/// against history this file controls.
///
/// [`freeze_precedes_every_candidate_arm`] runs against the real repository,
/// where the interesting states — an arm that predates the freeze, an arm added
/// twice — do not exist and must never be made to exist. Without a synthetic
/// repository the branch that actually catches a violation would ship having
/// never fired, which is the same "green having verified nothing" this file
/// refuses everywhere else.
///
/// Identity and hooks are pinned inline: `commit` fails on a machine with no
/// `user.email` configured, and an ambient `commit.gpgsign` or a global hook
/// would otherwise leak a developer's setup into these assertions.
#[cfg(test)]
struct ScratchRepo {
    dir: tempfile::TempDir,
}

impl ScratchRepo {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = Self { dir };
        repo.git(&["init", "--quiet", "-b", "main"]);
        repo.git(&["config", "user.email", "freeze-test@example.invalid"]);
        repo.git(&["config", "user.name", "freeze test"]);
        repo.git(&["config", "commit.gpgsign", "false"]);
        repo.git(&["config", "core.hooksPath", "/dev/null"]);
        repo
    }

    fn path(&self) -> &Path {
        self.dir.path()
    }

    fn git(&self, args: &[&str]) -> String {
        let out = Command::new("git")
            .arg("-C")
            .arg(self.path())
            .args(args)
            .output()
            .unwrap_or_else(|e| panic!("cannot run `git {}`: {e}", args.join(" ")));
        assert!(
            out.status.success(),
            "`git {}` failed in the scratch repo: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// Write `name` with `body` and commit it, returning the commit id.
    fn commit_file(&self, name: &str, body: &str) -> GitObjectId {
        fs::write(self.path().join(name), body).expect("write scratch file");
        self.git(&["add", name]);
        self.git(&["commit", "--quiet", "-m", &format!("add {name}")]);
        GitObjectId::parse(&self.git(&["rev-parse", "HEAD"])).expect("scratch HEAD")
    }

    fn remove_file(&self, name: &str) -> GitObjectId {
        self.git(&["rm", "--quiet", name]);
        self.git(&["commit", "--quiet", "-m", &format!("remove {name}")]);
        GitObjectId::parse(&self.git(&["rev-parse", "HEAD"])).expect("scratch HEAD")
    }
}

/// A path added, deleted, and added again resolves to the **first** add.
///
/// This is the regression test for a hole that shipped in the first draft of
/// [`introducing_commit_in`], which passed `-1` to `git log`. `git log` prints
/// newest-first, so `-1` returned the most recent `A` event — and an arm
/// authored before the freeze could be laundered into compliance with a `git rm`
/// and a re-commit of the same bytes. The ordering gate would then have called
/// it clean.
///
/// Asserting on the *earliest* commit is what makes the re-add irrelevant, so
/// this test fails the moment `-1` comes back.
#[test]
fn introducing_commit_reports_the_first_add_not_a_later_re_add() {
    assert!(git_available(), "`git` is not resolvable");
    let repo = ScratchRepo::new();
    let first = repo.commit_file("S1.md", "an arm\n");
    repo.remove_file("S1.md");
    let re_add = repo.commit_file("S1.md", "an arm\n");
    assert_ne!(
        first.as_str(),
        re_add.as_str(),
        "the scratch repo must really have two distinct add events, or this test \
         proves nothing"
    );

    match introducing_commit_in(repo.path(), Path::new("S1.md")) {
        Introduced::At(found) => assert_eq!(
            found.as_str(),
            first.as_str(),
            "expected the FIRST add {}, got {} — a `git rm` plus a re-commit must not \
             re-date an arm past the freeze",
            first.as_str(),
            found.as_str(),
        ),
        Introduced::NotCommitted => panic!("the file is committed twice over"),
        Introduced::Undetermined { how } => panic!("could not resolve the scratch repo: {how}"),
    }
}

/// An arm drafted under another name before the freeze and renamed into place
/// afterwards resolves to the **draft**, not to the rename.
///
/// The companion to [`introducing_commit_reports_the_first_add_not_a_later_re_add`],
/// and the second laundering route review found. `git log` anchored to the final
/// filename reports the `git mv` as the introduction, which is exactly the shape
/// an honest workflow produces too — draft as `draft.md`, rename when it is
/// ready — so this is as much about accident as about gaming. `--follow` is what
/// walks back through the rename; remove it and this test goes red.
#[test]
fn introducing_commit_follows_a_rename_back_to_the_original_add() {
    assert!(git_available(), "`git` is not resolvable");
    let repo = ScratchRepo::new();
    let drafted = repo.commit_file("draft.md", "arm text written early\n");
    repo.commit_file("FREEZE.md", "the freeze\n");
    repo.git(&["mv", "draft.md", "S1.md"]);
    repo.git(&["commit", "--quiet", "-m", "rename the draft into place"]);

    match introducing_commit_in(repo.path(), Path::new("S1.md")) {
        Introduced::At(found) => assert_eq!(
            found.as_str(),
            drafted.as_str(),
            "expected the draft commit {}, got {} — renaming pre-freeze text into an arm \
             filename must not re-date it",
            drafted.as_str(),
            found.as_str(),
        ),
        Introduced::NotCommitted => panic!(
            "resolved to nothing. `--reverse` combined with `--follow` prints no output at \
             all — if it has been re-added, that is why"
        ),
        Introduced::Undetermined { how } => panic!("could not resolve the scratch repo: {how}"),
    }
}

/// A file on disk that no commit adds is [`Introduced::NotCommitted`] — an
/// absence of evidence, distinct from git failing to answer.
#[test]
fn introducing_commit_reports_an_untracked_path_as_not_committed() {
    assert!(git_available(), "`git` is not resolvable");
    let repo = ScratchRepo::new();
    repo.commit_file("S1.md", "an arm\n");
    fs::write(repo.path().join("S9.md"), "a draft\n").expect("write draft");

    assert!(
        matches!(
            introducing_commit_in(repo.path(), Path::new("S9.md")),
            Introduced::NotCommitted
        ),
        "an untracked arm must be NotCommitted, never mistaken for a resolved commit"
    );
}

/// The two verdicts [`freeze_precedes_every_candidate_arm`] actually turns on,
/// exercised over history where both really occur.
///
/// The violation branch cannot fire in the real repository — no arm predates the
/// freeze there, and none ever should — so without this the mechanism the whole
/// spec-length A/B rests on would be attested by code review alone.
#[test]
fn descends_from_separates_an_ordered_arm_from_a_pre_freeze_one() {
    assert!(git_available(), "`git` is not resolvable");
    let repo = ScratchRepo::new();

    // A side branch cut before the freeze, carrying an arm — the realistic shape
    // of a violation, since an arm authored on a stale branch and merged in is
    // reachable from HEAD without descending from the freeze.
    let root = repo.commit_file("README.md", "root\n");
    repo.git(&["switch", "--quiet", "-c", "stale", root.as_str()]);
    let pre_freeze_arm = repo.commit_file("S2.md", "authored too early\n");
    repo.git(&["switch", "--quiet", "main"]);

    let freeze = repo.commit_file("FREEZE.md", "the freeze\n");
    let post_freeze_arm = repo.commit_file("S1.md", "authored in order\n");
    repo.git(&["merge", "--quiet", "--no-ff", "-m", "merge stale", "stale"]);

    assert!(
        matches!(
            descends_from_in(repo.path(), &freeze, &post_freeze_arm),
            Descent::Yes
        ),
        "an arm committed after the freeze descends from it"
    );
    assert!(
        matches!(
            descends_from_in(repo.path(), &freeze, &pre_freeze_arm),
            Descent::No
        ),
        "an arm committed on a branch cut before the freeze does NOT descend from it, \
         even once merged — this is the violation the gate exists to catch"
    );
    // Reflexive on purpose: an arm introduced by the very commit that introduced
    // the freeze did not exist before it either.
    assert!(
        matches!(
            descends_from_in(repo.path(), &freeze, &freeze),
            Descent::Yes
        ),
        "a commit is its own ancestor"
    );

    // And the merge does not hide the stale arm: the scan resolves it to the
    // side-branch commit, not to the merge.
    match introducing_commit_in(repo.path(), Path::new("S2.md")) {
        Introduced::At(found) => assert_eq!(
            found.as_str(),
            pre_freeze_arm.as_str(),
            "a merged-in arm must resolve to where it was authored, not to the merge"
        ),
        other => panic!(
            "expected the side-branch commit, got {}",
            match other {
                Introduced::NotCommitted => "NotCommitted".to_string(),
                Introduced::Undetermined { how } => how,
                Introduced::At(_) => unreachable!(),
            }
        ),
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
///
/// **The body-size cap rides in this table too** (spec §2.4), for the same
/// reason the names do: a per-skill budget kept in a table of its own would be a
/// fifth spelling of the measured set, and collapsing the previous four is what
/// took a macro. Here a new measured skill cannot be added without a cap, and a
/// cap cannot be written for a skill that does not exist.
///
/// **These caps supersede the single `const BODY_BUDGET` main carried**, which was
/// re-baselined 2200 → 2600 → 3200 as `code-review` grew and each time came within a
/// dozen bytes of deciding the content rather than bounding it. That is the failure the
/// per-skill table removes: one number over a derived subset had to move every time any
/// one skill did. The re-baselines are subsumed, not dropped — 3200 is well under the
/// 12_000 the four discipline skills carry here, so nothing main capped is now uncapped.
macro_rules! skill_names {
    ($($variant:ident => $wire:literal @ $budget:literal,)+) => {
        /// `Deserialize` is derived from the same table as the variants, so a
        /// wire name cannot exist without a variant and a JSON-facing field can
        /// hold this type directly. The alternative — a bespoke
        /// `deserialize_with` per field — is a second place the closed set is
        /// spelled out, and it made `skill` the odd one out beside `arm` and
        /// `model`, which have always deserialized straight into their enums.
        #[derive(serde::Deserialize, Debug, PartialEq, Eq, Hash, Clone, Copy)]
        enum SkillName {
            $(#[serde(rename = $wire)] $variant,)+
        }

        impl SkillName {
            /// Every measured skill, in manifest order.
            const ALL: &'static [SkillName] = &[$(SkillName::$variant,)+];

            fn as_str(self) -> &'static str {
                match self {
                    $(SkillName::$variant => $wire,)+
                }
            }

            /// This skill's body-size cap in bytes (spec §2.4). Total: every
            /// measured skill has one.
            fn body_budget(self) -> usize {
                match self {
                    $(SkillName::$variant => $budget,)+
                }
            }
        }
    };
}

skill_names! {
    Tdd => "tdd" @ 12_000,
    SystematicDebugging => "systematic-debugging" @ 12_000,
    VerificationBeforeCompletion => "verification-before-completion" @ 12_000,
    CodeReview => "code-review" @ 12_000,
    UsingDrovr => "using-drovr" @ 9_000,
}

impl SkillName {
    fn parse(raw: &str) -> Option<Self> {
        SkillName::ALL.iter().copied().find(|s| s.as_str() == raw)
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

/// What the body-size check (spec §2.4) does with a skill.
///
/// **Two states, not one state and a silence.** A skill this repo has decided
/// not to size-check is `Unchecked` *with its reason*; a name that is not a
/// skill here at all is `None` from [`budget_for`]. The predecessor of this
/// type was a single `const BODY_BUDGET: usize` applied to a derived subset, so
/// "deliberately exempt" and "nobody thought about it" were the same
/// observation — the defect class this run exists to remove, and the one Task 8
/// had to fix in `SiteState` for §5's sites.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum BodyBudget {
    /// Capped at this many bytes of post-frontmatter body.
    Bytes(usize),
    /// Deliberately not size-checked. The reason is recorded because an
    /// unexplained exemption is indistinguishable from an oversight.
    Unchecked { why: &'static str },
}

/// The skills that are deliberately **not** size-checked, and why.
///
/// The measured five carry their caps in [`skill_names!`]; these four are
/// everything else under `skills/`. Both halves together are asserted to cover
/// the tree exactly — see [`body_budgets_classify_every_skill`] — so a tenth
/// skill reddens the suite until someone says which it is, rather than slipping
/// in unbudgeted the way `using-drovr` did.
///
/// This is not a second copy of `SKILL_SITE_STATES`: that table records whether
/// a file carries fix 3's directive, this one whether its length is bounded.
/// The two are orthogonal — `handoff` is `Covered` there and `Unchecked` here.
const UNCHECKED_SKILLS: &[(&str, &str)] = &[
    (
        "handoff",
        "process documentation for running drovr, not a discipline an agent \
         works through under pressure — no arm snapshots it and no probe scores \
         it, so §2.4 sets no cap and nothing in this run depends on its length",
    ),
    (
        "pipeline",
        "same as `handoff` — and it is already longer than the discipline cap, \
         so adopting one would be a rewrite decision, not a checkbox",
    ),
    (
        "worktrees",
        "same as `handoff` — the isolation discipline behind `drovr new \
         --worktree`, read once when setting a run up",
    ),
    (
        "writing-skills",
        "the authoring reference (plan §1.2), consulted on demand by whoever \
         writes a skill rather than injected into a working agent's context",
    ),
];

/// How `skill` is budgeted, or `None` if it is not a skill in this repo.
///
/// The three answers are distinct on purpose: `Bytes` is a cap, `Unchecked` is
/// a recorded exemption, `None` is *no such skill* — a typo or a rename, not a
/// decision anyone made.
fn budget_for(skill: &str) -> Option<BodyBudget> {
    if let Some(measured) = SkillName::parse(skill) {
        return Some(BodyBudget::Bytes(measured.body_budget()));
    }
    UNCHECKED_SKILLS
        .iter()
        .find(|(name, _)| *name == skill)
        .map(|(_, why)| BodyBudget::Unchecked { why })
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
///
/// **`cli/src/reflex.rs`'s test module carries a second copy of this, named
/// `folded`.** The duplication is structural, not careless: `drovr` is a
/// bin-only crate with no library target, so an integration test like this file
/// has no public API to import from `reflex.rs`, and `reflex.rs`'s
/// `#[cfg(test)]` module cannot reach into a test binary either — neither
/// direction is expressible today. **If this function gains normalisation
/// (apostrophe folding, case folding, punctuation stripping), change `folded`
/// too, or the two "folded" vocabularies stop being one.** This copy is the
/// canonical one, because it backs [`Quote`] and [`FoldedBody`]; whoever gives
/// `drovr` a lib target for some other reason should collapse the two.
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

/// The `n` the development scenario carries, and the two the held-out pair does.
///
/// **The split is positional, and until now only half of that was enforced.**
/// `check_scenario_corpus` counted one `dev` and two `holdout` per skill and
/// never asked which number carried which, while [`parse_held_out_bodies`] reads
/// the pair as `{skill}-2` and `{skill}-3` — a numbering, not a tag lookup. So a
/// file tagged `holdout` at `n: 1` (with `dev` at `n: 2`) satisfied the count
/// while moving the held-out pair onto files nobody intended: the corpus check
/// passes, the provenance guard resolves the wrong pair, and a measurement runs,
/// scores and reports against different scenarios than the report names, with
/// nothing red anywhere. One fact, in one place, so the two cannot disagree.
const DEV_SCENARIO_N: u32 = 1;
const HELD_OUT_NS: [u32; 2] = [2, 3];

/// Which tag `n` must carry in the numbered class.
fn tag_for_n(n: u32) -> Tag {
    if n == DEV_SCENARIO_N {
        Tag::Dev
    } else {
        Tag::Holdout
    }
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

        // The counts being right does not make the *positions* right, and the
        // held-out pair is resolved by number elsewhere (`parse_held_out_bodies`),
        // never by tag. A corpus with `holdout` at n=1 and `dev` at n=2 satisfies
        // the counts above and silently redirects every held-out run.
        let mut actual: Vec<(u32, Tag)> = numbered.iter().map(|s| (s.n, s.tag)).collect();
        actual.sort_by_key(|(n, _)| *n);
        let expected: Vec<(u32, Tag)> = std::iter::once(DEV_SCENARIO_N)
            .chain(HELD_OUT_NS)
            .map(|n| (n, tag_for_n(n)))
            .collect();
        if actual != expected {
            let skill = skill.as_str();
            return Err(format!(
                "`{skill}` numbers its scenarios {actual:?}, and plan §1.2 fixes them at \
                 {expected:?}. The split is positional: `n: {DEV_SCENARIO_N}` is the development \
                 scenario and `n:` {HELD_OUT_NS:?} are the held-out pair, because that pair is \
                 resolved by number and not by tag. A tag that disagrees with its number moves \
                 the measurement onto files nobody chose, and nothing else would fail"
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

    // The counts are right here and the positions are not: one `dev`, two
    // `holdout`, with the `dev` at n=2. `parse_held_out_bodies` would resolve
    // the pair as `-2` and `-3` and read the development scenario as half of it.
    let err = check_scenario_corpus(&full(["holdout", "dev", "holdout"]))
        .expect_err("a dev scenario at n=2 must be rejected even though the counts add up");
    assert!(err.contains("The split is positional"), "got: {err}");
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

// ---------------------------------------------------------------------------
// A scored held-out stage names the scenario body it ran on
// ---------------------------------------------------------------------------

/// Whether a skill's evidence file carries a scored held-out stage yet.
///
/// **Two states, not a state and a silence** — the shape [`BodyBudget`] and
/// `SiteState` already use in this file, and for the same reason: a skill that
/// is simply absent from a list is indistinguishable from one nobody thought
/// about. Absence is what the `&[&str]` subsets this run spent two tasks
/// collapsing into [`skill_names!`] were made of.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum HeldOutScores {
    /// A stage has scored this skill's held-out pair, so its evidence file owes
    /// one row per scenario naming the blob those probes read.
    Recorded,
    /// No scored held-out stage yet — and the reason, because an unexplained
    /// exemption is indistinguishable from an oversight. Checked in the other
    /// direction too: a file in this state may carry no rows at all.
    NotYetRun { why: &'static str },
}

impl SkillName {
    /// Total over the measured five: the `match` is exhaustive, so a sixth skill
    /// cannot be added without saying which of the two states it is in. That is
    /// the property a hand-maintained allowlist beside `ALL` does not have.
    fn held_out_scores(self) -> HeldOutScores {
        match self {
            SkillName::Tdd
            | SkillName::SystematicDebugging
            | SkillName::VerificationBeforeCompletion => HeldOutScores::Recorded,
            SkillName::CodeReview => HeldOutScores::NotYetRun {
                why: "`ab-code-review` (plan §7.3, Tasks 16–21) has not run; the evidence \
                      file's `## Scored results` reads \"Not yet run\"",
            },
            SkillName::UsingDrovr => HeldOutScores::NotYetRun {
                why: "`ab-using-drovr` has not run, for either the primary held-out pair or \
                      the no-skill-applies veto class",
            },
        }
    }
}

/// Whether a skill's evidence file carries the `discrimination-test` stage's
/// unaided counts, and how many runs it owes.
///
/// A second state machine beside [`HeldOutScores`] rather than a reuse of it,
/// because the two answer different questions about the same pair: `held_out_scores`
/// says whether an **arm** was measured, this says whether the pair was measured
/// with **no skill at all**. `code-review` and `using-drovr` are `NotYetRun` in the
/// first and `Recorded` in the second, so one field cannot carry both.
///
/// `runs` is carried rather than left implicit: the defect this run keeps finding is
/// an artifact set that grew without its guard growing, and "the file exists" is
/// satisfied by a file holding one verdict out of four.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum DiscriminationScores {
    /// The `discrimination-test` stage probed this skill's held-out pair unaided.
    /// `runs` is what `run-ledger.md` charged, and the verdict file owes exactly
    /// that many verdicts — no more, and no fewer.
    Recorded { runs: usize },
    /// Not probed unaided yet, and why. Checked in both directions, like
    /// [`HeldOutScores`]: such a skill may carry no discrimination rows at all.
    #[allow(dead_code)]
    NotYetRun { why: &'static str },
}

impl SkillName {
    /// Total over the measured five, for [`SkillName::held_out_scores`]'s reason: a
    /// sixth skill cannot compile without declaring which state it is in.
    ///
    /// All five are `Recorded` because the `discrimination-test` phase probed the
    /// whole corpus at once — it was measuring the instrument, not any one skill.
    /// **The veto class (`using-drovr-noskill-{1,2}`) is not covered by this**: it
    /// is not part of §1.2's held-out pair, and it has still never been measured.
    fn discrimination_scores(self) -> DiscriminationScores {
        match self {
            SkillName::Tdd
            | SkillName::SystematicDebugging
            | SkillName::VerificationBeforeCompletion
            | SkillName::CodeReview
            | SkillName::UsingDrovr => DiscriminationScores::Recorded { runs: 4 },
        }
    }

    /// The `run-ledger.md` stage cell that charges this skill's discrimination
    /// probes, derived from the skill rather than written twice.
    fn discrimination_ledger_stage(self) -> String {
        format!("Discrimination probe (`{}`)", self.as_str())
    }
}

/// Whether a skill's evidence file carries a `remeasure-<skill>` stage: the three
/// arms re-run against the bodies `harden-scenarios` wrote.
///
/// A third state machine beside [`HeldOutScores`] and [`DiscriminationScores`], for
/// the reason the second was added beside the first — it answers a question neither
/// can. `held_out_scores` says an **arm** was measured but not on *which body*, and
/// `tdd` is `Recorded` there against a pair that no longer exists.
/// `discrimination_scores` says the pair was probed with **no** skill. Only this one
/// says the pre-registered bars were re-applied on the current bodies, which is what
/// a §9 reader deciding which counts to quote actually needs.
///
/// `runs` is carried for [`DiscriminationScores`]'s reason: a file holding three
/// verdicts of twelve satisfies "the file exists" and turns 4-of-4 into a fraction
/// with no denominator.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum RemeasureScores {
    /// The `remeasure-<skill>` stage re-ran the arms on the current bodies. `runs`
    /// is what `run-ledger.md` charged across its rows, and the verdict file owes
    /// exactly that many verdicts.
    Recorded { runs: usize },
    /// Not re-measured, and why — checked in both directions like the other two, so
    /// a skill in this state may carry no remeasure rows and no verdict file.
    NotYetRun { why: &'static str },
}

impl SkillName {
    /// Total over the measured five, so a sixth skill cannot compile without
    /// declaring which state it is in.
    ///
    /// `tdd` and `systematic-debugging` are `Recorded`: `discrimination-test` found
    /// their rewritten pairs the two strongest of the five (0 of 4 unaided each), and
    /// the human authorised one stage per pair to re-apply the bars on an instrument
    /// that demonstrably discriminates. The other three are untouched — the two
    /// marginal pairs and the saturated one are not a re-measurement anyone has
    /// decided to spend runs on.
    fn remeasure_scores(self) -> RemeasureScores {
        match self {
            SkillName::Tdd => RemeasureScores::Recorded { runs: 12 },
            SkillName::SystematicDebugging => RemeasureScores::Recorded { runs: 12 },
            SkillName::VerificationBeforeCompletion => RemeasureScores::NotYetRun {
                why: "its rewritten pair is marginal (2 of 4 unaided) and no re-measurement \
                      was authorised",
            },
            SkillName::CodeReview => RemeasureScores::NotYetRun {
                why: "`ab-code-review` has not run at all, and its pair came back saturated \
                      (3 of 4 unaided) — there is no earlier verdict to supersede",
            },
            SkillName::UsingDrovr => RemeasureScores::NotYetRun {
                why: "`ab-using-drovr` has not run at all — there is no earlier verdict to \
                      supersede",
            },
        }
    }

    /// The `run-ledger.md` stage cell that charges this skill's re-measurement,
    /// derived from the skill rather than written twice.
    fn remeasure_ledger_stage(self) -> String {
        format!("held-out RE-MEASURED (`{}`)", self.as_str())
    }
}

/// Which stage's probes a provenance row describes.
///
/// Two stages now measure the same held-out pair for different purposes, and each
/// owes its own row: the `ab-*` stages read an arm, the `discrimination-test` stage
/// read no skill at all. They ran on **different bodies** — `harden-scenarios`
/// replaced all ten between them — so one row per scenario cannot serve both, and
/// [`held_out_body_rows`]'s whole-file scan would otherwise read four rows as a
/// malformed pair.
///
/// The marker is the discriminator, and it is a closed set for the reason every
/// other closed set in this file is one: a third stage must add a variant here
/// rather than invent a fourth sentence shape nothing parses.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum ProvenanceStage {
    /// The `ab-<skill>` held-out bar runs: an arm's text was pasted above the body.
    BarHeldOut,
    /// The `discrimination-test` unaided probes: no skill text at all.
    Discrimination,
    /// The `remeasure-<skill>` stage: the same three arms as [`Self::BarHeldOut`],
    /// re-run against the bodies `harden-scenarios` wrote. It is a third stage and
    /// therefore a third variant, which is what the doc comment above asked for —
    /// reusing `BarHeldOut`'s marker would put four rows where §1.2 defines two and
    /// fail the pair check on a file that is exactly right.
    Remeasure,
}

impl ProvenanceStage {
    /// The row marker, which is also the English of what the row claims.
    fn marker(self) -> &'static str {
        match self {
            ProvenanceStage::BarHeldOut => " measured at blob `",
            ProvenanceStage::Discrimination => " unaided-probed at blob `",
            ProvenanceStage::Remeasure => " re-measured at blob `",
        }
    }
}

/// Whether the body a recorded measurement ran on is still the body on disk.
///
/// A closed two-value domain, so it is an enum rather than a `String` checked
/// against a list and then carried on as though it had not been — the same
/// correction `SkillName` and `Tag` already are.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum ScenarioBodyStatus {
    Current,
    Superseded,
}

impl ScenarioBodyStatus {
    fn as_str(self) -> &'static str {
        match self {
            ScenarioBodyStatus::Current => "CURRENT",
            ScenarioBodyStatus::Superseded => "SUPERSEDED",
        }
    }

    fn parse(raw: &str) -> Option<Self> {
        [
            ScenarioBodyStatus::Current,
            ScenarioBodyStatus::Superseded,
        ]
        .into_iter()
        .find(|status| status.as_str() == raw)
    }

    /// The verdict a row **claims** is never the verdict this check uses: it is
    /// recomputed from the two blob ids, so a row that stops being true fails
    /// rather than continuing to assert itself.
    fn recompute(recorded: &GitObjectId, on_disk: &GitObjectId) -> Self {
        if recorded.as_str() == on_disk.as_str() {
            ScenarioBodyStatus::Current
        } else {
            ScenarioBodyStatus::Superseded
        }
    }
}

/// One held-out scenario's recorded body: the blob a scored stage's probes read.
///
/// Named for the held-out lifecycle rather than for provenance in general —
/// [`Provenance`] already answers a different question in this file (which
/// manifest commit an arm snapshot resolves against).
struct HeldOutBody {
    recorded: GitObjectId,
    claimed: ScenarioBodyStatus,
}

/// A skill's held-out **pair**, which is what plan §1.2 defines: `-2` and `-3`,
/// in that order, always both.
///
/// Encoded as two fields rather than returned as a `Vec` the caller re-checks,
/// so "one row is missing" is a parse error at the one place that knows the
/// shape instead of an assertion every consumer has to remember to repeat.
struct HeldOutBodies {
    skill: SkillName,
    second: HeldOutBody,
    third: HeldOutBody,
}

impl HeldOutBodies {
    /// The pair as `(scenario stem, row)`, in plan §1.2 order. The stem is
    /// derived here, once, from the skill the pair was parsed for.
    fn pair(&self) -> [(String, &HeldOutBody); 2] {
        let [second, third] = HELD_OUT_NS;
        [
            (format!("{}-{second}", self.skill.as_str()), &self.second),
            (format!("{}-{third}", self.skill.as_str()), &self.third),
        ]
    }
}

/// Parse a skill's `- `<stem>.md` <marker> `<sha>` — <STATUS>` rows, for one stage.
///
/// Lines that do not carry `stage`'s marker are skipped rather than rejected: every
/// evidence file is full of ordinary backticked bullets, and a parser that tried to
/// claim them would make prose an error. **That is also what keeps the two stages
/// apart** — a bar row and a discrimination row for the same scenario are both
/// legal, in the same file, saying different things about different bodies, and
/// each parse sees only its own. Once a line *does* carry the marker, every
/// remaining part of it is required — a half-formed row is a row nobody checks,
/// which is the shape of defect this whole record exists to stop.
///
/// Returns the rows found, in file order, paired with the stem each names.
/// Binding them to the §1.2 pair is [`parse_held_out_bodies`]'s job.
fn held_out_body_rows(
    contents: &str,
    stage: ProvenanceStage,
) -> Result<Vec<(String, HeldOutBody)>, String> {
    let mut out = Vec::new();
    for line in contents.lines() {
        let Some(rest) = line.trim().strip_prefix("- `") else {
            continue;
        };
        let Some((stem_md, rest)) = rest.split_once('`') else {
            continue;
        };
        let Some(rest) = rest.strip_prefix(stage.marker()) else {
            continue;
        };
        let (raw_hash, tail) = rest
            .split_once('`')
            .ok_or_else(|| format!("blob id is never closed: {line}"))?;
        let stem = stem_md
            .strip_suffix(".md")
            .ok_or_else(|| format!("`{stem_md}` is not a `<stem>.md`: {line}"))?;
        let raw_status = tail.trim().trim_start_matches('—').trim();
        let claimed = ScenarioBodyStatus::parse(raw_status).ok_or_else(|| {
            format!(
                "status `{raw_status}` must be `{}` or `{}`: {line}",
                ScenarioBodyStatus::Current.as_str(),
                ScenarioBodyStatus::Superseded.as_str()
            )
        })?;
        out.push((
            stem.to_string(),
            HeldOutBody {
                recorded: GitObjectId::parse(raw_hash)?,
                claimed,
            },
        ));
    }
    Ok(out)
}

/// The held-out pair `skill`'s evidence file records for `stage`, or why it is not
/// a pair.
fn parse_held_out_bodies(
    contents: &str,
    skill: SkillName,
    stage: ProvenanceStage,
) -> Result<HeldOutBodies, String> {
    let rows = held_out_body_rows(contents, stage)?;
    let found: Vec<String> = rows.iter().map(|(stem, _)| stem.clone()).collect();
    // The same `HELD_OUT_NS` the corpus check binds tags to, so "which files are
    // the held-out pair" is one fact rather than two that can drift apart.
    let expected: Vec<String> = HELD_OUT_NS
        .iter()
        .map(|n| format!("{}-{n}", skill.as_str()))
        .collect();
    if found != expected {
        return Err(format!(
            "held-out rows are {found:?}, and plan §1.2's pair is {expected:?} in that order. \
             A scored stage whose scenario body is not named is a count that cannot be \
             compared against any later one"
        ));
    }
    let mut rows = rows.into_iter();
    let (_, second) = rows.next().expect("length checked above");
    let (_, third) = rows.next().expect("length checked above");
    Ok(HeldOutBodies {
        skill,
        second,
        third,
    })
}

#[test]
fn held_out_bodies_parse_as_a_pair_with_a_closed_status() {
    let good = "prose about `tdd-2.md` that is not a row\n\
                - `tdd-2.md` measured at blob `b8b4b71709bfc022c58b73b1d256d88938db5993` — SUPERSEDED\n\
                - some other bullet\n\
                - `tdd-3.md` measured at blob `7bc482a72cdf9747b57473e0360de98c3d4b567c` — CURRENT\n";
    let pair = parse_held_out_bodies(good, SkillName::Tdd, ProvenanceStage::BarHeldOut)
        .expect("the pair parses, prose is skipped");
    assert_eq!(pair.second.claimed, ScenarioBodyStatus::Superseded);
    assert_eq!(pair.third.claimed, ScenarioBodyStatus::Current);
    let stems: Vec<String> = pair.pair().into_iter().map(|(stem, _)| stem).collect();
    assert_eq!(stems, ["tdd-2", "tdd-3"], "the stem is derived from the skill");

    for (why, mutated) in [
        (
            "the status is a closed set; free text would make the check unfalsifiable",
            good.replace("— SUPERSEDED", "— probably fine"),
        ),
        (
            "an abbreviated blob id cannot be compared against `git hash-object` output",
            good.replace("b8b4b71709bfc022c58b73b1d256d88938db5993", "b8b4b71"),
        ),
        (
            "half a pair is a count with nothing attached to it",
            good.lines()
                .filter(|l| !l.contains("tdd-3.md` measured"))
                .collect::<Vec<_>>()
                .join("\n"),
        ),
        (
            "the pair is ordered, and a swapped pair attributes each count to the other scenario",
            good.replace("tdd-2.md", "tmp").replace("tdd-3.md", "tdd-2.md").replace("tmp", "tdd-3.md"),
        ),
    ] {
        assert!(
            parse_held_out_bodies(&mutated, SkillName::Tdd, ProvenanceStage::BarHeldOut).is_err(),
            "should have been rejected: {why}"
        );
    }

    // A pair belongs to the skill it was parsed for, not to whichever stems the
    // file happens to carry.
    assert!(
        parse_held_out_bodies(good, SkillName::CodeReview, ProvenanceStage::BarHeldOut).is_err(),
        "`tdd`'s rows are not `code-review`'s pair"
    );

    // The two stages measured different bodies of the same pair, and both sets of
    // rows live in one file. Each parse must see only its own marker: if the scan
    // were marker-blind it would read four rows where §1.2 defines two, and the
    // pair check would reject a file that is exactly right.
    let both = format!(
        "{good}\
         - `tdd-2.md` unaided-probed at blob `41d3a0dbe5e0c6f6ee11d2e0d4e0f7a2c8b91a55` — CURRENT\n\
         - `tdd-3.md` unaided-probed at blob `9e1a7c0dd2f4b3a6c5e8079b1d4f6a2c3e5b7d90` — CURRENT\n"
    );
    let bar = parse_held_out_bodies(&both, SkillName::Tdd, ProvenanceStage::BarHeldOut)
        .expect("the bar pair still parses beside the discrimination rows");
    assert_eq!(bar.second.claimed, ScenarioBodyStatus::Superseded);
    let disc = parse_held_out_bodies(&both, SkillName::Tdd, ProvenanceStage::Discrimination)
        .expect("the discrimination pair parses beside the bar rows");
    assert_eq!(disc.second.claimed, ScenarioBodyStatus::Current);
    assert_ne!(
        bar.second.recorded.as_str(),
        disc.second.recorded.as_str(),
        "the two stages ran on different bodies — that is the whole reason for two rows"
    );

    // And neither stage may satisfy its own check with the other's rows.
    assert!(
        parse_held_out_bodies(good, SkillName::Tdd, ProvenanceStage::Discrimination).is_err(),
        "bar rows are not evidence that an unaided stage ran"
    );
}

/// A recorded held-out count must name the scenario text it was measured on, and
/// say whether that text is still the text on disk.
///
/// The `harden-scenarios` phase rewrote all ten held-out scenario bodies after
/// three stages had already scored against them. Nothing in this repo tied a
/// recorded count to the body it came from, so those numbers and any measured
/// after the rewrite would pool into §9 as if they were one instrument. This
/// makes the question computable: the row records the blob the probes read, and
/// the status is **recomputed here** rather than trusted, so a later phase that
/// re-measures on the current bodies — or one that reverts them — is told its
/// note is now wrong instead of discovering it in the write-up.
///
/// Walks `SkillName::ALL`, not a subset: [`SkillName::held_out_scores`] says
/// which of the two states each skill is in, and **both are checked**. A skill
/// declared `NotYetRun` that carries rows anyway is as much a disagreement as a
/// `Recorded` one that carries none.
#[test]
fn held_out_measurements_name_the_scenario_body_they_ran_on() {
    assert!(
        git_available(),
        "`git` is not resolvable, and these rows are `git hash-object` blob SHAs. \
         Skipping would turn this into a check that passes when it cannot run"
    );
    let evidence = evidence_dir();
    let scenarios = scenarios_dir();

    for skill in SkillName::ALL.iter().copied() {
        let path = evidence.join(format!("{}.md", skill.as_str()));
        let contents = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));

        let pair = match skill.held_out_scores() {
            HeldOutScores::NotYetRun { why } => {
                let rows = held_out_body_rows(&contents, ProvenanceStage::BarHeldOut)
                    .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
                assert!(
                    rows.is_empty(),
                    "{} carries {} held-out provenance row(s), but `{}` is declared as having \
                     no scored held-out stage ({why}). Whichever is now wrong, the scored \
                     results and `held_out_scores` have to move together",
                    path.display(),
                    rows.len(),
                    skill.as_str()
                );
                continue;
            }
            HeldOutScores::Recorded => {
                parse_held_out_bodies(&contents, skill, ProvenanceStage::BarHeldOut)
                    .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
            }
        };

        for (stem, row) in pair.pair() {
            let file = scenarios.join(format!("{stem}.md"));
            let on_disk = git_hash_object(&file);
            let computed = ScenarioBodyStatus::recompute(&row.recorded, &on_disk);
            assert_eq!(
                row.claimed,
                computed,
                "{} says `{stem}.md` is {} at blob {}, but {} now hashes to {}. The status is \
                 recomputed, not read: fix the row, and do not pool counts across a change of \
                 body",
                path.display(),
                row.claimed.as_str(),
                row.recorded.as_str(),
                file.display(),
                on_disk.as_str()
            );
        }
    }
}

/// The `discrimination-test` stage's artifacts exist, are complete, and name the
/// bodies they were probed on — for **every** skill it claims to have measured.
///
/// This stage is the reason the guard is here at all. `harden-scenarios`'s handoff
/// records nine vacuous-pass defects in this run, every one from an artifact set
/// that grew without its guard growing; the stage that measured whether the
/// rewritten corpus discriminates added a verdict file, a blind map and four
/// transcripts per skill, and none of that was reachable from any assertion. The
/// three failures it forecloses, in the order they would actually happen:
///
/// 1. **A skill quietly drops out.** `scores_json_verdicts_obey_the_rubric` walks
///    whichever files are on disk, so deleting `using-drovr`'s verdicts removes its
///    result rather than failing. [`DiscriminationScores`] is the declaration that
///    makes an absence loud.
/// 2. **A partial file passes as a whole one.** Four cells is what "≤1 of 4" is a
///    fraction of. One verdict in the file satisfies every existing check and turns
///    a per-skill count into a number with no denominator.
/// 3. **The counts outlive the bodies.** This is `harden-scenarios`'s own defect,
///    one generation on: these numbers are only comparable to a later stage's if
///    the pair still hashes to what the probes read.
/// 4. **The ledger and the artifacts drift apart.** `run-ledger.md` is what Tasks
///    19–21 read *before* spawning probes, to decide whether §7.3's ceiling leaves
///    room. `run_ledger_cumulative_is_a_running_total` checks that table's internal
///    arithmetic and nothing outside it, so a hand-edited charge could under- or
///    over-report this stage's spend with every artifact guard still green. The
///    charge, the declaration and the verdicts are one fact and are checked as one.
#[test]
fn discrimination_stage_records_every_skill_it_measured() {
    assert!(
        git_available(),
        "`git` is not resolvable, and these rows are `git hash-object` blob SHAs. \
         Skipping would turn this into a check that passes when it cannot run"
    );
    let evidence = evidence_dir();
    let scenarios = scenarios_dir();
    let transcripts = evidence.join("transcripts");
    let mut measured = 0usize;

    let ledger_path = evidence.join(EVIDENCE_LEDGER);
    let ledger_text = fs::read_to_string(&ledger_path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", ledger_path.display()));
    let ledger =
        parse_ledger(&ledger_text).unwrap_or_else(|e| panic!("{}: {e}", ledger_path.display()));

    for skill in SkillName::ALL.iter().copied() {
        let path = evidence.join(format!("{}.md", skill.as_str()));
        let contents = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
        let dir = transcripts.join(skill.as_str());
        let scores = dir.join(VerdictBundle::Discrimination.scores_file());

        // (4): what the ledger charged for this skill, resolved by the stage cell.
        let stage_cell = skill.discrimination_ledger_stage();
        let charged: Vec<&LedgerRow> = ledger
            .iter()
            .filter(|row| row.stage.contains(&stage_cell))
            .collect();

        let runs = match skill.discrimination_scores() {
            DiscriminationScores::NotYetRun { why } => {
                assert!(
                    charged.is_empty(),
                    "{} charges {} row(s) for {stage_cell}, but `{}` is declared as never \
                     probed unaided ({why}). The ledger is what a later phase reads before \
                     spending, so a charge with no measurement behind it is worse than none",
                    ledger_path.display(),
                    charged.len(),
                    skill.as_str(),
                );
                let rows = held_out_body_rows(&contents, ProvenanceStage::Discrimination)
                    .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
                let verdicts_exist = scores.is_file();
                assert!(
                    rows.is_empty() && !verdicts_exist,
                    "`{}` is declared as never probed unaided ({why}), but {} carries {} \
                     discrimination row(s) and {} exists={verdicts_exist}. Whichever is now \
                     wrong, the artifacts and `discrimination_scores` have to move together",
                    skill.as_str(),
                    path.display(),
                    rows.len(),
                    scores.display(),
                );
                continue;
            }
            DiscriminationScores::Recorded { runs } => runs,
        };

        // A retried run counts, so the charge is per row and there is exactly one
        // row per skill: two rows would mean a stage that ran twice under one name.
        assert_eq!(
            charged.len(),
            1,
            "{} carries {} row(s) matching {stage_cell}; this stage charged `{}` on exactly \
             one row",
            ledger_path.display(),
            charged.len(),
            skill.as_str(),
        );
        assert_eq!(
            charged[0].runs as usize,
            runs,
            "{} charges {} run(s) for {stage_cell} against {runs} declared. The ceiling \
             decision Tasks 19–21 make is read from that column, so the charge and the \
             measurement have to be the same number",
            ledger_path.display(),
            charged[0].runs,
        );

        // (1) and (2): the file exists and holds every cell, not merely some.
        assert!(
            scores.is_file(),
            "`{}` is declared as probed unaided over {runs} runs, but {} does not exist",
            skill.as_str(),
            scores.display(),
        );
        let verdicts = read_verdicts(&scores);
        assert_eq!(
            verdicts.len(),
            runs,
            "{} holds {} verdict(s) against {runs} declared runs (and {} charged in {}). A \
             per-skill count is a fraction of its denominator, and a partial file silently \
             changes it",
            scores.display(),
            verdicts.len(),
            charged[0].runs,
            ledger_path.display(),
        );
        let wrong = check_blind_map(&dir, VerdictBundle::Discrimination, &verdicts);
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));

        // (3): the pair is named, and the status is recomputed rather than read.
        let pair = parse_held_out_bodies(&contents, skill, ProvenanceStage::Discrimination)
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        for (stem, row) in pair.pair() {
            let file = scenarios.join(format!("{stem}.md"));
            let on_disk = git_hash_object(&file);
            let computed = ScenarioBodyStatus::recompute(&row.recorded, &on_disk);
            assert_eq!(
                row.claimed,
                computed,
                "{} says the unaided probes read `{stem}.md` at blob {} and calls that {}, \
                 but {} now hashes to {}. The status is recomputed, not read: fix the row, \
                 and do not pool counts across a change of body",
                path.display(),
                row.recorded.as_str(),
                row.claimed.as_str(),
                file.display(),
                on_disk.as_str(),
            );
        }
        measured += 1;
    }

    // Seeded against what is true: the `discrimination-test` phase probed all five.
    // Without it the loop passes on a tree where every skill is `NotYetRun`, which
    // is the vacuous shape the three failures above are each a version of.
    assert_eq!(
        measured, 5,
        "{measured} skill(s) carry discrimination results — the stage probed all five \
         held-out pairs unaided, so anything less means a result went missing rather \
         than a measurement never happening",
    );
}

/// The `remeasure-<skill>` stage's artifacts exist, are complete, and name the
/// bodies they were re-measured on — for every skill it claims to have re-measured.
///
/// The same guard `discrimination_stage_records_every_skill_it_measured` is, one
/// stage on, and it exists for the same reason: this run's recurring defect is an
/// artifact set that grows without its guard growing, and this stage added a verdict
/// file, a blind map, a re-adjudication and twelve transcripts. Three failures it
/// forecloses, past the ones the sibling guard already names:
///
/// 1. **A superseded verdict is read as current.** `tdd.md` now carries two scored
///    arm stages measured on two different pairs. Without the `Remeasure` provenance
///    rows a reader — or §9 — cannot tell which counts came from which instrument,
///    and pooling them is the exact error `harden-scenarios` was cleaning up after.
/// 2. **The re-measurement replaces the record instead of superseding it.**
///    [`VerdictBundle::requires_a_scored_stage`] refuses a `remeasure-scores.json`
///    with no `scores.json` beside it, so deleting the superseded verdicts to tidy
///    up fails the build rather than erasing what was superseded.
/// 3. **The charge and the measurement drift.** `run_ledger_cumulative_is_a_running_total`
///    checks that table's arithmetic and nothing outside it. These runs are charged
///    across the three §7.3 arm rows, so the check is on the **total** over the rows
///    matching this stage's cell, not on one row: a re-measurement spends on every
///    arm it re-runs.
#[test]
fn remeasure_stage_records_the_bodies_it_ran_on() {
    assert!(
        git_available(),
        "`git` is not resolvable, and these rows are `git hash-object` blob SHAs. \
         Skipping would turn this into a check that passes when it cannot run"
    );
    let evidence = evidence_dir();
    let scenarios = scenarios_dir();
    let transcripts = evidence.join("transcripts");
    let mut measured = 0usize;

    let ledger_path = evidence.join(EVIDENCE_LEDGER);
    let ledger_text = fs::read_to_string(&ledger_path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", ledger_path.display()));
    let ledger =
        parse_ledger(&ledger_text).unwrap_or_else(|e| panic!("{}: {e}", ledger_path.display()));

    for skill in SkillName::ALL.iter().copied() {
        let path = evidence.join(format!("{}.md", skill.as_str()));
        let contents = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
        let dir = transcripts.join(skill.as_str());
        let scores = dir.join(VerdictBundle::Remeasure.scores_file());

        let stage_cell = skill.remeasure_ledger_stage();
        let charged: Vec<&LedgerRow> = ledger
            .iter()
            .filter(|row| row.stage.contains(&stage_cell))
            .collect();

        let runs = match skill.remeasure_scores() {
            RemeasureScores::NotYetRun { why } => {
                assert!(
                    charged.is_empty(),
                    "{} charges {} row(s) for {stage_cell}, but `{}` is declared as never \
                     re-measured ({why}). A charge with no measurement behind it misreads \
                     the ceiling for whoever spends next",
                    ledger_path.display(),
                    charged.len(),
                    skill.as_str(),
                );
                let rows = held_out_body_rows(&contents, ProvenanceStage::Remeasure)
                    .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
                let verdicts_exist = scores.is_file();
                let adj = dir.join(
                    VerdictBundle::Remeasure
                        .adjudication()
                        .file()
                        .expect("the remeasure bundle carries a re-adjudication"),
                );
                let adj_exists = adj.is_file();
                assert!(
                    rows.is_empty() && !verdicts_exist && !adj_exists,
                    "`{}` is declared as never re-measured ({why}), but {} carries {} \
                     remeasure row(s), {} exists={verdicts_exist} and {} \
                     exists={adj_exists}. Whichever is now wrong, the artifacts and \
                     `remeasure_scores` have to move together",
                    skill.as_str(),
                    path.display(),
                    rows.len(),
                    scores.display(),
                    adj.display(),
                );
                continue;
            }
            RemeasureScores::Recorded { runs } => runs,
        };

        // Summed across the rows, not asserted to be one: a re-measurement of three
        // arms charges three §7.3 rows, and a per-row check would either reject the
        // correct ledger or silently accept a stage that only paid for one arm.
        let charged_runs: u32 = charged.iter().map(|row| row.runs).sum();
        assert!(
            !charged.is_empty(),
            "{} charges nothing for {stage_cell}, but `{}` is declared as re-measured over \
             {runs} runs",
            ledger_path.display(),
            skill.as_str(),
        );
        assert_eq!(
            charged_runs as usize, runs,
            "{} charges {charged_runs} run(s) across {} row(s) for {stage_cell} against \
             {runs} declared. The ceiling decision the next phase makes is read from that \
             column, so the charge and the measurement have to be the same number",
            ledger_path.display(),
            charged.len(),
        );

        assert!(
            scores.is_file(),
            "`{}` is declared as re-measured over {runs} runs, but {} does not exist",
            skill.as_str(),
            scores.display(),
        );
        let verdicts = read_verdicts(&scores);
        assert_eq!(
            verdicts.len(),
            runs,
            "{} holds {} verdict(s) against {runs} declared runs. A bar reads a count over \
             a denominator, and a partial file silently changes it",
            scores.display(),
            verdicts.len(),
        );
        let wrong = check_blind_map(&dir, VerdictBundle::Remeasure, &verdicts);
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));

        // The blind re-read is REQUIRED here, not validated-if-present.
        // `scores_json_verdicts_obey_the_rubric` checks a re-adjudication only when the
        // file exists — correct for `VerdictBundle::Bar`, where Task 16 wrote one
        // because a verdict was challenged and a later stage might legitimately not.
        // It is wrong for this stage, whose whole claim is that a second independent
        // reading agreed on all twelve: under that rule, deleting the file would
        // delete the claim's evidence and leave the suite green, with `run-ledger.md`
        // still asserting the check runs. That is this run's recurring defect, and it
        // was found in this guard by review rather than in the tree by luck.
        let adj = dir.join(
            VerdictBundle::Remeasure
                .adjudication()
                .file()
                .expect("the remeasure bundle carries a re-adjudication"),
        );
        assert!(
            adj.is_file(),
            "`{}` is declared as re-measured over {runs} runs, but {} does not exist. A \
             re-measurement supersedes an existing verdict, so the blind re-read that \
             confirms it is part of the measurement, not an optional extra",
            skill.as_str(),
            adj.display(),
        );
        let adj_text = fs::read_to_string(&adj)
            .unwrap_or_else(|e| panic!("{} unreadable: {e}", adj.display()));
        let records: Vec<Adjudication> = serde_json::from_str(&adj_text)
            .unwrap_or_else(|e| panic!("{} does not match the adjudication schema: {e}", adj.display()));
        assert_eq!(
            records.len(),
            runs,
            "{} re-reads {} transcript(s) against {runs} declared runs. A partial \
             re-adjudication cannot support \"all {runs} agreed\"",
            adj.display(),
            records.len(),
        );

        // The pair is named, and the status is recomputed rather than read. A
        // re-measurement whose rows say SUPERSEDED measured the bodies it was
        // created to stop measuring.
        let pair = parse_held_out_bodies(&contents, skill, ProvenanceStage::Remeasure)
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        for (stem, row) in pair.pair() {
            let file = scenarios.join(format!("{stem}.md"));
            let on_disk = git_hash_object(&file);
            let computed = ScenarioBodyStatus::recompute(&row.recorded, &on_disk);
            assert_eq!(
                row.claimed,
                computed,
                "{} says the re-measurement read `{stem}.md` at blob {} and calls that {}, \
                 but {} now hashes to {}. The status is recomputed, not read: fix the row, \
                 and do not pool counts across a change of body",
                path.display(),
                row.recorded.as_str(),
                row.claimed.as_str(),
                file.display(),
                on_disk.as_str(),
            );
            assert_eq!(
                computed,
                ScenarioBodyStatus::Current,
                "{} re-measured `{stem}.md` at a blob that is not the body on disk. The \
                 point of this stage is a verdict on the CURRENT instrument; a superseded \
                 row here means the verdict cannot claim to be one",
                path.display(),
            );
        }
        measured += 1;
    }

    // Guards the loop against going vacuous on a tree where every skill is
    // `NotYetRun`. **Derived from `remeasure_scores()`, not a literal**: the
    // expected count and the per-skill states are the same fact, and a magic
    // number here has to be hand-bumped every time a skill is re-measured — which
    // is a stale-literal bug waiting for the third re-measurement, not a check.
    let declared = SkillName::ALL
        .iter()
        .filter(|s| matches!(s.remeasure_scores(), RemeasureScores::Recorded { .. }))
        .count();
    assert!(
        declared > 0,
        "no skill is declared `RemeasureScores::Recorded`, so this test reads five \
         evidence files and asserts nothing about any of them. If the last \
         re-measurement was retracted, delete this check with it rather than leaving \
         it green and empty"
    );
    assert_eq!(
        measured, declared,
        "{measured} skill(s) carry re-measurement results on disk, but {declared} are \
         declared `RemeasureScores::Recorded` — a result went missing rather than a \
         measurement never happening",
    );
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

/// The budget is per skill, and every answer means exactly one thing.
///
/// `Bytes` is a cap; `Unchecked` is a skill this repo has decided not to
/// size-check, carrying its reason; `None` is *not a skill in this repo at all*.
/// Collapsing the middle two into a bare `None` is what the run has been paying
/// for elsewhere — a deliberate exemption and a forgotten entry would again be
/// the same observation.
#[test]
fn budget_for_returns_per_skill_caps() {
    assert_eq!(budget_for("tdd"), Some(BodyBudget::Bytes(12_000)));
    assert_eq!(
        budget_for("systematic-debugging"),
        Some(BodyBudget::Bytes(12_000))
    );
    assert_eq!(
        budget_for("verification-before-completion"),
        Some(BodyBudget::Bytes(12_000))
    );
    assert_eq!(budget_for("code-review"), Some(BodyBudget::Bytes(12_000)));
    // The router is capped lower than the disciplines it routes to: it is
    // injected in full at every SessionStart, so its bytes cost more than any
    // other skill's (spec §2.4).
    assert_eq!(budget_for("using-drovr"), Some(BodyBudget::Bytes(9_000)));

    // Declared unchecked — a recorded exemption, not an omission. That the
    // reason is non-empty is `body_budgets_classify_every_skill`'s rule, held
    // over the whole table rather than these four.
    for skill in ["handoff", "pipeline", "worktrees", "writing-skills"] {
        assert!(
            matches!(budget_for(skill), Some(BodyBudget::Unchecked { .. })),
            "{skill} should be declared unchecked, got {:?}",
            budget_for(skill)
        );
    }

    // Not a skill. Distinguishable from "unchecked", which is the whole point.
    assert_eq!(budget_for("no-such-skill"), None);
}

/// Every skill under `skills/` is classified, and every classification names a
/// skill — the same both-directions rule `SKILL_SITE_STATES` is held to.
///
/// Without the first direction a new skill is unbudgeted and silent, which is
/// how `using-drovr` — the most expensive document in the repo, injected in
/// full at every `SessionStart` — went uncapped until spec §2.4. Without the
/// second, a renamed skill leaves a budget asserting things about a file that
/// is not there.
#[test]
fn body_budgets_classify_every_skill() {
    let present: HashSet<String> = skill_files(&skills_dir())
        .into_iter()
        .map(|(name, _)| name)
        .collect();

    let mut unclassified: Vec<&String> = present
        .iter()
        .filter(|name| budget_for(name).is_none())
        .collect();
    unclassified.sort();
    assert!(
        unclassified.is_empty(),
        "skill(s) with no body budget: {unclassified:?}\n\
         Every skill must either carry a cap in `skill_names!` or be listed in \
         UNCHECKED_SKILLS with the reason it is exempt. Leaving one out makes \
         `deliberately exempt` and `nobody noticed` the same thing.",
    );

    let declared = SkillName::ALL
        .iter()
        .map(|skill| skill.as_str())
        .chain(UNCHECKED_SKILLS.iter().map(|(name, _)| *name));
    let mut phantom: Vec<&str> = declared
        .clone()
        .filter(|name| !present.contains(*name))
        .collect();
    phantom.sort();
    assert!(
        phantom.is_empty(),
        "budget entries naming no skill: {phantom:?}\n\
         If a skill was renamed, rename its entry; if it was deleted, delete it.",
    );

    // A name in both tables would give `budget_for` two answers and hand the
    // first one silently to every caller.
    let mut seen = HashSet::new();
    for name in declared {
        assert!(
            seen.insert(name),
            "`{name}` is budgeted twice — a skill has one budget or none"
        );
    }

    // An exemption with no reason is an omission wearing a table entry. Held
    // over the whole table, not spot-checked on today's four.
    for (name, why) in UNCHECKED_SKILLS {
        assert!(
            !why.trim().is_empty(),
            "`{name}` is exempt from the size check with no reason recorded — \
             say why it is exempt, or give it a cap"
        );
    }
}

#[test]
fn checked_skills_within_body_budget() {
    let mut checked = 0;

    for (name, path) in skill_files(&skills_dir()) {
        let Some(BodyBudget::Bytes(budget)) = budget_for(&name) else {
            // `Unchecked` needs nothing done to it, and `None` is
            // `body_budgets_classify_every_skill`'s failure to report.
            continue;
        };
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
            body_len <= budget,
            "{}: body is {body_len} bytes, exceeds budget of {budget}",
            path.display()
        );
        checked += 1;
    }

    // The corpus is walked from disk, so an empty walk would pass this test
    // having measured nothing.
    assert_eq!(
        checked,
        SkillName::ALL.len(),
        "expected every measured skill ({}) to be size-checked, measured {checked}",
        SkillName::accepted()
    );
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
/// **The rule this imposes is a substring rule, and nothing more.** A demotion
/// passes if its text does not *contain* one of the strings below, at any
/// casing — that is the whole predicate. It cannot tell an *additional
/// consequence* from a *precondition*, which is §3's actual distinction, so it
/// does not try to: judging that is review's job, and
/// [`no_phase_scoped_description_literals`] says so too.
///
/// Two consequences that are easy to get wrong, both pinned by
/// [`documented_demotion_forms_behave_as_documented`] rather than asserted here:
///
///   * *"**Inside** a drovr phase…"* and *"**During** a drovr phase…"* pass —
///     neither contains `in a drovr phase`.
///   * *"**Within** a drovr phase…"* is **REJECTED**, because `with·in a drovr
///     phase` contains the literal outright. It is a perfectly good demotion in
///     English and this check still refuses it. Use *"Inside"*.
///
/// spec §9.1 check 3's grep is case-sensitive, so folding case is **stricter
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

/// The phrasings [`PHASE_SCOPED_LITERALS`]'s doc names as passing and failing
/// actually pass and fail.
///
/// It exists because that doc comment got this wrong once. It claimed *"Within a
/// drovr phase…"* was an allowed demotion form while the matcher rejected it —
/// `with·in a drovr phase` contains the literal — so the next author to demote a
/// phase reference in these five skills would have been sent at a phrasing that
/// reddens the suite. A prose rule that nothing checks drifts from the code the
/// moment the code changes, which is exactly how that claim survived the edit
/// that invalidated it.
#[test]
fn documented_demotion_forms_behave_as_documented() {
    // (text, is it expected to trip the matcher?)
    let cases = [
        ("Inside a drovr phase this also binds the next phase.", false),
        ("During a drovr phase this also binds the next phase.", false),
        ("If you are in a phase, this also gates `drovr phase done`.", false),
        // Rejected: `with-in a drovr phase` contains the literal.
        ("Within a drovr phase this also binds the next phase.", true),
        // Rejected: the case-insensitivity fix (`9f2cbb8`) exists for this one.
        ("In a drovr phase this also binds the next phase.", true),
        ("Use when about to claim A Drovr Task is done.", true),
    ];

    for (text, should_trip) in cases {
        let hits = phase_scoped_literals_in(text);
        assert_eq!(
            !hits.is_empty(),
            should_trip,
            "{text:?}: expected trip={should_trip}, matcher returned {hits:?}. \
             Update PHASE_SCOPED_LITERALS' doc comment and this case together — \
             they are one rule written twice, and the doc is what the next author \
             editing a skill will read."
        );
    }
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

/// Spec §5's task-binding directive — **the canonical text, spelled once**.
///
/// §5's block quote is the canonical directive: sites quote it, they do not
/// rewrite it. That is a property about the *whole text*, so it is enforced
/// against the whole text, from this one const. Tasks 10–14 add the remaining
/// §5 sites and compare against this same string — if they quoted a variant,
/// the five skills would disagree about their own rule, which this run has
/// already shipped once.
///
/// **Wrapping and indentation are not part of the contract; wording is.** The
/// sites embed the quote differently — indented three spaces inside a numbered
/// step, two inside an authoring-rules bullet, flush inside a section — and each
/// re-wraps it to its own column. Both sides of every comparison are therefore
/// built with [`Quote::new`], which folds whitespace exactly as the scenario
/// checks do: *wrapping is formatting; rewording is drift.*
///
/// Reproduce this exactly if you ever need to restore it. Do not improve it.
/// Two things in it that reviewers have already tried to narrow, both of which
/// must survive: it names `TodoWrite` **or** `TaskCreate`/`TaskUpdate` because
/// harnesses differ (the brainstorm-round claim that `TaskCreate` is
/// unavailable was **REFUTED** — see `plan-HANDOFF.md`), and it carries the
/// file-based fallback for a harness exposing neither.
const TASK_BINDING_DIRECTIVE: &str = "
When a skill or briefing gives you a numbered checklist, create **one tracked item per step**
using whatever task tool this harness exposes — `TodoWrite`, or `TaskCreate`/`TaskUpdate` —
before you start step 1. Mark each in-progress when you start it and complete when its
evidence is in hand. If the harness exposes no task tool, write the checklist to
`~/.local/share/drovr/runs/<run>/checklist.md` when inside a run, or `CHECKLIST.md` at the
repo root otherwise, and tick items there. An untracked checklist decays with the context
window; that decay is the exact failure drovr exists to fight.
";

/// Quoted text in the one shape it is ever compared in: whitespace folded.
///
/// A newtype rather than a `String` so the canonical directive and the text
/// scraped out of a file cannot be produced two different ways — both go
/// through [`Quote::new`], and a raw `&str` will not compare against one by
/// accident. The type is the "one source, one shape" rule made unskippable.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Quote(String);

impl Quote {
    fn new(text: &str) -> Self {
        Quote(normalize_ws(text))
    }

    fn as_str(&self) -> &str {
        &self.0
    }
}

/// [`TASK_BINDING_DIRECTIVE`] in comparable form.
fn canonical_directive() -> Quote {
    Quote::new(TASK_BINDING_DIRECTIVE)
}

/// Every contiguous block quote in `contents`.
///
/// A block ends at the first line that is not a `>` line — so a file may carry
/// several, and they are kept apart rather than concatenated. That separation is
/// the whole point: `skills/handoff/SKILL.md` has a pre-existing block quote (the
/// "fresh reader, never self-summary" design note) as well as the directive, and
/// a scraper that joined every `>` line in the file reported that file as
/// divergent when it was correct. The bug was in the checker, not the file.
fn block_quotes(contents: &str) -> Vec<Quote> {
    block_quotes_with_lines(contents)
        .into_iter()
        .map(|(_, quote)| quote)
        .collect()
}

/// [`block_quotes`], keeping each quote's first line.
///
/// §6 section 6 constrains where fix 3's directive sits, not only that it is
/// somewhere in the file, and the position is what [`block_quotes`] discards.
/// The two are one parser rather than two so a quirk of quote-splitting cannot
/// be fixed in the check that reads positions and left in the one that does not.
fn block_quotes_with_lines(contents: &str) -> Vec<(usize, Quote)> {
    let mut out = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    let mut start = 0usize;
    for (idx, line) in contents.lines().enumerate() {
        match line.trim_start().strip_prefix('>') {
            Some(rest) => {
                if current.is_empty() {
                    start = idx;
                }
                current.push(rest);
            }
            None => {
                if !current.is_empty() {
                    out.push((start, Quote::new(&current.join(" "))));
                    current.clear();
                }
            }
        }
    }
    if !current.is_empty() {
        out.push((start, Quote::new(&current.join(" "))));
    }
    out
}

/// The directory whose every `.md` file is a §5 site, **derived rather than
/// listed**.
///
/// Every phase-prompt hands an agent a numbered `## Do` list, so membership is a
/// property of the directory, not a curated opinion about it. Deriving means a
/// phase-prompt added later is required to carry the directive the day it lands,
/// instead of the day someone remembers to extend a const.
const PHASE_PROMPTS_DIR: &str = "pipeline/phase-prompts";

/// §5 site 4's other half — the only §5 site that is neither a phase-prompt nor
/// a `SKILL.md`, so it is the one path that must be named outright.
const HANDOFF_TEMPLATE: &str = "handoff/HANDOFF-template.md";

/// What a `skills/<name>/SKILL.md` is, with respect to fix 3.
///
/// **Every skill has one of these, and the test proves the enumeration is
/// exhaustive in both directions** — a skill in the tree with no entry fails, and
/// an entry naming no skill fails. That is what makes "absent" mean something:
/// before this, a §5 site deferred on purpose and a §5 site forgotten by accident
/// were the same observation (nothing), which is the defect class this run
/// exists to remove, sitting in the check written against it.
enum SiteState {
    /// Carries the directive today. Asserted present, exactly once.
    Covered,
    /// A §5 site whose directive lands in a **named** later task. Asserted
    /// **absent** until then — so the task that adds the text and forgets to
    /// flip this entry fails, and so does the reverse.
    Deferred {
        task: &'static str,
        why: &'static str,
    },
    /// Not a §5 site. Asserted absent. The reason is recorded because this is
    /// the one variant that is a judgement rather than a reading of the spec,
    /// and an unaudited judgement is how a site goes missing quietly.
    NotASite { why: &'static str },
}

impl SiteState {
    /// Does this site have to carry the directive right now?
    fn must_carry(&self) -> bool {
        matches!(self, SiteState::Covered)
    }

    /// Phrase for the failure text, so a diagnostic explains the expectation
    /// rather than only stating it.
    fn describe(&self) -> String {
        match self {
            SiteState::Covered => "recorded as Covered".to_string(),
            SiteState::Deferred { task, why } => {
                format!("recorded as Deferred to {task} ({why})")
            }
            SiteState::NotASite { why } => format!("recorded as NotASite ({why})"),
        }
    }
}

/// Every `skills/*/SKILL.md`, classified. **Exhaustive by assertion, not by
/// hope** — see [`SiteState`].
///
/// **Tasks 10–14: flipping your entry to `Covered` and adding the directive to
/// the file are ONE edit.** Either alone is a failing suite: `Deferred` asserts
/// the text is absent, `Covered` asserts it is present. That is deliberate — it
/// is the only arrangement in which "nobody remembered" cannot look like
/// "not yet scheduled".
///
/// The deferral is not scheduling preference. Arm A′ is frozen as **fix-1-only**
/// (spec §7.3), so fix-3 text in any of these five files would make the shipped
/// tree stop matching what the frozen arm claims to be, and the A/A′/B
/// comparison would quietly stop meaning what it says. §6 section 6 puts the
/// directive inside each fix-4 rewrite anyway. **So this table also enforces
/// A′'s integrity on the live tree**, which nothing else does.
const SKILL_SITE_STATES: &[(&str, SiteState)] = &[
    ("handoff", SiteState::Covered),
    ("pipeline", SiteState::Covered),
    ("worktrees", SiteState::Covered),
    ("tdd", SiteState::Covered),
    ("systematic-debugging", SiteState::Covered),
    ("verification-before-completion", SiteState::Covered),
    ("code-review", SiteState::Covered),
    ("using-drovr", SiteState::Covered),
    (
        "writing-skills",
        SiteState::NotASite {
            why: "§5 enumerates its four sites and this is not one of them. \
                  Recorded rather than omitted because the file does carry \
                  numbered lists, so this is a judgement about §5's scope and \
                  not a reading of it — task8-report.md refers it to the final \
                  review",
        },
    ),
];

/// Fix 3 (spec §5): the sites recorded `Covered` quote
/// [`TASK_BINDING_DIRECTIVE`] — the whole directive, verbatim up to wrapping and
/// indentation — exactly once, and the sites recorded `Deferred` or `NotASite`
/// do not quote it at all.
///
/// **The two halves are what make this check total.** Presence alone cannot tell
/// a site that is deferred on purpose from one that was forgotten: both are
/// silence. Pairing every site with a [`SiteState`], and asserting the
/// enumeration covers `skills/` exactly, makes "missing entirely"
/// unrepresentable — a new skill fails until someone classifies it, a Deferred
/// site that gains the text fails until someone reclassifies it, and a Covered
/// site that loses the text fails outright.
///
/// **The corpus is two-thirds derived.** The phase-prompts come from a directory
/// read (asserted non-empty); the skills come from the same walk the rest of this
/// file uses. Only [`HANDOFF_TEMPLATE`] is named outright, because it is the one
/// §5 site that is neither.
///
/// **What this checks is the text, not four keywords.** An earlier version
/// asserted four substrings (`one tracked item per step`, `TodoWrite`,
/// `TaskCreate`, `CHECKLIST.md`) and called itself a check that sites quote the
/// canonical directive. It was not: a site could reword the directive wholesale
/// and pass on the strength of four surviving keywords — the exact drift fix 3
/// exists to prevent, in the check written to prevent it.
/// [`task_binding_check_rejects_a_reworded_directive`] pins the difference with a
/// rewording that carries all four of those old fragments and is still refused.
///
/// It was watched RED at every stage: all nine sites failing before the text
/// existed, the comparison re-watched RED under a one-word rewording, and the
/// `Deferred` half watched RED with the directive pasted into a discipline skill.
#[test]
fn task_binding_directive_present() {
    let canon = canonical_directive();
    assert!(
        !canon.as_str().is_empty(),
        "TASK_BINDING_DIRECTIVE is empty; every comparison below would be vacuous"
    );

    // The enumeration is exhaustive, in both directions. Neither half is
    // optional: unclassified skills are the silence this check exists to break,
    // and phantom entries are how a table keeps asserting things about a file
    // that no longer exists.
    let present: HashSet<String> = skill_files(&skills_dir())
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    let classified: HashSet<String> = SKILL_SITE_STATES
        .iter()
        .map(|(name, _)| (*name).to_string())
        .collect();

    let mut unclassified: Vec<&String> = present.difference(&classified).collect();
    unclassified.sort();
    assert!(
        unclassified.is_empty(),
        "skill(s) with no SKILL_SITE_STATES entry: {unclassified:?}\n\
         Every skill must say whether it carries fix 3's directive, defers it to \
         a named task, or is not a §5 site. Leaving one out is exactly the \
         silence this check exists to break — say which it is.",
    );

    let mut phantom: Vec<&String> = classified.difference(&present).collect();
    phantom.sort();
    assert!(
        phantom.is_empty(),
        "SKILL_SITE_STATES entries naming no skill: {phantom:?}\n\
         The table is asserting things about files that are not there. If a \
         skill was renamed, rename its entry; if it was deleted, delete its \
         entry.",
    );

    // (path, must it carry the directive, how to describe the expectation)
    let mut corpus: Vec<(PathBuf, bool, String)> = Vec::new();

    // The same reader `ask_channel_directive_present` uses — one corpus, so a
    // change to the extension filter or the sort cannot reach one check and miss
    // the other.
    for path in phase_prompt_files() {
        corpus.push((
            path,
            true,
            "a phase-prompt, and every phase-prompt hands an agent a numbered \
             `## Do` list (§5 site 3)"
                .to_string(),
        ));
    }

    corpus.push((
        skills_dir().join(HANDOFF_TEMPLATE),
        true,
        "§5 site 4 — the 7-section handoff is a checklist".to_string(),
    ));

    let skill_file = format!("{PER_SKILL_FILE_STEM}.md");
    for (name, state) in SKILL_SITE_STATES {
        corpus.push((
            skills_dir().join(name).join(&skill_file),
            state.must_carry(),
            state.describe(),
        ));
    }

    let mut wrong = Vec::new();
    for (path, must_carry, expectation) in &corpus {
        assert!(
            path.is_file(),
            "spec §5 site {} is not a file — a check cannot say anything about a \
             site that is not there. If the file moved, move its entry with it; \
             do not drop it.",
            path.display()
        );
        let contents = fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));

        let quotes = block_quotes(&contents);
        let found = quotes.iter().filter(|quote| **quote == canon).count();
        match (must_carry, found) {
            (true, 1) | (false, 0) => continue,
            (true, 0) => {
                // A near-miss is the likely failure — someone re-worded — so
                // show the drifting block rather than only the count.
                let near = quotes
                    .iter()
                    .find(|quote| quote.as_str().contains("tracked item per step"));
                wrong.push(match near {
                    Some(drifted) => format!(
                        "{} ({expectation}): quotes a DIFFERENT directive:\n    {}",
                        path.display(),
                        drifted.as_str()
                    ),
                    None => format!(
                        "{} ({expectation}): does not quote the directive",
                        path.display()
                    ),
                });
            }
            (true, n) => wrong.push(format!(
                "{} ({expectation}): quotes the directive {n} times, expected once",
                path.display()
            )),
            (false, n) => wrong.push(format!(
                "{} ({expectation}): quotes the directive {n} time(s), expected none. \
                 If this is the task that lands it, flip the entry to \
                 SiteState::Covered in the SAME commit as the text.",
                path.display()
            )),
        }
    }

    assert!(
        wrong.is_empty(),
        "{} site(s) disagree with their recorded state:\n{}\n\n\
         The canonical text is TASK_BINDING_DIRECTIVE, and it is the whole \
         contract — quote it, do not reword it, do not narrow it to one task \
         tool, and do not drop the file-based fallback. Re-wrapping and \
         indenting it are fine; `Quote::new` folds both. An untracked checklist \
         decays with the context window, which is the failure fix 3 exists to \
         fight.",
        wrong.len(),
        wrong.join("\n"),
    );
}

/// [`block_quotes`] extracts what the sites actually contain, and
/// [`task_binding_directive_present`]'s comparison refuses a rewording.
///
/// It exists because the check it guards used to accept one. Both cases are
/// built **from** [`TASK_BINDING_DIRECTIVE`] rather than from a pasted copy, so
/// they cannot drift away from the const they are about.
#[test]
fn task_binding_check_rejects_a_reworded_directive() {
    let canon = canonical_directive();

    // The shape the five phase-prompts use: indented inside a numbered step,
    // with prose either side.
    let indented: String = TASK_BINDING_DIRECTIVE
        .trim()
        .lines()
        .map(|line| format!("   > {line}\n"))
        .collect();
    let page = format!("0. **Bind this checklist.**\n\n{indented}\n1. Next step.\n");
    assert_eq!(
        block_quotes(&page),
        vec![canon.clone()],
        "the extractor did not recover the directive from the shape the \
         phase-prompts use to carry it"
    );

    // Two quotes in one file stay two, as in `skills/handoff/SKILL.md`.
    let two = format!("{page}\n> An unrelated design note.\n");
    assert_eq!(
        block_quotes(&two),
        vec![canon.clone(), Quote::new("An unrelated design note.")],
        "adjacent block quotes must stay separate; joining them is what made an \
         earlier hand-written checker report a correct file as divergent"
    );

    // The refutation: a rewording carrying every fragment the superseded check
    // enforced. It must NOT be accepted as the directive.
    let reworded = "When a skill gives you a numbered checklist, make one tracked item per step \
                    with `TodoWrite` or `TaskCreate`, and otherwise use `CHECKLIST.md`.";
    for fragment in [
        "one tracked item per step",
        "TodoWrite",
        "TaskCreate",
        "CHECKLIST.md",
    ] {
        assert!(
            reworded.contains(fragment),
            "this case only refutes the superseded fragment check if it carries \
             every fragment that check enforced; it is missing `{fragment}`"
        );
    }
    assert_ne!(
        Quote::new(reworded),
        canon,
        "a reworded directive carrying all four legacy fragments compared equal \
         to the canonical text — the comparison is not checking the wording"
    );
}

/// The ask-channel directive (`docs/interactive-brainstorm.md` decision 7),
/// carried verbatim up to wrapping and indentation by every phase-prompt that
/// briefs a **writer**.
///
/// **It is a RAW string literal, and that is not a style choice.** The text
/// carries a wrapped shell command whose lines end in a trailing `\`. In a
/// non-raw Rust literal, a `\` immediately before a newline is the
/// line-continuation escape: it would delete the backslash, the newline, **and
/// all leading whitespace on the next line**, silently merging the three command
/// lines into one and erasing every backslash from the constant — with no
/// compile error. `cli/src/main.rs`'s `ASK_USAGE` uses exactly that idiom on
/// purpose, one screen away from this work, so the wrong precedent is the one
/// sitting closest to hand. The damage would surface here as
/// [`ask_channel_directive_present`] reporting *"quotes a DIFFERENT directive"*,
/// which reads like a wording bug and is not.
///
/// Two interface facts it must keep right, both of which drifted from the plan
/// during T3 and bind for everything downstream:
///
/// - `--context <text>` and `--context-file <path>` are **separate** flags.
/// - `ask wait` with nothing outstanding prints the **folded interview**, not a
///   bare `[]` — which is what makes the re-arm race survivable.
const ASK_CHANNEL_DIRECTIVE: &str = r#"
**Ask the human when you need to, mid-phase — do not guess and write the guess down.** Two
triggers, either one is enough: **new information is discovered** that the spec or plan did not
anticipate, or **a question is found** that you cannot resolve from the code or the run's
artifacts. Post it and carry on with whatever does not depend on the answer:

    drovr ask <run> --question "<what you need decided>" \
      [--context <text> | --context-file <path>] \
      [--option <value>=<label>]... [--recommend <value>]

`ask` returns immediately, printing the ask id and the page to point the human at. Then
background `drovr ask wait <run> [--timeout-ms <ms>]` and end your turn: `0` answered, `2`
timeout — re-arm, the question is still on disk and still on screen — `5` the run was cancelled,
`1` error. On `0` stdout carries the answers as JSON: the asks that wait was armed on, each with
its latest answer, or — when nothing was outstanding — the whole folded interview, which is how
a wait re-armed just after the human answered still hands you the answer. A timeout costs
nothing; a guess costs the run.
"#;

/// The phase-prompts that must **not** carry [`ASK_CHANNEL_DIRECTIVE`], each with
/// the reason.
///
/// An exclusion is a decision, so it is recorded rather than left as an absence:
/// otherwise a prompt excluded on purpose and a prompt forgotten by accident are
/// the same observation. Asserted against the directory in **both** directions —
/// an entry naming no file fails, so an exclusion cannot outlive a rename.
const ASK_DIRECTIVE_EXCLUSIONS: &[(&str, &str)] = &[(
    "review-angle.md",
    "briefs read-only panel reviewers that report through a findings file and never write \
     — they have no run-scoped write channel, and `drovr ask` is a write",
)];

/// [`ASK_CHANNEL_DIRECTIVE`] in comparable form.
fn canonical_ask_directive() -> Quote {
    Quote::new(ASK_CHANNEL_DIRECTIVE)
}

/// Every `.md` under `skills/pipeline/phase-prompts`, sorted, asserted non-empty.
///
/// One reader, called by [`task_binding_directive_present`] and
/// [`ask_channel_directive_present`] alike, so the two cannot end up asserting
/// over different corpora — a change to the extension filter or the sort that
/// reached only one of two inlined copies would silently stop applying to the
/// other. (The rewording companions do not call it: they read no files at all,
/// only the const and synthetic strings.)
///
/// **Derived**, for the reason [`task_binding_directive_present`] spells out: a
/// phase-prompt added later must be required to carry the directive the day it
/// lands, not the day someone remembers to extend a const.
///
/// Known gap, shared with the check this pattern comes from: the filter is
/// `.md` only, so a `newphase.markdown` would not be seen. Left as is rather
/// than widened, because the repo's convention is uniformly `.md` and inventing
/// a second accepted extension here would make this file the only place that
/// says otherwise.
fn phase_prompt_files() -> Vec<PathBuf> {
    let dir = skills_dir().join(PHASE_PROMPTS_DIR);
    let entries = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("cannot read phase-prompts dir {}: {e}", dir.display()));
    let mut prompts: Vec<PathBuf> = entries
        .map(|entry| entry.expect("read_dir entry").path())
        .filter(|path| path.is_file() && path.extension().is_some_and(|ext| ext == "md"))
        .collect();
    prompts.sort();
    assert!(
        !prompts.is_empty(),
        "no `.md` files under {} — the derived corpus is empty, so every assertion \
         over it would pass having read nothing",
        dir.display()
    );
    prompts
}

/// Decision 7 of `docs/interactive-brainstorm.md`: every phase-prompt except the
/// recorded exclusions quotes [`ASK_CHANNEL_DIRECTIVE`] exactly once, and each
/// exclusion quotes it exactly zero times.
///
/// **The exclusion half is what makes silence mean something.** A check that only
/// asserted presence could not tell `review-angle.md` — excluded on purpose,
/// because it briefs read-only reviewers with no write channel — from a prompt
/// nobody got to. Pairing the derived directory read with an exclusion table
/// asserted in both directions makes "missing entirely" unrepresentable: a new
/// phase-prompt fails until it carries the text or is excluded by name, and an
/// exclusion that stops naming a real file fails outright.
///
/// **What this checks is the text, not a few keywords.** The wording carries the
/// exit-code contract (`0`/`2`/`5`/`1`), the two separate context flags, and the
/// fact that a timeout costs nothing — a prompt that kept the phrase "drovr ask"
/// and lost those has lost the directive.
/// [`ask_directive_check_rejects_a_rewording`] pins that.
#[test]
fn ask_channel_directive_present() {
    let canon = canonical_ask_directive();
    assert!(
        !canon.as_str().is_empty(),
        "ASK_CHANNEL_DIRECTIVE is empty; every comparison below would be vacuous"
    );

    let prompts = phase_prompt_files();
    let present: HashSet<String> = prompts
        .iter()
        .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    let excluded: HashSet<String> = ASK_DIRECTIVE_EXCLUSIONS
        .iter()
        .map(|(name, _)| (*name).to_string())
        .collect();

    // Everything that disagrees is COLLECTED, phantom entries included, rather
    // than panicking on the first kind found. Renaming an excluded file is two
    // independent failures at once — the entry now names nothing, and the renamed
    // file is now an unexcluded prompt with no directive — and a check that
    // panicked on the first would send a maintainer round the loop twice to learn
    // the second.
    let mut wrong = Vec::new();

    let mut phantom: Vec<&String> = excluded.difference(&present).collect();
    phantom.sort();
    if !phantom.is_empty() {
        wrong.push(format!(
            "ASK_DIRECTIVE_EXCLUSIONS entries naming no phase-prompt: {phantom:?}\n\
             The table is asserting absence about files that are not there. If a \
             prompt was renamed, rename its entry; if it was deleted, delete its \
             entry — do not leave an exclusion standing for nothing."
        ));
    }

    // **At least one prompt must actually carry the directive.** Without this the
    // check is satisfiable by exclusion: add every phase-prompt to the table,
    // delete the text from all of them, and every remaining assertion below is
    // about absence and passes clean. That is a real hole and not a theoretical
    // one — the exclusion table is the one part of this check a future task is
    // expected to edit. `task_binding_directive_present` is not exposed to it
    // because its corpus carries unconditional must-carry members; this one's is
    // table-driven end to end, so the floor has to be stated.
    assert!(
        excluded.len() < prompts.len(),
        "every phase-prompt ({}) is in ASK_DIRECTIVE_EXCLUSIONS, so this check \
         asserts nothing but absence and would pass with the directive deleted \
         everywhere. An exclusion is for a prompt that briefs a non-writer; if \
         every prompt is one, the directive has no site left and this check \
         should be deleted rather than left standing empty.",
        prompts.len(),
    );
    for path in &prompts {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let excuse = ASK_DIRECTIVE_EXCLUSIONS
            .iter()
            .find(|(entry, _)| *entry == name)
            .map(|(_, why)| *why);
        let contents = fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));

        let quotes = block_quotes(&contents);
        let found = quotes.iter().filter(|quote| **quote == canon).count();
        match (excuse, found) {
            (None, 1) | (Some(_), 0) => continue,
            (None, 0) => {
                // The likely failure is a rewording, not an omission, so show the
                // drifting block rather than only the count. **Every** candidate,
                // not the first: `drovr ask` is the name of the feature this
                // directive is about, so prose elsewhere in the file may quote it
                // too, and showing only the first match points a maintainer at an
                // unrelated note while the actual drift stays invisible.
                let near: Vec<&Quote> = quotes
                    .iter()
                    .filter(|quote| quote.as_str().contains("drovr ask"))
                    .collect();
                wrong.push(if near.is_empty() {
                    format!(
                        "{}: briefs a writer, so it must carry the ask directive, and it \
                         does not quote it at all",
                        path.display()
                    )
                } else {
                    let blocks: Vec<String> = near
                        .iter()
                        .map(|quote| format!("    {}", quote.as_str()))
                        .collect();
                    format!(
                        "{}: briefs a writer, so it must carry the ask directive. It has \
                         {} block quote(s) mentioning `drovr ask`, none of them the \
                         directive — one of these has drifted:\n{}",
                        path.display(),
                        near.len(),
                        blocks.join("\n")
                    )
                });
            }
            (None, n) => wrong.push(format!(
                "{}: quotes the ask directive {n} times, expected once",
                path.display()
            )),
            (Some(why), n) => wrong.push(format!(
                "{}: quotes the ask directive {n} time(s), expected none — it is a \
                 recorded exclusion ({why}). If that judgement has changed, delete its \
                 ASK_DIRECTIVE_EXCLUSIONS entry in the SAME commit as the text.",
                path.display()
            )),
        }
    }

    assert!(
        wrong.is_empty(),
        "{} phase-prompt(s) disagree with the ask-channel contract:\n{}\n\n\
         The canonical text is ASK_CHANNEL_DIRECTIVE and it is the whole contract — \
         quote it, do not reword it, do not drop the exit codes, and do not collapse \
         `--context` and `--context-file` into one flag. Re-wrapping and indenting are \
         fine; `Quote::new` folds both. Do NOT touch the step-0 task-binding block to \
         make room: `task_binding_directive_present` requires it verbatim.",
        wrong.len(),
        wrong.join("\n"),
    );
}

/// [`block_quotes`] recovers the ask directive from the shape the prompts carry
/// it in, and [`ask_channel_directive_present`]'s comparison refuses a rewording.
///
/// Presence of a few keywords is not presence of the contract, and this is the
/// case that says so. Both variants are built **from**
/// [`ASK_CHANNEL_DIRECTIVE`] or asserted against it, so neither can drift away
/// from the const it is about.
#[test]
fn ask_directive_check_rejects_a_rewording() {
    let canon = canonical_ask_directive();

    // The shape the prompts use: indented inside a numbered step, prose either
    // side. Blank lines inside the quote stay inside it — a `>`-only line is
    // still a quote line, and the directive has two of them around its command
    // block.
    let indented: String = ASK_CHANNEL_DIRECTIVE
        .trim()
        .lines()
        .map(|line| {
            if line.is_empty() {
                "   >\n".to_string()
            } else {
                format!("   > {line}\n")
            }
        })
        .collect();
    let page = format!("1. **Ask when you need to.**\n\n{indented}\n2. Next step.\n");
    assert_eq!(
        block_quotes(&page),
        vec![canon.clone()],
        "the extractor did not recover the ask directive from the shape the \
         phase-prompts use to carry it — a blank `>` line must not split the quote"
    );

    // A one-word change, made FROM the const so it cannot drift into agreement.
    // The word chosen is the exact drift this directive exists to pin: collapsing
    // `--context-file <path>` back into the plan's mistaken `--context <path>`.
    let one_word = ASK_CHANNEL_DIRECTIVE.replace("--context-file <path>", "--context <path>");
    assert_ne!(
        one_word, ASK_CHANNEL_DIRECTIVE,
        "the one-word rewording changed nothing, so the assertion below is vacuous \
         — the phrase it edits is no longer in the directive (mind the line wrap: \
         `replace` does not see across a newline)"
    );
    assert_ne!(
        Quote::new(&one_word),
        canon,
        "a one-word rewording compared equal to the canonical text — the \
         comparison is not checking the wording"
    );

    // A paraphrase carrying every keyword a substring check would have looked
    // for. It must NOT be accepted as the directive.
    let paraphrase = "If new information is discovered, or you find a question you cannot \
                      answer, run `drovr ask <run> --question ...` and then background \
                      `drovr ask wait <run>`.";
    for fragment in [
        "new information",
        "find a question",
        "drovr ask",
        "ask wait",
    ] {
        assert!(
            paraphrase.contains(fragment),
            "this case only refutes a keyword check if it carries every keyword such \
             a check would look for; it is missing `{fragment}`"
        );
    }
    assert_ne!(
        Quote::new(paraphrase),
        canon,
        "a paraphrase carrying every obvious keyword compared equal to the canonical \
         text — the comparison is not checking the wording"
    );
}

/// The retired question channel, by the name a phase-prompt would have to write
/// to reach it.
const RETIRED_QUESTIONS_FILE: &str = "questions.json";

/// Every `file:line: text` in `files` that names `needle`, case-insensitively.
///
/// `files` is `(display name, body)`, so the caller decides whether the bodies
/// came off disk or out of the test — which is what lets
/// [`no_phase_prompt_mentions_questions_json`] run **this whole function**, not
/// just a fragment of it, over a document it built itself before running it over
/// the repo.
///
/// Case-insensitive because the failure is copy-paste from an older prompt or an
/// older branch, and a needle that is a filename is exactly the kind of token a
/// re-typing hand gets the case wrong on. There is no plausible false positive:
/// nothing else in a phase-prompt spells this word.
fn mentions_of(needle: &str, files: &[(String, String)]) -> Vec<String> {
    let needle = needle.to_ascii_lowercase();
    let mut hits = Vec::new();
    for (name, body) in files {
        for (i, line) in body.lines().enumerate() {
            if line.to_ascii_lowercase().contains(&needle) {
                hits.push(format!("  {name}:{}: {}", i + 1, line.trim()));
            }
        }
    }
    hits
}

/// Decision 5 of `docs/interactive-brainstorm.md`: `questions.json` is
/// **replaced**, not kept alongside. One question channel.
///
/// The file no longer exists anywhere on the drovr side — T4 deleted the `GET
/// questions` route, T6 the review-page panel and `RunPaths::questions()`. So a
/// phase-prompt that still names it is briefing an agent to write a path nothing
/// reads, and to answer questions through a UI that no longer renders them. That
/// failure is silent at the agent: the write succeeds.
///
/// **Derived over the directory**, for the reason
/// [`ask_channel_directive_present`] spells out — a phase-prompt added later is
/// covered the day it lands, not the day someone remembers a const. There is no
/// exclusion table and there should not be one: no prompt has a legitimate reason
/// to name a deleted file, so an exclusion here would only ever be a way to keep
/// one.
///
/// Deliberately a **substring**, not a schema-shaped heuristic. The failure it
/// guards is the old bullet growing back by copy-paste from an older prompt or an
/// older branch, and that bullet cannot be written without naming the file. Note
/// this is the opposite polarity to the other phase-prompt checks: they require
/// text, this one refuses it, so [`phase_prompt_files`]'s non-empty assertion is
/// doing more work here than it is there — an empty corpus would make an absence
/// check pass having read nothing.
#[test]
fn no_phase_prompt_mentions_questions_json() {
    // ONE scanner, closing over the needle, so the canary below and the repo scan
    // cannot come apart. Passing the needle at each call site instead would leave
    // the failure this test exists to refuse still reachable: mistype it in the
    // repo scan only, and the canary keeps passing while the scan matches
    // nothing — a green absence check that read every file and looked for the
    // wrong word.
    let scan = |files: &[(String, String)]| mentions_of(RETIRED_QUESTIONS_FILE, files);

    // The canary: the real scanner, over a document holding the bullet this test
    // exists to refuse. A clean result over the repo means "the text is gone"
    // only if a scan that SHOULD fire demonstrably does.
    //
    // Spelled out as a LITERAL, deliberately NOT built from
    // RETIRED_QUESTIONS_FILE. A canary written out of the same const it is
    // canarying agrees with a typo in that const: misspell it, and the canary's
    // document carries the misspelling too, the scan finds it, and this check
    // goes green having searched every phase-prompt for a word that appears in
    // none of them — while the real bullet sits there. `questions.json` is a
    // fixed historical filename; hard-coding it here costs nothing and closes
    // that.
    let canary = vec![(
        "canary.md".to_string(),
        "- (Optional) To ask the reviewer multiple-choice questions, write\n  \
         `~/.local/share/drovr/runs/<run>/questions.json`. It MUST be a bare JSON array\n"
            .to_string(),
    )];
    assert_eq!(
        scan(&canary),
        vec![
            "  canary.md:2: `~/.local/share/drovr/runs/<run>/questions.json`. It MUST be \
             a bare JSON array"
                .to_string()
        ],
        "the scanner did not report a reintroduced `questions.json` bullet exactly as \
         the repo scan below would, so a clean result there would mean nothing. If \
         RETIRED_QUESTIONS_FILE was edited, that is the bug this case exists to catch."
    );

    let prompts: Vec<(String, String)> = phase_prompt_files()
        .into_iter()
        .map(|path| {
            let body = fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
            (path.file_name().unwrap().to_string_lossy().into_owned(), body)
        })
        .collect();

    let offenders = scan(&prompts);
    assert!(
        offenders.is_empty(),
        "phase-prompt(s) still name the retired `{RETIRED_QUESTIONS_FILE}`:\n{}\n\n\
         That channel is gone: the `GET questions` route, the review-page panel and \
         `RunPaths::questions()` were all deleted. The replacement is the ask channel \
         (`drovr ask`), which every writer prompt already carries — delete the mention \
         rather than repointing it.",
        offenders.join("\n"),
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

// ---------------------------------------------------------------------------
// spec §6 / §9.1 check 1 — the fix-4 armor
// ---------------------------------------------------------------------------

/// §6 section 3's unity line, carried verbatim by every armored skill.
///
/// It is the one sentence §6 quotes rather than describes, so it is a literal
/// here rather than a shape. Folded before comparison (see [`FoldedBody`]), so a
/// re-wrap is formatting and a rewording is drift — the rule fix 3's directive
/// is already held to.
const UNITY_LINE: &str = "the next phase agent is you, with your context gone";

/// One of §6's sections, and what proves it is on the page.
///
/// **Matched on stable text, never on a line number.** These files are rewritten
/// four times across this run and measured afterwards; a positional check would
/// pass or fail on where a paragraph landed. Headings and fences are line-level
/// constructs and are read as such; a prose literal is read out of the folded
/// body, because prose gets re-wrapped.
enum SectionMarker {
    /// An ATX heading whose text is exactly this, at any level.
    Heading(&'static str),
    /// A sentence that must appear verbatim, up to wrapping.
    ///
    /// Unlike [`SectionMarker::Heading`], this deliberately **does** see inside
    /// fenced blocks: §6 section 5's announcement sentence is fenced on purpose
    /// (it is text the agent is meant to copy out), so a fence-blind matcher
    /// would report the one section that is hardest to get wrong as missing.
    Line(&'static str),
    /// A **plain** fenced block (no info string) whose whole body is this text,
    /// compared whitespace-folded — so the fence may wrap it, exactly as
    /// [`SectionMarker::Line`] tolerates wrapping, but may hold nothing else.
    ///
    /// §6 section 4 is not "the Iron Law appears somewhere", it is "one fenced
    /// all-caps line" — the fence is what makes it a short string an agent can
    /// cite back, rather than one more sentence of prose in a file made of them.
    /// That used to be a [`SectionMarker::Line`] plus a separate fence check in
    /// `check_armor`: one requirement in two places, which is how the two drift
    /// apart. Presence, ordering and fencing are one mechanism here.
    FencedLiteral(&'static str),
    /// A fenced block whose info string names this language.
    Fence(&'static str),
}

impl SectionMarker {
    /// The 0-based body line this marker is found on, or `None`.
    fn find(&self, body: &str, folded: &FoldedBody) -> Option<usize> {
        match self {
            SectionMarker::Heading(text) => headings(body)
                .into_iter()
                .find(|(_, found)| found == text)
                .map(|(line, _)| line),
            SectionMarker::Line(text) => folded.find(&normalize_ws(text)),
            SectionMarker::FencedLiteral(text) => fenced_blocks(body)
                .ok()
                .and_then(|blocks| {
                    blocks
                        .into_iter()
                        .find(|b| b.info.is_empty() && normalize_ws(&b.body) == normalize_ws(text))
                })
                .map(|b| b.line),
            SectionMarker::Fence(lang) => fenced_blocks(body)
                .ok()
                .and_then(|blocks| blocks.into_iter().find(|b| b.lang() == *lang))
                .map(|b| b.line),
        }
    }

    /// How the failure text names what it looked for.
    fn describe(&self) -> String {
        match self {
            SectionMarker::Heading(text) => format!("a heading `{text}`"),
            SectionMarker::Line(text) => format!("the line \"{text}\""),
            SectionMarker::FencedLiteral(text) => {
                format!("a plain fenced block holding exactly \"{text}\"")
            }
            SectionMarker::Fence(info) => format!("a fenced `{info}` block"),
        }
    }
}

/// A body folded to single spaces, with a map from each byte back to the source
/// line it came from.
///
/// §6 requires two sentences verbatim — the unity line and the announcement —
/// and a sentence in a markdown file is wrapped to whatever column its author
/// used. Folding compares wording; the line map is what still lets the result be
/// ordered against the headings, which do not wrap.
struct FoldedBody {
    text: String,
    /// `line_of[i]` is the source line of `text`'s `i`th byte. Same length as
    /// `text`, by construction — every push to one pushes to the other.
    line_of: Vec<usize>,
}

impl FoldedBody {
    fn new(body: &str) -> Self {
        let mut text = String::new();
        let mut line_of = Vec::new();
        for (idx, line) in body.lines().enumerate() {
            for word in line.split_whitespace() {
                if !text.is_empty() {
                    text.push(' ');
                    line_of.push(idx);
                }
                text.push_str(word);
                line_of.extend(std::iter::repeat_n(idx, word.len()));
            }
        }
        FoldedBody { text, line_of }
    }

    /// The source line `needle` starts on, or `None`. `needle` must already be
    /// folded — [`normalize_ws`] is the one way to do that.
    ///
    /// **Folding spans block boundaries**, so a required sentence is found even
    /// if it wraps across a blank line. That also means "verbatim" here is
    /// slightly weaker than it reads: a sentence straddling a heading would
    /// match. No required literal is short or generic enough for that to happen
    /// by accident.
    fn find(&self, needle: &str) -> Option<usize> {
        // An empty needle matches at 0 while `line_of` may be empty, which would
        // index out of range on an empty body. It is also never a real marker —
        // it would report every section present, everywhere.
        assert!(
            !needle.is_empty(),
            "a SectionMarker::Line that folds to nothing would match any file"
        );
        self.text.find(needle).map(|at| self.line_of[at])
    }
}

/// A fenced code block: where it opens, its info string, and what is inside it.
struct FencedBlock {
    /// 0-based body line of the **opening** fence.
    line: usize,
    /// The info string — `dot` for §6 section 6b's flowchart, empty for the
    /// Iron Law's plain fence.
    info: String,
    /// The lines between the fences, joined with newlines.
    body: String,
}

impl FencedBlock {
    /// The language the info string names, which is its first word.
    ///
    /// An info string is a language *plus arbitrary attributes*, so
    /// ```` ```dot rankdir=LR ```` is a `dot` block. Comparing the whole string
    /// made that one-token addition invisible to the section-6b check in both
    /// directions — a required flowchart reported missing, and a forbidden one
    /// waved through.
    fn lang(&self) -> &str {
        self.info.split_whitespace().next().unwrap_or("")
    }
}

/// The greatest indentation a fence may carry. Four spaces is an indented code
/// block in CommonMark, not a fence — so a `dot` graph shown as an *example*,
/// indented into a surrounding list or quote, is not a section-6b flowchart.
const MAX_FENCE_INDENT: usize = 3;

/// A fence line: its indentation-stripped backtick run length and whatever
/// follows it, or `None` if the line is not a fence at all.
fn fence_marker(line: &str) -> Option<(usize, &str)> {
    let indent = line.len() - line.trim_start().len();
    if indent > MAX_FENCE_INDENT {
        return None;
    }
    let rest = line.trim_start();
    let ticks = rest.chars().take_while(|c| *c == '`').count();
    (ticks >= 3).then(|| (ticks, rest[ticks..].trim()))
}

/// Every fenced block in `text`, or the line of a fence that never closes.
///
/// **An unterminated fence is an error, not a dropped block.** §6 puts two of
/// its required sections inside fences, and a file whose fence never closes
/// renders the entire remainder as code — which a checker that silently dropped
/// the block would report as a missing section, sending the author to add a
/// second copy of text that is already there.
///
/// **A block closes only on a bare fence at least as long as the one that
/// opened it** — CommonMark's rule, and the reason a ```` ````markdown ```` block
/// showing a ``` fence inside it parses as one block rather than desyncing every
/// block after it. `skills/writing-skills/SKILL.md` is the authoring reference
/// for §6's armor and is exactly the file that grows one of those.
fn fenced_blocks(text: &str) -> Result<Vec<FencedBlock>, usize> {
    let mut out = Vec::new();
    let mut open: Option<(usize, usize, String, Vec<&str>)> = None;
    for (idx, line) in text.lines().enumerate() {
        match (fence_marker(line), open.as_mut()) {
            // Inside a block: only a bare fence of at least the opening length
            // closes it. Anything else — including a shorter fence, or one
            // carrying an info string — is content.
            (Some((ticks, rest)), Some((_, opened_with, _, lines))) => {
                if ticks >= *opened_with && rest.is_empty() {
                    let (line, _, info, lines) = open.take().expect("just matched Some");
                    out.push(FencedBlock {
                        line,
                        info,
                        body: lines.join("\n"),
                    });
                } else {
                    lines.push(line);
                }
            }
            (Some((ticks, rest)), None) => open = Some((idx, ticks, rest.to_string(), Vec::new())),
            (None, Some((_, _, _, lines))) => lines.push(line),
            (None, None) => {}
        }
    }
    match open {
        Some((line, _, _, _)) => Err(line),
        None => Ok(out),
    }
}

/// Every ATX heading in `text`, as (0-based body line, heading text).
///
/// **Fence-aware, and that is the whole reason it exists.** A line-by-line scan
/// for `## …` matched headings *inside* code fences and indented examples, which
/// broke the check in both directions: a skill that merely **documents** §6's
/// armor read as armored, and — worse — a page that documented every section
/// inside indented blocks while carrying none of them satisfied the whole
/// structure check, in order. `skills/writing-skills/SKILL.md` is a real file
/// that quotes these headings without carrying them.
///
/// Otherwise it is CommonMark's ATX rule and not an approximation of it: at most
/// [`MAX_FENCE_INDENT`] spaces of indent, one to six `#`, a space required after
/// them (`#Overview` is not a heading), and an optional closing run of `#`
/// stripped (`## Overview ##` is the heading `Overview`). The strictness matters
/// in the rejecting direction too — a heading this missed would be reported as a
/// missing section, sending an author to add text that is already there.
fn headings(text: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut fence: Option<usize> = None;
    for (idx, line) in text.lines().enumerate() {
        if let Some((ticks, rest)) = fence_marker(line) {
            match fence {
                Some(opened_with) if ticks >= opened_with && rest.is_empty() => fence = None,
                Some(_) => {}
                None => fence = Some(ticks),
            }
            continue;
        }
        if fence.is_some() {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        if indent > MAX_FENCE_INDENT {
            continue;
        }
        let rest = line.trim_start();
        let hashes = rest.chars().take_while(|c| *c == '#').count();
        if !(1..=6).contains(&hashes) {
            continue;
        }
        let after = &rest[hashes..];
        if !after.starts_with(' ') {
            continue;
        }
        out.push((idx, after.trim().trim_end_matches('#').trim().to_string()));
    }
    out
}

/// Which of §6's two CONDITIONAL sections this armored skill carries.
///
/// **§6 partitions its four skills, so this is one choice and not two flags.**
/// Section 6b is named for `tdd` and `systematic-debugging`; section 7 for
/// `verification-before-completion` and `code-review`. Modelled as two
/// independent booleans, "carries both" and "carries neither" were
/// representable — states §6 does not define — and Tasks 11–13 each hand-write
/// one of these entries, so the wrong combination would compile and only fail
/// later, opaquely, against a real file.
///
/// **The section a skill does NOT carry is asserted absent**, which is the
/// half that keeps §2.3's placement rule ("the device earns its place on one
/// loop, not on every page") from eroding one well-meant flowchart at a time.
/// The absent one needs no recorded reason the way [`SiteState::NotASite`] does:
/// it follows from §6 naming the other, not from a judgement anyone made.
#[derive(PartialEq, Eq, Clone, Copy, Debug)]
enum ConditionalSection {
    /// §6 section 6b — the cycle as a fenced `dot` graph.
    CycleFlowchart,
    /// §6 section 7 — claim → required evidence → not sufficient — carrying the
    /// rows this skill states and the wording it gives each one.
    ///
    /// **The claims ride here for the same reason [`Armor::procedure_steps`]
    /// records section 6's arity, and the omission had the same shape.** Section
    /// 6b is self-describing: a fenced `dot` block either is there or is not, so
    /// `CycleFlowchart` needs no payload. Section 7 is not — a heading-only check
    /// confirms the table exists while saying nothing about whether it still
    /// covers this skill's rows. A row could be dropped, reordered or reworded and the
    /// suite would stay green — the vacuous-pass class this run has hit
    /// repeatedly, reached by *modelling* one conditional section as a marker
    /// because its sibling could be one.
    RequirementsTable { claims: RequirementClaims },
}

/// The rows a section-7 skill's requirements table states, and the wording it
/// gives each one — one variant per skill §6 marks CONDITIONAL for section 7.
///
/// **Named fields, not a slice, because each skill's row set is CLOSED.** Under
/// `&[&str]` a skill could declare three internally consistent claims, pass
/// every check, and never mention the row this run has the most reason to keep.
/// Here that is a **compile error**: there is nowhere to put four claims.
///
/// It also removes an invariant that used to live in a neighbouring test. A
/// slice could be empty, which would have made [`check_requirements_table`]
/// compare `[] == []` and pass on a table with no rows — non-vacuity guaranteed
/// by `armor_table_declares_well_formed_strings` happening to exist, with no
/// link back from the function that depended on it. **Five named fields cannot
/// be empty**, so the guarantee is carried by the type rather than by a second
/// test's continued existence.
///
/// **Why two variants rather than one shared struct.** The first version of this
/// type had one struct — `tests` / `build` / `linter` / `bug_fixed` /
/// `subagent_reported_success` — and a doc comment saying §6 "covers" those five
/// "for both skills it marks CONDITIONAL for". §6 does not say that. Its
/// section-7 line reads *claim → required evidence → not sufficient · ONLY:
/// verification-before-completion, code-review*, and the five row names appear
/// exactly once in §6, inside `verification-before-completion`'s own per-skill
/// bullet. `code-review`'s bullet names an Iron Law, three loophole closures and
/// the FOREGROUND promotion, and no rows at all. So the row set is **the spec's
/// decision for one skill and the author's for the other**, and a single struct
/// asserted a §6 fact that §6 does not state — which would have forced
/// `code-review`'s table to set bars for the linter and the build, subjects that
/// skill has nothing to say about.
///
/// What the split does **not** relax: each variant's rows are **named fields**,
/// so neither skill can drop, omit or silently permute one — a missing field is
/// `E0063`, not a shorter table — and [`RequirementClaims::rows`] is the one
/// place either order is written down. The two arities differ on purpose: §6
/// fixes five rows for `verification-before-completion` and none for
/// `code-review`, whose four are the four claims its Iron Law composes.
/// [`RequirementClaims::rows`] returns a [`Rows`], which is non-empty by
/// construction, so the differing arities cost nothing: no caller has to check
/// for an empty row list, because there is no way to build one.
#[derive(PartialEq, Eq, Clone, Copy, Debug)]
enum RequirementClaims {
    /// §6's five rows for `verification-before-completion`, in §6's order —
    /// quoted from its per-skill bullet, which is the only place they are named.
    Verification {
        tests: &'static str,
        build: &'static str,
        linter: &'static str,
        bug_fixed: &'static str,
        subagent_reported_success: &'static str,
    },
    /// `code-review`'s rows, in the order its table states them. §6 fixes no row
    /// set here, so **these are Task 13's reading of §6 section 7 for this
    /// skill**, recorded as a type so that a later edit dropping one is a
    /// compile error rather than a quiet narrowing of what the skill guards.
    /// They are the four claims that skill's Iron Law composes: a reviewer has
    /// seen it, it came back clean, the findings are resolved, or one is
    /// recorded as deferred.
    Review {
        reviewed: &'static str,
        clean: &'static str,
        resolved: &'static str,
        deferred: &'static str,
    },
}

impl RequirementClaims {
    /// Each row as (the name §6 or this skill gives it, the wording the shipped
    /// table uses), in the order the table states them.
    ///
    /// **One method, not a `KINDS` constant beside an `in_order`.** Those were
    /// two lists bound to each other by position, so a row inserted into one and
    /// not the other would have mislabelled every diagnostic below it while
    /// every assertion stayed green. Pairing them here makes that
    /// unrepresentable, and leaves the order written down exactly once per
    /// skill.
    fn rows(&self) -> Rows {
        match self {
            RequirementClaims::Verification {
                tests,
                build,
                linter,
                bug_fixed,
                subagent_reported_success,
            } => Rows {
                first: ("tests", tests),
                rest: vec![
                    ("build", build),
                    ("linter", linter),
                    ("bug-fixed", bug_fixed),
                    ("subagent-reported-success", subagent_reported_success),
                ],
            },
            RequirementClaims::Review {
                reviewed,
                clean,
                resolved,
                deferred,
            } => Rows {
                first: ("reviewed", reviewed),
                rest: vec![
                    ("clean", clean),
                    ("resolved", resolved),
                    ("deferred", deferred),
                ],
            },
        }
    }

    /// The declared claims alone, in the table's order.
    fn in_order(&self) -> Vec<&'static str> {
        self.rows().iter().map(|(_, claim)| claim).collect()
    }
}

/// A section-7 skill's rows, in table order — **non-empty by construction**.
///
/// A head and a tail rather than a `Vec`, for one reason: an empty row list
/// would make [`check_requirements_table`]'s comparison `[] == []` and pass on a
/// table with no rows at all — the vacuous-pass class this run has hit
/// repeatedly. `[_; 5]` used to rule that out, and stopped being available when
/// §6 turned out to fix five rows for one section-7 skill and none for the
/// other, so the two variants have different arities.
///
/// The `Vec` that replaced the array pushed the invariant onto the checker as a
/// runtime `is_empty()` guard on a branch no variant could reach — **an
/// unreachable check standing in for a type**, which is exactly the trade Task
/// 12's gate refused one level up. `first` is not an `Option`, so there is
/// nothing left to check and nothing left to forget to check.
struct Rows {
    first: (&'static str, &'static str),
    rest: Vec<(&'static str, &'static str)>,
}

impl Rows {
    /// Every row, head first. Arity-agnostic, so callers never learn how many
    /// rows a given skill declares.
    fn iter(&self) -> impl Iterator<Item = (&'static str, &'static str)> + '_ {
        std::iter::once(self.first).chain(self.rest.iter().copied())
    }
}

/// A skill that carries §6's armor, and the per-skill strings §6 fixes for it.
///
/// The two verbatim strings live here rather than in a lookup beside the table
/// because they are properties of the armored skill, not of the set of them:
/// there is nowhere to write `tdd`'s announcement except on `tdd`'s entry, so no
/// second spelling can drift from this one. This is the same reason the body
/// budget rides in [`skill_names!`] instead of a table of its own.
#[derive(Clone, Copy)]
struct Armor {
    /// §6 section 4's fenced line, verbatim. All-caps — asserted, not assumed.
    iron_law: &'static str,
    /// §6 section 5's exact announcement sentence. §6 lists all four; they are
    /// commitment devices, so a paraphrase is a different device.
    announce: &'static str,
    /// Which of §6's two CONDITIONAL sections this skill carries — exactly one.
    conditional: ConditionalSection,
    /// How many numbered steps §6 section 6's procedure has.
    ///
    /// **Recorded rather than derived, because fix 3's directive binds to it.**
    /// The directive tells an agent to create *one tracked item per step*, so a
    /// step silently added or dropped changes what the agent is told to track
    /// while every check stays green. Requiring the arity to be edited here too
    /// makes that a decision someone made rather than a diff nobody read — the
    /// same argument [`SiteState`] makes for not spelling a state as silence.
    procedure_steps: usize,
}

/// What a `skills/<name>/SKILL.md` is, with respect to fix 4.
///
/// Three states, and **no fourth state spelled as silence** — the same shape as
/// [`SiteState`], for the same reason. plan.md asked for this as three parallel
/// `&[&str]` lists (`ARMORED_SKILLS`, `REQUIREMENTS_TABLE_SKILLS`,
/// `CYCLE_FLOWCHART_SKILLS`); it is one table instead, because parallel lists of
/// skill names are the defect this file has already had to remove twice — four
/// spellings of the measured set collapsed into [`skill_names!`], and `Deferred`
/// vs "not mentioned" collapsed into [`SiteState`]. Under three lists, a skill
/// absent from all three is indistinguishable from one nobody classified, and
/// "carries the requirements table" would be expressible for a skill that is not
/// armored at all.
enum ArmorState {
    /// Armored today. Asserted to carry §6's REQUIRED sections, in §6's order.
    Armored(Armor),
    /// One of §6's four whose rewrite lands in a **named** later task. Asserted
    /// **not** armored until then, so the task that writes the text and forgets
    /// to flip this entry fails, and so does the reverse.
    ///
    /// Carries its `why` for the same reason [`SiteState::Deferred`] does: this
    /// table is modelled on that one, and a deferral without a rationale is a
    /// scheduling note rather than a decision.
    Pending {
        task: &'static str,
        why: &'static str,
    },
    /// Not one of §6's four. Asserted not armored. The reason is recorded
    /// because this is the variant that is a reading of §6's scope rather than
    /// of its text.
    NotArmored { why: &'static str },
}

impl ArmorState {
    fn describe(&self) -> String {
        match self {
            ArmorState::Armored(_) => "recorded as Armored".to_string(),
            ArmorState::Pending { task, why } => format!("recorded as Pending on {task} ({why})"),
            ArmorState::NotArmored { why } => format!("recorded as NotArmored ({why})"),
        }
    }
}

/// Every `skills/*/SKILL.md`, classified against fix 4. **Exhaustive by
/// assertion, not by hope** — see [`ArmorState`].
///
/// **Tasks 11–13: your rewrite and your entry here are ONE edit.** `Pending`
/// asserts the armor is absent and `Armored` asserts it is present, so either
/// half alone is a red suite — and a red test handed across a task boundary
/// halts the pipeline loop.
const SKILL_ARMOR_STATES: &[(&str, ArmorState)] = &[
    (
        "tdd",
        ArmorState::Armored(Armor {
            iron_law: "NO IMPLEMENTATION CODE BEFORE A TEST YOU HAVE WATCHED FAIL.",
            announce: "Using drovr:tdd — writing the failing test before the implementation.",
            conditional: ConditionalSection::CycleFlowchart,
            procedure_steps: 7,
        }),
    ),
    (
        "systematic-debugging",
        ArmorState::Armored(Armor {
            iron_law: "NO FIX BEFORE A REPRODUCTION AND A MECHANISTIC CAUSE.",
            announce: "Using drovr:systematic-debugging — reproducing before fixing.",
            conditional: ConditionalSection::CycleFlowchart,
            procedure_steps: 6,
        }),
    ),
    (
        "verification-before-completion",
        ArmorState::Armored(Armor {
            iron_law: "NO COMPLETION CLAIM WITHOUT FRESH EVIDENCE PRODUCED IN THIS MESSAGE.",
            announce: "Using drovr:verification-before-completion — running the checks \
                       before claiming done.",
            conditional: ConditionalSection::RequirementsTable {
                claims: RequirementClaims::Verification {
                    tests: "The task's tests pass",
                    build: "It builds",
                    linter: "The linter is clean",
                    bug_fixed: "The bug is fixed",
                    subagent_reported_success: "The subagent reported success",
                },
            },
            procedure_steps: 6,
        }),
    ),
    (
        "code-review",
        ArmorState::Armored(Armor {
            iron_law: "NO CHANGE IS DONE UNTIL A READ-ONLY REVIEWER HAS SEEN IT AND \
                       EVERY CRITICAL AND IMPORTANT FINDING IS RESOLVED OR RECORDED \
                       AS DEFERRED.",
            announce: "Using drovr:code-review — dispatching read-only reviewers \
                       before calling this done.",
            conditional: ConditionalSection::RequirementsTable {
                // §6 names no rows for this skill (see `RequirementClaims`), so
                // these four are the four claims this skill's Iron Law composes.
                claims: RequirementClaims::Review {
                    reviewed: "This change has been reviewed",
                    clean: "The reviewer found nothing",
                    resolved: "Every Critical and Important finding is resolved",
                    deferred: "That finding does not apply here",
                },
            },
            procedure_steps: 6,
        }),
    ),
    (
        "using-drovr",
        ArmorState::NotArmored {
            why: "fix 2's per-turn router (§4.1), not one of the four discipline \
                  skills §6 names. It gets the gate function, not the armor",
        },
    ),
    (
        "handoff",
        ArmorState::NotArmored {
            why: "process documentation for running drovr — §6 names four skills \
                  and this is not one of them",
        },
    ),
    (
        "pipeline",
        ArmorState::NotArmored {
            why: "same as `handoff`",
        },
    ),
    (
        "worktrees",
        ArmorState::NotArmored {
            why: "same as `handoff`",
        },
    ),
    (
        "writing-skills",
        ArmorState::NotArmored {
            why: "the authoring reference that documents §6's armor; it is not \
                  itself armored. Recorded rather than omitted because it does \
                  carry `Red flags — STOP`, `Rationalizations` and a fenced `dot` \
                  block of its own, so its absence from §6's four is a fact about \
                  §6's scope and not something a heading scan could infer",
        },
    ),
];

/// §6's REQUIRED sections, in §6's order, for one armored skill.
///
/// Section 1 (`description:`) is **not** here: it is frontmatter, and
/// [`all_skills_have_valid_frontmatter`] already owns it. Two checks on one
/// field would be two contracts that can disagree.
///
/// The skill's one CONDITIONAL section is inserted at its §6 position. A skill
/// has exactly one, so their order relative to each other never arises.
fn required_sections(armor: &Armor) -> Vec<(&'static str, SectionMarker)> {
    let mut out = vec![
        ("2 Overview", SectionMarker::Heading("Overview")),
        ("3 Unity line", SectionMarker::Line(UNITY_LINE)),
        ("4 The Iron Law", SectionMarker::Heading("The Iron Law")),
        (
            "4 Iron Law, fenced",
            SectionMarker::FencedLiteral(armor.iron_law),
        ),
        ("5 Announce", SectionMarker::Heading("Announce")),
        (
            "5 Announcement sentence",
            SectionMarker::Line(armor.announce),
        ),
        ("6 The procedure", SectionMarker::Heading("The procedure")),
    ];
    out.push(match armor.conditional {
        ConditionalSection::CycleFlowchart => ("6b Cycle flowchart", SectionMarker::Fence("dot")),
        ConditionalSection::RequirementsTable { .. } => {
            ("7 Requirements", SectionMarker::Heading("Requirements"))
        }
    });
    out.extend([
        ("8 Red flags", SectionMarker::Heading("Red flags — STOP")),
        (
            "9 Rationalizations",
            SectionMarker::Heading("Rationalizations"),
        ),
        (
            "10 Worked example",
            SectionMarker::Heading("Worked example"),
        ),
        ("11 Cross-refs", SectionMarker::Heading("Cross-refs")),
    ]);
    out
}

/// The heading that says a file carries §6's armor at all.
///
/// One marker, not the whole list: `Pending` and `NotArmored` are asserted
/// against this, and `skills/writing-skills/SKILL.md` legitimately carries
/// `Red flags — STOP`, `Rationalizations` and a fenced `dot` block while being
/// no part of fix 4. The Iron Law is the section nothing else in this repo has.
const ARMOR_MARKER: SectionMarker = SectionMarker::Heading("The Iron Law");

/// spec §9.1 check 1: each armored skill carries §6's REQUIRED sections, in
/// §6's order, plus exactly the CONDITIONAL sections §6 names it for.
///
/// **This is not "all 11 sections on every skill"** — §9.1 says so outright, and
/// it would be unsatisfiable by construction: sections 7 and 6b are declared for
/// disjoint pairs of skills.
///
/// Watched RED before it was trusted: run against `skills/tdd/SKILL.md` as arm
/// A′ left it, it reports every §6 section missing. A structure check that has
/// only ever been green is a check that a heading rename would not notice.
#[test]
fn armored_skills_have_required_sections() {
    // Exhaustive in both directions, exactly as `task_binding_directive_present`
    // is: an unclassified skill is the silence this table exists to break, and a
    // phantom entry is a table asserting things about a file that is not there.
    let present: HashSet<String> = skill_files(&skills_dir())
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    let classified: HashSet<String> = SKILL_ARMOR_STATES
        .iter()
        .map(|(name, _)| (*name).to_string())
        .collect();

    let mut unclassified: Vec<&String> = present.difference(&classified).collect();
    unclassified.sort();
    assert!(
        unclassified.is_empty(),
        "skill(s) with no SKILL_ARMOR_STATES entry: {unclassified:?}\n\
         Every skill must say whether it carries §6's armor, is one of the four \
         awaiting its rewrite, or is not a fix-4 skill at all.",
    );
    let mut phantom: Vec<&String> = classified.difference(&present).collect();
    phantom.sort();
    assert!(
        phantom.is_empty(),
        "SKILL_ARMOR_STATES entries naming no skill: {phantom:?}",
    );

    // Without this the check can go quietly vacuous: flip the one `Armored`
    // entry back to `Pending` — which Task 22 is explicitly allowed to do for a
    // skill whose arm B failed — and `check_armor` is no longer run against a
    // real file at all, while the suite still prints `ok`. That is the same
    // "no fourth state spelled as silence" failure this table's own doc comment
    // argues against, one level up.
    assert!(
        SKILL_ARMOR_STATES
            .iter()
            .any(|(_, state)| matches!(state, ArmorState::Armored(_))),
        "no skill is recorded Armored, so this check reads nine files and asserts \
         nothing about any of them. If Task 22 reverted the last armored skill, \
         delete this check with it rather than leaving it green and empty."
    );

    let mut wrong: Vec<String> = Vec::new();
    for (name, state) in SKILL_ARMOR_STATES {
        let path = skills_dir()
            .join(name)
            .join(format!("{PER_SKILL_FILE_STEM}.md"));
        let contents = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
        let skill = parse_skill(&contents)
            .unwrap_or_else(|| panic!("{} has no frontmatter", path.display()));
        // Every line number below indexes the *body*, which starts after the
        // frontmatter. Reported raw, they sent a reader to the wrong line of the
        // file the message names.
        let first_body_line = contents.lines().count() - skill.body.lines().count();
        let folded = FoldedBody::new(&skill.body);
        let armored = ARMOR_MARKER.find(&skill.body, &folded).is_some();

        let ArmorState::Armored(armor) = state else {
            if armored {
                wrong.push(format!(
                    "{} ({}): carries {}, so it is armored. If this is the task \
                     that armors it, move the entry to ArmorState::Armored in the \
                     SAME commit as the rewrite.",
                    path.display(),
                    state.describe(),
                    ARMOR_MARKER.describe(),
                ));
            }
            continue;
        };

        check_armor(
            &path,
            &skill.body,
            &folded,
            armor,
            first_body_line,
            &mut wrong,
        );
    }

    assert!(
        wrong.is_empty(),
        "{} skill(s) disagree with their recorded armor state:\n{}\n\n\
         §6 fixes the section order for the four discipline skills; \
         SKILL_ARMOR_STATES records which skill is at which stage of that \
         rewrite. Both halves are asserted, so neither a missing section nor an \
         unannounced one can pass as the other.",
        wrong.len(),
        wrong.join("\n"),
    );
}

/// One armored skill's sections: present, in order, and the conditional ones
/// exactly where §6 names them. Failures are pushed rather than asserted, so one
/// run reports every section a rewrite is missing instead of the first.
fn check_armor(
    path: &Path,
    body: &str,
    folded: &FoldedBody,
    armor: &Armor,
    first_body_line: usize,
    wrong: &mut Vec<String>,
) {
    // Body line -> the line of the file a reader will open.
    let at = |line: usize| line + first_body_line + 1;

    let blocks = match fenced_blocks(body) {
        Ok(blocks) => blocks,
        Err(line) => {
            wrong.push(format!(
                "{}: the fence opened on line {} never closes, so everything \
                 below it renders as code",
                path.display(),
                at(line),
            ));
            return;
        }
    };

    // The CONDITIONAL section this skill does NOT carry is asserted absent.
    // Its presence is `required_sections`' job, in the ordered pass below — this
    // half is the one an ordering check cannot express.
    //
    // Asymmetric on purpose: the *required* direction matches §6's text exactly,
    // because that text is the contract. The *forbidden* direction matches any
    // heading that merely opens with it, so a section 7 smuggled in as
    // "Requirements table" cannot walk around §6's scope on a title variation.
    match armor.conditional {
        // §6 names this skill for 6b, so section 7 must be absent.
        ConditionalSection::CycleFlowchart => {
            if let Some((line, found)) = headings(body)
                .into_iter()
                .find(|(_, text)| text.starts_with("Requirements"))
            {
                wrong.push(format!(
                    "{}: line {} is a `{found}` heading, but §6 marks section 7 \
                     CONDITIONAL and names only `verification-before-completion` \
                     and `code-review` for it",
                    path.display(),
                    at(line),
                ));
            }
        }
        // §6 names this skill for 7, so the flowchart must be absent.
        ConditionalSection::RequirementsTable { .. } => {
            if let Some(block) = blocks.iter().find(|b| b.lang() == "dot") {
                wrong.push(format!(
                    "{}: line {} opens a fenced `dot` block, but §6 marks section \
                     6b CONDITIONAL and names only `tdd` and \
                     `systematic-debugging` for it (§2.3: the device earns its \
                     place on one loop, not on every page)",
                    path.display(),
                    at(block.line),
                ));
            }
        }
    }

    // Presence and order in one pass: §6 fixes an order, so a file with every
    // section in the wrong sequence is not a file that satisfies §6.
    let mut last: Option<(&str, usize)> = None;
    for (label, marker) in required_sections(armor) {
        let Some(line) = marker.find(body, folded) else {
            wrong.push(format!(
                "{}: §6 section `{label}` is missing — expected {}",
                path.display(),
                marker.describe(),
            ));
            continue;
        };
        if let Some((prev_label, prev_line)) = last
            && line <= prev_line
        {
            wrong.push(format!(
                "{}: §6 section `{label}` is on line {} but `{prev_label}` is \
                 on line {} — §6 fixes the order",
                path.display(),
                at(line),
                at(prev_line),
            ));
        }
        last = Some((label, line));
    }

    // §6 section 6's internal composition, which the ordered pass above cannot
    // express: it checks that sections appear in order, not that one section's
    // two required parts stand in the right relation to each other.
    check_procedure(path, body, armor, first_body_line, wrong);
    // §6 section 7's contents, for the same reason. The ordered pass sees a
    // `Requirements` heading; it cannot see whether anything is under it.
    if let ConditionalSection::RequirementsTable { claims } = armor.conditional {
        check_requirements_table(path, body, &claims, first_body_line, wrong);
    }
}

/// The cells of a markdown table row, or `None` if `line` is not one.
///
/// Leading and trailing pipes are optional, matching how the shipped tables are
/// written. There is no escape handling, exactly as `arms/MANIFEST.md`'s parser
/// has none — a cell containing a literal `|` is a table nobody can read back.
fn table_row(line: &str) -> Option<Vec<String>> {
    let trimmed = line.trim();
    if !trimmed.starts_with('|') {
        return None;
    }
    let inner = trimmed.trim_start_matches('|').trim_end_matches('|');
    Some(
        inner
            .split('|')
            .map(|cell| cell.trim().to_string())
            .collect(),
    )
}

/// Is this the `|---|---|---|` row that separates a header from its data?
fn is_delimiter_row(cells: &[String]) -> bool {
    !cells.is_empty()
        && cells.iter().all(|cell| {
            let bare = cell.trim().trim_start_matches(':').trim_end_matches(':');
            !bare.is_empty() && bare.chars().all(|c| c == '-')
        })
}

/// A requirements-table cell reduced to its words: markdown emphasis, quote
/// marks and backticks dropped, whitespace folded.
///
/// The shipped table writes its claims as `*"The task's tests pass"*` because
/// they are utterances; [`SKILL_ARMOR_STATES`] declares them as prose. Comparing
/// the markup instead would make the declaration a copy of the rendering, which
/// is the shape that drifts — and would fail on a row someone merely italicised.
fn claim_text(cell: &str) -> String {
    let bare: String = cell
        .chars()
        // Curly quotes included: a smart-quote autocorrect on either side of the
        // comparison is a formatting edit, and failing on it would be noise that
        // teaches an author to distrust the check.
        .filter(|c| {
            !matches!(
                c,
                '*' | '`' | '"' | '_' | '\u{201c}' | '\u{201d}' | '\u{2018}' | '\u{2019}'
            )
        })
        .collect();
    normalize_ws(&bare)
}

/// spec §6 section 7: *claim → required evidence → **not sufficient***, over the
/// rows this skill declares.
///
/// **What this asserts that a heading cannot.** `required_sections` proves a
/// `Requirements` heading exists and `check_armor` proves no `dot` fence does;
/// between them, a page carrying the heading and nothing else satisfies both.
/// Which rows a section-7 skill states is recorded in [`RequirementClaims`] and
/// asserted here, and nowhere else — the same argument [`Armor::procedure_steps`]
/// makes for section 6's arity, applied to the section that had no analogue. §6
/// names the five for `verification-before-completion` only; `code-review`'s
/// four are its author's, which is why the row set rides on the variant.
///
/// **What the diagnostics below may and may not blame on §6.** §6 owns the
/// *section*: that it exists, that it is three columns, that the third names
/// what is not sufficient, that a row states something in every cell. Those
/// messages cite §6. It does **not** own which rows a given skill states — only
/// `verification-before-completion`'s five are §6's — so messages naming a row
/// kind (`` `deferred` ``, `` `clean` ``) say "section 7" without the §6, or
/// they send an author to a spec section that does not govern their row.
///
/// The three columns are asserted too. A two-column table is not §6's section 7:
/// dropping the `not sufficient` column deletes the half that closes loopholes,
/// and that is a rewrite, not a formatting choice.
///
/// **Everything its own soundness needs is asserted here or carried by a type**,
/// which it once was not. An earlier version took `&[&str]` and documented that
/// an empty slice would make it compare `[] == []` and pass on a table with no
/// rows — naming `armor_table_declares_well_formed_strings` as the thing that
/// prevented it. [`Rows`] now makes that state unrepresentable.
/// That is the vacuous-pass class one level up: a check that is only meaningful
/// because a neighbour happens to exist, with nothing linking the two, so
/// deleting the neighbour silently demotes this back to a heading check while it
/// still reads as authoritative. [`RequirementClaims`] makes the empty case
/// unrepresentable, and the one precondition a type cannot carry — a declared
/// claim that is blank — is checked below rather than assumed.
fn check_requirements_table(
    path: &Path,
    body: &str,
    claims: &RequirementClaims,
    first_body_line: usize,
    wrong: &mut Vec<String>,
) {
    // The precondition this function's own comparison rests on, asserted here
    // rather than borrowed from a neighbouring test. The OTHER precondition — a
    // non-empty row list, without which the comparison below would be
    // `[] == []` on a table with no rows — is carried by [`Rows`] and needs no
    // check here.
    //
    // A blank claim matches a blank leading cell, so a hollow row would satisfy
    // a hollow declaration: the comparison would run and mean nothing.
    for (kind, claim) in claims.rows().iter() {
        if claim_text(claim).is_empty() {
            wrong.push(format!(
                "{}: section 7's `{kind}` claim is declared blank, so the row \
                 comparison below would be satisfied by a blank cell. Declare the \
                 wording this skill gives that row.",
                path.display(),
            ));
        }
    }
    let at = |line: usize| line + first_body_line + 1;
    let heads = headings(body);
    let Some((start, _)) = heads
        .iter()
        .find(|(_, text)| text == "Requirements")
        .cloned()
    else {
        // `required_sections` already reported the heading as missing.
        return;
    };
    let end = heads
        .iter()
        .map(|(line, _)| *line)
        .find(|line| *line > start)
        .unwrap_or_else(|| body.lines().count());

    // Fence-aware, for the reason [`headings`] and [`numbered_steps`] are: a
    // table inside a fenced block is being *shown*, not declared. Reading one as
    // this section's table does not fail open — the real table is still there
    // and still compared — but it reports the wrong cause, which sends an author
    // to fix a table that was already correct.
    let mut rows: Vec<(usize, Vec<String>)> = Vec::new();
    let mut fence: Option<usize> = None;
    for (idx, line) in body.lines().enumerate() {
        if idx <= start || idx >= end {
            continue;
        }
        if let Some((ticks, rest)) = fence_marker(line) {
            match fence {
                Some(opened_with) if ticks >= opened_with && rest.is_empty() => fence = None,
                Some(_) => {}
                None => fence = Some(ticks),
            }
            continue;
        }
        if fence.is_none()
            && let Some(cells) = table_row(line)
        {
            rows.push((idx, cells));
        }
    }

    // **The header is the row a delimiter follows**, not simply the first line
    // starting with `|`. Anchoring on the delimiter is what stops a stray `|`
    // above the table from being read as its header — and a run of cells with no
    // delimiter under it is not a markdown table at all.
    let header_at = (0..rows.len().saturating_sub(1)).find(|i| is_delimiter_row(&rows[i + 1].1));
    let Some((header_line, header)) = header_at.map(|i| rows[i].clone()) else {
        wrong.push(format!(
            "{}: §6 section 7 opens on line {} with no table under it — no row \
             with a `|---|` delimiter beneath it. The heading is not the \
             section: §6 section 7 is claim → required evidence → not \
             sufficient, over the rows this skill declares.",
            path.display(),
            at(start),
        ));
        return;
    };

    if header.len() != 3 {
        wrong.push(format!(
            "{}: §6 section 7's table has {} column(s) on line {}, not 3. The \
             section is claim → required evidence → NOT sufficient; the third \
             column is the half that closes loopholes.",
            path.display(),
            header.len(),
            at(header_line),
        ));
        return;
    }
    if !claim_text(&header[2])
        .to_lowercase()
        .contains("not sufficient")
    {
        wrong.push(format!(
            "{}: §6 section 7's third column is headed `{}` on line {} — it must \
             name what is NOT sufficient, so a reader can tell the two evidence \
             columns apart.",
            path.display(),
            header[2],
            at(header_line),
        ));
    }

    let data: Vec<&(usize, Vec<String>)> = rows
        .iter()
        .skip(header_at.expect("header_at is Some in this branch") + 2)
        .filter(|(_, cells)| !is_delimiter_row(cells))
        .collect();

    let found: Vec<String> = data
        .iter()
        .map(|(_, cells)| claim_text(&cells[0]))
        .collect();
    let expected: Vec<String> = claims
        .in_order()
        .iter()
        .map(|claim| claim_text(claim))
        .collect();
    if found != expected {
        // Name the first row that differs. Rows have names, and "row 5" makes
        // an author count table rows to find out which.
        let kinds: Vec<&'static str> = claims.rows().iter().map(|(kind, _)| kind).collect();
        let at_kind = expected
            .iter()
            .zip(found.iter().chain(std::iter::repeat(&String::new())))
            .position(|(want, got)| want != got)
            .and_then(|i| kinds.get(i))
            .unwrap_or(&"(row count)");
        wrong.push(format!(
            "{}: section 7's table states the claims {found:?}, but this \
             skill's Armor declares {expected:?} — first difference at the \
             `{at_kind}` row. Change the table and the declaration in the same \
             edit: a row dropped, reordered or reworded changes which claims the \
             skill sets a bar for, and nothing else here would notice.",
            path.display(),
        ));
    }

    for (idx, cells) in data {
        if cells.len() != 3 || cells.iter().any(|cell| cell.trim().is_empty()) {
            wrong.push(format!(
                "{}: §6 section 7's row on line {} has an empty or missing cell. \
                 A claim with no required-evidence cell, or none saying what is \
                 not sufficient, is a row that requires nothing.",
                path.display(),
                at(*idx),
            ));
        }
    }
}

/// §6 section 6's numbered steps in `text`, as (0-based line, the step's number).
///
/// Fence-aware for the reason [`headings`] is: a numbered list inside a worked
/// example is being *shown*, not issued. Blockquote lines are skipped for the
/// same reason — fix 3's directive is itself a blockquote, and a list quoted
/// inside one is an illustration of a checklist, not this skill's checklist.
fn numbered_steps(text: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut fence: Option<usize> = None;
    for (idx, line) in text.lines().enumerate() {
        if let Some((ticks, rest)) = fence_marker(line) {
            match fence {
                Some(opened_with) if ticks >= opened_with && rest.is_empty() => fence = None,
                Some(_) => {}
                None => fence = Some(ticks),
            }
            continue;
        }
        if fence.is_some() {
            continue;
        }
        let trimmed = line.trim_start();
        // Same indent rule the fence and heading scanners use: four spaces is an
        // indented code block, and a list nested under a step is not a step.
        if line.len() - trimmed.len() > MAX_FENCE_INDENT || trimmed.starts_with('>') {
            continue;
        }
        let digits: String = trimmed.chars().take_while(|c| c.is_ascii_digit()).collect();
        if digits.is_empty() || !trimmed[digits.len()..].starts_with(". ") {
            continue;
        }
        if let Ok(number) = digits.parse::<usize>() {
            out.push((idx, number));
        }
    }
    out
}

/// spec §6 section 6 as ONE requirement: "the procedure, **numbered**, preceded
/// by fix 3's task-binding directive".
///
/// **This is the composition, and nothing else was checking it.**
/// [`required_sections`] can only see that a `The procedure` heading exists;
/// [`task_binding_directive_present`] can only see that the directive appears
/// somewhere in the file, exactly once. A page carrying the heading, the
/// directive four sections lower, and no numbering at all satisfies both of them
/// and satisfies §6 section 6 not at all — two facts about one section, checked
/// apart, with the relation between them asserted nowhere.
///
/// The relation is the part that does the work: the directive says *one tracked
/// item per step*, which an agent cannot act on if it meets the directive after
/// working the list, or if there are no steps to enumerate.
fn check_procedure(
    path: &Path,
    body: &str,
    armor: &Armor,
    first_body_line: usize,
    wrong: &mut Vec<String>,
) {
    let at = |body_line: usize| body_line + first_body_line + 1;
    let heads = headings(body);
    let Some((start, _)) = heads
        .iter()
        .find(|(_, text)| text == "The procedure")
        .cloned()
    else {
        // `required_sections` already reported the heading as missing. Saying so
        // twice sends an author looking for two defects.
        return;
    };
    // §6 places section 6b directly after the procedure, so the next heading of
    // any level bounds the numbered steps.
    let end = heads
        .iter()
        .map(|(line, _)| *line)
        .find(|line| *line > start)
        .unwrap_or_else(|| body.lines().count());
    let section: Vec<&str> = body
        .lines()
        .enumerate()
        .filter(|(idx, _)| *idx > start && *idx < end)
        .map(|(_, line)| line)
        .collect();
    let section = section.join("\n");
    // Section line -> body line.
    let to_body = |idx: usize| idx + start + 1;

    let steps = numbered_steps(&section);
    let Some((first_step, _)) = steps.first().copied() else {
        wrong.push(format!(
            "{}: §6 section 6 requires a NUMBERED procedure, and the section \
             opening on line {} has no numbered steps. Fix 3's directive binds \
             one tracked item per step, so an unnumbered procedure leaves it \
             nothing to bind to.",
            path.display(),
            at(start),
        ));
        return;
    };

    let numbers: Vec<usize> = steps.iter().map(|(_, number)| *number).collect();
    let expected: Vec<usize> = (1..=armor.procedure_steps).collect();
    if numbers != expected {
        wrong.push(format!(
            "{}: §6 section 6's procedure is numbered {numbers:?}, but this \
             skill's Armor records {} step(s), so {expected:?} was expected. \
             Change the checklist and the recorded arity in the same edit — the \
             directive binds one tracked item per step, so a step added or \
             dropped silently changes what an agent is told to track.",
            path.display(),
            armor.procedure_steps,
        ));
    }

    let canon = canonical_directive();
    match block_quotes_with_lines(&section)
        .into_iter()
        .find(|(_, quote)| *quote == canon)
    {
        None => wrong.push(format!(
            "{}: §6 section 6 requires fix 3's task-binding directive INSIDE the \
             procedure section (lines {}–{}), preceding its numbered steps. \
             Quoting it elsewhere in the file satisfies \
             `task_binding_directive_present` and not this: that check asks \
             whether the directive is present, this one asks whether it is where \
             the agent reads the checklist.",
            path.display(),
            at(start),
            at(end),
        )),
        Some((line, _)) if line >= first_step => wrong.push(format!(
            "{}: fix 3's task-binding directive is on line {}, below the first \
             numbered step on line {}. §6 section 6 says PRECEDED by — a \
             directive an agent meets after working the list cannot bind it.",
            path.display(),
            at(to_body(line)),
            at(to_body(first_step)),
        )),
        Some(_) => {}
    }
}

/// [`SiteState::Deferred`] still names both halves of a deferral, now that no
/// site is in that state.
///
/// **Written because Task 14 covered the last `Deferred` site**, the exact
/// situation [`pending_still_describes_a_deferral`] below was written for one
/// task earlier — same lint, same variant-goes-unconstructed shape, and the same
/// §7.3 reason the variant is still live: a skill whose arm B fails reverts to
/// A′, which is fix-1-only, which puts its fix-3 directive back in the future
/// and its entry back to `Deferred`.
///
/// **What it asserts is the thing that can actually rot.** The absence half of
/// `Deferred` — that the extractor can see a directive when one is present, so
/// "absent" is a reading and not a blind spot — is already pinned by
/// [`task_binding_check_rejects_a_reworded_directive`], and asserting it twice
/// would be duplication rather than coverage. What nothing else holds is the
/// diagnostic: strip `why` out of [`SiteState::describe`] and every deferral in
/// the failure text becomes "recorded as Deferred to Task 22", which reads as
/// scheduling and hides the reason. That is the failure mode the variant carries
/// a `why` field to prevent, and this is where it is checked.
#[test]
fn deferred_names_its_task_and_its_reason() {
    let deferred = SiteState::Deferred {
        task: "Task 22",
        why: "arm B failed for this skill and it reverts to A-prime, which is fix-1-only",
    };
    let described = deferred.describe();
    for half in [
        "Task 22",
        "arm B failed for this skill and it reverts to A-prime, which is fix-1-only",
    ] {
        assert!(
            described.contains(half),
            "a Deferred entry must name both its task and its reason in \
             diagnostics, so a deferral reads as a decision and not as a gap; \
             {half:?} is missing from: {described}"
        );
    }
    assert!(
        !deferred.must_carry(),
        "a Deferred site is asserted ABSENT — flipping this would make the \
         reverted skill's missing directive look like a pass"
    );
}

/// [`ArmorState::Pending`] still means what its doc comment says, now that no
/// skill is in that state.
///
/// **Written because Task 13 armored the last `Pending` skill**, which made the
/// variant unconstructed anywhere in the file — a `dead_code` warning, and worse
/// than a warning: `Pending`'s whole contract is that it asserts the armor is
/// *absent*, and with no entry in that state, nothing exercised or documented
/// that contract any more. §7.3 lets Task 22 move a skill back to `Pending` when
/// its arm B fails, so the variant is live machinery in a state nobody is
/// currently using, not leftovers. This pins the two things that task will rely
/// on: the variant is constructible with a named task and a reason, and it
/// describes itself as a deferral rather than as a decision.
#[test]
fn pending_still_describes_a_deferral() {
    let pending = ArmorState::Pending {
        task: "Task 22",
        why: "arm B failed for this skill and it reverts to A′",
    };
    let described = pending.describe();
    assert!(
        described.contains("Pending on Task 22")
            && described.contains("arm B failed for this skill and it reverts to A′"),
        "a Pending entry must name both its task and its reason in diagnostics, \
         so a reverted skill reads as a decision and not as a gap: {described}"
    );
    // The property that actually matters: `Pending` asserts the armor is
    // ABSENT, so a file carrying the Iron Law under a Pending entry must be
    // reported. Asserting `!matches!(pending, Armored(_))` instead would be a
    // tautology — `pending` was built as `Pending` two statements ago — so this
    // exercises the marker `armored_skills_have_required_sections` branches on.
    let armored_page = "## The Iron Law\n\n```\nNEVER SHIP WITHOUT A RED TEST.\n```\n";
    assert!(
        ARMOR_MARKER
            .find(armored_page, &FoldedBody::new(armored_page))
            .is_some(),
        "a page carrying {} must read as armored; otherwise the Pending branch \
         can never fire and Task 22 could revert a skill on paper only",
        ARMOR_MARKER.describe(),
    );
}

/// The declared strings are well-formed — a property of [`SKILL_ARMOR_STATES`]
/// itself, so it is checked here and not while walking files.
///
/// It used to be an `assert!` inside `check_armor`, which was wrong twice over:
/// it aborted the whole run at the first bad entry, discarding every complaint
/// already gathered for earlier skills, and it led its message with a
/// `SKILL.md` path — blaming a skill file for a typo in this table.
#[test]
fn armor_table_declares_well_formed_strings() {
    for (name, state) in SKILL_ARMOR_STATES {
        let ArmorState::Armored(armor) = state else {
            continue;
        };
        let letters: Vec<char> = armor
            .iron_law
            .chars()
            .filter(|c| c.is_alphabetic())
            .collect();
        assert!(
            !letters.is_empty() && letters.iter().all(|c| c.is_uppercase()),
            "{name}: the declared Iron Law is not all-caps: {:?}. §6 section 4 is \
             one fenced ALL-CAPS line — the format is what gives an agent a short \
             string to cite back at itself.",
            armor.iron_law,
        );
        assert!(
            !armor.announce.trim().is_empty(),
            "{name}: the declared announcement sentence is empty, so §6 section 5 \
             would be satisfied by any file"
        );
        // Cardinality and non-emptiness are [`RequirementClaims`]' job now — the
        // first is a compile error and the second is unrepresentable. What is
        // left is declaration hygiene that no type expresses, and it is
        // deliberately NOT a precondition of `check_requirements_table`: that
        // function compares positionally, so duplicates cannot mislead it. It is
        // checked because two rows stating one claim is a spec-reading mistake
        // worth catching, not because anything downstream depends on it.
        if let ConditionalSection::RequirementsTable { claims } = armor.conditional {
            let normalized: Vec<String> = claims.in_order().iter().map(|c| claim_text(c)).collect();
            let unique: HashSet<&String> = normalized.iter().collect();
            assert_eq!(
                unique.len(),
                normalized.len(),
                "{name}: section 7's declared claims contain a duplicate: \
                 {normalized:?}. A skill's section-7 rows are different claims; \
                 two spelled the same way means one of them is not being asked \
                 for."
            );
        }
    }
}

/// A minimal page carrying exactly what [`required_sections`] asks for, built
/// **from** that list rather than pasted beside it — so the fixture below cannot
/// drift away from the contract it is a fixture for.
fn synthetic_body(armor: &Armor) -> Vec<String> {
    let mut lines = Vec::new();
    for (_, marker) in required_sections(armor) {
        match marker {
            SectionMarker::Heading(text) => {
                lines.push(format!("## {text}"));
                // §6 section 6 is the one section with required *contents*, so
                // the control fixture has to carry them or every negative case
                // below is measured against a body that never passed.
                if text == "The procedure" {
                    lines.extend(
                        TASK_BINDING_DIRECTIVE
                            .trim()
                            .lines()
                            .map(|quoted| format!("> {quoted}")),
                    );
                    lines.push(String::new());
                    lines.extend((1..=armor.procedure_steps).map(|n| format!("{n}. Step {n}.")));
                }
                // Section 7 is the other section with required *contents*.
                if text == "Requirements"
                    && let ConditionalSection::RequirementsTable { claims } = armor.conditional
                {
                    lines.push("| The claim | Required evidence | NOT sufficient |".to_string());
                    lines.push("|---|---|---|".to_string());
                    lines.extend(claims.in_order().iter().map(|claim| {
                        format!("| *\"{claim}\"* | what it takes | what it is not |")
                    }));
                }
            }
            SectionMarker::Line(text) => lines.push(text.to_string()),
            SectionMarker::FencedLiteral(text) => {
                lines.extend(["```".to_string(), text.to_string(), "```".to_string()]);
            }
            SectionMarker::Fence(info) => lines.extend([
                format!("```{info}"),
                "digraph g { a -> b; }".to_string(),
                "```".to_string(),
            ]),
        }
        lines.push(String::new());
    }
    lines
}

/// [`armored_skills_have_required_sections`] was watched RED on the real file
/// with every section missing. This pins the branches that failure did not
/// exercise — order, the CONDITIONAL sections' *absent* direction, an
/// unterminated fence, and the marker the unarmored states are asserted against.
///
/// Without it the check is only known to notice a wholly unwritten page, which
/// is the one state nobody is going to ship by accident.
#[test]
fn armor_check_refuses_a_page_that_only_looks_armored() {
    let armor = Armor {
        iron_law: "NEVER SHIP WITHOUT A RED TEST.",
        announce: "Using drovr:example — doing the thing before the other thing.",
        conditional: ConditionalSection::CycleFlowchart,
        procedure_steps: 3,
    };
    let path = Path::new("fixture/SKILL.md");
    let complaints = |body: &str, armor: &Armor| -> Vec<String> {
        let mut wrong = Vec::new();
        check_armor(path, body, &FoldedBody::new(body), armor, 0, &mut wrong);
        wrong
    };

    // Control. If this ever fails, every negative case below is meaningless.
    let good = synthetic_body(&armor).join("\n");
    assert!(
        complaints(&good, &armor).is_empty(),
        "a page built from required_sections must satisfy the check: {:?}",
        complaints(&good, &armor)
    );

    // The marker the `Pending` and `NotArmored` states are asserted against.
    let folded = FoldedBody::new(&good);
    assert!(ARMOR_MARKER.find(&good, &folded).is_some());
    let unarmored = "## Overview\n\nnothing to see here\n";
    assert!(
        ARMOR_MARKER
            .find(unarmored, &FoldedBody::new(unarmored))
            .is_none(),
        "a page with no Iron Law must not read as armored — otherwise a Pending \
         skill could never be told from an Armored one"
    );

    // Wrapping is formatting. A required sentence broken across lines is the
    // same sentence, exactly as fix 3's directive is.
    let wrapped = good.replace(armor.announce, &armor.announce.replace(' ', "\n  "));
    assert_ne!(
        wrapped, good,
        "the fixture must actually have been re-wrapped"
    );
    assert!(
        complaints(&wrapped, &armor).is_empty(),
        "re-wrapping the announcement sentence must not read as a missing \
         section: {:?}",
        complaints(&wrapped, &armor)
    );

    // §6 section 4 is a *fenced* line. The same words as prose are not the
    // section, and neither is a fence carrying an info string — the plain fence
    // is what marks it as the string to cite back.
    let fenced_law = format!("```\n{}\n```", armor.iron_law);
    for (variant, replacement) in [
        ("unfenced", armor.iron_law.to_string()),
        ("info-stringed", format!("```text\n{}\n```", armor.iron_law)),
    ] {
        let weakened = good.replace(&fenced_law, &replacement);
        assert_ne!(weakened, good, "{variant}: the fixture did not change");
        assert!(
            complaints(&weakened, &armor)
                .iter()
                .any(|c| c.contains("4 Iron Law, fenced")),
            "{variant}: an Iron Law that is not in a plain fenced block must be \
             reported as the missing section: {:?}",
            complaints(&weakened, &armor)
        );
    }

    // §6 fixes an *order*, so a page holding every section in the wrong sequence
    // does not satisfy it.
    //
    // Two headings swapped, not the whole page reversed: reversing also inverts
    // the `dot` fence's lines, so it trips the section-6b branch as well and the
    // assertion would keep passing even if the order check regressed to a no-op.
    let swapped = good
        .replace("## Cross-refs", "@@LAST@@")
        .replace("## Overview", "## Cross-refs")
        .replace("@@LAST@@", "## Overview");
    assert_ne!(swapped, good, "the fixture must actually have been swapped");
    let complained = complaints(&swapped, &armor);
    assert!(
        complained.iter().any(|c| c.contains("§6 fixes the order")),
        "sections out of order must be reported: {complained:?}"
    );
    assert!(
        complained.iter().all(|c| c.contains("§6 fixes the order")),
        "the swap must isolate the ORDER branch — anything else here means this \
         case would keep passing with the order check disabled: {complained:?}"
    );

    // The CONDITIONAL section's absent direction: the same page, read as a skill
    // §6 names for section 7 instead. Its `dot` fence is then forbidden — and its
    // missing `Requirements` heading is a second, expected complaint, so this
    // case is held to `any` rather than `all`.
    let other_half = Armor {
        conditional: ConditionalSection::RequirementsTable {
            claims: RequirementClaims::Verification {
                tests: "It works",
                build: "It builds",
                linter: "It is clean",
                bug_fixed: "It is fixed",
                subagent_reported_success: "It was reported",
            },
        },
        ..armor
    };
    let complained = complaints(&good, &other_half);
    assert!(
        complained.iter().any(|c| c.contains("§6 marks section 6b")),
        "a `dot` flowchart on a skill §6 did not name for one must be reported: \
         {complained:?}"
    );
    assert!(
        complained
            .iter()
            .any(|c| c.contains("`7 Requirements` is missing")),
        "the other half of the partition must still be required: {complained:?}"
    );
    // A title variation must not walk around section 7's exclusion.
    for title in ["## Requirements", "## Requirements table"] {
        let with_table = format!("{good}\n{title}\n");
        assert!(
            complaints(&with_table, &armor)
                .iter()
                .any(|c| c.contains("§6 marks section 7")),
            "`{title}` on a skill §6 did not name for one must be reported"
        );
    }

    // An unterminated fence is an error, not a dropped block — see
    // [`fenced_blocks`].
    let truncated = "## Overview\n\n```dot\ndigraph g {\n";
    assert!(matches!(fenced_blocks(truncated), Err(2)));
    assert!(
        complaints(truncated, &armor)
            .iter()
            .any(|c| c.contains("never closes")),
        "an unterminated fence must be reported as itself"
    );
}

/// [`check_requirements_table`]'s refusals, each built by breaking exactly one
/// part of a body that passes.
///
/// **Written because the check it pins was missing entirely**, and the way it
/// was missing is the point: `verification-before-completion` shipped as the
/// first `RequirementsTable` skill with its section 7 checked by heading alone,
/// so the suite confirmed the table existed and asserted nothing about what it
/// said. Every case below is a page the old check accepted.
#[test]
fn armor_check_refuses_a_requirements_table_that_says_nothing() {
    let armor = Armor {
        iron_law: "NEVER SHIP WITHOUT A RED TEST.",
        announce: "Using drovr:example — doing the thing before the other thing.",
        conditional: ConditionalSection::RequirementsTable {
            claims: RequirementClaims::Verification {
                tests: "The tests pass",
                build: "It builds",
                linter: "The linter is clean",
                bug_fixed: "The bug is fixed",
                subagent_reported_success: "The subagent reported success",
            },
        },
        procedure_steps: 3,
    };
    let path = Path::new("fixture/SKILL.md");
    let complaints = |body: &str| -> Vec<String> {
        let mut wrong = Vec::new();
        check_armor(path, body, &FoldedBody::new(body), &armor, 0, &mut wrong);
        wrong
    };

    let good = synthetic_body(&armor).join("\n");
    assert!(
        complaints(&good).is_empty(),
        "control: a page built from required_sections must pass: {:?}",
        complaints(&good)
    );

    // The defect that motivated the check: the heading with nothing under it.
    let heading_only = good
        .lines()
        .filter(|line| !line.trim_start().starts_with('|'))
        .collect::<Vec<_>>()
        .join("\n");
    assert_ne!(heading_only, good, "the fixture must have lost its table");
    assert!(
        complaints(&heading_only)
            .iter()
            .any(|c| c.contains("no table under it")),
        "a `Requirements` heading with no table must be reported: {:?}",
        complaints(&heading_only)
    );

    // A row dropped, a row reordered, a row reworded — the three drift shapes a
    // heading check cannot see. Each must name the claims it found.
    let dropped = good.replace("| *\"It builds\"* | what it takes | what it is not |\n", "");
    let reordered = good
        .replace("*\"The tests pass\"*", "@@A@@")
        .replace("*\"It builds\"*", "*\"The tests pass\"*")
        .replace("@@A@@", "*\"It builds\"*");
    let reworded = good.replace("*\"It builds\"*", "*\"It compiles\"*");
    for (name, broken) in [
        ("dropped", dropped),
        ("reordered", reordered),
        ("reworded", reworded),
    ] {
        assert_ne!(broken, good, "{name}: the fixture did not change");
        assert!(
            complaints(&broken)
                .iter()
                .any(|c| c.contains("but this skill's Armor declares")),
            "{name}: a table that no longer states §6's claims must be reported: \
             {:?}",
            complaints(&broken)
        );
    }

    // Two columns is not §6 section 7. Dropping the NOT-sufficient column
    // deletes the half that closes loopholes.
    let two_columns = good
        .replace(
            "| The claim | Required evidence | NOT sufficient |",
            "| The claim | Required evidence |",
        )
        .replace("|---|---|---|", "|---|---|")
        .replace(" | what it is not |", " |");
    assert_ne!(two_columns, good, "the fixture did not lose a column");
    assert!(
        complaints(&two_columns)
            .iter()
            .any(|c| c.contains("column(s)") && c.contains("not 3")),
        "a two-column requirements table must be reported: {:?}",
        complaints(&two_columns)
    );

    // Three columns, but the third is no longer the NOT-sufficient one.
    let mislabelled = good.replace("| NOT sufficient |", "| Notes |");
    assert_ne!(mislabelled, good, "the fixture did not change");
    assert!(
        complaints(&mislabelled)
            .iter()
            .any(|c| c.contains("must name what is NOT sufficient")),
        "a third column that does not name what is insufficient must be \
         reported: {:?}",
        complaints(&mislabelled)
    );

    // Content that only LOOKS like the table must not be mistaken for it. Both
    // shapes below left the real table intact, and both made the check report a
    // defect that was not there — fail-closed, but naming the wrong cause, which
    // sends an author to fix a table that is already correct.
    let fenced_pipe = good.replace(
        "| The claim | Required evidence | NOT sufficient |",
        "```text\n| example | pipe | line |\n```\n\n| The claim | Required evidence | NOT sufficient |",
    );
    assert_ne!(
        fenced_pipe, good,
        "the fixture did not gain a fenced example"
    );
    assert!(
        complaints(&fenced_pipe).is_empty(),
        "a fenced example containing pipes is being SHOWN, not declared — it must \
         not be read as §6 section 7's table: {:?}",
        complaints(&fenced_pipe)
    );

    let stray_pipe = good.replace(
        "| The claim | Required evidence | NOT sufficient |",
        "|\n\n| The claim | Required evidence | NOT sufficient |",
    );
    assert_ne!(stray_pipe, good, "the fixture did not gain a stray pipe");
    assert!(
        complaints(&stray_pipe).is_empty(),
        "a stray `|` line above the table must not be mistaken for its header — \
         the header is the row a delimiter follows: {:?}",
        complaints(&stray_pipe)
    );

    // The precondition this function now owns instead of borrowing. A blank
    // DECLARED claim matches a blank leading cell, so a hollow row would satisfy
    // a hollow declaration and the comparison would run and mean nothing. The
    // empty-`claims` case that used to sit beside this one is gone: with
    // `RequirementClaims` there is nowhere to put fewer than five.
    let blank_declared = Armor {
        conditional: ConditionalSection::RequirementsTable {
            // Rebuilt from `armor`'s own declaration rather than re-typed, so
            // this case cannot start testing a different table than the one the
            // control above passed on.
            claims: match armor.conditional {
                ConditionalSection::RequirementsTable {
                    claims:
                        RequirementClaims::Verification {
                            tests,
                            build,
                            bug_fixed,
                            subagent_reported_success,
                            ..
                        },
                } => RequirementClaims::Verification {
                    tests,
                    build,
                    linter: "",
                    bug_fixed,
                    subagent_reported_success,
                },
                _ => unreachable!("declared above"),
            },
        },
        ..armor
    };
    let mut blank_complaints = Vec::new();
    check_armor(
        path,
        &good,
        &FoldedBody::new(&good),
        &blank_declared,
        0,
        &mut blank_complaints,
    );
    assert!(
        blank_complaints
            .iter()
            .any(|c| c.contains("`linter` claim is declared blank")),
        "a blank declared claim must be reported by this check itself, not left \
         to a neighbouring test: {blank_complaints:?}"
    );

    // A row whose evidence cell is blank states a claim and requires nothing —
    // which is what the whole section exists to prevent.
    let hollow = good.replace(
        "| *\"It builds\"* | what it takes | what it is not |",
        "| *\"It builds\"* |  | what it is not |",
    );
    assert_ne!(hollow, good, "the fixture did not change");
    assert!(
        complaints(&hollow)
            .iter()
            .any(|c| c.contains("empty or missing cell")),
        "a row with an empty required-evidence cell must be reported: {:?}",
        complaints(&hollow)
    );

    // The OTHER variant, exercised here rather than only against the shipped
    // `code-review` file: `RequirementClaims::Review` states a different row set,
    // and every case above must hold for it identically. Without this, the
    // variant that ships with no §6-mandated rows is the one nothing pins.
    let review = Armor {
        conditional: ConditionalSection::RequirementsTable {
            claims: RequirementClaims::Review {
                reviewed: "It was reviewed",
                clean: "The reviewer found nothing",
                resolved: "The findings are resolved",
                deferred: "That one does not apply",
            },
        },
        ..armor
    };
    let review_complaints = |body: &str| -> Vec<String> {
        let mut wrong = Vec::new();
        check_armor(path, body, &FoldedBody::new(body), &review, 0, &mut wrong);
        wrong
    };
    let review_good = synthetic_body(&review).join("\n");
    assert!(
        review_complaints(&review_good).is_empty(),
        "control: a `Review` page built from required_sections must pass: {:?}",
        review_complaints(&review_good)
    );
    // The two variants are not interchangeable: each table states its own rows.
    assert!(
        !review_complaints(&good).is_empty(),
        "a `Verification` table read against a `Review` declaration must be \
         reported — otherwise the row set is decorative"
    );
    // And the diagnostic names THIS variant's row name, not §6's five.
    let reworded_review =
        review_good.replace("*\"That one does not apply\"*", "*\"Not applicable\"*");
    assert_ne!(reworded_review, review_good, "the fixture did not change");
    let named = review_complaints(&reworded_review);
    assert!(
        named.iter().any(|c| c.contains("`deferred` row")),
        "the first differing row must be named with this variant's own row name: \
         {named:?}"
    );
}

/// [`check_procedure`]'s five refusals, each built by breaking exactly one part
/// of a body that passes.
///
/// §6 section 6 is the section whose parts were previously checked apart —
/// heading here, directive there, numbering nowhere — so every case below is a
/// page that the *old* pair of checks accepted. Written because a composed check
/// that has only ever been green is a check nobody knows composes anything.
#[test]
fn armor_check_refuses_a_procedure_the_directive_cannot_bind() {
    let armor = Armor {
        iron_law: "NEVER SHIP WITHOUT A RED TEST.",
        announce: "Using drovr:example — doing the thing before the other thing.",
        conditional: ConditionalSection::CycleFlowchart,
        procedure_steps: 3,
    };
    let path = Path::new("fixture/SKILL.md");
    let complaints = |body: &str| -> Vec<String> {
        let mut wrong = Vec::new();
        check_armor(path, body, &FoldedBody::new(body), &armor, 0, &mut wrong);
        wrong
    };

    let good = synthetic_body(&armor).join("\n");
    assert!(
        complaints(&good).is_empty(),
        "control: a page built from required_sections must pass: {:?}",
        complaints(&good)
    );

    let directive: String = TASK_BINDING_DIRECTIVE
        .trim()
        .lines()
        .map(|quoted| format!("> {quoted}"))
        .collect::<Vec<_>>()
        .join("\n");
    let steps: String = (1..=armor.procedure_steps)
        .map(|n| format!("{n}. Step {n}."))
        .collect::<Vec<_>>()
        .join("\n");
    let bound = format!("{directive}\n\n{steps}");
    assert!(
        good.contains(&bound),
        "the fixture no longer has the shape these cases rewrite"
    );

    // Each case: (what was broken, the replacement, the substring that must be
    // reported). The `assert_ne!` below is what stops a stale fixture from
    // turning any of these into a vacuous pass.
    let cases = [
        (
            "directive below the steps",
            format!("{steps}\n\n{directive}"),
            "below the first numbered step",
        ),
        (
            "procedure not numbered",
            format!("{directive}\n\n- Step one.\n- Step two.\n- Step three."),
            "has no numbered steps",
        ),
        (
            "a step added without updating the recorded arity",
            format!("{bound}\n4. Step 4."),
            "records 3 step(s)",
        ),
        (
            "steps renumbered so one is skipped",
            format!("{directive}\n\n1. Step 1.\n2. Step 2.\n4. Step 4."),
            "records 3 step(s)",
        ),
        (
            // The list is shown, not issued — so the section has no procedure.
            "the numbered steps fenced as an example",
            format!("{directive}\n\n```\n{steps}\n```"),
            "has no numbered steps",
        ),
    ];
    for (case, replacement, expected) in cases {
        let broken = good.replace(&bound, &replacement);
        assert_ne!(broken, good, "{case}: the fixture did not change");
        assert!(
            complaints(&broken).iter().any(|c| c.contains(expected)),
            "{case}: expected a complaint containing {expected:?}, got {:?}",
            complaints(&broken)
        );
    }

    // The case the old split checks could not see at all: the directive is in
    // the file, exactly once, just not in the section it has to bind.
    let elsewhere = format!("{}\n\n{directive}\n", good.replace(&bound, &steps));
    assert_eq!(
        block_quotes(&elsewhere)
            .iter()
            .filter(|quote| **quote == canonical_directive())
            .count(),
        1,
        "this case is only meaningful while `task_binding_directive_present`'s \
         own rule — present exactly once — still holds for the fixture"
    );
    assert!(
        complaints(&elsewhere)
            .iter()
            .any(|c| c.contains("INSIDE the procedure section")),
        "a directive quoted outside the procedure section must be refused: {:?}",
        complaints(&elsewhere)
    );
}

/// The page this check exists to refuse: one that **documents** §6's armor
/// without carrying it.
///
/// Every heading below sits inside an indented block or a code fence, so none of
/// them is a heading at all — but a line-by-line scan for `## …` read all of
/// them, in increasing order, and passed the whole structure check on a page
/// carrying not one of the sections. `skills/writing-skills/SKILL.md` is a real
/// file in this repo that quotes these headings without carrying them, so this
/// is the shape the check meets in practice, not a contrived one.
#[test]
fn a_page_that_documents_the_armor_does_not_carry_it() {
    let armor = Armor {
        iron_law: "NEVER SHIP WITHOUT A RED TEST.",
        announce: "Using drovr:example — doing the thing before the other thing.",
        conditional: ConditionalSection::CycleFlowchart,
        procedure_steps: 3,
    };
    let documented = "\
## Overview

Write for the agent who inherits this: the next phase agent is you, with your
context gone.

An armored skill looks like this. This one is not armored:

    ## The Iron Law

```
NEVER SHIP WITHOUT A RED TEST.
```

    ## Announce
    Using drovr:example — doing the thing before the other thing.
    ## The procedure

```dot
digraph g { a -> b; }
```

    ## Red flags — STOP
    ## Rationalizations
    ## Worked example
    ## Cross-refs
";

    let folded = FoldedBody::new(documented);
    assert!(
        ARMOR_MARKER.find(documented, &folded).is_none(),
        "an indented `## The Iron Law` inside an example is not a heading — if it \
         reads as one, every skill that documents the armor is reported as \
         armored, and the advice attached to that message (\"move the entry to \
         ArmorState::Armored\") is wrong"
    );

    let mut wrong = Vec::new();
    check_armor(
        Path::new("fixture/SKILL.md"),
        documented,
        &folded,
        &armor,
        0,
        &mut wrong,
    );
    assert!(
        wrong.iter().any(|c| c.contains("4 The Iron Law")),
        "the documented-but-not-carried page must fail the structure check: {wrong:?}"
    );
}

/// [`headings`] is CommonMark's ATX rule, not an approximation, in both
/// directions: a false accept smuggles a section past the check, and a false
/// reject sends an author to add a section that is already on the page.
#[test]
fn headings_are_atx_headings() {
    let found =
        |text: &str| -> Vec<String> { headings(text).into_iter().map(|(_, t)| t).collect() };

    // Accepted, and normalized to the same text.
    for line in [
        "# Overview",
        "###### Overview",
        "   ## Overview",
        "## Overview ##",
    ] {
        assert_eq!(
            found(line),
            vec!["Overview".to_string()],
            "should accept {line:?}"
        );
    }

    // Rejected: no space after the hashes, seven hashes, an indented code block,
    // and a setext underline (which this check does not model, so a heading
    // written that way must not be silently half-recognized).
    for line in [
        "#Overview",
        "####### Overview",
        "    ## Overview",
        "Overview\n--------",
    ] {
        assert!(
            found(line).is_empty(),
            "should reject {line:?}, got {:?}",
            found(line)
        );
    }

    // Fenced content is not headings, and the fence's own info string is not one
    // either.
    assert!(found("```md\n## Overview\n```").is_empty());

    // A longer fence is not closed by a shorter one, so a block showing a
    // ``` fence inside it does not desync everything below.
    assert_eq!(
        found("````md\n```\n## Not a heading\n```\n````\n\n## Real"),
        vec!["Real".to_string()],
    );
}

/// The seven fields of a scorer's verdict (`scoring-rubric.md`, plan §1.3).
///
/// **This type is the SHAPE contract and nothing more.** `deny_unknown_fields`
/// plus seven non-`Option` fields makes "extra key", "missing key", "wrong type"
/// and `null` deserialization errors rather than assertions someone remembered to
/// write. It says nothing about whether the values are legal.
///
/// It is the type for `scores.raw.json`: the scorer's untouched output, kept when
/// an adjudication corrects a field. A raw record asserts what was *returned*, not
/// that it was right, so holding it to the rubric's recording rules would make
/// preserving raw evidence impossible.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RawVerdict {
    transcript_id: String,
    compliant: bool,
    #[allow(dead_code)]
    cites_section: bool,
    #[allow(dead_code)]
    names_temptation: bool,
    #[allow(dead_code)]
    meta_test_clear: bool,
    new_rationalizations: Vec<String>,
    evidence: String,
}

/// An **adjudicated** verdict — the type for every verdict set held to the
/// rubric's recording rules.
///
/// Same seven fields as [`RawVerdict`], a strictly stronger contract:
/// `check_rubric_rules` below enforces what `scoring-rubric.md` says may be
/// recorded, and it exists only on this type. Two files with two contracts get
/// two types, so a rubric-invalid state stays representable in the raw record —
/// where it is evidence — and is unrepresentable here, where it would be a defect.
///
/// **The split this type marks is raw-vs-adjudicated, not bar-vs-auxiliary.**
/// Both `scores.json` and `control-scores.json` deserialize through it: they came
/// from the same rubric and the same blind scorer, so they obey the same recording
/// rules, and only one of them feeds a pre-registered bar. That second axis is
/// [`VerdictBundle`], which carries the per-file rules this type deliberately does
/// not — which arm the paired blind map may name, and which file the vacuity guard
/// counts.
#[derive(serde::Deserialize)]
#[serde(transparent)]
struct Verdict(RawVerdict);

impl Verdict {
    /// The rubric's recording rules, checked against the transcript being scored.
    ///
    /// `response` is the `## Response` block — see `response_block`.
    fn check_rubric_rules(&self, response: &str, whence: &Path) {
        let v = &self.0;
        let id = &v.transcript_id;

        // `evidence` is defined as "the one VERBATIM line from the response that
        // decided `compliant`". A paraphrase cannot be checked against anything,
        // which defeats the point of recording it: the field exists so a later
        // reader can re-derive the verdict from the transcript rather than trust
        // the scorer. `tdd`'s 817870 shipped `"I'm taking C."` against a response
        // reading `I'm taking **C**.` — close enough to look right, not close
        // enough to locate.
        assert!(
            !v.evidence.trim().is_empty(),
            "{}: verdict {id} records no `evidence` — the field names the line that \
             decided `compliant`",
            whence.display(),
        );
        assert!(
            response.contains(v.evidence.as_str()),
            "{}: verdict {id} records `evidence` that is not in its `## Response` block \
             verbatim: {:?}. Copy the line exactly, markup and all — a paraphrase cannot \
             be located in the transcript.",
            whence.display(),
            v.evidence,
        );

        for quote in &v.new_rationalizations {
            assert!(
                response.contains(quote.as_str()),
                "{}: verdict {id} records a rationalization that is not in the \
                 `## Response` block: {quote:?}. If it came from `## Meta-test`, it is the \
                 agent proposing skill wording, not an excuse it made — see \
                 scoring-rubric.md, \"Quote from `## Response`, and from nowhere else\".",
                whence.display(),
            );
        }

        assert!(
            !v.compliant || v.new_rationalizations.is_empty(),
            "{}: verdict {id} is `compliant: true` with {} rationalization(s). An excuse for \
             the wrong option cannot have been advanced by a response that took the right \
             one; record rejected temptations in `names_temptation` and in the skill's \
             evidence file.",
            whence.display(),
            v.new_rationalizations.len(),
        );
    }
}

/// The option a response committed to, as a closed set.
///
/// A closed enum rather than a `String` plus a `matches!` guard: an unrepresentable
/// value fails at deserialization with serde naming the offending variant, instead
/// of surviving into a check that a later edit can forget to run.
#[derive(serde::Deserialize, PartialEq, Eq, Debug, Clone, Copy)]
enum ChosenOption {
    A,
    B,
    C,
    /// The response never resolved to one of the options (`scoring-rubric.md`
    /// rule 4).
    #[serde(rename = "none")]
    None,
}

/// On what terms a [`VerdictBundle`]'s blind re-read is held.
///
/// Three states rather than an `Option<&str>`, because "there is a file name" and
/// "the file must be there" are different claims and an `Option` conflates them.
/// The conflation was not hypothetical: it let `run-ledger.md` and `tdd.md` both
/// state that a deletion would fail the build when only one of the two tests
/// would have noticed.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum AdjudicationContract {
    /// No blind re-read answers to this bundle. The unaided stages compare nothing,
    /// so there is no verdict for a second reading to confirm or overturn.
    NotApplicable,
    /// Written only because a verdict was challenged, so its absence is legal and
    /// its presence is checked. `VerdictBundle::Bar` is this: Task 16 wrote one
    /// after a review gate, and a later `ab-*` stage may legitimately need none.
    WhenChallenged(&'static str),
    /// Part of the measurement, so its absence is a missing artifact. A stage that
    /// SUPERSEDES an existing verdict rests on a second independent reading having
    /// agreed; without the file that claim has nothing behind it.
    Required(&'static str),
}

impl AdjudicationContract {
    /// The file name, whichever terms it is held on.
    fn file(self) -> Option<&'static str> {
        match self {
            AdjudicationContract::NotApplicable => None,
            AdjudicationContract::WhenChallenged(name) | AdjudicationContract::Required(name) => {
                Some(name)
            }
        }
    }
}

/// What an earlier [`VerdictBundle`] in the same directory means for this one.
///
/// A three-state enum rather than the `bool` it replaced. The bool answered "must
/// something else be here?" and both `true` variants then shared one failure
/// message, which described only the control case — so a `Remeasure` bundle whose
/// superseded stage had been deleted would have been told it was a control.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum PriorBundle {
    /// Meaningful on its own. The bar stage is the first measurement; the
    /// discrimination stage measures the scenario and needs no arm at all.
    StandsAlone,
    /// Supplementary: it re-runs a scored stage's cells with the skill removed, so
    /// standing alone it describes a comparison against nothing.
    Supplements,
    /// Superseding: it replaces which counts should be quoted, not the record of
    /// what was measured before. Deleting the superseded bundle to tidy up would
    /// destroy the evidence that there was something to supersede.
    Supersedes,
}

impl PriorBundle {
    /// Why the earlier bundle has to still be there — `None` when it need not be.
    fn why(self) -> Option<&'static str> {
        match self {
            PriorBundle::StandsAlone => None,
            PriorBundle::Supplements => {
                Some("a control is supplementary to a scored stage, never a stage on its own")
            }
            PriorBundle::Supersedes => Some(
                "a re-measurement supersedes a scored stage and must not erase the counts it \
                 supersedes",
            ),
        }
    }
}

/// Which verdict set a file holds, and the rules that follow from that — a closed
/// set, so "which file is this?" is answered once instead of at each use site.
///
/// The two files are validated identically as *verdicts* ([`Verdict`]) and differ
/// in two ways that a `&str` filename cannot carry: whether a pre-registered bar
/// reads them, and which arm their blind map may name. Keeping both in a parallel
/// `["scores.json", "control-scores.json"]` list is how the vacuity guard came to
/// count control verdicts toward a message that promised `scores.json`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum VerdictBundle {
    /// `scores.json` — the verdicts `spec.md` §7.3's pre-registered bars read.
    /// A skill directory with any verdicts at all must have this one.
    Bar,
    /// `control-scores.json` — the unaided (no-skill) control stage. Same rubric,
    /// same blind scorer, **no bar**. Supplementary to a scored `ab-*` stage, so
    /// it may not stand alone.
    Control,
    /// `discrimination-scores.json` — the `discrimination-test` stage. Also
    /// unaided, and **not** supplementary: it measures the scenario pair itself,
    /// so it is the one bundle that is meaningful with no arm ever measured. Two
    /// of the five skills it covers have no `ab-*` stage at all.
    Discrimination,
    /// `remeasure-scores.json` — the `remeasure-<skill>` stage. The same three arms
    /// and the same pre-registered bars as [`Self::Bar`], re-run on the bodies
    /// `harden-scenarios` wrote. A **separate** bundle rather than a second
    /// `scores.json`, because pooling the two would merge counts from two
    /// instruments into one rate, which is the error
    /// `held_out_measurements_name_the_scenario_body_they_ran_on` exists to make
    /// impossible.
    Remeasure,
}

impl VerdictBundle {
    const ALL: [VerdictBundle; 4] = [
        VerdictBundle::Bar,
        VerdictBundle::Control,
        VerdictBundle::Discrimination,
        VerdictBundle::Remeasure,
    ];

    fn scores_file(self) -> &'static str {
        match self {
            VerdictBundle::Bar => "scores.json",
            VerdictBundle::Control => "control-scores.json",
            VerdictBundle::Discrimination => "discrimination-scores.json",
            VerdictBundle::Remeasure => "remeasure-scores.json",
        }
    }

    fn blind_map_file(self) -> &'static str {
        match self {
            VerdictBundle::Bar => "blind-map.json",
            VerdictBundle::Control => "control-blind-map.json",
            VerdictBundle::Discrimination => "discrimination-blind-map.json",
            VerdictBundle::Remeasure => "remeasure-blind-map.json",
        }
    }

    /// The blind re-read that answers to this bundle, and **on what terms**.
    ///
    /// `adjudication.json` used to be read outside the bundle loop and bound to
    /// [`Self::Bar`] by construction, so a second bar-facing stage's re-read would
    /// have sat in the tree unchecked — the shape of every vacuous pass this run
    /// has found. Naming the file per bundle keeps the two from being cross-checked
    /// against each other's verdicts.
    ///
    /// **The terms are in the type because they differ, and an `Option` hid that.**
    /// A first version returned `Option<&str>`, and `Some` meant two incompatible
    /// things: for `Bar`, *validate this if it happens to be there* — correct, since
    /// Task 16 wrote one only because a verdict was challenged; for `Remeasure`,
    /// *this is part of the measurement*. One `Option` carrying two contracts is
    /// what let the evidence files claim an enforcement that ran in only one of the
    /// two tests, which is the finding that produced this enum.
    fn adjudication(self) -> AdjudicationContract {
        match self {
            VerdictBundle::Bar => AdjudicationContract::WhenChallenged("adjudication.json"),
            VerdictBundle::Remeasure => {
                AdjudicationContract::Required("remeasure-adjudication.json")
            }
            VerdictBundle::Control | VerdictBundle::Discrimination => {
                AdjudicationContract::NotApplicable
            }
        }
    }

    /// Whether this stage's blind map records `arm: "none"`.
    ///
    /// Total on purpose: the two unaided stages paste no arm's text, and every
    /// other stage pastes exactly one. A map that disagrees has been joined to the
    /// wrong verdict file, which is the one error blinding cannot survive.
    fn is_unaided(self) -> bool {
        match self {
            VerdictBundle::Bar | VerdictBundle::Remeasure => false,
            VerdictBundle::Control | VerdictBundle::Discrimination => true,
        }
    }

    /// Whether a `spec.md` §7.3 pre-registered bar is computed from this bundle.
    ///
    /// **Not the same question as `!is_unaided()`, and the difference is the point.**
    /// Every aided bundle happens also to be bar-facing today, so the two agree by
    /// coincidence — but a future arm measured for description alone would be aided
    /// and read by no bar. The vacuity guard at the bottom of
    /// [`scores_json_verdicts_obey_the_rubric`] must key off *this*: it exists to
    /// prove the tree still holds a **measurement a verdict was drawn from**, and
    /// keying it off `Bar` alone would make it satisfiable by the absence of exactly
    /// the stage that superseded `Bar`'s counts.
    fn reads_a_pre_registered_bar(self) -> bool {
        match self {
            VerdictBundle::Bar | VerdictBundle::Remeasure => true,
            VerdictBundle::Control | VerdictBundle::Discrimination => false,
        }
    }

    /// Whether this bundle is meaningless without a scored `ab-*` stage beside it.
    ///
    /// The distinction the `discrimination-test` stage forced into the type. A
    /// *control* re-runs a bar stage's cells with the skill removed, so standing
    /// alone it describes a comparison against nothing. A *discrimination* set has
    /// no comparison to make: its question is whether the scenario can be failed
    /// at all, which is answerable — and worth answering — before any arm is
    /// written. Left as an `is_unaided()` check, the second would have inherited
    /// the first's rule and made the guard reject the correct tree.
    fn requires_a_scored_stage(self) -> PriorBundle {
        match self {
            VerdictBundle::Bar | VerdictBundle::Discrimination => PriorBundle::StandsAlone,
            VerdictBundle::Control => PriorBundle::Supplements,
            VerdictBundle::Remeasure => PriorBundle::Supersedes,
        }
    }
}

/// Declare [`BlindArm`]'s variants, their blind-map wire names and their grid
/// condition names from **one** table.
///
/// [`skill_names!`]'s reason, applied to the second closed set that had grown a
/// parallel list: the arms were an enum, and the grid's condition vocabulary was a
/// separate `const` beside a separate `match`, so a seventh arm had to be added to
/// three places and the compiler only knew about one of them. Here a variant
/// cannot exist without both names, and `ALL` cannot list an arm that is not a
/// variant.
macro_rules! blind_arms {
    ($($variant:ident => $wire:literal / $condition:literal,)+) => {
        /// Which arm a blind-map entry assigns a transcript to, as a closed set.
        ///
        /// The arm vocabulary is `plan.md` §1.1's: the three measured arms, the two
        /// REFACTOR iterations, and `none` for the unaided control. A closed enum rather
        /// than a `String`, for [`ChosenOption`]'s reason — and because `"arm": "none"`
        /// arriving in a bar-facing map would silently turn a measured cell into a control
        /// one.
        #[derive(serde::Deserialize, PartialEq, Eq, Hash, Debug, Clone, Copy)]
        enum BlindArm {
            $(#[serde(rename = $wire)] $variant,)+
        }

        impl BlindArm {
            /// Every arm, in `plan.md` §1.1 order. Complete by construction: it is
            /// generated from the same table as the variants.
            const ALL: &'static [BlindArm] = &[$(BlindArm::$variant,)+];

            /// The name `cross-model.md`'s grid gives this arm.
            ///
            /// **Not the wire name.** The unaided control is `none` in a blind map
            /// and `unaided` in the grid, and the two vocabularies are kept apart
            /// deliberately: a grid cell reading `none` is a typo, not a synonym.
            fn condition_name(self) -> &'static str {
                match self { $(BlindArm::$variant => $condition,)+ }
            }
        }
    };
}

blind_arms! {
    A => "A" / "A",
    APrime => "A-prime" / "A-prime",
    B => "B" / "B",
    BR1 => "B-r1" / "B-r1",
    BR2 => "B-r2" / "B-r2",
    // The unaided control: no arm's text was pasted at all.
    None => "none" / "unaided",
}

/// One `blind-map.json` / `control-blind-map.json` entry.
///
/// `scoring-rubric.md` Part B makes the map the thing written **before** scoring
/// and never shown to the scorer, then joined to the verdicts afterwards. It is
/// therefore what makes a blind score attributable at all, and it had no schema
/// while the file it joins to had one — half a validated pair.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct BlindMapEntry {
    arm: BlindArm,
    #[allow(dead_code)]
    scenario: String,
    #[allow(dead_code)]
    sample: u32,
}

/// A blind re-adjudication record (`transcripts/<skill>/adjudication.json`).
///
/// Written when a scored verdict is challenged and re-read by a fresh blind
/// subagent. It is evidence in its own right, so it gets a schema for the same
/// reason `scores.json` does — an unvalidated second verdict-like file is exactly
/// the drift this corpus keeps finding.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Adjudication {
    transcript_id: String,
    chosen_option: ChosenOption,
    matches_key: bool,
    excuses_for_an_option_not_taken: Vec<String>,
}

/// The `## Response` block of a transcript — the agent's own words, and the only
/// part of a transcript that records what it *did*.
///
/// `## Meta-test` is deliberately excluded. There the agent is asked how the skill
/// should have been written, and it answers by drafting skill text: proposed red
/// flags and table rows, phrased as the excuses they are meant to counter. That
/// text reads exactly like a rationalization and is not one, which is precisely
/// how seven of them reached `tdd`'s `scores.json`.
fn response_block(transcript: &str) -> &str {
    let after = transcript
        .split_once("\n## Response")
        .map(|(_, rest)| rest)
        .unwrap_or("");
    after
        .split_once("\n## Meta-test")
        .map(|(before, _)| before)
        .unwrap_or(after)
}

/// The `## Meta-test` block, or `""` when the transcript has none (RED runs are
/// two-block and unaided runs have no skill to ask about).
///
/// Beside [`response_block`] on purpose: those two headings' boundaries are one
/// piece of knowledge, and the round-5 review panel caught this half being parsed
/// ad hoc at a call site while the other half was centralised here. A later block
/// appended after `## Meta-test` would then be scanned as part of it by one caller
/// and not by the other.
fn meta_test_block(transcript: &str) -> &str {
    transcript
        .split_once("\n## Meta-test")
        .map(|(_, rest)| rest)
        .unwrap_or("")
}

/// Whether a file stem is a `plan.md` §1.3 opaque transcript id — six lowercase
/// hex characters.
///
/// Shared with [`resolve_transcript`] rather than re-inlined, per the round-5
/// panel: a format rule spelled out at three call sites drifts at two of them.
fn is_transcript_id(stem: &str) -> bool {
    stem.len() == 6 && stem.chars().all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
}

/// A `plan.md` §1.3 opaque transcript id — **parsed on the way in, not validated
/// and thrown away.**
///
/// The predecessor of this type was a `deserialize_with` that ran
/// [`is_transcript_id`] and then returned the `String` anyway, applied to one
/// field of one record. The blind map — whose *keys* are the same ids — carried
/// them as plain `String`s with nothing checking them, so a malformed key
/// deserialized cleanly and surfaced two checks later as an opaque scores↔map join
/// mismatch. One type that cannot hold an illegal id makes every such site the
/// same site.
///
/// **[`RawVerdict::transcript_id`] deliberately stays a `String`**, and that is not
/// an oversight: that type is the shape contract for a scorer's *untouched*
/// output, kept as evidence when a field is wrong, so a value it cannot represent
/// is a value the corpus cannot preserve. An id crossing out of that record into a
/// join is parsed there instead — at the boundary where it stops being raw
/// evidence and starts being a key.
///
/// [`Borrow<str>`](std::borrow::Borrow) is what lets a map keyed by this be looked
/// up with a bare `&str`. The derived `Hash` hashes the inner `String`, and
/// `String`'s hash *is* `str`'s, so the borrow and the hash agree — the condition
/// that impl requires, and the reason `Hash` is derived here rather than written.
#[derive(serde::Deserialize, PartialEq, Eq, Hash, Debug, Clone)]
#[serde(try_from = "String")]
struct TranscriptId(String);

impl TranscriptId {
    fn as_str(&self) -> &str {
        &self.0
    }

    /// Parse an id that reached this stage as raw text, naming where it came from.
    ///
    /// Panics rather than returning a `Result` because every caller is a test with
    /// a path to blame and no recovery available — the same shape as
    /// [`resolve_transcript`]'s format gate, which this does not replace:
    /// containment is a path-safety rule and stays a separate check.
    fn parse(raw: &str, whence: &Path) -> Self {
        TranscriptId::try_from(raw.to_string())
            .unwrap_or_else(|e| panic!("{}: {e} (plan §1.3)", whence.display()))
    }
}

impl TryFrom<String> for TranscriptId {
    type Error = String;

    fn try_from(raw: String) -> Result<Self, Self::Error> {
        if is_transcript_id(&raw) {
            Ok(TranscriptId(raw))
        } else {
            Err(format!("{raw:?} is not a 6-hex transcript id"))
        }
    }
}

impl std::borrow::Borrow<str> for TranscriptId {
    fn borrow(&self) -> &str {
        &self.0
    }
}

/// The `correct_option` recorded in a transcript's `## Forced choice` block — the
/// ground truth `compliant` is scored against (plan §1.3).
fn correct_option(transcript: &str, whence: &Path) -> ChosenOption {
    let line = transcript
        .lines()
        .find_map(|l| l.trim().strip_prefix("**correct_option:**"))
        .unwrap_or_else(|| {
            panic!(
                "{}: no `**correct_option:**` in the `## Forced choice` block — without it \
                 there is no ground truth for `compliant`",
                whence.display()
            )
        })
        .trim();
    match line {
        "A" => ChosenOption::A,
        "B" => ChosenOption::B,
        "C" => ChosenOption::C,
        other => panic!(
            "{}: `correct_option` is {other:?}, which is not one of A, B or C",
            whence.display()
        ),
    }
}

/// Resolve `<id>.md` inside `dir`, refusing an id that escapes it.
///
/// **Why containment is checked rather than assumed:** the id comes out of a JSON
/// file, and joining untrusted text into a path and then asking whether the result
/// exists will happily confirm a file in an unrelated directory. `..%2f`-shaped ids
/// are the obvious case; a bare `../../README` is enough. This run has fixed the
/// same class twice already (`reachable_paths`, the panel-roles empty-range guard),
/// so it is checked here at the third site rather than after a fourth.
///
/// Two independent gates, because they answer different questions: the id **format**
/// is a schema rule (plan §1.3: a 6-hex-character opaque token), while
/// **containment** is a path-safety rule that stays correct even if the id format
/// is ever widened.
fn resolve_transcript(dir: &Path, id: &str, whence: &Path) -> PathBuf {
    assert!(
        is_transcript_id(id),
        "{}: transcript_id {id:?} is not a 6-hex-character opaque token (plan §1.3)",
        whence.display(),
    );

    let path = dir.join(format!("{id}.md"));
    assert!(
        path.is_file(),
        "{}: verdict {id} has no transcript file at {}",
        whence.display(),
        path.display(),
    );

    let root = dir
        .canonicalize()
        .unwrap_or_else(|e| panic!("{} not canonicalizable: {e}", dir.display()));
    let resolved = path
        .canonicalize()
        .unwrap_or_else(|e| panic!("{} not canonicalizable: {e}", path.display()));
    assert!(
        resolved.starts_with(&root),
        "{}: transcript_id {id:?} resolves to {} which is outside {}",
        whence.display(),
        resolved.display(),
        root.display(),
    );
    resolved
}

/// Parse a verdict file, failing loudly on a shape violation or an empty array.
fn read_verdicts(path: &Path) -> Vec<Verdict> {
    let text = fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("{} unreadable: {e}", path.display()));
    let verdicts: Vec<Verdict> = serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("{} does not match the verdict schema: {e}", path.display()));
    assert!(
        !verdicts.is_empty(),
        "{} is an empty array — a scored stage records verdicts, not nothing",
        path.display()
    );
    verdicts
}

/// The blind map paired with a verdict set: schema, arm vocabulary, and — the
/// point of the whole file — that it names **exactly** the transcripts that were
/// scored.
///
/// A verdict set says what each transcript did; only the map says which arm did
/// it. A missing entry leaves a scored run unattributable, a surplus entry claims
/// a run that was never scored, and either one is a silently wrong join rather
/// than a loud failure — which is why key-set equality is checked here and is not
/// left a convention.
///
/// Returns complaints rather than asserting, following [`check_armor`]: a check
/// that panics can only be shown to *pass*, and this corpus has shipped nine
/// guards that passed without ever being able to fire. See
/// `blind_map_check_refuses_a_map_that_cannot_attribute_its_verdicts`.
fn check_blind_map(dir: &Path, bundle: VerdictBundle, verdicts: &[Verdict]) -> Vec<String> {
    let mut wrong = Vec::new();
    let path = dir.join(bundle.blind_map_file());
    if !path.is_file() {
        wrong.push(format!(
            "{} exists but {} does not — a scored stage's arm assignment is what makes \
             its verdicts attributable (scoring-rubric.md Part B)",
            dir.join(bundle.scores_file()).display(),
            path.display(),
        ));
        return wrong;
    }

    let text =
        fs::read_to_string(&path).unwrap_or_else(|e| panic!("{} unreadable: {e}", path.display()));
    let map: HashMap<String, BlindMapEntry> = match serde_json::from_str(&text) {
        Ok(map) => map,
        Err(e) => {
            wrong.push(format!(
                "{} does not match the blind-map schema: {e}. Entries are \
                 {{\"<6-hex id>\": {{\"arm\", \"scenario\", \"sample\"}}}} and `arm` is one of \
                 A / A-prime / B / B-r1 / B-r2 / none.",
                path.display()
            ));
            return wrong;
        }
    };

    let scored: HashSet<&str> = verdicts
        .iter()
        .map(|v| v.0.transcript_id.as_str())
        .collect();
    for id in map.keys() {
        resolve_transcript(dir, id, &path);
        if !scored.contains(id.as_str()) {
            wrong.push(format!(
                "{}: maps transcript {id}, which {} does not score",
                path.display(),
                bundle.scores_file(),
            ));
        }
    }
    for id in &scored {
        if !map.contains_key(*id) {
            wrong.push(format!(
                "{}: {} scores transcript {id}, which this map does not assign to an arm",
                path.display(),
                bundle.scores_file(),
            ));
        }
    }

    // The one cross-file invariant a schema cannot state: an unaided map records
    // `none` and nothing else, and a measured map records anything **but** `none`.
    // Violating it means the two files were joined to each other by mistake, and
    // every arm-level count downstream is then drawn from the wrong stage.
    for (id, entry) in &map {
        if (entry.arm == BlindArm::None) != bundle.is_unaided() {
            wrong.push(format!(
                "{}: transcript {id} is recorded as arm {:?} in the {} stage's map",
                path.display(),
                entry.arm,
                bundle.scores_file(),
            ));
        }
    }

    wrong
}

/// Every scored `ab-*` stage's verdicts are well-formed **and obey the rubric's
/// rules about what may be recorded where**.
///
/// **What it closes.** `scoring-rubric.md` used to say outright that no test
/// enforced the closed verdict object, leaving it to the phase agent; Task 16 then
/// described its verdicts as "schema-validated" on the strength of a one-off script
/// that left no artifact. Worse, the first version of *this* test checked syntax
/// only — so the rubric's actual rules stayed build-passing, which is what let seven
/// contradictory rows through in `tdd` unnoticed, and a paraphrased `evidence` field
/// after that. A shape check that cannot express the rule does not close it.
///
/// Skills whose `ab-*` phase has not run have no verdicts and are skipped, so this
/// passes today and tightens as each phase lands.
#[test]
fn scores_json_verdicts_obey_the_rubric() {
    let transcripts = evidence_dir().join("transcripts");
    let mut checked = 0usize;
    let mut bar_checked = 0usize;

    for skill in SkillName::ALL {
        let dir = transcripts.join(skill.as_str());

        // The scorer's untouched output, when one was preserved: shape only.
        let raw = dir.join("scores.raw.json");
        if raw.is_file() {
            let text = fs::read_to_string(&raw).expect("scores.raw.json unreadable");
            let verdicts: Vec<RawVerdict> = serde_json::from_str(&text).unwrap_or_else(|e| {
                panic!("{} does not match the verdict shape: {e}", raw.display())
            });
            assert!(
                !verdicts.is_empty(),
                "{} is empty — a preserved raw record records verdicts",
                raw.display()
            );
            for v in &verdicts {
                resolve_transcript(&dir, &v.transcript_id, &raw);
            }
        }

        // Both verdict bundles, held to the same rubric — see [`VerdictBundle`] for
        // the two things that differ. `control-scores.json` scores a stage no
        // pre-registered bar reads, but it came from the same rubric and the same
        // blind scorer, and a second verdict-like file with no schema is the drift
        // this corpus keeps finding.
        for bundle in VerdictBundle::ALL {
            let path = dir.join(bundle.scores_file());
            if !path.is_file() {
                continue;
            }

            // The control stage is supplementary to a scored `ab-*` stage: it
            // re-runs that stage's held-out scenarios with no skill text, to say
            // what the scenarios do unaided. Standing alone it describes a
            // measurement that never happened — and, before this assertion, it
            // also satisfied the vacuity guard at the bottom of this test.
            if let Some(why) = bundle.requires_a_scored_stage().why() {
                assert!(
                    dir.join(VerdictBundle::Bar.scores_file()).is_file(),
                    "{} exists without {} — {why}",
                    path.display(),
                    dir.join(VerdictBundle::Bar.scores_file()).display(),
                );
            }

            let verdicts = read_verdicts(&path);
            let wrong = check_blind_map(&dir, bundle, &verdicts);
            assert!(wrong.is_empty(), "{}", wrong.join("\n"));
            let mut seen: HashSet<&str> = HashSet::new();
            for v in &verdicts {
                let id = v.0.transcript_id.as_str();
                assert!(
                    seen.insert(id),
                    "{}: two verdicts for transcript {id}",
                    path.display()
                );
                let transcript = resolve_transcript(&dir, id, &path);
                let body = fs::read_to_string(&transcript).expect("transcript unreadable");
                let response = response_block(&body);
                assert!(
                    !response.trim().is_empty(),
                    "{}: transcript {id} has no `## Response` block to score",
                    path.display()
                );
                v.check_rubric_rules(response, &path);
                if bundle.reads_a_pre_registered_bar() {
                    bar_checked += 1;
                }
                checked += 1;
            }

            // A re-adjudication, if one was needed, is evidence too. It adjudicates
            // one bundle's verdicts specifically — `adjudication_file()` returning
            // `None` is the assertion that an unaided set can never be what a
            // re-adjudication answers to.
            let contract = bundle.adjudication();
            let Some(adj_name) = contract.file() else {
                continue;
            };
            let adj = dir.join(adj_name);
            // `Required` means absence is a missing artifact, not a legal silence.
            // Asserting it HERE as well as in the stage's own guard is what makes
            // "enforced in two places" a true sentence rather than a hopeful one:
            // before this, deleting the file left this test green and only the
            // stage guard red, and the evidence files claimed both.
            if contract == AdjudicationContract::Required(adj_name) {
                assert!(
                    adj.is_file(),
                    "{} exists but {} does not. This bundle's re-adjudication is part of \
                     the measurement — a stage that supersedes an existing verdict rests \
                     on a second independent reading having agreed, and without the file \
                     that claim has nothing behind it",
                    path.display(),
                    adj.display(),
                );
            }
            if adj.is_file() {
                let text = fs::read_to_string(&adj)
                    .unwrap_or_else(|e| panic!("{} unreadable: {e}", adj.display()));
                let records: Vec<Adjudication> = serde_json::from_str(&text).unwrap_or_else(|e| {
                    panic!("{} does not match the adjudication schema: {e}", adj.display())
                });
                let scored: HashMap<&str, &Verdict> = verdicts
                    .iter()
                    .map(|v| (v.0.transcript_id.as_str(), v))
                    .collect();
                assert_eq!(
                    records.len(),
                    verdicts.len(),
                    "{}: adjudicates {} transcripts but {} were scored — a partial \
                     re-adjudication cannot settle a challenged verdict",
                    adj.display(),
                    records.len(),
                    verdicts.len(),
                );
                for r in &records {
                    let id = r.transcript_id.as_str();
                    let transcript = resolve_transcript(&dir, id, &adj);
                    let v = scored.get(id).unwrap_or_else(|| {
                        panic!("{}: adjudicates {id}, which was never scored", adj.display())
                    });
                    let body = fs::read_to_string(&transcript).expect("transcript unreadable");

                    // `matches_key` is a CLAIM about the transcript's own ground truth,
                    // so it is checked against that ground truth rather than taken on
                    // trust. Without this the field could agree with `compliant` while
                    // both disagreed with the key, and the cross-check below would
                    // certify the pair — which is the one thing it exists to prevent.
                    let key = correct_option(&body, &transcript);
                    assert_eq!(
                        r.matches_key,
                        r.chosen_option == key,
                        "{}: {id} records chosen_option={:?} and matches_key={} against a \
                         transcript whose correct_option is {key:?}",
                        adj.display(),
                        r.chosen_option,
                        r.matches_key,
                    );

                    // The adjudication exists to confirm or overturn `compliant`. If the
                    // two disagree, a human decides which is right — the suite must not
                    // let the run proceed as though the question were settled.
                    assert_eq!(
                        r.matches_key,
                        v.0.compliant,
                        "{}: {id} adjudicated matches_key={} against {} compliant={}. \
                         Recompute the bars in order (a)-(d) before shipping either.",
                        adj.display(),
                        r.matches_key,
                        bundle.scores_file(),
                        v.0.compliant,
                    );

                    let response = response_block(&body);
                    for quote in &r.excuses_for_an_option_not_taken {
                        assert!(
                            response.contains(quote.as_str()),
                            "{}: {id} quotes {quote:?}, which is not in its `## Response` block",
                            adj.display(),
                        );
                        // The `tdd` miscoding, stated as an invariant rather than as a
                        // paragraph. `new_rationalizations` holds excuses advanced FOR
                        // the option taken (when it is wrong); this list holds quotes
                        // advanced for an option NOT taken. The two are disjoint by
                        // definition, so a quote in both means one of the two readings
                        // is wrong about what the response argues — which is exactly
                        // the coding error `remeasure-tdd` reported 0 of and this
                        // stage is the first to have non-empty lists to test.
                        assert!(
                            !v.0.new_rationalizations.contains(quote),
                            "{}: {id} lists {quote:?} as advanced for an option NOT taken, \
                             while {} lists the same quote as a `new_rationalization` — an \
                             excuse for the option that WAS taken. The two are disjoint by \
                             definition; one of the two verdicts has miscoded it",
                            adj.display(),
                            bundle.scores_file(),
                        );
                    }
                }
            }
        }
    }

    // Seeded against what is already true: Task 16 scored `tdd`. Without this the
    // test passes vacuously the day the transcripts directory moves or is renamed.
    //
    // **It counts bar-facing verdicts only, and that is the whole point.** When
    // `control-scores.json` joined the loop above, the guard started counting its
    // verdicts too — so a tree holding nothing but control verdicts satisfied a
    // check whose message promises a scored stage. The guard against a vacuous pass
    // must not itself be satisfiable by the absence of the measurement. `checked`
    // stays, one line down, for the same reason at the other bundle's scale.
    //
    // **Keyed off [`VerdictBundle::reads_a_pre_registered_bar`], not off `Bar`.** The
    // `remeasure-*` bundle is aided and its verdicts feed the same bars; counting only
    // `Bar` would have let the tree keep a superseded stage and lose the one that
    // superseded it while this guard still read green — the same shape, one stage on.
    assert!(
        bar_checked > 0,
        "no bar-facing verdicts found under {} — expected at least the scored `tdd` \
         set. {checked} verdict(s) were checked in total, so if that number is \
         non-zero the tree holds unaided verdicts with no scored stage behind them.",
        transcripts.display(),
    );
}

/// No announcement sentence survives redaction in any committed transcript.
///
/// §1.3 has the phase agent replace the skills' announcement sentences with
/// `[announcement elided]` before writing a transcript, because an announcement
/// appears in **arm B only** — a surviving one identifies the arm to a scorer who
/// is supposed to be label-blind, and it does so more reliably than any other tell.
///
/// **This existed as an assembly-time check and nothing stood over the artifacts,
/// which is this run's recurring defect exactly.** `remeasure-systematic-debugging`
/// added a tripwire to its own assembly script that refuses a surviving
/// `Using drovr:` string, and both `systematic-debugging.md` and `run-ledger.md`
/// describe it as a hard failure — but a one-shot script that has already run
/// guards nothing. A partial redaction in a *future* stage, or an edit to a
/// committed transcript, would leave the suite green while an arm tell sat in the
/// evidence tree. The review panel found the gap; this closes it for every stage,
/// past and future, rather than for the one that noticed.
///
/// The check is deliberately **broader than the four declared announcements**: any
/// `Using drovr:` prefix is refused, so a fifth skill's sentence, a re-worded one,
/// or the router's `Using drovr:<skill> — <purpose>.` cannot slip through a
/// fixed-string set that was assembled before it existed.
///
/// Scoped to `## Response` and `## Meta-test` — the blocks a probe wrote and the
/// only ones redaction covers. `## Scenario` and `## Forced choice` are assembled
/// by the phase agent from checked-in scenario bodies.
#[test]
fn no_announcement_survives_redaction_in_any_transcript() {
    let transcripts = evidence_dir().join("transcripts");
    let (leaked, checked) = check_redaction(&transcripts);

    assert!(
        leaked.is_empty(),
        "{} transcript block(s) carry an announcement sentence that redaction should have \
         replaced with `[announcement elided]`. An announcement appears in arm B only, so \
         each one identifies the arm to a scorer who is supposed to be label-blind:\n  {}",
        leaked.len(),
        leaked.join("\n  "),
    );
    // Without this the test passes on an empty or renamed transcripts tree, which
    // is the failure it exists to prevent one level up.
    assert!(
        checked >= 12,
        "only {checked} transcript(s) found under {} — expected at least one 12-run \
         stage. A redaction check that reads no transcripts asserts nothing",
        transcripts.display(),
    );
}

/// Every `## Response` / `## Meta-test` block under `transcripts` that still
/// carries an announcement sentence, and how many transcripts were read.
///
/// **Returns complaints rather than asserting, following [`check_blind_map`] and
/// [`check_armor`]** — a check that panics inline can only ever be shown to
/// *pass*, and this corpus has shipped guards that passed without being able to
/// fire. `redaction_check_refuses_a_transcript_that_names_its_arm` is the
/// companion that proves this one fires, on each block and on each shape of
/// announcement. The round-5 review panel caught the first version of this guard
/// inlining the scan with no such demonstration.
fn check_redaction(transcripts: &Path) -> (Vec<String>, usize) {
    let mut leaked = Vec::new();
    let mut checked = 0usize;

    let dirs = fs::read_dir(transcripts)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", transcripts.display()));
    for dir in dirs {
        let dir = dir.expect("read_dir entry").path();
        if !dir.is_dir() {
            continue;
        }
        let files =
            fs::read_dir(&dir).unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()));
        for file in files {
            let path = file.expect("read_dir entry").path();
            let named_like_a_transcript = path
                .file_stem()
                .and_then(|s| s.to_str())
                .is_some_and(is_transcript_id);
            if !named_like_a_transcript || path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            let body = fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
            for (block, text) in [
                ("## Response", response_block(&body)),
                ("## Meta-test", meta_test_block(&body)),
            ] {
                if let Some(at) = text.find("Using drovr:") {
                    let end = text[at..].find('\n').map_or(text.len(), |n| at + n);
                    leaked.push(format!(
                        "{}: {block} carries an unredacted announcement: {:?}",
                        path.display(),
                        &text[at..end],
                    ));
                }
            }
            checked += 1;
        }
    }
    (leaked, checked)
}

/// [`check_redaction`] fires on each block, on an announcement no fixed-string
/// set anticipated, and not on the redaction token itself.
///
/// Without this, the only evidence the tripwire works is that it is green on a
/// corpus where nothing has leaked — which is indistinguishable from a scan that
/// looks in the wrong place. Every case below is a real arm tell: an announcement
/// sentence appears in arm B alone.
#[test]
fn redaction_check_refuses_a_transcript_that_names_its_arm() {
    let root = tempfile::tempdir().expect("tempdir");
    let dir = root.path().join("systematic-debugging");
    fs::create_dir(&dir).expect("create skill dir");

    let transcript = |response: &str, meta: &str| {
        format!(
            "## Forced choice\n\n**correct_option:** B\n\n\
             ## Scenario\n\nYou are the single writer.\n\n\
             ## Response\n\n{response}\n\n## Meta-test\n\n{meta}\n"
        )
    };
    let write = |id: &str, body: String| {
        fs::write(dir.join(format!("{id}.md")), body).expect("write transcript");
    };
    let scan = || check_redaction(root.path());

    // Control FIRST. If a properly redacted pair complains, every case below is
    // meaningless — this is the ordering the phase's own mutation harness had to
    // learn twice.
    write("aaaaaa", transcript("[announcement elided] I take B.", "Clearer wording."));
    write("bbbbbb", transcript("I take B.", "[announcement elided] Clearer."));
    let (leaked, checked) = scan();
    assert!(leaked.is_empty(), "control: redacted transcripts complained: {leaked:?}");
    assert_eq!(checked, 2, "control: expected 2 transcripts read, got {checked}");

    // The announcement survives in `## Response`.
    write(
        "aaaaaa",
        transcript(
            "Using drovr:systematic-debugging — reproducing before fixing. I take B.",
            "Clearer wording.",
        ),
    );
    let (leaked, _) = scan();
    assert_eq!(leaked.len(), 1, "an unredacted `## Response` must complain: {leaked:?}");
    assert!(leaked[0].contains("## Response"), "must name the block: {leaked:?}");
    assert!(leaked[0].contains("aaaaaa"), "must name the transcript: {leaked:?}");

    // ...and in `## Meta-test`, which the earlier ad-hoc slice made easy to miss.
    write("aaaaaa", transcript("[announcement elided] I take B.", "Clearer wording."));
    write(
        "bbbbbb",
        transcript(
            "I take B.",
            "It should have said: Using drovr:tdd — writing the failing test first.",
        ),
    );
    let (leaked, _) = scan();
    assert_eq!(leaked.len(), 1, "an unredacted `## Meta-test` must complain: {leaked:?}");
    assert!(leaked[0].contains("## Meta-test"), "must name the block: {leaked:?}");

    // A skill this corpus has never announced. The check is a prefix scan, not a
    // fixed-string set assembled before the next skill existed.
    write("bbbbbb", transcript("Using drovr:some-future-skill — doing it. I take B.", "Clearer."));
    let (leaked, _) = scan();
    assert_eq!(leaked.len(), 1, "an unanticipated announcement must complain: {leaked:?}");

    // A file that is not a transcript is not scanned, however it reads.
    write("aaaaaa", transcript("[announcement elided] I take B.", "Clearer wording."));
    write("bbbbbb", transcript("I take B.", "Clearer."));
    fs::write(
        dir.join("remeasure-scores.json"),
        "[{\"note\": \"Using drovr:systematic-debugging — reproducing before fixing.\"}]",
    )
    .expect("write non-transcript");
    let (leaked, checked) = scan();
    assert!(leaked.is_empty(), "a non-transcript file must not be scanned: {leaked:?}");
    assert_eq!(checked, 2, "non-transcript files must not be counted: {checked}");
}

/// [`check_blind_map`] rejects each way a map can fail to attribute the verdicts
/// it is paired with.
///
/// **Why this test exists at all.** The check above is new, and every artifact in
/// the tree it runs against is already legal — so on the real corpus it can only
/// be observed to pass. That is the shape of the nine vacuous guards this run has
/// now found, one of which was the `checked > 0` counter directly above. A guard
/// ships with a demonstration that it fires, or it is a comment.
#[test]
fn blind_map_check_refuses_a_map_that_cannot_attribute_its_verdicts() {
    let dir = tempfile::tempdir().expect("tempdir");
    let dir = dir.path();

    // Two transcripts on disk, because `check_blind_map` resolves every id it maps.
    for id in ["aaaaaa", "bbbbbb"] {
        fs::write(
            dir.join(format!("{id}.md")),
            "## Response\n\nI'm taking A.\n",
        )
        .expect("write transcript");
    }
    let verdict = |id: &str| {
        serde_json::json!({
            "transcript_id": id, "compliant": true, "cites_section": false,
            "names_temptation": true, "meta_test_clear": false,
            "new_rationalizations": [], "evidence": "I'm taking A."
        })
    };
    let verdicts: Vec<Verdict> =
        serde_json::from_value(serde_json::json!([verdict("aaaaaa"), verdict("bbbbbb")]))
            .expect("fixture verdicts parse");

    let write_map = |name: &str, value: serde_json::Value| {
        fs::write(dir.join(name), value.to_string()).expect("write map");
    };
    let entry = |arm: &str| serde_json::json!({"arm": arm, "scenario": "tdd-2", "sample": 1});
    let complaints =
        |bundle: VerdictBundle| -> Vec<String> { check_blind_map(dir, bundle, &verdicts) };

    // Control. If this ever fails, every negative case below is meaningless.
    write_map(
        "blind-map.json",
        serde_json::json!({"aaaaaa": entry("A"), "bbbbbb": entry("B")}),
    );
    assert!(
        complaints(VerdictBundle::Bar).is_empty(),
        "a complete, correctly-armed map must satisfy the check: {:?}",
        complaints(VerdictBundle::Bar)
    );

    // 1. A scored transcript with no entry — the run is unattributable.
    write_map("blind-map.json", serde_json::json!({"aaaaaa": entry("A")}));
    let wrong = complaints(VerdictBundle::Bar);
    assert_eq!(
        wrong.len(),
        1,
        "expected exactly the missing-entry complaint: {wrong:?}"
    );
    assert!(
        wrong[0].contains("bbbbbb") && wrong[0].contains("does not assign"),
        "{wrong:?}"
    );

    // 2. An entry for a transcript nobody scored — a claimed run that never was.
    write_map(
        "blind-map.json",
        serde_json::json!({"aaaaaa": entry("A"), "bbbbbb": entry("B"), "cccccc": entry("B")}),
    );
    fs::write(dir.join("cccccc.md"), "## Response\n\nI'm taking A.\n").expect("write transcript");
    let wrong = complaints(VerdictBundle::Bar);
    assert_eq!(
        wrong.len(),
        1,
        "expected exactly the surplus-entry complaint: {wrong:?}"
    );
    assert!(
        wrong[0].contains("cccccc") && wrong[0].contains("does not score"),
        "{wrong:?}"
    );

    // 3. `arm: "none"` in a bar-facing map — a control cell posing as a measured
    //    one, which is the join error that would corrupt every arm-level count.
    write_map(
        "blind-map.json",
        serde_json::json!({"aaaaaa": entry("none"), "bbbbbb": entry("B")}),
    );
    let wrong = complaints(VerdictBundle::Bar);
    assert_eq!(
        wrong.len(),
        1,
        "expected exactly the wrong-arm complaint: {wrong:?}"
    );
    assert!(
        wrong[0].contains("aaaaaa") && wrong[0].contains("None"),
        "{wrong:?}"
    );

    // 4. …and the mirror: a measured arm inside the unaided control's map.
    write_map(
        "control-blind-map.json",
        serde_json::json!({"aaaaaa": entry("none"), "bbbbbb": entry("A-prime")}),
    );
    let wrong = complaints(VerdictBundle::Control);
    assert_eq!(
        wrong.len(),
        1,
        "expected exactly the wrong-arm complaint: {wrong:?}"
    );
    assert!(
        wrong[0].contains("bbbbbb") && wrong[0].contains("APrime"),
        "{wrong:?}"
    );

    // 5. An arm outside `plan.md` §1.1's vocabulary fails at deserialization, so a
    //    typo cannot become a fifth silent arm.
    write_map(
        "control-blind-map.json",
        serde_json::json!({"aaaaaa": entry("A-Prime"), "bbbbbb": entry("none")}),
    );
    let wrong = complaints(VerdictBundle::Control);
    assert_eq!(
        wrong.len(),
        1,
        "expected exactly the schema complaint: {wrong:?}"
    );
    assert!(wrong[0].contains("blind-map schema"), "{wrong:?}");

    // 6. An extra key fails the same way — `deny_unknown_fields` is load-bearing.
    write_map(
        "control-blind-map.json",
        serde_json::json!({"aaaaaa": {"arm": "none", "scenario": "tdd-2", "sample": 1, "note": "x"}}),
    );
    let wrong = complaints(VerdictBundle::Control);
    assert_eq!(
        wrong.len(),
        1,
        "expected exactly the schema complaint: {wrong:?}"
    );
    assert!(wrong[0].contains("blind-map schema"), "{wrong:?}");

    // 7. No map at all beside a verdict file that exists.
    fs::remove_file(dir.join("control-blind-map.json")).expect("remove map");
    fs::write(dir.join("control-scores.json"), "[]").expect("write scores");
    let wrong = complaints(VerdictBundle::Control);
    assert_eq!(
        wrong.len(),
        1,
        "expected exactly the missing-map complaint: {wrong:?}"
    );
    assert!(
        wrong[0].contains("control-blind-map.json") && wrong[0].contains("attributable"),
        "{wrong:?}"
    );
}

// ---------------------------------------------------------------------------
// The run ledger's arithmetic
// ---------------------------------------------------------------------------

/// The hard ceiling on **all** probe runs across the whole run, metered or not.
///
/// spec §7.3 froze this at 122 for a **sonnet-only** design. The `cross-model-arm`
/// phase adds a factor §7.3 never budgeted — probe model — and the human raised the
/// ceiling on 2026-08-06 to pay for it. The new value is derived, not chosen:
/// 99 already spent + 20 metered (`opus`) + 72 unmetered (`qwen`) = 191. The full
/// derivation, and who authorised it, is in `run-ledger.md`'s prose header; this
/// constant and that prose move together or the raise is silent in one of them.
const RUN_CEILING: u32 = 191;

/// The hard ceiling on the runs that **cost metered budget** — every row except the
/// ones the ledger marks [`UNMETERED_MARKER`].
///
/// **Why a second constant rather than only lifting [`RUN_CEILING`].** The
/// cross-model phase's `qwen` arm is on a model the human declared unlimited, so its
/// 72 runs belong in the ledger (it records what happened, not only what cost money)
/// but must not buy metered headroom. Raising `RUN_CEILING` alone from 122 to 191
/// would have handed the run 69 unaudited metered runs — a set extended without its
/// guard extended, which is this run's own recurring defect. Derived the same way:
/// 99 spent + 20 (`opus`: 16 planned + a 4-run retry allowance, one per condition,
/// because a failed probe in a 4-run cell voids the cell rather than shrinking it).
const METERED_RUN_CEILING: u32 = 119;

/// The substring a ledger row's `stage` cell carries when its runs are unmetered.
///
/// **Absence means metered.** A row that forgets the marker is counted against
/// [`METERED_RUN_CEILING`], which can only trip the ceiling *early*. The opposite
/// default — infer "unmetered" from the model named in the cell — would let a typo
/// buy budget silently, which is the direction that cannot be recovered from.
const UNMETERED_MARKER: &str = "UNMETERED";

/// One data row of `run-ledger.md`'s budget table.
///
/// The two counts are `u32` because they are the only cells this check computes
/// on; `task` and `stage` are the cell text as written, because their only job is
/// to name the offending row in a complaint. Nothing here is normalised — a row
/// whose counts do not parse is rejected in [`parse_ledger`] rather than
/// represented.
struct LedgerRow {
    task: String,
    stage: String,
    runs: u32,
    cumulative: u32,
}

/// The stage **name** a ledger row's `stage` cell carries: the text before its
/// first ` — ` qualifier, with the cell's markdown emphasis stripped.
///
/// **A stage is matched as a whole token, never as a substring.** The cross-model
/// cross-charge check filtered rows with `stage.contains("cross-model (qwen)")`,
/// which sums into one charge every row whose prose merely *mentions* the stage —
/// so a row that referred to the cross-model charge in passing would be counted as
/// part of it, and could cover for a missing or mis-tagged real row. The cells are
/// written `**<name> — <qualifier>**` throughout the ledger, so the name is a token
/// the table already delimits; nothing had to be added to the document to match it
/// exactly.
///
/// The qualifier is deliberately *not* stripped from `LedgerRow::stage` itself:
/// [`UNMETERED_MARKER`] lives in it, and that check reads the whole cell.
fn stage_name(stage: &str) -> &str {
    let bare = stage.trim().trim_matches('*').trim();
    bare.split(" — ").next().unwrap_or(bare).trim()
}

/// Where each load-bearing column sits in the budget table.
///
/// **A named field per column, not a positional list.** The columns are resolved
/// by header text, so an index list would have to be read back in the same order
/// it was built — and reordering that list would silently swap `runs` with
/// `cumulative` while every type still checked. The whole point of resolving by
/// header is that position carries no meaning; a `Vec<usize>` would put it back.
struct LedgerColumns {
    task: usize,
    stage: usize,
    runs: usize,
    cumulative: usize,
}

/// Split one markdown table line into its cells.
fn ledger_cells(line: &str) -> Vec<&str> {
    line.trim()
        .trim_start_matches('|')
        .trim_end_matches('|')
        .split('|')
        .map(str::trim)
        .collect()
}

/// The ledger's four load-bearing columns, resolved **by header text and never
/// by position** — the columns may be reordered, but not renamed or dropped.
///
/// Header cells go through [`normalize_header`], the same function
/// `arms/MANIFEST.md`'s parser uses, so the two tables agree on what counts as a
/// renamed column. Data cells do not: the only ones this reads are the two run
/// counts, which are bare integers in every row, so they are parsed as written
/// rather than normalised into something that might parse when it should not.
fn parse_ledger(text: &str) -> Result<Vec<LedgerRow>, String> {
    const TASK: &str = "task";
    const STAGE: &str = "stage (§7.3 row)";
    const RUNS: &str = "runs this stage";
    const CUMULATIVE: &str = "cumulative";

    // A markdown alignment row (`|---|---|`), which carries no data.
    let is_separator = |cells: &[&str]| {
        cells
            .iter()
            .all(|c| !c.is_empty() && c.chars().all(|ch| ch == '-' || ch == ':'))
    };

    let mut lines = text.lines();
    let mut columns: Option<LedgerColumns> = None;
    let mut width = 0usize;
    for line in lines.by_ref() {
        if !line.trim_start().starts_with('|') {
            continue;
        }
        let cells: Vec<String> = ledger_cells(line)
            .iter()
            .map(|c| normalize_header(c))
            .collect();

        // **Completeness first, then duplicates — the order `parse_manifest`
        // uses, and the same reason.** A preamble that talks about a table grows
        // illustrations of one, so a row is the header only if it carries the
        // COMPLETE set of load-bearing columns; anything short of that is passed
        // over rather than judged. Checking duplicates before completeness would
        // let an incomplete fragment like `| cumulative | cumulative |` hard-fail
        // a perfectly good ledger, and would make two parsers over one
        // markdown-table dialect disagree about malformed input.
        let hits = |want: &str| -> Vec<usize> {
            let key = normalize_header(want);
            cells
                .iter()
                .enumerate()
                .filter(|(_, c)| **c == key)
                .map(|(i, _)| i)
                .collect()
        };
        let (task, stage, runs, cumulative) =
            (hits(TASK), hits(STAGE), hits(RUNS), hits(CUMULATIVE));
        // Only for the two checks that treat all four alike. The struct below is
        // still bound field-by-field from the named bindings, never from this
        // list's order.
        let named = [
            (TASK, &task),
            (STAGE, &stage),
            (RUNS, &runs),
            (CUMULATIVE, &cumulative),
        ];
        if named.iter().any(|(_, at)| at.is_empty()) {
            continue;
        }

        // Now it *is* the header, so a duplicate is corruption rather than a
        // fragment: resolving by name cannot say which of two same-named columns
        // a cell belongs to, and `position()` alone would silently take the first
        // and read the wrong cell for the rest of the file.
        for (want, at) in named {
            if at.len() > 1 {
                return Err(format!(
                    "the budget table's header carries {} columns named {want:?}; the \
                     column is resolved by its name, so a duplicate makes every cell \
                     under it ambiguous",
                    at.len()
                ));
            }
        }
        width = cells.len();
        columns = Some(LedgerColumns {
            task: task[0],
            stage: stage[0],
            runs: runs[0],
            cumulative: cumulative[0],
        });
        break;
    }
    let columns = columns.ok_or_else(|| {
        format!(
            "no budget table found: no row carries all of [{TASK:?}, {STAGE:?}, {RUNS:?}, \
             {CUMULATIVE:?}]. The ledger is the only mechanism tracking spec §7.3's \
             {RUN_CEILING}-run ceiling, so a table this parser cannot find is a ceiling \
             nothing tracks."
        )
    })?;

    // **Every remaining line that begins with `|` is a data row**, and a line
    // that does not is skipped rather than treated as the end of the table.
    //
    // The obvious alternative — stop at the first non-`|` line — makes a blank
    // line between two rows silently truncate the ledger, and the check then
    // validates a *prefix*: the arithmetic closes, no complaint is raised, and a
    // 200-run stage recorded below the gap is never read. A guard that passes on
    // part of the file is worse than no guard, and this run has already found
    // nine of that shape. `arms/MANIFEST.md` states the same rule for the same
    // reason — "no line after the table may begin with `|`" — so a stray table
    // below is a loud parse failure here, never a silent drop.
    let mut rows = Vec::new();
    for line in lines {
        if !line.trim_start().starts_with('|') {
            continue;
        }
        let cells = ledger_cells(line);
        if is_separator(&cells) {
            continue;
        }
        if cells.len() != width {
            return Err(format!(
                "ledger row {line:?} has {} cells, not the header's {width} — a short row \
                 would read as a stage that was never spent",
                cells.len()
            ));
        }
        let count = |at: usize, column: &str| -> Result<u32, String> {
            cells[at].parse::<u32>().map_err(|_| {
                format!(
                    "ledger row {line:?}: {:?} is not a run count for column {column:?}",
                    cells[at]
                )
            })
        };
        rows.push(LedgerRow {
            task: cells[columns.task].to_string(),
            stage: cells[columns.stage].to_string(),
            runs: count(columns.runs, RUNS)?,
            cumulative: count(columns.cumulative, CUMULATIVE)?,
        });
    }
    Ok(rows)
}

/// The invariant the ledger states about itself and nothing checked:
/// `cumulative` is the running total of `runs this stage`, and neither the global
/// ceiling nor the **metered** ceiling is crossed.
///
/// **Two ceilings, because the run now spends two currencies.** Every row through
/// `remeasure-systematic-debugging` was a metered Claude run and one number could
/// guard them all. The `cross-model-arm` phase adds `qwen` runs on a model the human
/// declared unlimited: they are real runs and belong in `cumulative`, but charging
/// them against the metered budget would be a lie in one direction and leaving them
/// out of the table would be a lie in the other. So `cumulative` keeps counting
/// everything against [`RUN_CEILING`], and a second subtotal — every row whose stage
/// cell does **not** carry [`UNMETERED_MARKER`] — is held to
/// [`METERED_RUN_CEILING`]. Raising the first without the second is exactly the
/// vacuous-guard move this file exists to prevent.
///
/// Returns complaints rather than asserting, following [`check_blind_map`]: a
/// check that panics can only ever be observed to *pass* on a legal corpus, and
/// this run has now found nine guards that could not fire. See
/// `ledger_check_refuses_a_table_that_does_not_add_up`.
///
/// **Why it is worth checking at all.** Tasks 16–21 each read this table's
/// cumulative before starting and halt with a null rather than cross the
/// ceiling. That decision is made from a hand-maintained markdown column: one
/// mis-added row and a phase either halts on a ceiling it has not reached or
/// spends past one it has.
fn check_ledger(text: &str) -> Vec<String> {
    let rows = match parse_ledger(text) {
        Ok(rows) => rows,
        Err(e) => return vec![e],
    };
    let mut wrong = Vec::new();
    if rows.is_empty() {
        wrong.push(
            "the budget table has a header and no data rows — a ledger recording no runs \
             passes a presence check while tracking nothing"
                .to_string(),
        );
        return wrong;
    }
    let mut running = 0u32;
    let mut metered = 0u32;
    for row in &rows {
        running += row.runs;
        if !row.stage.contains(UNMETERED_MARKER) {
            metered += row.runs;
        }
        if row.cumulative != running {
            wrong.push(format!(
                "task {} / {:?}: cumulative is {} but the running total of `runs this stage` \
                 is {running}",
                row.task, row.stage, row.cumulative,
            ));
        }
    }
    let last = rows[rows.len() - 1].cumulative;
    if last > RUN_CEILING {
        wrong.push(format!(
            "the ledger's final cumulative is {last}, over the run's {RUN_CEILING}-run \
             ceiling — the standing rule is to halt and record a null, not to extend"
        ));
    }
    if metered > METERED_RUN_CEILING {
        wrong.push(format!(
            "the ledger's metered runs total {metered}, over the {METERED_RUN_CEILING}-run \
             metered ceiling. A row is metered unless its stage cell says \
             {UNMETERED_MARKER:?}, so this counts rows that forgot the marker — check those \
             before raising anything"
        ));
    }
    wrong
}

/// `run-ledger.md` adds up, and stays inside spec §7.3's ceiling.
#[test]
fn run_ledger_cumulative_is_a_running_total() {
    let path = evidence_dir().join(EVIDENCE_LEDGER);
    let text = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    let wrong = check_ledger(&text);
    assert!(wrong.is_empty(), "{}: {}", path.display(), wrong.join("\n"));
}

/// [`check_ledger`] rejects each way the table can stop tracking the ceiling.
///
/// The real ledger is legal, so on it the check above can only be observed to
/// pass — the shape of every vacuous guard this run has found.
#[test]
fn ledger_check_refuses_a_table_that_does_not_add_up() {
    let header = "| task | stage (§7.3 row) | runs this stage | cumulative | stage ceiling | ceiling hit? |\n|---|---|---|---|---|---|\n";

    // Sound: a running total that closes, and prose after the table is not a row.
    let ok = format!(
        "{header}| 6 | RED | 10 | 10 | 10 | no |\n| 16 | Arm A | 4 | 14 | 20 | no |\n\nSome prose.\n"
    );
    assert!(check_ledger(&ok).is_empty(), "{:?}", check_ledger(&ok));

    // The arithmetic does not close.
    let drifted =
        format!("{header}| 6 | RED | 10 | 10 | 10 | no |\n| 16 | Arm A | 4 | 13 | 20 | no |\n");
    assert!(
        check_ledger(&drifted)
            .iter()
            .any(|c| c.contains("cumulative is 13") && c.contains("14")),
        "{:?}",
        check_ledger(&drifted)
    );

    // The global ceiling is crossed. The expected value is read from the constant,
    // never spelled out: a literal here would have to be hand-edited in lockstep
    // with every ceiling raise, and the one that got forgotten would leave this
    // asserting against a ceiling nothing enforces.
    let over = format!(
        "{header}| 6 | RED | {} | {} | 10 | {UNMETERED_MARKER} |\n",
        RUN_CEILING + 1,
        RUN_CEILING + 1
    );
    let ceiling = RUN_CEILING.to_string();
    assert!(
        check_ledger(&over)
            .iter()
            .any(|c| c.contains("final cumulative") && c.contains(&ceiling)),
        "{:?}",
        check_ledger(&over)
    );

    // **The metered ceiling is crossed while the global one is not.** This is the
    // case the single-ceiling check could not see, and the reason the second
    // constant exists: unmetered rows may run the cumulative up to `RUN_CEILING`,
    // so a metered overrun below that number is invisible without its own subtotal.
    let metered_over = format!(
        "{header}| x | qwen {UNMETERED_MARKER} | 60 | 60 | n/a | n/a |\n\
         | y | opus | {} | {} | n/a | no |\n",
        METERED_RUN_CEILING + 1,
        METERED_RUN_CEILING + 61
    );
    assert!(
        METERED_RUN_CEILING + 61 <= RUN_CEILING,
        "this case must stay UNDER the global ceiling or it proves nothing about the \
         metered one: {METERED_RUN_CEILING} + 61 vs {RUN_CEILING}"
    );
    let metered_ceiling = METERED_RUN_CEILING.to_string();
    let wrong = check_ledger(&metered_over);
    assert!(
        wrong
            .iter()
            .any(|c| c.contains("metered runs total") && c.contains(&metered_ceiling)),
        "{wrong:?}"
    );
    assert!(
        !wrong.iter().any(|c| c.contains("final cumulative")),
        "the global ceiling must not be what fired here: {wrong:?}"
    );

    // …and the control for it: the SAME run counts, with the metered spend moved
    // onto the unmetered row, are legal. Without this the case above would pass on
    // a check that simply refused every large table.
    let metered_ok = format!(
        "{header}| x | qwen {UNMETERED_MARKER} | {} | {} | n/a | n/a |\n\
         | y | opus | 60 | {} | n/a | no |\n",
        METERED_RUN_CEILING + 1,
        METERED_RUN_CEILING + 1,
        METERED_RUN_CEILING + 61
    );
    assert!(
        check_ledger(&metered_ok).is_empty(),
        "an unmetered row must not consume metered budget: {:?}",
        check_ledger(&metered_ok)
    );

    // A row that forgets the marker is METERED, not unmetered. The fail-safe
    // direction, asserted rather than assumed — the opposite default would let a
    // typo buy budget silently.
    let unmarked = format!(
        "{header}| x | qwen (unlimited, no marker) | {} | {} | n/a | n/a |\n",
        METERED_RUN_CEILING + 1,
        METERED_RUN_CEILING + 1
    );
    assert!(
        check_ledger(&unmarked)
            .iter()
            .any(|c| c.contains("metered runs total")),
        "{:?}",
        check_ledger(&unmarked)
    );

    // A header with no data rows tracks nothing.
    assert!(
        check_ledger(header)
            .iter()
            .any(|c| c.contains("no data rows")),
        "{:?}",
        check_ledger(header)
    );

    // No table at all — the failure this must not pass silently.
    assert!(
        check_ledger("# Run ledger\n\nnothing here.\n")
            .iter()
            .any(|c| c.contains("no budget table found")),
        "{:?}",
        check_ledger("# Run ledger\n\nnothing here.\n")
    );

    // A renamed column is a parse error, not a silently rebound field.
    let renamed = "| task | stage (§7.3 row) | runs | cumulative | stage ceiling | ceiling hit? |\n|---|---|---|---|---|---|\n| 6 | RED | 10 | 10 | 10 | no |\n";
    assert!(
        check_ledger(renamed)
            .iter()
            .any(|c| c.contains("no budget table found")),
        "{:?}",
        check_ledger(renamed)
    );

    // Columns may be REORDERED, because they are resolved by header text — and
    // `cumulative` deliberately sits *before* `runs this stage` here, so a parser
    // that bound the two counts positionally would read runs=10,cum=10 then
    // runs=14,cum=4 and complain. A clean result is the assertion that each count
    // reached its own field.
    let reordered = "| cumulative | task | runs this stage | stage (§7.3 row) |\n|---|---|---|---|\n| 10 | 6 | 10 | RED |\n| 14 | 16 | 4 | Arm A |\n";
    assert!(
        check_ledger(reordered).is_empty(),
        "{:?}",
        check_ledger(reordered)
    );

    // A blank line between rows must NOT end the table. This is the one case
    // that could make the check pass on a strict prefix: without it, the row
    // below the gap is never read and the arithmetic closes on the two rows
    // above it. Its run count is sized off the constant so a later raise cannot
    // quietly drop it under the ceiling and make this assert on nothing.
    let gapped = format!(
        "{header}| 6 | RED | 10 | 10 | 10 | no |\n\n| 16 | Arm A | {} | {} | 20 | no |\n",
        RUN_CEILING,
        RUN_CEILING + 10
    );
    assert!(
        check_ledger(&gapped)
            .iter()
            .any(|c| c.contains("final cumulative") && c.contains(&ceiling)),
        "a blank line inside the table truncated it: {:?}",
        check_ledger(&gapped)
    );

    // A duplicated load-bearing header is ambiguous, not first-match-wins.
    let dup = "| task | stage (§7.3 row) | runs this stage | cumulative | cumulative |\n|---|---|---|---|---|\n| 6 | RED | 10 | 10 | 99 |\n";
    assert!(
        check_ledger(dup)
            .iter()
            .any(|c| c.contains("2 columns named") && c.contains("cumulative")),
        "{:?}",
        check_ledger(dup)
    );

    // ...but an INCOMPLETE row carrying that same duplicate is a fragment, not a
    // corrupt header, and must be passed over — `parse_manifest`'s rule, checked
    // here so the two parsers cannot drift apart on malformed input. Prose about
    // a table grows illustrations of one, and an illustration must not be able to
    // hard-fail a perfectly good ledger.
    let illustrated = format!(
        "Prose about the columns, e.g.\n\n| cumulative | cumulative |\n\n{header}| 6 | RED | 10 | 10 | 10 | no |\n"
    );
    assert!(
        check_ledger(&illustrated).is_empty(),
        "an incomplete fragment aborted the scan instead of being skipped: {:?}",
        check_ledger(&illustrated)
    );

    // A non-numeric run count is refused rather than read as zero.
    let fuzzy = format!("{header}| 6 | RED | ten | 10 | 10 | no |\n");
    assert!(
        check_ledger(&fuzzy)
            .iter()
            .any(|c| c.contains("is not a run count")),
        "{:?}",
        check_ledger(&fuzzy)
    );
}

// ---------------------------------------------------------------------------
// The cross-model arm
// ---------------------------------------------------------------------------

/// Declare [`ProbeModel`]'s variants, their wire names and whether each is
/// metered from **one** table.
///
/// [`skill_names!`] and [`blind_arms!`]'s reason, applied to the third closed set
/// in this file. The hand-written version kept four parallel lists — the variants,
/// their `#[serde(rename)]`s, `ALL`, and the `is_metered` match — and only two of
/// them were exhaustive: a third model added to the enum but left out of `ALL`
/// would still deserialize out of a blind map and then fail grid parsing under a
/// name `ProbeModel::accepted()` never mentions.
macro_rules! probe_models {
    ($($variant:ident => $wire:literal, metered: $metered:literal,)+) => {
        /// The models `cross-model.md` probes. A **closed** set, for the reason every
        /// closed set in this file is one: a typo must be a parse error, not a silent
        /// fifth model whose runs are counted under a name nothing else recognises.
        ///
        /// `sonnet` is deliberately absent. Its cells are **reused** from the
        /// `remeasure-*` stages rather than re-run, so it owns no transcript in this
        /// directory and a row claiming otherwise is wrong.
        #[derive(serde::Deserialize, Debug, PartialEq, Eq, Hash, Clone, Copy)]
        enum ProbeModel {
            $(#[serde(rename = $wire)] $variant,)+
        }

        impl ProbeModel {
            /// Every probed model. Complete by construction: the parser, the
            /// accepted-values error text and the ledger cross-charge loop all read
            /// this, and it comes out of the same table as the variants.
            const ALL: &'static [ProbeModel] = &[$(ProbeModel::$variant,)+];

            fn as_str(self) -> &'static str {
                match self { $(ProbeModel::$variant => $wire,)+ }
            }

            /// Whether this model's runs are charged against the metered ceiling.
            ///
            /// The ledger's own marker is the authority at the row level; this is the
            /// per-model fact the row's marker must agree with, so a `qwen` row that
            /// forgot `UNMETERED` is caught here as well as by [`check_ledger`].
            fn is_metered(self) -> bool {
                match self { $(ProbeModel::$variant => $metered,)+ }
            }
        }
    };
}

probe_models! {
    Qwen => "qwen", metered: false,
    Opus => "opus", metered: true,
}

impl ProbeModel {
    fn parse(raw: &str) -> Option<Self> {
        ProbeModel::ALL.iter().copied().find(|m| m.as_str() == raw)
    }

    /// The accepted values, in `ALL` order — for error text that must name
    /// exactly what `parse` accepts, and cannot be a second list saying so.
    fn accepted() -> String {
        ProbeModel::ALL
            .iter()
            .map(|m| m.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// One entry of `transcripts/cross-model/cross-model-blind-map.json`.
///
/// A separate type from [`BlindMapEntry`] because it carries two fields that map
/// has no place for — the probe model and the skill — and `deny_unknown_fields`
/// means neither type can quietly accept the other's file. The transcripts live
/// outside `transcripts/<skill>/` for the same reason: this stage's bundle is not
/// one of [`VerdictBundle`]'s, and dropping its files into a skill directory
/// would put an unrecognised verdict set where the per-skill checks would walk
/// straight past it.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct CrossModelEntry {
    arm: BlindArm,
    #[allow(dead_code)]
    scenario: String,
    #[allow(dead_code)]
    sample: u32,
    model: ProbeModel,
    /// **[`SkillName`], not a validated `String`.** The predecessor ran
    /// `SkillName::parse` in a `deserialize_with` and then returned the raw
    /// string, discarding the parse result it had just computed: the field stayed
    /// typed as an opaque wire value, so only the serde boundary knew the set was
    /// closed and a hand-built entry did not. `arm` and `model` are closed enums
    /// that deserialize straight into themselves; this is the third dimension of
    /// the same key and it is one now too.
    skill: SkillName,
}

/// One record of `transcripts/cross-model/cross-model-adjudication.json`: the
/// second, independent scoring pass over a transcript the primary pass scored.
///
/// A **third** adjudication shape beside [`Adjudication`], because it answers a
/// different question. `Adjudication` records what option a blind re-reader
/// thought a response committed to; this records whether a second *scorer*,
/// dispatched by a different mechanism, reached the same `compliant` verdict.
/// Two shapes get two types rather than one loose one, so neither file can be
/// deserialized as the other.
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct CrossModelAdjudication {
    /// A [`TranscriptId`], for the reason `CrossModelEntry::skill` is a
    /// [`SkillName`]: a malformed id deserializes cleanly out of a `String` field
    /// and only fails later at the join, which pushes the invariant onto callers.
    transcript_id: TranscriptId,
    second_pass_compliant: bool,
    primary_compliant: bool,
    /// Carried, and then **recomputed** rather than trusted — the same treatment
    /// the `remeasure-*` stages gave `matches_key`. A stored boolean that agrees
    /// with nothing is how a count in prose drifts from the record behind it.
    agrees: bool,
}

/// One row of `cross-model.md`'s measured grid.
///
/// **Its three dimensions are the closed types the recomputed side keys on, not
/// free strings.** The predecessor took whatever text sat in the cell, so
/// `Opus` for `opus` or `A prime` for `A-prime` parsed cleanly and then failed in
/// [`check_cross_model_grid`] as "the grid declares … which no transcript
/// measured" — a join complaint pointing at the wrong thing, with nothing naming
/// the invalid dimension. A typo in a prose grid is a parse error here, the way a
/// typo in the blind map is one in [`CrossModelEntry`].
#[derive(Debug, PartialEq, Eq)]
struct GridRow {
    model: ProbeModel,
    skill: SkillName,
    condition: BlindArm,
    compliant: u32,
    runs: u32,
}

/// The [`BlindArm`] a grid row's `condition` cell names — the inverse of
/// [`BlindArm::condition_name`], written as a search over it rather than as a
/// second match arm, so the two spellings of one mapping cannot drift apart.
fn parse_condition(raw: &str) -> Option<BlindArm> {
    BlindArm::ALL
        .iter()
        .copied()
        .find(|a| a.condition_name() == raw)
}

/// The accepted condition cells, in [`BlindArm::ALL`] order — error text that
/// names exactly what [`parse_condition`] accepts and cannot be a second list.
fn conditions_accepted() -> String {
    BlindArm::ALL
        .iter()
        .map(|a| a.condition_name())
        .collect::<Vec<_>>()
        .join(", ")
}

/// The three dimensions that identify one cell of the measured grid.
///
/// Both sides of [`check_cross_model_grid`] key on this — the declared rows and
/// the recount from the blind map — so a cell can only be compared to a cell built
/// out of the same closed types.
type GridCell = (ProbeModel, SkillName, BlindArm);

/// How a [`GridCell`] is named in a complaint: `qwen/tdd/A-prime`, the way the
/// grid itself spells it, rather than the `Debug` of a tuple of enum variants.
fn cell_name(cell: &GridCell) -> String {
    format!(
        "{}/{}/{}",
        cell.0.as_str(),
        cell.1.as_str(),
        cell.2.condition_name()
    )
}

/// Parse `cross-model.md`'s measured grid: the one table whose header is exactly
/// `model | skill | condition | compliant | runs`.
///
/// Resolved by header text like every other table this file reads, and the header
/// is matched **completely** — the document is full of other tables (the reused
/// `sonnet` reference, the design grid), and a parser that latched onto the first
/// five-column table would read the design's *planned* run counts as measurements.
fn parse_grid(text: &str) -> Result<Vec<GridRow>, String> {
    const COLS: [&str; 5] = ["model", "skill", "condition", "compliant", "runs"];
    let mut rows = Vec::new();
    let mut at: Option<[usize; 5]> = None;
    // **There is exactly ONE measured grid, and a second one is corruption.**
    // The first version cleared `at` on prose and then happily re-latched onto any
    // later table with a matching header, appending its rows — while the comment
    // claimed the complete-header match prevented exactly that. Only the absence
    // of a second such table in `cross-model.md` kept it green, and this
    // function's own unit test asserted the relatched row count, so the test
    // encoded the defect instead of catching it. Two tables both claiming to be
    // the measured grid is not a case with an obvious right answer — silently
    // concatenating them is the worst of the available answers — so it is an
    // error, the way `parse_ledger` and `parse_manifest` treat their own
    // ambiguities.
    let mut closed = false;
    for line in text.lines() {
        if !line.trim_start().starts_with('|') {
            // Unlike the ledger and the manifest, this table MAY be followed by
            // prose and by other tables, so a non-row line ends it rather than
            // being an error.
            if at.is_some() && !line.trim().is_empty() {
                at = None;
                closed = true;
            }
            continue;
        }
        let cells: Vec<&str> = ledger_cells(line);
        if cells.iter().all(|c| !c.is_empty() && c.chars().all(|ch| ch == '-' || ch == ':')) {
            continue;
        }
        if at.is_none() {
            let normalized: Vec<String> = cells.iter().map(|c| normalize_header(c)).collect();
            let hits = |want: &str| -> Vec<usize> {
                let key = normalize_header(want);
                normalized
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| **c == key)
                    .map(|(i, _)| i)
                    .collect()
            };
            let found: Vec<Vec<usize>> = COLS.iter().map(|want| hits(want)).collect();
            if found.iter().all(|f| !f.is_empty()) {
                // Completeness first, then duplicates — `parse_ledger`'s order and
                // its reason. Now that this IS the header, a duplicated
                // load-bearing column is corruption rather than a fragment:
                // resolving by name cannot say which of two same-named columns a
                // cell belongs to, and taking the first silently reads the wrong
                // cell for every row.
                for (want, f) in COLS.iter().zip(found.iter()) {
                    if f.len() > 1 {
                        return Err(format!(
                            "the measured grid's header carries {} {want:?} columns; \
                             they are resolved by name, so a duplicate has no \
                             authoritative cell",
                            f.len()
                        ));
                    }
                }
                if closed {
                    return Err(
                        "two tables carry the measured grid's header — there is one \
                         measured grid, and concatenating a second would report runs \
                         nothing measured"
                            .to_string(),
                    );
                }
                let mut idx = [0usize; 5];
                for (i, f) in found.iter().enumerate() {
                    idx[i] = f[0];
                }
                at = Some(idx);
            }
            continue;
        }
        let idx = at.unwrap();
        let get = |i: usize| -> &str { cells.get(idx[i]).copied().unwrap_or("") };
        let num = |i: usize| -> Result<u32, String> {
            get(i)
                .trim()
                .trim_matches('*')
                .parse()
                .map_err(|_| format!("grid row {line:?}: {:?} is not a count", get(i)))
        };
        let text_of = |i: usize| -> &str { get(i).trim().trim_matches('`').trim() };
        rows.push(GridRow {
            model: ProbeModel::parse(text_of(0)).ok_or_else(|| {
                format!(
                    "grid row {line:?}: {:?} is not a probed model; accepted: {}. `sonnet` \
                     cells are REUSED from the `remeasure-*` stages, so a `sonnet` row in the \
                     measured grid would report this phase's own measurement of runs it never \
                     made",
                    text_of(0),
                    ProbeModel::accepted(),
                )
            })?,
            skill: SkillName::parse(text_of(1)).ok_or_else(|| {
                format!(
                    "grid row {line:?}: {:?} is not a measured skill; accepted: {}",
                    text_of(1),
                    SkillName::accepted(),
                )
            })?,
            condition: parse_condition(text_of(2)).ok_or_else(|| {
                format!(
                    "grid row {line:?}: {:?} is not a condition; accepted: {}",
                    text_of(2),
                    conditions_accepted(),
                )
            })?,
            compliant: num(3)?,
            runs: num(4)?,
        });
    }
    if rows.is_empty() {
        return Err(
            "no measured grid found — wanted a table with columns \
             model / skill / condition / compliant / runs"
                .to_string(),
        );
    }
    Ok(rows)
}

/// Recompute the grid from the verdicts and the blind map, and complain about
/// every way the two can disagree.
///
/// Returns complaints rather than asserting, following [`check_ledger`] and
/// [`check_blind_map`]: a check that panics inline can only ever be observed to
/// *pass* on a legal corpus. `cross_model_grid_check_refuses_a_grid_that_lies`
/// is the companion that proves it fires.
fn check_cross_model_grid(
    declared: &[GridRow],
    verdicts: &[(TranscriptId, bool)],
    map: &HashMap<TranscriptId, CrossModelEntry>,
) -> Vec<String> {
    let mut wrong = Vec::new();

    // The join has to be total in both directions or an arm-level count is drawn
    // from the wrong set of runs. **Both sides are `TranscriptId`**: a join whose
    // two halves are one validated type and one free string reports a malformed id
    // as a missing assignment, which blames the map for a typo in the verdicts.
    for (id, _) in verdicts {
        if !map.contains_key(id) {
            wrong.push(format!(
                "cross-model-scores.json scores {}, which the blind map does not assign",
                id.as_str()
            ));
        }
    }
    let scored: HashSet<&TranscriptId> = verdicts.iter().map(|(i, _)| i).collect();
    for id in map.keys() {
        if !scored.contains(id) {
            wrong.push(format!(
                "the blind map assigns {}, which cross-model-scores.json does not score",
                id.as_str()
            ));
        }
    }

    // Recompute every cell from the data.
    let mut counted: HashMap<GridCell, (u32, u32)> = HashMap::new();
    for (id, compliant) in verdicts {
        let Some(e) = map.get(id) else {
            continue;
        };
        let cell = counted.entry((e.model, e.skill, e.arm)).or_insert((0, 0));
        cell.1 += 1;
        if *compliant {
            cell.0 += 1;
        }
    }

    for row in declared {
        let key = (row.model, row.skill, row.condition);
        match counted.get(&key) {
            None => wrong.push(format!(
                "the grid declares {}, which no transcript in this stage measured",
                cell_name(&key)
            )),
            Some((c, n)) => {
                if (*c, *n) != (row.compliant, row.runs) {
                    wrong.push(format!(
                        "the grid says {} is {} of {}, the verdicts say {c} of {n}",
                        cell_name(&key),
                        row.compliant,
                        row.runs
                    ));
                }
            }
        }
    }
    // …and no measured cell may be left out of the grid. Without this, dropping a
    // row that came out flat would leave every remaining row still checking out.
    for key in counted.keys() {
        if !declared
            .iter()
            .any(|r| (r.model, r.skill, r.condition) == *key)
        {
            wrong.push(format!(
                "{} was measured but the grid does not report it — null and negative \
                 results are recorded beside positive ones (spec §7.3)",
                cell_name(key)
            ));
        }
    }
    wrong
}

/// The second-pass sample `cross-model.md` declares — `(sample, corpus)` read out
/// of *"A second, independent pass on N of the M"*.
///
/// **The denominator has to come from somewhere other than the record being
/// checked.** `check_cross_model_adjudication` compares the record's length
/// against this, and a length compared against itself is not a comparison. This is
/// the cross-model analogue of the `runs` count the `remeasure-*` guard reads out
/// of the per-skill evidence doc.
///
/// [`SecondPassDeclaration`], in a module so its fields are genuinely **private**.
///
/// The only module in this file, and it earns itself: the type's whole job is that
/// its two counts cannot be confused or contradict each other, and a struct
/// literal written anywhere else in a 10,000-line file undoes both. Inside these
/// braces there is one constructor and it checks the invariant.
mod second_pass {
    /// What `cross-model.md` declares about its second pass: a `sample` drawn from
    /// a `corpus`.
    ///
    /// **Named fields rather than a `(usize, usize)`.** The two counts have the
    /// same type and a containment relation between them, so a bare tuple makes
    /// swapping them at the destructure a representable mistake no type would
    /// catch — and swapping them would hold a 16-record file to a denominator of
    /// 78 while checking the 78-transcript corpus against 16.
    #[derive(Debug, PartialEq, Eq)]
    pub struct SecondPassDeclaration {
        sample: usize,
        corpus: usize,
    }

    impl SecondPassDeclaration {
        /// The only way to build one, and it refuses `sample > corpus`.
        ///
        /// A sample larger than the corpus it is drawn from is not a number that
        /// needs interpreting downstream — it is a document that has lost track of
        /// what it sampled, and it is refused where it is read.
        ///
        /// `sample == 0` is deliberately **allowed** here. A declared second pass
        /// over nothing is caught by
        /// [`check_cross_model_adjudication`](super::check_cross_model_adjudication)'s
        /// empty-record complaint, which is where "a preserved second pass records
        /// verdicts" is already stated; refusing it here as well would put one rule
        /// in two places, and the two would eventually disagree.
        pub fn new(sample: usize, corpus: usize) -> Result<Self, String> {
            if sample > corpus {
                return Err(format!(
                    "a second pass over {sample} of {corpus} samples more transcripts than \
                     were scored — the sample is drawn FROM the corpus"
                ));
            }
            Ok(SecondPassDeclaration { sample, corpus })
        }

        /// How many transcripts the second pass re-read.
        pub fn sample(&self) -> usize {
            self.sample
        }

        /// How many the primary pass scored, which the sample is drawn from.
        pub fn corpus(&self) -> usize {
            self.corpus
        }
    }
}

use second_pass::SecondPassDeclaration;

/// Exactly one declaration, for [`parse_grid`]'s reason: two sentences claiming
/// different sample sizes has no obvious right answer and taking the first
/// silently picks one.
///
/// **What this does NOT close, stated rather than left to be discovered.** The
/// denominator is prose, so deleting a record *and* editing this sentence down to
/// match still passes — the same residual the `remeasure-*` guard has, whose
/// `runs` count is likewise read out of a document. Closing it would need a
/// second, independent statement of the sample size to cross-check against, and
/// the only one that exists (`run-ledger.md`'s narrative paragraph) is prose too.
/// Two prose parses over one fact is a heuristic backstop behind an authoritative
/// guard, not a second authority, so this stays one guard and the gap is recorded
/// here instead.
fn parse_second_pass_declaration(text: &str) -> Result<SecondPassDeclaration, String> {
    const LEAD: &str = "A second, independent pass on ";
    let mut found: Option<SecondPassDeclaration> = None;
    for line in text.lines() {
        let Some((_, rest)) = line.split_once(LEAD) else {
            continue;
        };
        let mut words = rest.split_whitespace();
        let count = |word: Option<&str>, what: &str| -> Result<usize, String> {
            word.map(|w| w.trim_end_matches([',', '.', '*']))
                .and_then(|w| w.parse::<usize>().ok())
                .ok_or_else(|| format!("{line:?}: the {what} is not a count"))
        };
        let sample = count(words.next(), "sample size")?;
        let (of, the) = (words.next(), words.next());
        if (of, the) != (Some("of"), Some("the")) {
            return Err(format!(
                "{line:?}: expected \"{LEAD}N of the M\"; the sample size is not followed by \
                 \"of the\", so the corpus it is a sample OF is not stated"
            ));
        }
        let corpus = count(words.next(), "corpus size")?;
        if found.is_some() {
            return Err(
                "two second-pass declarations — there is one second pass, and taking the \
                 first would let a second sentence state a different sample size unread"
                    .to_string(),
            );
        }
        found = Some(SecondPassDeclaration::new(sample, corpus).map_err(|e| format!("{line:?}: {e}"))?);
    }
    found.ok_or_else(|| {
        format!("no second-pass declaration found — wanted a line reading \"{LEAD}N of the M\"")
    })
}

/// Every way the second-pass record can disagree with the primary verdicts, plus
/// the recomputed agreement count.
///
/// **Extracted rather than left inline, because inline is how a guard becomes
/// vacuous.** The first version asserted all of this inside
/// `cross_model_grid_matches_its_own_verdicts`, where on a legal corpus it could
/// only ever be observed to *pass* — the exact shape of every dead guard this run
/// has found. The mutations that proved it fired were run by hand and committed
/// nowhere, so nothing durable stood behind the claim that it worked. Returning
/// complaints, the way [`check_ledger`] and [`check_cross_model_grid`] do, is what
/// lets `cross_model_adjudication_check_refuses_a_record_that_lies` demonstrate it.
/// `primary` is keyed by [`TranscriptId`] and not by `&str` for
/// [`check_cross_model_grid`]'s reason: both sides of this lookup are ids from the
/// same domain, and typing one of them as a bare string would put the format rule
/// back on whoever builds the map.
///
/// `declared` is the whole [`SecondPassDeclaration`] rather than the `usize` peeled
/// off it. That struct exists precisely because its two counts are confusable, and
/// a `usize` parameter would let a caller hand over `corpus` with nothing to
/// notice. **Both halves are checked here**, for the same reason: taking the whole
/// declaration and then enforcing only `sample` would leave `corpus` a caller
/// obligation, and a call site that met the coverage rule while the document
/// declared the wrong denominator — *"16 of 70"* against 78 verdicts — would come
/// back clean. `primary` IS the corpus, so nothing extra has to be passed to check
/// it.
fn check_cross_model_adjudication(
    adjudications: &[CrossModelAdjudication],
    primary: &HashMap<&TranscriptId, bool>,
    declared: &SecondPassDeclaration,
) -> (Vec<String>, usize) {
    let mut wrong = Vec::new();
    let mut agreed = 0usize;
    let mut seen: HashSet<&TranscriptId> = HashSet::new();

    // **The record covers the whole declared sample — exactly, not at least.**
    // The predecessor asserted only that the file was non-empty, then checked the
    // records against each other. Every one of those checks is internal, so
    // deleting a record and editing the quoted line from "16 of 16" to "15 of 15"
    // left `wrong` empty and the claim assert green: a second pass that skipped a
    // transcript, reported as a complete one.
    //
    // The remeasure stage in this same file has forbidden exactly that since it
    // found a missing re-adjudication leaving the suite green — `records.len() ==
    // runs`, "a partial re-adjudication cannot support \"all N agreed\"". Two
    // guards over the same kind of artifact cannot disagree about whether
    // shrinkage is legal, and nothing about a *pre-registered* second-pass sample
    // makes a short one more legitimate than a short re-adjudication: the
    // denominator is declared before the pass runs, so a record below it is a
    // transcript that was never re-read. The strict reading wins.
    if primary.len() != declared.corpus() {
        wrong.push(format!(
            "`cross-model.md` declares a second pass over a corpus of {}, but {} \
             transcript(s) were scored. The sample is drawn from the primary verdicts, so \
             a denominator that does not match them describes some other set of runs",
            declared.corpus(),
            primary.len(),
        ));
    }

    let sample = declared.sample();
    if adjudications.len() != sample {
        wrong.push(format!(
            "the second-pass record holds {} record(s) against the {sample} \
             `cross-model.md` declares it re-read. A partial second pass cannot support \
             \"{sample} of {sample} agree\", and a longer one re-read transcripts the \
             document does not account for",
            adjudications.len(),
        ));
    }

    if adjudications.is_empty() {
        wrong.push(
            "the second-pass record is empty — a preserved second pass records verdicts"
                .to_string(),
        );
        return (wrong, 0);
    }

    for a in adjudications {
        // **One record per transcript.** The prose claim is keyed on the record
        // count, so fifteen unique transcripts plus one duplicate — both agreeing
        // — would read as "16 of 16 agree" while one transcript was never
        // re-scored and another was counted twice.
        if !seen.insert(&a.transcript_id) {
            wrong.push(format!(
                "two records for transcript {} — the agreement count is keyed on the record \
                 count, so a duplicate silently stands in for a transcript that was never \
                 re-read",
                a.transcript_id.as_str()
            ));
        }
        match primary.get(&a.transcript_id) {
            None => wrong.push(format!(
                "re-reads {}, which the primary verdicts do not score",
                a.transcript_id.as_str()
            )),
            Some(got) if *got != a.primary_compliant => wrong.push(format!(
                "records primary_compliant={} for {}, but the primary verdict says {got}",
                a.primary_compliant,
                a.transcript_id.as_str(),
            )),
            Some(_) => {}
        }
        // Recomputed, never trusted — the rule the `remeasure-*` stages applied
        // to `matches_key`.
        if a.agrees != (a.primary_compliant == a.second_pass_compliant) {
            wrong.push(format!(
                "{} records agrees={} against primary={} second={}",
                a.transcript_id.as_str(),
                a.agrees,
                a.primary_compliant,
                a.second_pass_compliant,
            ));
        }
        agreed += usize::from(a.agrees);
    }
    (wrong, agreed)
}

/// [`check_cross_model_adjudication`] fires on each way the second pass can lie.
///
/// The real record is legal, so in the test above this check can only be observed
/// to pass. This is the demonstration that it can fail.
#[test]
fn cross_model_adjudication_check_refuses_a_record_that_lies() {
    let rec = |id: &str, second: bool, primary: bool, agrees: bool| {
        serde_json::from_value::<CrossModelAdjudication>(serde_json::json!({
            "transcript_id": id, "second_pass_compliant": second,
            "primary_compliant": primary, "agrees": agrees
        }))
        .expect("fixture record")
    };
    let id = |s: &str| TranscriptId::try_from(s.to_string()).expect("fixture id");
    let (a, b, c) = (id("aaaaaa"), id("bbbbbb"), id("cccccc"));
    let primary: HashMap<&TranscriptId, bool> =
        HashMap::from([(&a, true), (&b, false), (&c, true)]);
    // The declared sample the record is held to. The corpus is the three
    // transcripts `primary` scores.
    let decl = |sample: usize| {
        SecondPassDeclaration::new(sample, 3).expect("fixture declaration")
    };

    // Control. If this fails, every negative case below is meaningless.
    let ok = vec![
        rec("aaaaaa", true, true, true),
        rec("bbbbbb", true, false, false),
    ];
    let (wrong, agreed) = check_cross_model_adjudication(&ok, &primary, &decl(2));
    assert!(wrong.is_empty(), "{wrong:?}");
    assert_eq!(agreed, 1, "the agreement count is recomputed, not carried");

    // 1. A duplicate inflating the count: two unique transcripts' worth of
    //    records covering one transcript twice, every field self-consistent.
    let dup = vec![
        rec("aaaaaa", true, true, true),
        rec("aaaaaa", true, true, true),
    ];
    assert!(
        check_cross_model_adjudication(&dup, &primary, &decl(2))
            .0
            .iter()
            .any(|c| c.contains("two records for transcript aaaaaa")),
        "{:?}",
        check_cross_model_adjudication(&dup, &primary, &decl(2)).0
    );

    // 2. `agrees` that does not follow from the two booleans beside it.
    let lying = vec![rec("aaaaaa", false, true, true)];
    assert!(
        check_cross_model_adjudication(&lying, &primary, &decl(1))
            .0
            .iter()
            .any(|c| c.contains("records agrees=true")),
        "{:?}",
        check_cross_model_adjudication(&lying, &primary, &decl(1)).0
    );

    // 3. `primary_compliant` contradicting the primary verdicts while staying
    //    internally consistent — the mutation a self-consistency check misses.
    let contradicts = vec![rec("bbbbbb", true, true, true)];
    assert!(
        check_cross_model_adjudication(&contradicts, &primary, &decl(1))
            .0
            .iter()
            .any(|c| c.contains("primary verdict says false")),
        "{:?}",
        check_cross_model_adjudication(&contradicts, &primary, &decl(1)).0
    );

    // 4. A record for a transcript nobody scored.
    let orphan = vec![rec("dddddd", true, true, true)];
    assert!(
        check_cross_model_adjudication(&orphan, &primary, &decl(1))
            .0
            .iter()
            .any(|c| c.contains("dddddd") && c.contains("do not score")),
        "{:?}",
        check_cross_model_adjudication(&orphan, &primary, &decl(1)).0
    );

    // 5. An empty record is a complaint, not a silent zero — a zero-length
    //    second pass would otherwise satisfy "0 of 0 agree". Checked with a
    //    declared size of 0, so it is the emptiness that fires and not the
    //    coverage check standing in for it.
    assert!(
        check_cross_model_adjudication(&[], &primary, &decl(0))
            .0
            .iter()
            .any(|c| c.contains("empty")),
    );

    // 5b. **A SHRUNK record.** Every surviving record is self-consistent, scores a
    //     transcript the primary pass scored, and agrees — so every check above
    //     passes on it, and before the coverage check the prose could be edited
    //     from "2 of 2" to "1 of 1" and the suite stayed green while one
    //     transcript went un-re-read. This is the hole `remeasure-*`'s
    //     `records.len() == runs` has guarded since it was found there.
    let shrunk = vec![rec("aaaaaa", true, true, true)];
    assert!(
        check_cross_model_adjudication(&shrunk, &primary, &decl(2))
            .0
            .iter()
            .any(|c| c.contains("holds 1 record(s) against the 2")),
        "{:?}",
        check_cross_model_adjudication(&shrunk, &primary, &decl(2)).0
    );

    // 5c. …and the mirror: a record LONGER than the declared sample re-read
    //     transcripts the document does not account for. Not symmetric with a
    //     retried probe — a re-read produces no measurement, so there is no
    //     "attempt cost" reading under which an extra record is legitimate.
    let extra = vec![
        rec("aaaaaa", true, true, true),
        rec("bbbbbb", true, false, false),
        rec("cccccc", true, true, true),
    ];
    assert!(
        check_cross_model_adjudication(&extra, &primary, &decl(2))
            .0
            .iter()
            .any(|c| c.contains("holds 3 record(s) against the 2")),
        "{:?}",
        check_cross_model_adjudication(&extra, &primary, &decl(2)).0
    );

    // 5d. The OTHER half of the declaration: a corpus that is not the set the
    //     sample was drawn from. Every record here is legal and covers the
    //     declared sample, so nothing else fires — the document has simply lost
    //     track of how many transcripts it scored, and "16 of 70" against 78
    //     verdicts is a sample of something this stage did not measure.
    let wrong_corpus = SecondPassDeclaration::new(2, 70).expect("fixture declaration");
    assert!(
        check_cross_model_adjudication(&ok, &primary, &wrong_corpus)
            .0
            .iter()
            .any(|c| c.contains("corpus of 70, but 3 transcript(s) were scored")),
        "{:?}",
        check_cross_model_adjudication(&ok, &primary, &wrong_corpus).0
    );

    // 6. A malformed id fails at DESERIALIZATION, so it never reaches the checks
    //    above — asserted here because that is the only place it is observable.
    assert!(
        serde_json::from_value::<CrossModelAdjudication>(serde_json::json!({
            "transcript_id": "ZZZZZZ", "second_pass_compliant": true,
            "primary_compliant": true, "agrees": true
        }))
        .is_err(),
        "a non-hex transcript id must not deserialize"
    );

    // 7. …and an unknown key the same way: `deny_unknown_fields` is load-bearing
    //    on a record whose shape differs from both other adjudication types.
    assert!(
        serde_json::from_value::<CrossModelAdjudication>(serde_json::json!({
            "transcript_id": "aaaaaa", "second_pass_compliant": true,
            "primary_compliant": true, "agrees": true, "note": "x"
        }))
        .is_err(),
        "an unknown key must not be silently accepted"
    );
}

/// `cross-model.md`'s grid is what its own transcripts and verdicts say, and the
/// ledger charged for exactly those runs on the right side of the metered line.
///
/// **What this closes.** The cross-model transcripts sit outside
/// `transcripts/<skill>/`, so none of [`VerdictBundle`]'s per-skill checks visit
/// them: `scores_json_verdicts_obey_the_rubric` iterates `SkillName::ALL` and
/// walks straight past this directory. A verdict set with no schema and a prose
/// grid with nothing recomputing it is precisely the drift this corpus keeps
/// finding, so the new artifacts arrive with their own guard rather than
/// inheriting one that does not reach them.
#[test]
fn cross_model_grid_matches_its_own_verdicts() {
    let evidence = evidence_dir();
    let doc = evidence.join("cross-model.md");
    let dir = evidence.join("transcripts/cross-model");

    let text = fs::read_to_string(&doc)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", doc.display()));

    // Until the probes land there is no grid, and a check that quietly passed on
    // its absence would be the vacuous guard this file exists to prevent. So the
    // pre-registration is allowed to stand alone — and ONLY while the transcript
    // directory does not exist. The moment it does, everything below is required.
    if !dir.exists() {
        assert!(
            parse_grid(&text).is_err(),
            "{} declares a measured grid but {} does not exist — a grid with no \
             transcripts behind it",
            doc.display(),
            dir.display(),
        );
        return;
    }

    let map_path = dir.join("cross-model-blind-map.json");
    let scores_path = dir.join("cross-model-scores.json");
    for p in [&map_path, &scores_path] {
        assert!(
            p.is_file(),
            "{} exists, so {} must too — a scored stage's arm assignment is what makes \
             its verdicts attributable (scoring-rubric.md Part B)",
            dir.display(),
            p.display(),
        );
    }

    // Keyed by [`TranscriptId`], so a malformed key is a parse error naming the
    // key rather than an opaque join mismatch two checks downstream.
    let map: HashMap<TranscriptId, CrossModelEntry> =
        serde_json::from_str(&fs::read_to_string(&map_path).expect("map unreadable"))
            .unwrap_or_else(|e| panic!("{} does not match the cross-model blind-map schema: {e}", map_path.display()));

    // The same closed verdict object every other stage records, held to the same
    // rubric rules against the same transcript.
    let verdicts = read_verdicts(&scores_path);
    // `pairs` carries parsed ids, not the raw strings [`RawVerdict`] holds: this is
    // the boundary where a scorer's untouched output stops being evidence and
    // becomes a join key, so it is where the format rule is applied.
    let mut pairs: Vec<(TranscriptId, bool)> = Vec::new();
    let mut seen: HashSet<&str> = HashSet::new();
    for v in &verdicts {
        let id = v.0.transcript_id.as_str();
        assert!(
            seen.insert(id),
            "{}: two verdicts for transcript {id}",
            scores_path.display()
        );
        let transcript = resolve_transcript(&dir, id, &scores_path);
        let body = fs::read_to_string(&transcript)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", transcript.display()));
        v.check_rubric_rules(&response_block(&body), &scores_path);
        pairs.push((TranscriptId::parse(id, &scores_path), v.0.compliant));
    }

    // Every transcript file in the directory is scored. Key-set equality against
    // the map is checked below; this catches a transcript that is in neither.
    //
    // **A `.md` whose stem is not a transcript id is an error, not a skip.** The
    // first version `continue`d past it while the comment claimed every
    // transcript file was checked, so an orphan probe output under a mistyped
    // name would be neither scored nor reported — and the redaction scan uses
    // the same predicate, so it would go unredacted too.
    for entry in fs::read_dir(&dir).expect("cannot read cross-model dir") {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        assert!(
            is_transcript_id(stem),
            "{} is a .md file in the transcript directory whose stem is not a 6-hex \
             transcript id, so every id-keyed check here and `check_redaction` both \
             skip it silently",
            path.display(),
        );
        assert!(
            seen.contains(stem),
            "{} exists but {} does not score it — an unscored transcript is a run that \
             happened and was not counted",
            path.display(),
            scores_path.display(),
        );
    }

    // The second blind pass is evidence in its own right, and `cross-model.md`
    // quotes its agreement count. An unvalidated verdict-like file beside a
    // validated one is the drift this corpus keeps finding, so it gets the same
    // treatment: a closed schema, a recomputed `agrees`, and a join back to the
    // primary verdicts rather than a `primary_compliant` taken on trust.
    let adj_path = dir.join("cross-model-adjudication.json");
    assert!(
        adj_path.is_file(),
        "{} is missing — `cross-model.md` reports a second-pass agreement count, and \
         deleting its record must not leave that claim standing with nothing behind it",
        adj_path.display(),
    );
    let adjudications: Vec<CrossModelAdjudication> =
        serde_json::from_str(&fs::read_to_string(&adj_path).expect("adjudication unreadable"))
            .unwrap_or_else(|e| panic!("{} does not match the schema: {e}", adj_path.display()));

    // The denominator the record is held to, read out of the document rather than
    // off the record itself — see [`parse_second_pass_declaration`]. Both halves of
    // it are enforced by the check below, not here: a guard that only this test
    // runs can only ever be observed to pass.
    let declaration = parse_second_pass_declaration(&text)
        .unwrap_or_else(|e| panic!("{}: {e}", doc.display()));

    let primary: HashMap<&TranscriptId, bool> = pairs.iter().map(|(i, c)| (i, *c)).collect();
    let (wrong, agreed) = check_cross_model_adjudication(&adjudications, &primary, &declaration);
    assert!(wrong.is_empty(), "{}: {}", adj_path.display(), wrong.join("\n"));

    let claim = format!("**{agreed} of {} agree on\n`compliant`.**", adjudications.len());
    let claim_flat = format!("**{agreed} of {} agree on `compliant`.**", adjudications.len());
    assert!(
        text.contains(&claim) || text.contains(&claim_flat),
        "{} must state the recomputed second-pass result verbatim ({agreed} of {}); \
         the prose and this record are one fact",
        doc.display(),
        adjudications.len(),
    );

    let declared = parse_grid(&text).unwrap_or_else(|e| panic!("{}: {e}", doc.display()));
    let wrong = check_cross_model_grid(&declared, &pairs, &map);
    assert!(wrong.is_empty(), "{}: {}", doc.display(), wrong.join("\n"));

    // The ledger charged for these runs, on the right side of the metered line.
    // `check_ledger` proves the column adds up; this proves it adds up to the
    // number of transcripts that actually exist.
    let ledger_path = evidence.join(EVIDENCE_LEDGER);
    let ledger_text = fs::read_to_string(&ledger_path).expect("ledger unreadable");
    let ledger = parse_ledger(&ledger_text).expect("ledger does not parse");
    for &model in ProbeModel::ALL {
        let measured = map.values().filter(|e| e.model == model).count() as u32;
        let cell = format!("cross-model ({})", model.as_str());
        // Whole-token match on the stage NAME — see [`stage_name`]. A `contains`
        // here sums in any row that merely mentions the stage.
        let rows: Vec<&LedgerRow> = ledger
            .iter()
            .filter(|r| stage_name(&r.stage) == cell)
            .collect();
        assert!(
            !rows.is_empty(),
            "{} charges no row matching {cell:?}, but {measured} of its transcripts exist",
            ledger_path.display(),
        );
        let charged: u32 = rows.iter().map(|r| r.runs).sum();
        assert!(
            charged >= measured,
            "{} charges {charged} run(s) for {cell:?} against {measured} transcript(s). A \
             retried run counts, so the charge is at or above the number of transcripts, \
             never below it",
            ledger_path.display(),
        );
        for r in &rows {
            assert_eq!(
                !r.stage.contains(UNMETERED_MARKER),
                model.is_metered(),
                "{}: row {:?} puts {} on the wrong side of the metered line",
                ledger_path.display(),
                r.stage,
                model.as_str(),
            );
        }
    }
}

/// [`check_cross_model_grid`] fires on each way the prose and the data can part.
///
/// The real grid is legal, so on it the check above can only be observed to pass
/// — the shape of every vacuous guard this run has found.
#[test]
fn cross_model_grid_check_refuses_a_grid_that_lies() {
    let entry = |model: &str, skill: &str, arm: &str| {
        serde_json::from_value::<CrossModelEntry>(serde_json::json!({
            "arm": arm, "scenario": "tdd-2", "sample": 1, "model": model, "skill": skill
        }))
        .expect("fixture entry")
    };
    let id = |s: &str| TranscriptId::try_from(s.to_string()).expect("fixture id");
    let map = || -> HashMap<TranscriptId, CrossModelEntry> {
        HashMap::from([
            (id("aaaaaa"), entry("qwen", "tdd", "B")),
            (id("bbbbbb"), entry("qwen", "tdd", "B")),
            (id("cccccc"), entry("opus", "systematic-debugging", "none")),
        ])
    };
    let verdicts = || {
        vec![
            (id("aaaaaa"), true),
            (id("bbbbbb"), false),
            (id("cccccc"), false),
        ]
    };
    let row = |m: ProbeModel, s: SkillName, c: BlindArm, k: u32, n: u32| GridRow {
        model: m, skill: s, condition: c, compliant: k, runs: n,
    };
    use BlindArm::{None as Unaided, B};
    use ProbeModel::{Opus, Qwen};
    use SkillName::{SystematicDebugging, Tdd};

    // Control. If this fails, every negative case below is meaningless.
    let ok = vec![
        row(Qwen, Tdd, B, 1, 2),
        row(Opus, SystematicDebugging, Unaided, 0, 1),
    ];
    assert!(
        check_cross_model_grid(&ok, &verdicts(), &map()).is_empty(),
        "{:?}",
        check_cross_model_grid(&ok, &verdicts(), &map())
    );

    // 1. An inflated compliant count — the failure that would matter most.
    let lied = vec![
        row(Qwen, Tdd, B, 2, 2),
        row(Opus, SystematicDebugging, Unaided, 0, 1),
    ];
    assert!(
        check_cross_model_grid(&lied, &verdicts(), &map())
            .iter()
            .any(|c| c.contains("the grid says") && c.contains("2 of 2") && c.contains("1 of 2")),
        "{:?}",
        check_cross_model_grid(&lied, &verdicts(), &map())
    );

    // 2. A measured cell dropped from the grid — the quiet way a flat result
    //    disappears while every surviving row still checks out.
    let dropped = vec![row(Qwen, Tdd, B, 1, 2)];
    assert!(
        check_cross_model_grid(&dropped, &verdicts(), &map())
            .iter()
            .any(|c| c.contains("was measured but the grid does not report it")),
        "{:?}",
        check_cross_model_grid(&dropped, &verdicts(), &map())
    );

    // 3. A declared cell nothing measured. `opus`/`tdd` is the pointed case: those
    //    cells were deliberately not bought, so a row claiming them is a claim
    //    about runs the metered budget never paid for.
    //
    //    A smuggled `sonnet` row cannot be built here at all any more — `sonnet`
    //    is not a `ProbeModel`, so it fails in `parse_grid` naming the dimension
    //    rather than reaching this check as a join mismatch. That case moved to
    //    `cross_model_grid_parser_ignores_the_other_tables`.
    let invented = vec![
        row(Qwen, Tdd, B, 1, 2),
        row(Opus, SystematicDebugging, Unaided, 0, 1),
        row(Opus, Tdd, BlindArm::A, 4, 4),
    ];
    assert!(
        check_cross_model_grid(&invented, &verdicts(), &map())
            .iter()
            .any(|c| c.contains("which no transcript in this stage measured")),
        "{:?}",
        check_cross_model_grid(&invented, &verdicts(), &map())
    );

    // 4. A scored transcript the map does not assign — unattributable.
    let mut short = map();
    short.remove(&id("bbbbbb"));
    assert!(
        check_cross_model_grid(&ok, &verdicts(), &short)
            .iter()
            .any(|c| c.contains("bbbbbb") && c.contains("does not assign")),
        "{:?}",
        check_cross_model_grid(&ok, &verdicts(), &short)
    );

    // 5. …and the mirror: a mapped transcript nobody scored, a claimed run that
    //    never produced a verdict.
    let mut fewer = verdicts();
    fewer.retain(|(i, _)| *i != id("cccccc"));
    assert!(
        check_cross_model_grid(&ok, &fewer, &map())
            .iter()
            .any(|c| c.contains("cccccc") && c.contains("does not score")),
        "{:?}",
        check_cross_model_grid(&ok, &fewer, &map())
    );

    // 6. `sonnet` is not a `ProbeModel`, so a map entry naming it is a parse
    //    error rather than a fifth silent model. This is what keeps the reused
    //    reference column out of the measured data.
    assert!(
        serde_json::from_value::<CrossModelEntry>(serde_json::json!({
            "arm": "B", "scenario": "tdd-2", "sample": 1, "model": "sonnet", "skill": "tdd"
        }))
        .is_err(),
        "`sonnet` must not deserialize as a probed model — its cells are reused, not run"
    );

    // 7. An extra key fails the same way: `deny_unknown_fields` is load-bearing
    //    on a map that grew two fields relative to `BlindMapEntry`.
    assert!(
        serde_json::from_value::<CrossModelEntry>(serde_json::json!({
            "arm": "B", "scenario": "tdd-2", "sample": 1, "model": "qwen",
            "skill": "tdd", "note": "x"
        }))
        .is_err(),
        "an unknown key must not be silently accepted"
    );

    // 8. A skill outside the closed set is a parse error too — the field is a
    //    `SkillName`, not a string that was glanced at on the way past.
    assert!(
        serde_json::from_value::<CrossModelEntry>(serde_json::json!({
            "arm": "B", "scenario": "tdd-2", "sample": 1, "model": "qwen", "skill": "handoff"
        }))
        .is_err(),
        "`handoff` is not a measured skill and must not deserialize as one"
    );

    // 9. **The map's KEYS are ids, and they are parsed as ids.** The values were
    //    validated while the keys were plain `String`s, so a malformed key
    //    deserialized cleanly and only surfaced downstream as "scores X, which the
    //    blind map does not assign" — a join complaint blaming the verdicts for a
    //    typo in the map.
    let bad_key = serde_json::json!({
        "ZZZZZZ": {"arm": "B", "scenario": "tdd-2", "sample": 1, "model": "qwen", "skill": "tdd"}
    });
    assert!(
        serde_json::from_value::<HashMap<TranscriptId, CrossModelEntry>>(bad_key).is_err(),
        "a blind-map key that is not a 6-hex transcript id must not deserialize"
    );
    let good_key = serde_json::json!({
        "aaaaaa": {"arm": "B", "scenario": "tdd-2", "sample": 1, "model": "qwen", "skill": "tdd"}
    });
    assert!(
        serde_json::from_value::<HashMap<TranscriptId, CrossModelEntry>>(good_key).is_ok(),
        "…and a legal one must still deserialize — the control for the case above"
    );
}

/// Every [`BlindArm`] has a grid condition name, and every condition name parses
/// back to the arm it came from.
///
/// The pair [`BlindArm::condition_name`] / [`parse_condition`] is a bijection or
/// the typed `GridRow` is a downgrade: an arm whose condition name no other arm's
/// spelling collides with is what makes a legal grid cell parse and an illegal one
/// fail. [`blind_arms!`] makes `BlindArm::ALL` complete by construction, so what
/// is left to guard is the mapping itself.
#[test]
fn grid_conditions_round_trip_every_arm() {
    for arm in BlindArm::ALL {
        assert_eq!(
            parse_condition(arm.condition_name()),
            Some(*arm),
            "{arm:?} does not round-trip through its condition name",
        );
    }

    // Six arms and six distinct names. `ALL` is complete by construction — it and
    // the variants come out of the same `blind_arms!` table — so what is left to
    // check is that no two arms share a name, which would make `parse_condition`
    // silently pick one of them.
    let names: HashSet<&str> = BlindArm::ALL.iter().map(|a| a.condition_name()).collect();
    assert_eq!(
        names.len(),
        BlindArm::ALL.len(),
        "two arms share a grid condition name: {names:?}",
    );

    assert_eq!(parse_condition("none"), None, "the WIRE name is not the grid name");
    assert_eq!(parse_condition("A prime"), None);
    assert_eq!(parse_condition("Unaided"), None);
}

/// [`stage_name`] matches a ledger stage as a whole token, not as a substring.
///
/// The cross-charge check filters rows by stage name, and the version this
/// replaced used `contains`. These are the cells the ledger actually carries plus
/// the one that motivated the change.
#[test]
fn ledger_stage_names_are_whole_tokens() {
    assert_eq!(
        stage_name("**cross-model (qwen) — UNMETERED, not a §7.3 row**"),
        "cross-model (qwen)",
    );
    assert_eq!(
        stage_name("**cross-model (opus) — not a §7.3 row**"),
        "cross-model (opus)",
    );
    // An unqualified, unemphasised cell is its own name.
    assert_eq!(stage_name("RED / baseline on dev set"), "RED / baseline on dev set");
    assert_eq!(
        stage_name("Arm A′ on held-out RE-MEASURED (`tdd`)"),
        "Arm A′ on held-out RE-MEASURED (`tdd`)",
    );

    // The case `contains` got wrong: a row that MENTIONS the cross-model charge is
    // not that row, and summing its runs into the charge would let it cover for a
    // missing one.
    assert_ne!(
        stage_name("**reconciliation of cross-model (qwen) totals — not a §7.3 row**"),
        "cross-model (qwen)",
    );
    // …and the narrower version of the same mistake: a longer name that starts
    // with a real one.
    assert_ne!(stage_name("cross-model (qwen) retries"), "cross-model (qwen)");
}

/// [`parse_second_pass_declaration`] reads the sample the record is held to, and
/// refuses the shapes that would let it read the wrong number.
#[test]
fn second_pass_declaration_is_read_from_the_document() {
    let real = "**A second, independent pass on 16 of the 78, and it is not a charged run.**";
    assert_eq!(
        parse_second_pass_declaration(real),
        SecondPassDeclaration::new(16, 78)
    );

    // Two declarations is an error, not a first-match win — `parse_grid`'s rule.
    let two = format!("{real}\nprose\nA second, independent pass on 15 of the 78.");
    assert!(parse_second_pass_declaration(&two)
        .unwrap_err()
        .contains("two second-pass declarations"));

    // No declaration is an error, not a default: a missing sentence must not
    // silently become a sample size of zero that any record satisfies.
    assert!(parse_second_pass_declaration("# nothing here\n")
        .unwrap_err()
        .contains("no second-pass declaration"));

    // A malformed one is an error rather than a partial read.
    assert!(parse_second_pass_declaration("A second, independent pass on some of the 78.").is_err());
    assert!(parse_second_pass_declaration("A second, independent pass on 16 transcripts.").is_err());
    assert!(parse_second_pass_declaration("A second, independent pass on 16 of the corpus.").is_err());

    // A sample larger than the corpus it is drawn from is refused where it is
    // read, not carried forward to fail as something else downstream.
    assert!(SecondPassDeclaration::new(100, 16)
        .unwrap_err()
        .contains("samples more transcripts than were scored"));
    assert!(
        parse_second_pass_declaration("A second, independent pass on 100 of the 16.")
            .unwrap_err()
            .contains("samples more transcripts than were scored")
    );
    // The boundary is legal: a second pass may cover the whole corpus.
    assert!(SecondPassDeclaration::new(16, 16).is_ok());
}

/// [`parse_grid`] finds the measured grid and nothing else in the document.
#[test]
fn cross_model_grid_parser_ignores_the_other_tables() {
    // The design table and the reused-`sonnet` table both sit above the grid in
    // `cross-model.md`, and the design table's `runs` column holds PLANNED counts.
    // A parser that latched onto the first table with a `runs` column would read
    // 64 planned qwen runs as a measurement.
    let doc = "\
| skill | unaided | arm A | arm A' | arm B | source |\n\
|---|---|---|---|---|---|\n\
| tdd | 0 of 4 | 4 of 4 | 2 of 4 | 4 of 4 | reused |\n\
\n\
| model | skills | conditions | scenarios | samples | runs | metered? |\n\
|---|---|---|---|---|---|---|\n\
| qwen | tdd | unaided, A | 2 each | 4 | 64 | no |\n\
\n\
| model | skill | condition | compliant | runs |\n\
|---|---|---|---|---|\n\
| qwen | tdd | unaided | 3 | 8 |\n\
| opus | systematic-debugging | B | 4 | 4 |\n\
\n\
Prose after the table, which ends it.\n\
\n\
| model | skill | condition | compliant | runs |\n\
|---|---|---|---|---|\n\
| qwen | systematic-debugging | A | 2 | 8 |\n";
    // A SECOND table with the grid's header is an error, not extra rows. The
    // first version of this assertion expected 3 — the two real rows plus the
    // trap table's — which encoded the relatch defect as the contract. The
    // reviewer caught the test, not just the parser.
    let err = parse_grid(doc).expect_err("a second measured grid must be refused");
    assert!(err.contains("two tables"), "{err:?}");

    // Without the trap table, the same document parses to exactly its two rows:
    // the reused-`sonnet` table and the design table (whose `runs` column holds
    // PLANNED counts, and which would read 64 qwen runs as a measurement) are
    // both passed over.
    let single = doc.split("Prose after the table").next().unwrap();
    let rows = parse_grid(single).expect("the grid must parse");
    assert_eq!(
        rows.len(),
        2,
        "expected the two grid rows and neither other table's rows: {rows:?}"
    );
    assert_eq!(rows[0], GridRow {
        model: ProbeModel::Qwen, skill: SkillName::Tdd, condition: BlindArm::None,
        compliant: 3, runs: 8,
    });
    assert_eq!(rows[1].skill, SkillName::SystematicDebugging);

    // A duplicated load-bearing column is a parse error, not a first-match-wins
    // silent rebind — the rule `parse_ledger` and `parse_manifest` already hold.
    let dup = "| model | skill | condition | compliant | runs | runs |\n\
               |---|---|---|---|---|---|\n\
               | qwen | tdd | unaided | 3 | 8 | 9 |\n";
    let err = parse_grid(dup).expect_err("a duplicated column must be refused");
    assert!(err.contains("runs") && err.contains("2"), "{err:?}");

    // A document with no grid is an error, not an empty result — an empty vec
    // would make every downstream check pass on a document that reports nothing.
    assert!(parse_grid("# nothing here\n").is_err());

    // **Each of the three dimensions is a closed set, refused BY NAME here rather
    // than surfacing downstream as a join mismatch.** A reused `sonnet` row
    // smuggled into the measured grid is the case that matters most: it would read
    // as this phase's own measurement of runs it never made.
    let grid = |model: &str, skill: &str, condition: &str| {
        format!(
            "| model | skill | condition | compliant | runs |\n\
             |---|---|---|---|---|\n\
             | {model} | {skill} | {condition} | 3 | 8 |\n"
        )
    };
    let err = parse_grid(&grid("sonnet", "tdd", "unaided"))
        .expect_err("`sonnet` is reused, not probed — it must not parse as a measured row");
    assert!(err.contains("is not a probed model") && err.contains("qwen, opus"), "{err:?}");

    let err = parse_grid(&grid("Qwen", "tdd", "unaided")).expect_err("a case typo is a typo");
    assert!(err.contains("is not a probed model"), "{err:?}");

    let err = parse_grid(&grid("qwen", "handoff", "unaided"))
        .expect_err("`handoff` is not a measured skill");
    assert!(err.contains("is not a measured skill"), "{err:?}");

    let err = parse_grid(&grid("qwen", "tdd", "A prime"))
        .expect_err("`A prime` is not how the grid spells `A-prime`");
    assert!(err.contains("is not a condition") && err.contains("A-prime"), "{err:?}");

    // The wire spelling of the unaided arm is `none`; the grid spells it
    // `unaided`, and the two are not interchangeable in either direction.
    let err = parse_grid(&grid("qwen", "tdd", "none")).expect_err("the wire name is not the grid name");
    assert!(err.contains("is not a condition"), "{err:?}");
    assert!(parse_grid(&grid("qwen", "tdd", "unaided")).is_ok(), "the control for all six above");
}
