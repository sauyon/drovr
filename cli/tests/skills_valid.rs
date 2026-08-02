//! Validates every `skills/*/SKILL.md` in the repo and enforces a body-size
//! budget on the four `drovr:*` methodology skills.
//!
//! Three assertions:
//!   1. **All** skills have valid frontmatter: a leading `---` block containing
//!      non-empty `name:` and `description:`, and `name:` equals the directory
//!      name.
//!   2. The four methodology skills (tdd, systematic-debugging,
//!      verification-before-completion, code-review) each have a
//!      post-frontmatter body of at most 2200 bytes. The pre-existing skills
//!      (using-drovr, handoff, pipeline) are NOT size-checked.
//!   3. The arm A snapshots under `docs/skill-evidence/arms/A/` still hash to
//!      the values `arms/MANIFEST.md` records. Arm A is the pre-fix baseline the
//!      whole measurement is compared against, and it is unrecoverable without a
//!      checkout once fix 1 rewrites the live `description:` lines — so this is a
//!      tripwire, not a formality.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Body-size budget (bytes) for the methodology skills.
const BODY_BUDGET: usize = 2200;

/// Skills subject to the body-size budget.
const METHODOLOGY_SKILLS: &[&str] = &[
    "tdd",
    "systematic-debugging",
    "verification-before-completion",
    "code-review",
];

fn skills_dir() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../skills"))
}

/// Root of the per-arm skill snapshots (`docs/skill-evidence/arms/`).
fn arms_dir() -> PathBuf {
    PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../docs/skill-evidence/arms"
    ))
}

/// The five skills snapshotted into every measurement arm.
const ARM_SNAPSHOT_SKILLS: &[&str] = &[
    "tdd",
    "systematic-debugging",
    "verification-before-completion",
    "code-review",
    "using-drovr",
];

/// A parsed SKILL.md: the frontmatter `name`/`description` and the body after
/// the closing `---`.
struct Skill {
    name: Option<String>,
    description: Option<String>,
    body: String,
}

/// Parse a SKILL.md's leading `---` frontmatter block. Returns `None` if the
/// file does not begin with a `---` fence or the fence is never closed.
///
/// Uses `split_inclusive('\n')` so each segment retains its line terminator.
/// That makes the running byte offset exact for both LF and CRLF endings — a
/// `\r\n` line's `\r` is part of the segment, so no per-line fixups are needed.
fn parse_skill(contents: &str) -> Option<Skill> {
    let mut segments = contents.split_inclusive('\n');

    // The file must start with a `---` line. `trim()` tolerates a leading
    // UTF-8 BOM and the line's own terminator.
    let first = segments.next()?;
    if first.trim() != "---" {
        return None;
    }

    let mut name = None;
    let mut description = None;
    let mut closed = false;
    // Byte length of the frontmatter (including both fences) so we can slice the
    // body out of the original string. `first.len()` includes its terminator.
    let mut consumed = first.len();

    for seg in segments.by_ref() {
        consumed += seg.len();
        if seg.trim() == "---" {
            closed = true;
            break;
        }
        if let Some(rest) = seg.strip_prefix("name:") {
            name = Some(rest.trim().to_string());
        } else if let Some(rest) = seg.strip_prefix("description:") {
            description = Some(rest.trim().to_string());
        }
    }

    if !closed {
        return None;
    }

    // Body is everything after the closing fence. Guard against `consumed`
    // running past the end (e.g. no trailing newline on the closing fence).
    let body = contents.get(consumed..).unwrap_or("").to_string();

    Some(Skill {
        name,
        description,
        body,
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
                 (expected the skill as the file stem or the parent directory)"
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
    let arms = arms_dir();
    let manifest_path = arms.join("MANIFEST.md");
    let contents = fs::read_to_string(&manifest_path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", manifest_path.display()));
    let rows =
        parse_manifest(&contents).unwrap_or_else(|e| panic!("{}: {e}", manifest_path.display()));

    // Everything that does not need git runs first, so a git-less environment
    // still reports a corrupt manifest rather than only "git is missing".
    let mut to_verify = Vec::new();
    for skill in ARM_SNAPSHOT_SKILLS {
        let matches: Vec<&ManifestRow> = rows
            .iter()
            .filter(|r| r.arm == "A" && r.skill == *skill)
            .collect();
        // A second row for `(A, skill)` can no longer parse, so in practice this
        // catches the *missing* row — a skill dropped from the manifest.
        assert_eq!(
            matches.len(),
            1,
            "{}: expected exactly one arm A row for `{skill}`, found {}",
            manifest_path.display(),
            matches.len()
        );

        // The hash cell's 40-hex format is guaranteed by `BlobSha`, which
        // `parse_manifest` validates for every row — a malformed cell already
        // failed above, with the offending line quoted.
        let expected = matches[0].hash.clone();

        // `parse_manifest` already enforces the loose rule that fits every arm
        // (the skill is the path's stem or its parent). Arm A's shape is known
        // exactly, so it is held to the exact path.
        let expected_source = format!("skills/{skill}/SKILL.md");
        assert_eq!(
            matches[0].source_path,
            expected_source,
            "{}: arm A row for `{skill}` records source path `{}`, expected `{expected_source}`",
            manifest_path.display(),
            matches[0].source_path
        );

        let snapshot = arms.join("A").join(format!("{skill}.md"));
        assert!(
            snapshot.is_file(),
            "missing arm A snapshot {}",
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
        "`git` is not resolvable, so the arm A snapshot hashes cannot be verified. \
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
                "{} does not begin with a closed `---` frontmatter block",
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

#[test]
fn methodology_skills_within_body_budget() {
    let dir = skills_dir();

    for skill_name in METHODOLOGY_SKILLS {
        let path = dir.join(skill_name).join("SKILL.md");
        assert!(
            path.is_file(),
            "expected methodology skill at {}",
            path.display()
        );
        let contents = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
        let skill = parse_skill(&contents).unwrap_or_else(|| {
            panic!(
                "{} does not begin with a closed `---` frontmatter block",
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
