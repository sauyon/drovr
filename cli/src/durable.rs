//! Writes that survive a restart.
//!
//! # Why this is not [`RunState::save_in`](crate::run::RunState::save_in)
//!
//! `run.rs` already writes `state.json` through a temp file and a rename, and
//! its doc comment is explicit that the scope is *torn reads and nothing else*:
//!
//! > NOT durability. The temp file is not `fsync`ed and neither is the
//! > directory, so a power loss can still leave a stale (or, on some
//! > filesystems, empty) `state.json`. Deliberate: `save` runs in poll loops and
//! > an fsync per save is not worth it for a workflow whose herdr workspace does
//! > not survive a crash either.
//!
//! That trade is right for `state.json` and wrong for a reviewer's verdict. The
//! gate's whole job is to carry ONE human decision to an agent that may not read
//! it for hours, across a `drovr serve` restart that is expected rather than
//! exceptional — the server is always-on and respawned on demand. A verdict that
//! survives only until the next restart is not a gate. And unlike `state.json`,
//! these writes happen once per human click, so the fsync costs nothing that
//! matters.
//!
//! # What [`write`] guarantees
//!
//! * **Whole or absent.** A reader sees the previous file or the new one, never
//!   a splice, because the visible name only ever changes by `rename`.
//! * **Durable.** The bytes are `fsync`ed before the rename, and the directory
//!   is `fsync`ed after it, so a crash cannot resurrect the old contents or
//!   leave a zero-length file where a `rename` appeared to have landed.
//! * **Reported, and reported PRECISELY.** Every failure is an `Err`, and the
//!   error says whether the bytes reached the target anyway — see
//!   [`WriteFailed`]. A caller that cannot tell those apart will either refuse a
//!   decision that is already on disk or acknowledge one that is not, and both
//!   are the divergence this module exists to prevent.
//!
//! # What it does not guarantee
//!
//! Not serialized updates. Two concurrent writers to one path still clobber each
//! other whole-file; each result is a complete file, and which one wins is the
//! callers' problem (in the review server, a per-run mutex).

use std::fmt;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

/// Why a durable write failed — and whether the bytes landed regardless.
///
/// **`fs::rename` is the point of no return.** A POSIX same-directory rename is
/// atomic and immediately visible to every opener; nothing after it can put the
/// old contents back. So a failure *before* the rename and a failure *after* it
/// are not the same event, and collapsing them into one `io::Error` is a real
/// defect rather than a tidiness question: the review server decides whether to
/// advance its in-memory state from exactly this value, so the collapsed version
/// makes it refuse a verdict that a restart will happily read back — the precise
/// disk/memory divergence the gate is built to rule out.
#[derive(Debug)]
pub enum WriteFailed {
    /// Nothing reached the target. It still holds whatever it held before, and
    /// the caller may treat the operation as never having happened.
    NotWritten(io::Error),
    /// **The rename landed.** The target now holds the new bytes, every reader
    /// sees them, and a process restart reads them back. Only the parent
    /// directory's `fsync` failed — the step that protects the rename against a
    /// *power* loss, not against a process exiting. The caller should treat the
    /// write as having happened, and say so loudly.
    NotDurable(io::Error),
}

impl WriteFailed {
    /// Whether the new bytes are on disk despite the error.
    ///
    /// This is the question every caller actually has, which is why it is a
    /// method and not a `matches!` at each site.
    pub fn landed(&self) -> bool {
        matches!(self, WriteFailed::NotDurable(_))
    }

    /// Consume into a plain [`io::Error`], for a caller that has already decided
    /// the distinction does not change what it does.
    ///
    /// Deliberately consuming: taking `self` means a caller cannot reach the
    /// inner error while still holding the thing that knows whether the write
    /// landed, so "report the error" and "decide what it meant" cannot be done
    /// in the wrong order by accident. Callers that only need to *report* use
    /// [`Display`](fmt::Display), which already names both.
    pub fn into_error(self) -> io::Error {
        match self {
            WriteFailed::NotWritten(e) | WriteFailed::NotDurable(e) => e,
        }
    }
}

impl fmt::Display for WriteFailed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WriteFailed::NotWritten(e) => write!(f, "nothing was written: {e}"),
            WriteFailed::NotDurable(e) => write!(
                f,
                "the bytes ARE on disk and survive a restart, but the directory could not be \
                 fsynced, so a power loss could still lose them: {e}"
            ),
        }
    }
}

/// Write `bytes` to `path` atomically and durably.
///
/// The temp name carries pid plus a process-local counter rather than a fixed
/// `<name>.tmp`: two concurrent writers sharing one temp path would interleave
/// into the same inode and then rename that corruption into place, which is
/// precisely the failure this is meant to remove. It is dot-prefixed so a
/// `read_dir` that lists a run dir for the human does not show it.
///
/// Cleanup of the temp file on a failure path is **attempted, not guaranteed** —
/// `remove_file` can itself fail (the same full disk, a filesystem that just
/// went read-only), and the original error is what the caller needs, so a failed
/// cleanup is not allowed to displace it. A leaked `.<name>.tmp.<pid>.<n>` is
/// cosmetic: nothing enumerates inside a run dir (`list_runs_in` scans one level
/// above), and the file is dot-prefixed.
///
/// The temp file takes `File::create`'s mode (`0o666 & !umask`) rather than
/// inheriting the replaced file's, because a rename replaces the inode. Contained
/// by the data dir's `0700` (see `review::serve`), which is what actually keeps
/// other local users out; worth knowing if that containment ever changes.
pub fn write(path: &Path, bytes: &[u8]) -> Result<(), WriteFailed> {
    static SEQ: AtomicU64 = AtomicU64::new(0);

    let dir = match path.parent() {
        Some(d) => d,
        None => {
            return Err(WriteFailed::NotWritten(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{} has no parent directory to write into", path.display()),
            )))
        }
    };
    // An empty parent means a bare relative name like `feedback.json`, whose
    // directory is the process cwd. Naming it explicitly keeps the temp file
    // beside the target — a rename across filesystems is `EXDEV`, and that is
    // exactly what a temp file in the wrong directory would risk.
    let dir = if dir.as_os_str().is_empty() {
        Path::new(".")
    } else {
        dir
    };
    let stem = match path.file_name() {
        Some(n) => n.to_string_lossy().into_owned(),
        None => {
            return Err(WriteFailed::NotWritten(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{} does not name a file", path.display()),
            )))
        }
    };

    let tmp = dir.join(format!(
        ".{stem}.tmp.{}.{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));

    let staged = (|| -> io::Result<()> {
        #[cfg(test)]
        fault::check(path, fault::Fault::FailBeforeRename)?;
        let mut f = File::create(&tmp)?;
        f.write_all(bytes)?;
        // Before the rename, not after: a rename that lands while the data is
        // still only in the page cache is how a crash produces the zero-length
        // file this module is meant to rule out.
        f.sync_all()
    })();
    if let Err(e) = staged {
        let _ = fs::remove_file(&tmp);
        return Err(WriteFailed::NotWritten(e));
    }
    if let Err(e) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(WriteFailed::NotWritten(e));
    }
    // Past here the bytes ARE the file. Every remaining failure is NotDurable.
    sync_dir(dir).map_err(WriteFailed::NotDurable)
}

/// `fsync` the directory, so the rename itself is durable and not just ordered.
///
/// The `EINVAL` case is a kernel or filesystem that does not implement directory
/// `fsync` at all — classically vfat/exFAT, which give no durability guarantee
/// for directory entries in the first place. That is a statement about the
/// platform rather than about this write, and refusing over it would trade a
/// real, already-`fsync`ed write for a guarantee the filesystem was never going
/// to provide. Scoped to `sync_all` alone: an `EINVAL` out of `File::open` is
/// not the same claim, and swallowing it would report a directory as synced that
/// was never opened.
#[cfg(unix)]
fn sync_dir(dir: &Path) -> io::Result<()> {
    #[cfg(test)]
    fault::check(dir, fault::Fault::FailDirSync)?;
    let handle = File::open(dir)?;
    match handle.sync_all() {
        Err(e) if e.kind() == io::ErrorKind::InvalidInput => Ok(()),
        other => other,
    }
}

#[cfg(not(unix))]
fn sync_dir(_dir: &Path) -> io::Result<()> {
    // Windows cannot open a directory as a `File`, and its rename is already
    // ordered with respect to the synced file data.
    Ok(())
}

/// Path-keyed fault injection, so a test can produce the failures a filesystem
/// produces and `chmod` cannot.
///
/// The gap this closes is specific and was found by review: every `chmod 0555`
/// test seals the whole directory, so it can only ever fail the FIRST write in a
/// sequence. The failures that actually matter here are the later ones — a
/// commit that fails after a detail file already landed, and a directory `fsync`
/// that fails after the rename — and neither is reachable by permissions.
///
/// Keyed by absolute path rather than a thread-local, for two reasons: the
/// server's request handlers run on worker threads a test never touches, and
/// each test owns a unique `TempDir`, so two tests running in parallel cannot
/// collide on a key.
#[cfg(test)]
pub(crate) mod fault {
    use std::io;
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum Fault {
        /// Fail before anything is renamed — a `NotWritten`.
        FailBeforeRename,
        /// Fail the parent-directory `fsync` — a `NotDurable`, keyed on the
        /// DIRECTORY rather than the file, because that is what `sync_dir` sees.
        FailDirSync,
    }

    static INJECTED: Mutex<Vec<(PathBuf, Fault)>> = Mutex::new(Vec::new());

    /// Arm `fault` for exactly `path`. Returns a guard that disarms on drop, so
    /// a panicking test cannot leak a fault into another test's run.
    pub(crate) fn arm(path: &Path, fault: Fault) -> Armed {
        INJECTED
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push((path.to_path_buf(), fault));
        Armed(path.to_path_buf(), fault)
    }

    pub(crate) struct Armed(PathBuf, Fault);

    impl Drop for Armed {
        fn drop(&mut self) {
            let mut g = INJECTED.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(i) = g.iter().position(|(p, f)| p == &self.0 && *f == self.1) {
                g.remove(i);
            }
        }
    }

    pub(crate) fn check(path: &Path, fault: Fault) -> io::Result<()> {
        let armed = INJECTED
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .any(|(p, f)| p == path && *f == fault);
        if armed {
            return Err(io::Error::other(format!(
                "injected fault {fault:?} for {}",
                path.display()
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_replaces_the_previous_contents_whole() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("verdict.json");

        write(&target, b"first").unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"first");

        write(&target, b"second, and longer").unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"second, and longer");
    }

    #[test]
    fn write_leaves_no_temp_file_behind() {
        let tmp = tempfile::tempdir().unwrap();
        write(&tmp.path().join("verdict.json"), b"x").unwrap();

        let leftovers: Vec<_> = fs::read_dir(tmp.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n != "verdict.json")
            .collect();
        assert!(leftovers.is_empty(), "orphaned temp files: {leftovers:?}");
    }

    #[test]
    fn a_dir_sync_failure_after_the_rename_reports_that_the_bytes_landed() {
        // THE distinction this type exists for. `fs::rename` is the point of no
        // return: once it succeeds the new bytes are the file, and no later
        // failure can put the old ones back. A caller told only "Err" would
        // refuse a verdict that a restart will read back — the exact divergence
        // the gate is built to prevent.
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("verdict.json");
        write(&target, b"old").unwrap();

        let _armed = fault::arm(tmp.path(), fault::Fault::FailDirSync);
        let err = write(&target, b"new").expect_err("the injected fsync must surface");

        assert!(
            err.landed(),
            "a post-rename failure must report that the bytes landed: {err}"
        );
        assert_eq!(
            fs::read(&target).unwrap(),
            b"new",
            "the rename really did land, so the caller must be told so"
        );
    }

    #[test]
    fn a_failure_before_the_rename_reports_that_nothing_landed() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("verdict.json");
        write(&target, b"old").unwrap();

        let _armed = fault::arm(&target, fault::Fault::FailBeforeRename);
        let err = write(&target, b"new").expect_err("the injected failure must surface");

        assert!(
            !err.landed(),
            "nothing was renamed, so the caller must be free to treat it as a no-op: {err}"
        );
        assert_eq!(
            fs::read(&target).unwrap(),
            b"old",
            "the previous contents must be untouched"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_failed_write_reports_and_leaves_nothing() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("sealed");
        fs::create_dir(&dir).unwrap();
        fs::write(dir.join("verdict.json"), b"original").unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o555)).unwrap();

        // Running as root ignores directory permissions.
        if fs::write(dir.join(".probe"), b"").is_ok() {
            fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
            return;
        }

        let err = write(&dir.join("verdict.json"), b"new").expect_err("must not silently succeed");
        assert!(!err.landed(), "a sealed directory means nothing landed");
        assert!(
            matches!(err.into_error().kind(), io::ErrorKind::PermissionDenied),
            "unexpected error kind for a sealed directory"
        );

        fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
        // The previous file is intact: a failed durable write is a no-op, never
        // a truncation of what was already there.
        assert_eq!(fs::read(dir.join("verdict.json")).unwrap(), b"original");
        let names: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n != "verdict.json")
            .collect();
        assert!(names.is_empty(), "orphaned temp files: {names:?}");
    }
}
