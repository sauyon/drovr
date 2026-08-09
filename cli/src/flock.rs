//! The one advisory file lock drovr takes — and the only way it is released.
//!
//! An `flock` belongs to the **open file description**, not to the file
//! descriptor that names it. `fork(2)` copies the fd table, so a child born
//! while this process holds a lock gets a second fd on that same description,
//! lock included. Rust opens with `O_CLOEXEC`, so the copy goes away at `exec`
//! — but not before, and `close(2)` releases the lock only when it closes the
//! LAST fd on the description. A holder that "releases" by dropping its `File`
//! inside that window has therefore released nothing: `try_lock` on that inode
//! refuses with `WouldBlock` until the child execs, while nothing anywhere
//! actually claims it.
//!
//! drovr spawns `git` and `herdr` routinely, so that window is not
//! hypothetical; `docs/run-lock-fork-race/lock-red.txt` measures it (drovr#80).
//! The fix is that release is an explicit `flock(LOCK_UN)` — [`File::unlock`],
//! in [`FileLock`]'s `Drop` — never a bare drop of the `File`. `LOCK_UN` names
//! the description itself and does not care how many fds still point at it.
//!
//! That is what [`FileLock`]'s private field buys: with no constructor but
//! [`try_take`] and no way to get the `File` back out, a close-based release is
//! not spellable. See `crate::phase::acquire_run_lock`'s doc comment for the
//! fuller account of what such a lock serializes and why it refuses rather than
//! waits.

use std::fs::{File, OpenOptions, TryLockError};
use std::io::{self, Seek, Write};
use std::path::Path;

/// An exclusive advisory lock on a file, held for as long as the guard lives
/// and released explicitly when it dies.
///
/// The `File` is private on purpose — see the module doc. Handing it out (by
/// value, or by `&mut`) would let a caller drop it, and a drop is not a
/// release.
pub(crate) struct FileLock {
    file: File,
}

/// Open `path` (create if absent, never truncate) and take an exclusive
/// advisory lock on it. `Ok(None)` means someone else holds it right now;
/// `Err` is a real IO failure — a missing parent directory, say, which this
/// does not create.
pub(crate) fn try_take(path: &Path) -> io::Result<Option<FileLock>> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;

    match file.try_lock() {
        Ok(()) => Ok(Some(FileLock { file })),
        Err(TryLockError::WouldBlock) => Ok(None),
        Err(TryLockError::Error(e)) => Err(e),
    }
}

impl FileLock {
    /// Replace the file's whole contents.
    ///
    /// The guard owns the `File` so that release stays explicit, so a caller
    /// that needs to stamp something into a locked file (`server.pid`) comes
    /// through here. Truncating alone would not do it: `set_len` moves no file
    /// offset, so a second call would write past the bytes it just cut and
    /// leave a NUL-padded prefix behind — hence the rewind.
    ///
    /// (The `allow(dead_code)` is the last one left in this module: only the
    /// tests call this until `review.rs`'s `server.pid` lock moves onto it, and
    /// it goes when that lands. [`FileLock`] and [`try_take`] carried one for
    /// the same reason and no longer need it — `crate::phase::acquire_run_lock`
    /// calls them. `try_clone` never carried one; it is `#[cfg(test)]`.)
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn overwrite(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.file.set_len(0)?;
        self.file.rewind()?;
        self.file.write_all(bytes)?;
        self.file.flush()
    }

    /// TEST ONLY. A second fd on the SAME open file description — exactly what
    /// `fork()` hands a child. Returns a bare `File`, not a second guard: it is
    /// a shared fd, not a second claim, and nothing about it should release
    /// anything. It exists so drovr#80's fault can be staged deterministically
    /// without forking, and production has no use for it.
    ///
    /// It must stay a `dup(2)`. Re-opening the path would be a NEW description,
    /// which stages nothing — see `a_shared_description_does_not_keep_the_lock_after_drop`,
    /// which pins the property rather than assuming it.
    #[cfg(test)]
    pub(crate) fn try_clone(&self) -> io::Result<File> {
        self.file.try_clone()
    }
}

impl Drop for FileLock {
    /// ⭐ drovr#80 — the release, and the reason this type exists.
    ///
    /// `close(2)` would release the lock only if this fd were the last one on
    /// the open file description; `LOCK_UN` names the description itself, so an
    /// fd that a `fork` left in a child cannot hold the claim open past us. The
    /// `File` is closed by the drop that follows, as it always was — this only
    /// makes sure the lock is gone first.
    ///
    /// The result is discarded because there is no one to tell and nothing to
    /// do: the only plausible failure is a `File` already invalid, and the fd
    /// closes an instant later regardless.
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Read;
    use tempfile::TempDir;

    // drovr#80, at the primitive: a lock "released" by dropping its `File` is
    // not released at all while a second fd shares the open file description.
    // `try_clone` stands in for `fork()` — both produce a second fd on one
    // description — so the fault is staged deterministically rather than raced
    // for, and holds inside `nix build`'s sandbox as well as on a developer's
    // machine.
    #[test]
    fn a_shared_description_does_not_keep_the_lock_after_drop() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("run.lock");

        let mut held = try_take(&path).unwrap().expect("first claim");
        let mut shared = held
            .try_clone()
            .expect("a second fd on the SAME description — what fork() hands a child");

        // The staging fd has to be a `dup(2)`, and this is where that is
        // checked rather than assumed: were `try_clone` a fresh `open`, it
        // would be a NEW description, dropping the guard really would close
        // the locked description's last fd, and the rest of this test would
        // pass with no fix present at all. Two fds on ONE description share a
        // file offset; two opens do not. The guard just wrote two bytes, so a
        // genuinely shared fd is already at EOF.
        held.overwrite(b"ab").unwrap();
        let mut tail = String::new();
        shared.read_to_string(&mut tail).unwrap();
        assert_eq!(
            tail, "",
            "try_clone read back the bytes the guard just wrote, so it does not share the \
             guard's file offset and is therefore a second open file description, not a \
             dup(2) of the locked one — it stages nothing and this test proves nothing"
        );

        drop(held);

        assert!(
            try_take(&path).unwrap().is_some(),
            "the re-claim was refused while nothing held the lock — dropping the guard \
             closed OUR fd, but a close-based release does not release an open file \
             description another fd still shares, so the lock outlived its holder \
             (drovr#80). Release has to be an explicit unlock."
        );

        // Last, so the staging fd outlives the re-claim it is meant to block.
        drop(shared);
    }

    // The refusal is the point of the lock, and it has to survive the fix: an
    // unlock in `Drop` must not become an unlock anywhere earlier. This is the
    // in-process refusal `two_rehydrates_of_one_run_cannot_both_launch` rests
    // on, pinned at the primitive.
    #[test]
    fn a_live_holder_is_reported_as_none() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("run.lock");

        let held = try_take(&path).unwrap();
        assert!(held.is_some(), "the first claim on an unheld lock");
        assert!(
            try_take(&path).unwrap().is_none(),
            "a second claim taken while the first guard is alive must refuse"
        );

        drop(held);
    }

    #[test]
    fn overwrite_replaces_the_whole_file() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("server.pid");

        let mut lock = try_take(&path).unwrap().expect("first claim");
        lock.overwrite(b"9999").unwrap();
        // The second one is the one that bites: it has to cut the four bytes
        // AND go back to the start, or the file reads "\0\0\0\07".
        lock.overwrite(b"7").unwrap();
        drop(lock);

        assert_eq!(fs::read_to_string(&path).unwrap(), "7");
    }
}
