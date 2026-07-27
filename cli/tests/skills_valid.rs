//! Validates every `skills/*/SKILL.md` in the repo and enforces a body-size
//! budget on the four `drovr:*` methodology skills.
//!
//! Two assertions:
//!   1. **All** skills have valid frontmatter: a leading `---` block containing
//!      non-empty `name:` and `description:`, and `name:` equals the directory
//!      name.
//!   2. The four methodology skills (tdd, systematic-debugging,
//!      verification-before-completion, code-review) each have a
//!      post-frontmatter body of at most 2200 bytes. The pre-existing skills
//!      (using-drovr, handoff, pipeline) are NOT size-checked.

use std::fs;
use std::path::{Path, PathBuf};

/// Body-size budget (bytes) for the methodology skills.
///
/// Re-baselined 2200 → 2600 when `code-review` gained the "never write a reviewer's
/// prompt, pass `drovr code-review brief` output verbatim" rule. 2200 was set when that
/// skill's body was 2197 — three bytes of headroom — so any new rule had to be paid for
/// by degrading an existing one, and the cap had started editing the content rather than
/// bounding it. The point is to keep these four skills scannable, not to hold a number:
/// the largest body is now 2445, so 2600 leaves room for one more rule before the next
/// deliberate look.
const BODY_BUDGET: usize = 2600;

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
