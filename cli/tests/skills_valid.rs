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
fn git_hash_object(path: &Path) -> String {
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
    String::from_utf8(out.stdout)
        .unwrap_or_else(|e| {
            panic!(
                "`git hash-object {}` output is not utf-8: {e}",
                path.display()
            )
        })
        .trim()
        .to_string()
}

/// One data row of `arms/MANIFEST.md`'s table.
struct ManifestRow {
    arm: String,
    skill: String,
    hash: String,
}

/// Parse `arms/MANIFEST.md`'s markdown table.
///
/// The manifest is append-only and gains a row per arm per skill as later tasks
/// snapshot A′/B/B-r<i>/voice, so rows are matched on their `arm` and `skill`
/// cells rather than by position. Header and `---` separator rows are skipped,
/// and cells are trimmed of the backticks the table uses to typeset paths and
/// hashes.
///
/// Only lines *after* the `| arm | …` header count as data, so prose in the
/// preamble can never be mistaken for a row no matter what punctuation it grows.
fn parse_manifest(contents: &str) -> Vec<ManifestRow> {
    let mut rows = Vec::new();
    let mut in_table = false;

    for line in contents.lines() {
        let line = line.trim();
        let Some(inner) = line.strip_prefix('|') else {
            continue;
        };
        let cells: Vec<String> = inner
            .strip_suffix('|')
            .unwrap_or(inner)
            .split('|')
            .map(|c| c.trim().trim_matches('`').trim().to_string())
            .collect();
        if cells.len() < 4 {
            continue;
        }
        // Separator row (`|---|---|…`), possibly with alignment colons.
        if cells
            .iter()
            .all(|c| !c.is_empty() && c.chars().all(|ch| ch == '-' || ch == ':'))
        {
            continue;
        }
        // Header row — everything before it is preamble.
        if cells[0].eq_ignore_ascii_case("arm") {
            in_table = true;
            continue;
        }
        if !in_table {
            continue;
        }
        rows.push(ManifestRow {
            arm: cells[0].clone(),
            skill: cells[1].clone(),
            hash: cells[3].clone(),
        });
    }

    rows
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
    let rows = parse_manifest(&contents);

    // Everything that does not need git runs first, so a git-less environment
    // still reports a corrupt manifest rather than only "git is missing".
    let mut to_verify = Vec::new();
    for skill in ARM_SNAPSHOT_SKILLS {
        let matches: Vec<&ManifestRow> = rows
            .iter()
            .filter(|r| r.arm == "A" && r.skill == *skill)
            .collect();
        assert_eq!(
            matches.len(),
            1,
            "{}: expected exactly one arm A row for `{skill}`, found {}",
            manifest_path.display(),
            matches.len()
        );

        // A blank or malformed hash cell would otherwise only surface as a
        // confusing mismatch against a real SHA.
        let expected = matches[0].hash.clone();
        assert!(
            expected.len() == 40 && expected.chars().all(|c| c.is_ascii_hexdigit()),
            "{}: arm A row for `{skill}` records `{expected}`, which is not a 40-character hex blob SHA",
            manifest_path.display()
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
            "{} has drifted: `git hash-object --no-filters` is {actual}, MANIFEST.md records {expected}",
            snapshot.display(),
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
