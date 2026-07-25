//! Git-worktree isolation for a drovr run.
//!
//! `drovr new --worktree` creates a dedicated checkout at `.drovr/wt/<run>` on a
//! fresh branch `drovr/<run>`, and points the run's `project_dir` there. Because
//! every phase, the herdr workspace root, and code-review's base all key off
//! `project_dir`, that single redirection isolates the whole run from the
//! invoking checkout. `cmd_cleanup` prunes the worktree and (by default) keeps
//! the branch for the human to merge.
//!
//! Everything here shells out to `git`; the functions are pure enough to unit
//! test against a throwaway repo.

use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The branch a run's worktree checks out.
pub fn branch_name(run: &str) -> String {
    format!("drovr/{run}")
}

/// The repo-relative worktree path for a run.
fn rel_path(run: &str) -> String {
    format!(".drovr/wt/{run}")
}

/// `git -C <dir> <args>` → trimmed stdout on success, else an `io::Error`
/// carrying git's stderr.
fn git(dir: &Path, args: &[&str]) -> io::Result<String> {
    let out = Command::new("git").arg("-C").arg(dir).args(args).output()?;
    if !out.status.success() {
        return Err(io::Error::other(format!(
            "git {args:?} failed in {}: {}",
            dir.display(),
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

/// True if `dir` sits inside a git working tree.
pub fn is_git_repo(dir: &Path) -> bool {
    git(dir, &["rev-parse", "--is-inside-work-tree"])
        .map(|s| s == "true")
        .unwrap_or(false)
}

/// True if local branch `branch` already exists in the repo at `dir`.
fn branch_exists(dir: &Path, branch: &str) -> bool {
    git(
        dir,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}"),
        ],
    )
    .is_ok()
}

/// Append `line` to the repo's `.git/info/exclude` unless already present, so the
/// in-repo `.drovr/wt/` worktree dir never shows up as an untracked change. The
/// tracked `.gitignore` is deliberately left alone (locked decision 2). Uses
/// `--git-common-dir` so this is correct even when the repo is itself a worktree.
fn ensure_local_exclude(repo: &Path, line: &str) -> io::Result<()> {
    let common = git(repo, &["rev-parse", "--git-common-dir"])?;
    let common_dir = {
        let p = PathBuf::from(&common);
        if p.is_absolute() { p } else { repo.join(p) }
    };
    let exclude = common_dir.join("info").join("exclude");
    let existing = std::fs::read_to_string(&exclude).unwrap_or_default();
    if existing.lines().any(|l| l.trim() == line) {
        return Ok(());
    }
    if let Some(parent) = exclude.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut body = existing;
    if !body.is_empty() && !body.ends_with('\n') {
        body.push('\n');
    }
    body.push_str(line);
    body.push('\n');
    std::fs::write(&exclude, body)
}

/// Create the run's worktree: `.drovr/wt/<run>` on new branch `drovr/<run>` off
/// HEAD. Returns `(absolute_worktree_path, branch)`. Errors if `repo` is not a
/// git repo or the branch already exists — no silent fallback (locked decision 3:
/// `--worktree` means the user asked for isolation).
pub fn create(repo: &Path, run: &str) -> io::Result<(PathBuf, String)> {
    if !is_git_repo(repo) {
        return Err(io::Error::other(format!(
            "--worktree requires a git repo, but {} is not inside one",
            repo.display()
        )));
    }
    // Canonicalize the repo up front so the returned worktree path is absolute
    // without canonicalizing the freshly created worktree afterwards — that
    // ordering guarantees no failure path ever strands a half-created worktree.
    let repo = std::fs::canonicalize(repo)?;
    let rel = rel_path(run);
    let abs = repo.join(&rel);
    if abs.exists() {
        return Err(io::Error::other(format!(
            "worktree path {} already exists; remove it or choose another run name",
            abs.display()
        )));
    }
    // A repo with no commits has an unborn HEAD; `worktree add ... HEAD` would
    // fail with a cryptic "invalid reference: HEAD". Catch it with a clear message.
    if git(&repo, &["rev-parse", "--verify", "--quiet", "HEAD"]).is_err() {
        return Err(io::Error::other(
            "cannot create a worktree in a repo with no commits yet; make an initial commit first",
        ));
    }
    let branch = branch_name(run);
    if branch_exists(&repo, &branch) {
        return Err(io::Error::other(format!(
            "branch '{branch}' already exists; remove it or choose another run name"
        )));
    }
    // Create the worktree first; only touch `.git/info/exclude` once it succeeds,
    // so a failed create leaves no trace in the repo.
    git(&repo, &["worktree", "add", "-b", &branch, &rel, "HEAD"])?;
    ensure_local_exclude(&repo, ".drovr/wt/")?;
    Ok((abs, branch))
}

/// Stage everything and commit it in the worktree, returning `true` if a commit
/// was created and `false` if the tree was already clean (nothing to commit).
///
/// This is the locked-decision-4a "final commit at the completion boundary": the
/// intra-run flow stays working-tree-based (code-review diffs the working tree, so
/// it is untouched mid-run), and exactly one squash commit lands the run's work on
/// its branch when the run is finalized — so the branch is mergeable and the
/// worktree prunes cleanly.
pub fn commit_all(worktree_path: &Path, message: &str) -> io::Result<bool> {
    // Clean tree → nothing to do (avoid an empty commit).
    if git(worktree_path, &["status", "--porcelain"])?.is_empty() {
        return Ok(false);
    }
    git(worktree_path, &["add", "-A"])?;
    git(worktree_path, &["commit", "-q", "-m", message])?;
    Ok(true)
}

/// Prune the run's worktree. Runs `git worktree remove` from the **main** working
/// tree (first entry of `worktree list`) so removal never depends on cwd being the
/// worktree being deleted. Without `force`, git refuses a dirty worktree (modified
/// or untracked files) — that refusal is the safety net that keeps `cleanup` from
/// silently discarding uncommitted work. The branch is kept unless `delete_branch`
/// is given (only `--purge` passes it).
pub fn remove(worktree_path: &Path, delete_branch: Option<&str>, force: bool) -> io::Result<()> {
    let list = git(worktree_path, &["worktree", "list", "--porcelain"])?;
    let main = list
        .lines()
        .next()
        .and_then(|l| l.strip_prefix("worktree "))
        .map(PathBuf::from)
        .ok_or_else(|| io::Error::other("could not locate main working tree for removal"))?;

    let wt = worktree_path.to_string_lossy().into_owned();
    let mut args: Vec<&str> = vec!["worktree", "remove"];
    if force {
        args.push("--force");
    }
    args.push(&wt);
    git(&main, &args)?;

    if let Some(branch) = delete_branch {
        git(&main, &["branch", "-D", branch])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A throwaway git repo with one commit; returns the TempDir (keep it alive).
    fn repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let g = |args: &[&str]| {
            let out = Command::new("git")
                .arg("-C")
                .arg(dir.path())
                .args(args)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        };
        g(&["init", "-q"]);
        g(&["config", "user.email", "t@t.t"]);
        g(&["config", "user.name", "t"]);
        std::fs::write(dir.path().join("f.txt"), "hi").unwrap();
        g(&["add", "."]);
        g(&["commit", "-q", "-m", "init"]);
        dir
    }

    #[test]
    fn create_makes_worktree_branch_and_exclude() {
        let repo = repo();
        let (path, branch) = create(repo.path(), "myrun").unwrap();

        assert_eq!(branch, "drovr/myrun");
        assert!(
            path.ends_with(".drovr/wt/myrun"),
            "path: {}",
            path.display()
        );
        assert!(path.join("f.txt").exists(), "worktree must check out HEAD");
        assert!(branch_exists(repo.path(), "drovr/myrun"));

        // git sees it as a registered worktree.
        let list = git(repo.path(), &["worktree", "list"]).unwrap();
        assert!(list.contains(".drovr/wt/myrun"), "worktree list: {list}");

        // `.drovr/wt/` is excluded locally, and the tracked tree is untouched.
        let excl = std::fs::read_to_string(repo.path().join(".git/info/exclude")).unwrap();
        assert!(
            excl.lines().any(|l| l.trim() == ".drovr/wt/"),
            "exclude: {excl}"
        );
        assert!(
            !repo.path().join(".gitignore").exists(),
            "must not create .gitignore"
        );
    }

    #[test]
    fn exclude_is_idempotent() {
        let repo = repo();
        create(repo.path(), "a").unwrap();
        create(repo.path(), "b").unwrap();
        let excl = std::fs::read_to_string(repo.path().join(".git/info/exclude")).unwrap();
        let hits = excl.lines().filter(|l| l.trim() == ".drovr/wt/").count();
        assert_eq!(
            hits, 1,
            "exclude line must appear once, not per-run: {excl}"
        );
    }

    #[test]
    fn create_errors_on_non_git_dir() {
        let dir = tempfile::tempdir().unwrap();
        let err = create(dir.path(), "x").unwrap_err().to_string();
        assert!(err.contains("git repo"), "err: {err}");
    }

    #[test]
    fn create_errors_on_branch_collision() {
        let repo = repo();
        // Pre-create the branch drovr/dup so create() must refuse.
        git(repo.path(), &["branch", "drovr/dup"]).unwrap();
        let err = create(repo.path(), "dup").unwrap_err().to_string();
        assert!(err.contains("already exists"), "err: {err}");
    }

    #[test]
    fn create_errors_on_unborn_head() {
        // A git repo with zero commits → clear message, not a cryptic git error.
        let dir = tempfile::tempdir().unwrap();
        for args in [
            &["init", "-q"][..],
            &["config", "user.email", "t@t.t"][..],
            &["config", "user.name", "t"][..],
        ] {
            Command::new("git")
                .arg("-C")
                .arg(dir.path())
                .args(args)
                .output()
                .unwrap();
        }
        let err = create(dir.path(), "x").unwrap_err().to_string();
        assert!(err.contains("no commits yet"), "err: {err}");
    }

    #[test]
    fn create_errors_on_leftover_worktree_dir() {
        let repo = repo();
        // A stale `.drovr/wt/<run>` dir (e.g. from an interrupted run) must produce
        // a clear path-focused error, not a confusing branch message.
        std::fs::create_dir_all(repo.path().join(".drovr/wt/stale")).unwrap();
        let err = create(repo.path(), "stale").unwrap_err().to_string();
        assert!(
            err.contains("worktree path") && err.contains("already exists"),
            "err: {err}"
        );
    }

    #[test]
    fn failed_create_leaves_no_exclude() {
        let repo = repo();
        // Branch collision fails create() before the worktree is added; because the
        // exclude mutation now happens only after a successful add, a failed create
        // must leave `.git/info/exclude` untouched.
        git(repo.path(), &["branch", "drovr/x"]).unwrap();
        assert!(create(repo.path(), "x").is_err());
        let excl =
            std::fs::read_to_string(repo.path().join(".git/info/exclude")).unwrap_or_default();
        assert!(
            !excl.lines().any(|l| l.trim() == ".drovr/wt/"),
            "failed create must not write the exclude: {excl}"
        );
    }

    #[test]
    fn commit_all_commits_dirty_then_reports_clean() {
        let repo = repo();
        let (path, branch) = create(repo.path(), "c").unwrap();
        std::fs::write(path.join("work.txt"), "phase output").unwrap();

        // First call commits the accumulated work → true.
        assert!(commit_all(&path, "drovr(c): build a thing").unwrap());
        // Tree is now clean → a second call is a no-op, not an empty commit.
        assert!(!commit_all(&path, "drovr(c): build a thing").unwrap());

        // The branch tip carries the work and the message.
        let log = git(repo.path(), &["log", "--format=%s", &branch]).unwrap();
        assert!(log.contains("drovr(c): build a thing"), "log: {log}");
        let show = git(repo.path(), &["show", "--stat", &branch]).unwrap();
        assert!(
            show.contains("work.txt"),
            "commit must include work: {show}"
        );
    }

    #[test]
    fn commit_all_then_remove_is_clean_no_force() {
        let repo = repo();
        let (path, branch) = create(repo.path(), "cr").unwrap();
        std::fs::write(path.join("work.txt"), "x").unwrap();
        commit_all(&path, "drovr(cr): done").unwrap();
        // After committing, the tree is clean so a plain (non-force) remove succeeds
        // and the branch retains the commit.
        remove(&path, None, false).unwrap();
        assert!(!path.exists());
        assert!(branch_exists(repo.path(), &branch));
    }

    #[test]
    fn remove_prunes_clean_worktree_keeps_branch() {
        let repo = repo();
        let (path, branch) = create(repo.path(), "r").unwrap();
        remove(&path, None, false).unwrap();
        assert!(!path.exists(), "worktree dir must be gone");
        // Branch is kept for the human to merge.
        assert!(branch_exists(repo.path(), &branch), "branch must survive");
        let list = git(repo.path(), &["worktree", "list"]).unwrap();
        assert!(
            !list.contains(".drovr/wt/r"),
            "worktree deregistered: {list}"
        );
    }

    #[test]
    fn remove_refuses_dirty_without_force() {
        let repo = repo();
        let (path, _) = create(repo.path(), "d").unwrap();
        std::fs::write(path.join("untracked"), "x").unwrap();
        let err = remove(&path, None, false).unwrap_err().to_string();
        assert!(
            err.contains("modified or untracked") || err.contains("use --force"),
            "err: {err}"
        );
        assert!(path.exists(), "dirty worktree must be left intact");
    }

    #[test]
    fn remove_force_deletes_dirty_and_branch() {
        let repo = repo();
        let (path, branch) = create(repo.path(), "p").unwrap();
        std::fs::write(path.join("untracked"), "x").unwrap();
        remove(&path, Some(&branch), true).unwrap();
        assert!(!path.exists(), "force must remove dirty worktree");
        assert!(
            !branch_exists(repo.path(), &branch),
            "--purge deletes branch"
        );
    }
}
