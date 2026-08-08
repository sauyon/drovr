//! A test's environment, scoped to the thread that installed it.
//!
//! [`TestEnv::new`] hands a test a fresh scratch root and installs an overlay
//! on the calling thread. From that moment [`crate::env::var`] and
//! [`crate::env::var_os`] — the crate's only environment reads — answer from
//! the overlay and nothing else, so two tests running concurrently can no
//! longer resolve each other's `XDG_DATA_HOME`.
//!
//! This replaces a convention with a capability. The old shape was a
//! process-global write under a shared mutex whose contract lived in a doc
//! comment: a test that read the variable outside the lock, or ran before any
//! test had set it, resolved whatever the process happened to say and
//! *silently succeeded*. There is no such window here, because there is no
//! process-global write to lose a race against.
//!
//! # What the design rests on
//!
//! * **Thread scoping.** The installed overlay lives in a thread-local, so
//!   tests do not share it and no lock is needed to keep them apart. Crossing
//!   a thread boundary is explicit: [`TestEnv::handle`] → [`EnvHandle::enter`].
//! * **No fallthrough.** A key the overlay does not hold reads as absent, even
//!   when the process environment has it. That is deliberate: it turns a test
//!   which still writes the process environment into a loud failure rather
//!   than a quiet pass on somebody else's value.
//! * **Drop uninstalls.** Nesting is LIFO and `--test-threads=1` cannot leak
//!   state forward, because sequential tests on one thread are separated only
//!   by [`Drop`]. Each guard removes *its own* entry from a per-thread stack,
//!   so dropping out of construction order — which Rust permits and which the
//!   type system will not stop — is still correct rather than silently wrong.
//!   `let _ = TestEnv::new()` uninstalls immediately, and the next `data_dir()`
//!   then fails rather than resolving anything real — fail-closed by
//!   construction, so no lint is needed to catch the mis-binding.
//!
//! # The reads that stay raw
//!
//! `PATH` and `HOME` are read from the real process environment here, and
//! nowhere else in the crate ([`crate::env`]'s `RAW_EXCEPTIONS` names both).
//! They cannot go through the overlay they are themselves installing.
//!
//! [`canonical_ish`] and [`refuse_home_root`] additionally call
//! `std::env::current_dir()` and `std::env::temp_dir()`. Those are process
//! state the guard *wants* unoverlaid — it has to compare against the paths the
//! OS will actually use — but note that neither is caught by the chokepoint
//! test in [`crate::env`], which scans for `std::env::var` only. That is a
//! standing gap in the scan rather than a licence: a future `current_dir` or
//! `temp_dir` call added elsewhere in `src/` would not be reported.
//!
//! Neither bootstrap read is synchronised against anything, and no longer needs
//! to be. Reading the environ table concurrently with a write to it is undefined
//! behaviour, but there is no writer left in `src/`: nothing in the crate mutates
//! the process environment any more, so these two reads race nothing.
//!
//! The temp-root exemption reads the *real* `TMPDIR`, so a test that changed
//! `TMPDIR` process-globally would move it. That is correct rather than a hole:
//! `tempfile` reads the same source, so the exemption has to track wherever
//! scratch dirs are actually being created. Nothing in the suite writes
//! `TMPDIR`.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::sync::{Arc, PoisonError, RwLock};

/// The keys the home check applies to — the ones that name a root drovr will
/// go on to create and delete files under.
///
/// `HOME` is in the list because it names those roots too, one indirection
/// further out: `data_dir()` falls back to `$HOME/.local/share` when
/// `XDG_DATA_HOME` is absent, and config resolution has the same fallback.
/// Guarding only the XDG pair would leave `unset(XDG_DATA_HOME)` plus
/// `set(HOME, <the real home>)` as an unguarded route to exactly the directory
/// the guard exists for.
const DATA_KEY: &str = "XDG_DATA_HOME";
const CONFIG_KEY: &str = "XDG_CONFIG_HOME";
const HOME_KEY: &str = "HOME";
const GUARDED_KEYS: &[&str] = &[DATA_KEY, CONFIG_KEY, HOME_KEY];

/// A scratch environment installed on THIS thread.
///
/// Uninstalls on drop, so nesting is LIFO.
///
/// Deliberately `!Send`: moving an installed `TestEnv` to another thread would
/// make its `Drop` uninstall from the wrong thread's [`INSTALLED`] stack,
/// silently breaking the thread-scoping the whole design rests on. Cross a
/// thread boundary with [`TestEnv::handle`], which is the supported way.
pub struct TestEnv {
    cur: Arc<Overlay>,
    frame: FrameId,
    _not_send: PhantomData<*const ()>,
}

/// A `Send + Sync` reference to an overlay, for a thread a test spawns.
///
/// Holding one does not install anything; [`EnvHandle::enter`] does, on
/// whatever thread calls it.
#[derive(Clone)]
pub struct EnvHandle(Arc<Overlay>);

/// The guard [`EnvHandle::enter`] returns; uninstalls on drop.
///
/// `!Send` for the same reason [`TestEnv`] is: its `Drop` pops from
/// [`INSTALLED`] on whichever thread it happens to run on, so it must not be
/// the wrong one.
pub struct EnteredEnv {
    frame: FrameId,
    _not_send: PhantomData<*const ()>,
}

/// The variables themselves, plus the scratch dir whose lifetime they depend
/// on.
///
/// `pub(crate)`, not private: [`current`] exposes it in a `pub(crate)`
/// signature, and [`crate::env`] — a different module — calls [`Overlay::get_os`].
///
/// `tmp` lives in here rather than in [`TestEnv`] so that it outlives every
/// [`EnvHandle`] pointing at the same overlay: a spawned thread must not be
/// able to see its scratch root deleted out from under it because the parent
/// returned first.
pub(crate) struct Overlay {
    vars: RwLock<BTreeMap<String, OsString>>,
    tmp: tempfile::TempDir,
}

/// The thread-scoping invariant, checked by the compiler rather than by a doc
/// comment.
///
/// `TestEnv` and `EnteredEnv` must not be `Send`: their `Drop` pops
/// [`INSTALLED`] on whatever thread it runs on, so a guard that crossed a
/// thread boundary would uninstall from the wrong stack. `EnvHandle` must be
/// `Send`, because crossing that boundary is exactly its job. Swapping the
/// `PhantomData` marker for a `Send` one is an easy, silent regression; this
/// makes it a build failure.
///
/// `!Sync` matters just as much and is easy to overlook: `thread::scope` shares
/// a `&TestEnv` with a child thread without moving it, so a `Sync` `TestEnv`
/// would let a worker call `set()` on an overlay whose stack frame belongs to
/// another thread. `PhantomData<*const ()>` denies both, which is why it is
/// that marker and not, say, `PhantomData<Cell<()>>`.
///
/// The inherent `const` shadows the blanket trait `const` only when the bound
/// holds, which is what turns a negative bound — otherwise inexpressible on
/// stable — into a value.
const _: () = {
    struct Probe<T>(PhantomData<T>);
    trait Maybe {
        const SEND: bool = false;
        const SYNC: bool = false;
    }
    impl<T> Maybe for Probe<T> {}
    impl<T: Send> Probe<T> {
        const SEND: bool = true;
    }
    impl<T: Sync> Probe<T> {
        const SYNC: bool = true;
    }

    assert!(!Probe::<TestEnv>::SEND, "TestEnv must not be Send");
    assert!(!Probe::<TestEnv>::SYNC, "TestEnv must not be Sync");
    assert!(!Probe::<EnteredEnv>::SEND, "EnteredEnv must not be Send");
    assert!(!Probe::<EnteredEnv>::SYNC, "EnteredEnv must not be Sync");
    assert!(Probe::<EnvHandle>::SEND, "EnvHandle must be Send");
    assert!(Probe::<EnvHandle>::SYNC, "EnvHandle must be Sync");
};

thread_local! {
    /// Every overlay installed on this thread, innermost last.
    ///
    /// A stack rather than a saved "previous overlay" per guard, because Rust
    /// does not enforce that drops happen in reverse construction order.
    /// `drop(outer)` while an inner guard is still alive compiles, and so does
    /// a block that returns the inner `TestEnv` it built; a guard that restored
    /// a remembered predecessor would then clear the overlay out from under the
    /// live inner one, and reads would fall silently through to the process
    /// environment — precisely the failure this module exists to remove,
    /// reintroduced through its own `Drop`. Removing *one's own* entry, wherever
    /// it sits, is correct for any drop order rather than loud about the wrong
    /// ones.
    static INSTALLED: RefCell<Vec<(FrameId, Arc<Overlay>)>> = const { RefCell::new(Vec::new()) };

    /// Frame identities are handed out per thread, so the counter is too.
    static NEXT_FRAME: Cell<u64> = const { Cell::new(0) };
}

/// Which installation a guard is responsible for undoing.
///
/// Distinct from the overlay's identity: one overlay can be installed more than
/// once on a thread, and each guard must undo its own push.
#[derive(Clone, Copy, PartialEq, Eq)]
struct FrameId(u64);

/// Push `o` onto this thread's stack and return the frame's identity.
fn install(o: &Arc<Overlay>) -> FrameId {
    let id = NEXT_FRAME.with(|n| {
        let id = n.get();
        n.set(id + 1);
        FrameId(id)
    });
    INSTALLED.with(|s| s.borrow_mut().push((id, Arc::clone(o))));
    id
}

/// Remove the frame `id` names, wherever it currently sits.
///
/// By frame, not by overlay: the same overlay can legitimately appear twice on
/// one thread — `env.handle().enter()` on the installing thread does exactly
/// that — and then "the topmost entry holding this overlay" is not necessarily
/// the entry this guard pushed. Popping the wrong twin would leave a live guard
/// resolving somebody else's overlay, silently.
///
/// `try_with`, not `with`: a guard dropped during thread teardown, after the
/// thread-local is gone, must not turn into a second panic.
fn uninstall(id: FrameId) {
    let _ = INSTALLED.try_with(|s| {
        let mut stack = s.borrow_mut();
        if let Some(i) = stack.iter().rposition(|(x, _)| *x == id) {
            stack.remove(i);
        }
    });
}

impl TestEnv {
    /// A fresh scratch root, installed on this thread.
    ///
    /// Seeds exactly three keys:
    ///
    /// * `XDG_DATA_HOME`   = `<tmp>/data`
    /// * `XDG_CONFIG_HOME` = `<tmp>/config`
    /// * `PATH`            = the real process `PATH`, copied verbatim
    ///
    /// Nothing else. Neither directory is created; drovr's own `create_dir_all`
    /// does that.
    ///
    /// `PATH` is copied because `config.rs`'s `command_available` reads it to
    /// decide whether `claude` / `agent` / `codex` exist; not copying it would
    /// make every command "unavailable" and silently change what the
    /// agent-detection tests assert. It is seeded *into* the overlay, so the
    /// overlay stays the only thing read — a named exception, not a fallback.
    /// A hermetic `PATH` is a separate want; a test that needs one can
    /// [`set`](TestEnv::set) it.
    ///
    /// The two seeded roots are not run through the home check: they are inside
    /// the freshly created [`tempfile::TempDir`], which is under the system
    /// temp root by construction and therefore exempt either way.
    pub fn new() -> TestEnv {
        let tmp = tempfile::TempDir::new().expect("creating a TestEnv scratch dir");

        let mut vars = BTreeMap::new();
        vars.insert(DATA_KEY.to_string(), tmp.path().join("data").into_os_string());
        vars.insert(
            CONFIG_KEY.to_string(),
            tmp.path().join("config").into_os_string(),
        );
        if let Some(path) = real_path() {
            vars.insert("PATH".to_string(), path);
        }

        let cur = Arc::new(Overlay {
            vars: RwLock::new(vars),
            tmp,
        });
        let frame = install(&cur);
        TestEnv {
            cur,
            frame,
            _not_send: PhantomData,
        }
    }

    /// Overlay a variable for this thread.
    ///
    /// Takes `impl AsRef<OsStr>` so call sites can pass `&Path`, `PathBuf` or
    /// `&str` interchangeably, as the process-global writes it replaced did.
    ///
    /// # Panics
    ///
    /// If `key` is one of [`GUARDED_KEYS`] — `XDG_DATA_HOME`,
    /// `XDG_CONFIG_HOME` or `HOME` — and `val` is either not absolute, or
    /// resolves inside the real `$HOME` but outside the system temp root. See
    /// [`refuse_home_root`]. The check runs *before* the lock is taken, so a
    /// rejected write cannot poison the overlay.
    pub fn set(&self, key: &str, val: impl AsRef<OsStr>) {
        let val = val.as_ref();
        refuse_home_root(key, Path::new(val));
        self.refuse_shadowed("set", key);
        self.cur
            .vars
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(key.to_string(), val.to_os_string());
    }

    /// Mask a variable for this thread.
    ///
    /// Masks rather than falls through: the key reads as absent afterwards even
    /// if the process environment still has it.
    ///
    /// # Panics
    ///
    /// If `key` is one of [`GUARDED_KEYS`]. Taking a root away is a way through
    /// the check on naming one: with `XDG_CONFIG_HOME` absent and `HOME` never
    /// seeded, `config::config_path()`'s `unwrap_or_default()` yields a
    /// *relative* `.config/drovr/config.toml`, which resolves against the
    /// working directory and therefore inside the real `$HOME` for any checkout
    /// under it — silently. A root can be pointed somewhere else; it cannot be
    /// removed.
    pub fn unset(&self, key: &str) {
        if GUARDED_KEYS.contains(&key) {
            panic!(
                "drovr test guard: {key} cannot be unset — it names a root, and code that \
                 resolves one falls back to $HOME (or, worse, to a relative path) when it \
                 is absent. Point it at another temp dir with TestEnv::set instead.",
            );
        }
        self.refuse_shadowed("unset", key);
        self.cur
            .vars
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(key);
    }

    /// Refuse a write through a `TestEnv` that is not the innermost frame on
    /// this thread.
    ///
    /// [`current`] answers the shim from `INSTALLED.last()`, so a write into a
    /// shadowed overlay's map lands somewhere no `crate::env::var` will ever
    /// look: the call returns normally, the variable never changes, and the
    /// test asserts against the value it thinks it just set. That is the exact
    /// shape of silent failure this module exists to remove — the process-global
    /// write it replaces failed the same way, just for a different reason.
    ///
    /// The situation is reachable two ways. A test can hold two live `TestEnv`s
    /// and write through the outer one; or a fixture taking `&TestEnv` can be
    /// handed an environment other than the one its caller installed, which is
    /// why the sweep's fixtures take the environment as a parameter rather than
    /// building their own (`plan.md`, the T6–T12 shared rule). Neither is
    /// expressible in the type system — `TestEnv` is `!Send`, so the frame is
    /// known to be on *this* thread, but not that it is on top.
    ///
    /// A hard `panic!`, not a `debug_assert!`: the suite's own release-profile
    /// runs (`nix build`'s `checkPhase`) are exactly where a silent write would
    /// be hardest to notice.
    ///
    /// The predicate is OVERLAY identity, not [`FrameId`] identity, and the two
    /// genuinely differ here. `env.handle().enter()` on the installing thread
    /// pushes a *second frame over the same overlay* — a legitimate, tested
    /// shape — and a write through `env` while that guard is alive lands in the
    /// very map the reads come from. Comparing frames would refuse it. `Drop`
    /// still matches on frame, for the reason recorded there: with one overlay
    /// installed twice, "remove the topmost entry holding this overlay" removes
    /// the wrong one. Same two identities, opposite questions.
    ///
    /// A destroyed thread-local — `try_with` erring, which happens only while
    /// the thread is being torn down — is treated as "cannot prove shadowing"
    /// and stays quiet, matching [`uninstall`]'s precedent. It is the one case
    /// where a lost write is unobservable anyway: [`current`] is already `None`,
    /// so no read can follow it on this thread. Panicking there would be strictly
    /// worse than useless — a panic during TLS destruction while the thread is
    /// already unwinding aborts the process instead of failing one test.
    fn refuse_shadowed(&self, op: &str, key: &str) {
        let visible = INSTALLED
            .try_with(|s| {
                s.borrow()
                    .last()
                    .is_some_and(|(_, o)| Arc::ptr_eq(o, &self.cur))
            })
            .unwrap_or(true);
        if !visible {
            panic!(
                "drovr test guard: TestEnv::{op}({key:?}) through an environment that is not \
                 installed on this thread. Reads answer from the innermost TestEnv, so this \
                 write would be invisible — the variable would keep its old value and the \
                 assertion after it would test nothing. Write through the innermost TestEnv, \
                 or drop it first."
            );
        }
    }

    /// The scratch root — the parent of both seeded roots.
    ///
    /// Borrowed, because the [`tempfile::TempDir`] is an owned field of the
    /// [`Overlay`] this `TestEnv` holds.
    pub fn path(&self) -> &Path {
        self.cur.tmp.path()
    }

    /// The current value of `XDG_DATA_HOME`.
    ///
    /// Read live from the overlay, so it stays correct after a
    /// [`set`](TestEnv::set) override. By value rather than `&Path` because the
    /// value lives behind an `RwLock` guard that cannot outlive the call.
    ///
    /// # Panics
    ///
    /// If the key has been [`unset`](TestEnv::unset). There is no meaningful
    /// data root to report in that case, and the alternative — returning the
    /// seeded path the overlay no longer holds — would be a lie.
    pub fn data_root(&self) -> PathBuf {
        self.expect_root(DATA_KEY)
    }

    /// The current value of `XDG_CONFIG_HOME`; see [`data_root`](TestEnv::data_root).
    pub fn config_root(&self) -> PathBuf {
        self.expect_root(CONFIG_KEY)
    }

    fn expect_root(&self, key: &str) -> PathBuf {
        match self.cur.get_os(key) {
            Some(v) => PathBuf::from(v),
            None => panic!("{key} has been unset on this TestEnv, so it has no root to report"),
        }
    }

    /// Where [`crate::run::run_dir`] will resolve `name`. Does not create it.
    pub fn run_dir(&self, name: &str) -> PathBuf {
        self.data_root().join("drovr/runs").join(name)
    }

    /// A `Send + Sync` handle for a thread this test spawns.
    pub fn handle(&self) -> EnvHandle {
        EnvHandle(Arc::clone(&self.cur))
    }
}

impl Drop for TestEnv {
    fn drop(&mut self) {
        uninstall(self.frame);
    }
}

impl EnvHandle {
    /// Install the same overlay on the calling thread; uninstalls on drop.
    pub fn enter(&self) -> EnteredEnv {
        EnteredEnv {
            frame: install(&self.0),
            _not_send: PhantomData,
        }
    }
}

impl Drop for EnteredEnv {
    fn drop(&mut self) {
        uninstall(self.frame);
    }
}

impl Overlay {
    /// The overlay's answer for `key`, or `None` if it does not hold it.
    ///
    /// `None` means absent, not "ask the process" — [`crate::env`] converts it
    /// straight to `NotPresent`.
    pub(crate) fn get_os(&self, key: &str) -> Option<OsString> {
        self.vars
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(key)
            .cloned()
    }
}

/// The innermost overlay installed on this thread, if any. The shim's door.
pub(crate) fn current() -> Option<Arc<Overlay>> {
    INSTALLED
        .try_with(|s| s.borrow().last().map(|(_, o)| Arc::clone(o)))
        .unwrap_or(None)
}

/// The real `$HOME`, ignoring the overlay.
///
/// A bootstrap read: the home check exists to protect the real home, so it has
/// to look at the real environment. An overlaid `HOME` would let a test
/// disable the check by naming a different home, which is the shape of bypass
/// this whole module exists to remove.
///
/// `var_os`, not `var`: a `HOME` that is not valid UTF-8 is *set*, and folding
/// it in with "unset" would make the one input this function cannot decode also
/// the one that switches the guard off. A guard that fails open on an encoding
/// edge case is not a guard. Nothing downstream needs it as a `String` —
/// [`canonical_ish`] and [`is_forbidden_root`] work on `Path`.
fn real_home() -> Option<PathBuf> {
    // ENV-SHIM-RAW-OK: bootstrap-home
    let home = std::env::var_os("HOME")?;
    if home.is_empty() {
        return None;
    }
    Some(PathBuf::from(home))
}

/// The real `PATH`, ignoring the overlay. Seeded into every new overlay by
/// [`TestEnv::new`]; see its doc comment for why.
fn real_path() -> Option<OsString> {
    // ENV-SHIM-RAW-OK: bootstrap-path
    std::env::var_os("PATH")
}

/// Canonicalise as much of `p` as exists on disk, keeping the rest literal.
///
/// Plain `fs::canonicalize` fails outright on a path that has not been created
/// yet, and both sides of the home check routinely name paths that do not
/// exist: `TestEnv::new` seeds `<tmp>/data` before anything creates it. Falling
/// back to the *literal* path in that case is not good enough in either
/// direction:
///
/// * false negative — with a symlinked `$HOME` (say `/home/u` → `/data/u`), a
///   literal `$HOME/.local/share` does not start with the canonical
///   `/data/u`, and the guard waves through the exact path it exists to stop;
/// * false positive — with a symlinked temp root (macOS `/var` →
///   `/private/var`), a literal `<tmp>/data` does not start with the canonical
///   temp root, and the exemption stops applying to genuinely nested paths.
///
/// Resolving through the deepest ancestor that *does* exist closes both. The
/// only unresolvable case left is a path with no existing ancestor at all,
/// which on a Unix filesystem means none — `/` always exists.
///
/// A **relative** value is joined onto the working directory first. Without
/// that, the ancestor walk bottoms out at `""` and hands back a value that is
/// still relative, and `Path::starts_with` never matches a relative path
/// against an absolute `$HOME` — so the guard would wave through exactly the
/// values it exists to stop, on any machine whose checkout sits under `$HOME`,
/// which is where a developer's checkout usually is. The OS will resolve the
/// value against the working directory; the check has to see the same path the
/// OS will.
fn canonical_ish(p: &Path) -> PathBuf {
    if p.is_relative() {
        if let Ok(cwd) = std::env::current_dir() {
            // `cwd` is absolute, so this recurses at most once.
            return canonical_ish(&cwd.join(p));
        }
    }
    if let Ok(c) = std::fs::canonicalize(p) {
        return c;
    }
    match (p.parent(), p.file_name()) {
        (Some(parent), Some(name)) => canonical_ish(parent).join(name),
        _ => p.to_path_buf(),
    }
}

/// The rule, on already-canonicalised paths: inside `$HOME` and not inside the
/// system temp root.
///
/// Split out from [`refuse_home_root`] so it can be tested against every
/// quadrant without depending on where this particular machine puts its temp
/// dir.
///
/// **The exemption is the system temp root, not the calling `TestEnv`'s own
/// scratch dir.** Scoping it to `env.path()` looks tighter and breaks most of
/// the migration: the fixtures being migrated do not derive their scratch roots
/// from `env.path()`, they call `tempfile::tempdir()` independently, producing
/// a *sibling* — and `Path::starts_with` requires true nesting. On any machine
/// whose `TMPDIR` resolves under `$HOME` — the very case the exemption exists
/// for — the tighter rule would panic on essentially the whole suite.
fn is_forbidden_root(value: &Path, home: &Path, temp_root: &Path) -> bool {
    value.starts_with(home) && !value.starts_with(temp_root)
}

/// Panic if a test just named a scratch root inside the user's home directory.
///
/// `cargo test` destroyed the real `~/.local/share/drovr` twice, taking ~65
/// runs across four agents with it. The mechanism was not one bad test: tests
/// redirected `XDG_DATA_HOME` by mutating it process-globally, and the only
/// thing tying a test to the lock that made that safe was a doc comment.
///
/// The overlay removes the race, but not this: [`TestEnv::set`] is an
/// unrestricted setter, and a thread-scoped write into the real data root is
/// still a write into the real data root. So the property is *relocated*, from
/// the read path — where `run::data_dir` used to re-check the value it had just
/// resolved — to the write path, which is now the single door through which a
/// root can be named. A precondition *on* the capability is the capability;
/// a guard sitting beside one would be a second, weaker mechanism, which is why
/// the read-path check was deleted rather than kept as a backstop.
///
/// `$HOME` unset ⇒ nothing to protect ⇒ no check.
fn refuse_home_root(key: &str, value: &Path) {
    if !GUARDED_KEYS.contains(&key) {
        return;
    }

    // Absolute only, and this check comes first because it is unconditional —
    // it does not depend on $HOME being set or on where the check happens to
    // be standing. A relative value cleared against today's working directory
    // and then STORED relative is a time-of-check/time-of-use hole: set it
    // while the process sits in /tmp, `chdir` to $HOME, and the same stored
    // value now names the live data root, having been validated against
    // somewhere else entirely. Refusing the shape closes that outright, and
    // costs nothing: every root drovr's tests name is a TempDir path.
    if value.is_relative() {
        panic!(
            "drovr test guard: TestEnv::set({key}, {}) — a root must be an absolute path. \
             A relative one is resolved against the working directory at USE time, not at \
             this call, so it can be checked in one place and land in another. Pass an \
             absolute path: TestEnv::new() already seeded a scratch dir, and \
             TestEnv::path() is its root.",
            value.display(),
        );
    }

    let Some(home) = real_home() else {
        return;
    };
    let real_home = canonical_ish(&home);
    let temp_root = canonical_ish(&std::env::temp_dir());
    let real_value = canonical_ish(value);
    if is_forbidden_root(&real_value, &real_home, &temp_root) {
        panic!(
            "drovr test guard: TestEnv::set({key}, {}) named a root inside the real home \
             directory {} and outside the system temp root {} — under $HOME is where the \
             LIVE drovr data lives, and a test writing there is how ~/.local/share/drovr \
             got destroyed. Point it at a temp dir instead: TestEnv::new() already seeded \
             one, and TestEnv::path() is its root.",
            real_value.display(),
            real_home.display(),
            temp_root.display(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Barrier;
    use std::thread;

    const PROBE: &str = "DROVR_TEST_ENV_PROBE";

    /// Two threads, two overlays, neither sees the other's value.
    ///
    /// The barrier is the point: both overlays are installed and written before
    /// either is read, so the assertions run in the window where a
    /// process-global write would have had them clobbering each other.
    #[test]
    fn overlay_is_per_thread() {
        let gate = Arc::new(Barrier::new(2));
        let mut threads = Vec::new();
        for name in ["alpha", "beta"] {
            let gate = Arc::clone(&gate);
            threads.push(thread::spawn(move || {
                let env = TestEnv::new();
                env.set(PROBE, name);
                gate.wait();
                assert_eq!(crate::env::var(PROBE).as_deref(), Ok(name));
                env.data_root()
            }));
        }
        let roots: Vec<PathBuf> = threads
            .into_iter()
            .map(|t| t.join().expect("thread panicked"))
            .collect();
        assert_ne!(roots[0], roots[1], "each TestEnv gets its own scratch root");
    }

    /// Dropping uninstalls, which is the only thing separating sequential tests.
    ///
    /// Under `--test-threads=1` every test runs on the main thread, so the
    /// thread-local is *shared* across them and `Drop` is the whole isolation
    /// story. This asserts it.
    ///
    /// The post-drop half used to assert only that the dropped root was no
    /// longer *the* answer — all it could assert while `crate::env` still fell
    /// through to the process environment, and a weak property: it would also
    /// have held if the read had leaked some third value. Now that the
    /// fallthrough is gone the stronger statement is available and is the one
    /// worth pinning: after the drop there is no answer at all.
    #[test]
    fn overlay_does_not_leak_between_sequential_tests() {
        let root = {
            let env = TestEnv::new();
            let root = env.data_root();
            assert_eq!(
                crate::env::var(DATA_KEY).map(PathBuf::from),
                Ok(root.clone())
            );
            root
        };
        assert!(current().is_none(), "dropping a TestEnv uninstalls it");

        // Caught rather than `#[should_panic]`, so that the pre-drop half above
        // stays in the same test as the property it is the setup for.
        let after = std::panic::catch_unwind(|| crate::env::var(DATA_KEY).ok());
        let msg = after.expect_err(&format!(
            "reading {DATA_KEY} after the overlay was dropped must refuse, not \
             answer — the dropped root was {}",
            root.display(),
        ));
        let msg = panic_message(&msg);
        assert!(
            msg.contains("no TestEnv installed on this thread"),
            "expected the no-overlay refusal, got: {msg}",
        );
    }

    /// The payload of a `catch_unwind` error, as a string.
    ///
    /// `panic!` with a formatted message boxes a `String`; a literal one boxes a
    /// `&'static str`. Both shapes occur in this crate, and a test that checked
    /// only one would silently accept any panic of the other shape.
    fn panic_message(e: &Box<dyn std::any::Any + Send>) -> &str {
        if let Some(s) = e.downcast_ref::<String>() {
            s
        } else if let Some(s) = e.downcast_ref::<&str>() {
            s
        } else {
            "<panic payload was neither String nor &str>"
        }
    }

    #[test]
    fn nesting_restores_the_outer_overlay() {
        let outer = TestEnv::new();
        outer.set(PROBE, "outer");
        assert_eq!(outer.config_root(), outer.path().join("config"));
        {
            let inner = TestEnv::new();
            inner.set(PROBE, "inner");
            assert_eq!(crate::env::var(PROBE).as_deref(), Ok("inner"));
        }
        assert_eq!(crate::env::var(PROBE).as_deref(), Ok("outer"));
        assert_eq!(
            crate::env::var(DATA_KEY).map(PathBuf::from),
            Ok(outer.data_root())
        );
    }

    /// Dropping out of LIFO order uninstalls the right one.
    ///
    /// Nothing in the type system forces `drop(outer)` to come after
    /// `drop(inner)` — `drop(env)` compiles, and a fixture that returns an inner
    /// `TestEnv` out of a block drops the outer one first. A guard that just
    /// restored a saved "previous" would clear the overlay out from under the
    /// still-live inner env, and reads would silently fall through to the
    /// process environment: exactly the failure this module exists to remove,
    /// reintroduced through its own `Drop`.
    #[test]
    fn dropping_out_of_order_leaves_the_still_live_env_installed() {
        let outer = TestEnv::new();
        outer.set(PROBE, "outer");
        let inner = TestEnv::new();
        inner.set(PROBE, "inner");

        drop(outer);
        assert_eq!(crate::env::var(PROBE).as_deref(), Ok("inner"));
        assert_eq!(
            crate::env::var(DATA_KEY).map(PathBuf::from),
            Ok(inner.data_root())
        );

        drop(inner);
        assert!(current().is_none(), "the last env out uninstalls cleanly");
    }

    /// An `EnteredEnv` on the installing thread outlives the `TestEnv`.
    ///
    /// The overlay — and the scratch dir inside it — is kept alive by the `Arc`,
    /// not by the `TestEnv`, so the entered guard keeps resolving after its
    /// installer is gone.
    #[test]
    fn an_entered_guard_outlives_the_test_env_that_created_it() {
        let env = TestEnv::new();
        env.set(PROBE, "shared");
        let entered = env.handle().enter();
        let root = env.data_root();

        drop(env);
        assert_eq!(crate::env::var(PROBE).as_deref(), Ok("shared"));
        assert_eq!(crate::env::var(DATA_KEY).map(PathBuf::from), Ok(root));

        drop(entered);
        assert!(current().is_none());
    }

    /// A spawned thread that `enter()`s resolves the parent's roots — including
    /// through `run::data_dir`, which is what the migrated fixtures actually
    /// call.
    #[test]
    fn handle_enter_shares_the_same_roots() {
        let env = TestEnv::new();
        env.set(PROBE, "shared");
        let handle = env.handle();
        let seen = thread::spawn(move || {
            let _entered = handle.enter();
            (
                crate::env::var(PROBE).ok(),
                crate::run::data_dir(),
                crate::run::run_dir("some-run"),
            )
        })
        .join()
        .expect("thread panicked");
        assert_eq!(seen.0.as_deref(), Some("shared"));
        assert_eq!(seen.1, env.data_root().join("drovr"));
        assert_eq!(seen.2, env.run_dir("some-run"));
    }

    /// `unset` masks; it does not fall through to the process environment.
    #[test]
    fn an_unset_key_is_not_present() {
        let env = TestEnv::new();
        env.set(PROBE, "here");
        assert_eq!(crate::env::var(PROBE).as_deref(), Ok("here"));
        env.unset(PROBE);
        assert_eq!(crate::env::var(PROBE), Err(std::env::VarError::NotPresent));
        assert_eq!(crate::env::var_os(PROBE), None);

        // PATH is seeded from the real environment and still present...
        assert_eq!(crate::env::var_os("PATH"), real_path());
        // ...and unsetting it hides it even though the process still has it.
        env.unset("PATH");
        assert_eq!(crate::env::var_os("PATH"), None);
    }

    /// The relocated guard: naming the live data root is refused at the write.
    ///
    /// The direct successor to `run::tests::data_dir_refuses_to_resolve_inside_the_real_home`,
    /// and it exists *before* that test is deleted, deliberately.
    ///
    /// `catch_unwind` rather than `#[should_panic]` for two reasons: the panic
    /// message is asserted, and the whole assertion can be skipped when `$HOME`
    /// is unset, which `#[should_panic]` cannot express. The panic is contained
    /// here and takes no lock — nor does anything else in this module, which is
    /// why the poison cascade `forge.ko.ag/drovr/drovr/issues` records (under two headings
    /// naming the mutex, unquoted here because the name is the token this run
    /// removed from `src/`) can no longer happen. libtest still prints the
    /// caught panic; that is noise, not a failure.
    ///
    /// On a machine whose system temp root *contains* `$HOME` the exemption
    /// swallows this case and the assertion fails. Such a machine is not one
    /// this check can be made green on: the exemption is what lets the suite's
    /// own scratch dirs be named at all.
    #[test]
    fn setting_the_data_root_inside_the_real_home_panics() {
        let Some(home) = real_home() else {
            eprintln!("skipped: HOME is unset, so there is no real home to protect");
            return;
        };
        let env = TestEnv::new();
        let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            env.set(DATA_KEY, home.join(".local/share"));
        }))
        .expect_err("naming the live data root must panic");
        let msg = payload.downcast_ref::<String>().cloned().unwrap_or_default();
        assert!(msg.contains("inside the real home directory"), "{msg}");
        assert_eq!(
            env.data_root(),
            env.path().join("data"),
            "a refused set must not have modified the overlay",
        );
    }

    /// The exemption, so the check cannot be "fixed" into rejecting legitimate
    /// use.
    ///
    /// Deliberately an *independently created* `tempfile::TempDir`, not
    /// `env.path().join(…)`: that is the shape the migration actually uses —
    /// a sibling of `env.path()`, not a child — and testing only the child
    /// shape is what let a too-narrow draft of this rule survive review once.
    #[test]
    fn setting_a_root_inside_the_temp_dir_is_allowed() {
        let env = TestEnv::new();
        let other = tempfile::TempDir::new().expect("scratch dir");
        env.set(DATA_KEY, other.path());
        assert_eq!(env.data_root(), other.path());
        assert_eq!(crate::run::data_dir(), other.path().join("drovr"));
        env.set(CONFIG_KEY, other.path().join("cfg"));
        assert_eq!(env.config_root(), other.path().join("cfg"));
    }

    /// A write through a shadowed `TestEnv` is refused rather than lost.
    ///
    /// Found by task 6's review panel. Reads answer from `INSTALLED.last()`, so
    /// writing through the OUTER of two live environments used to return
    /// normally while changing nothing a `crate::env::var` would ever see — the
    /// variable kept its old value and the assertion after it tested nothing.
    /// That is the same silent-success failure the process-global write had,
    /// which is the whole reason this module exists, so it has to be loud.
    ///
    /// `catch_unwind` for the reasons given on
    /// `setting_the_data_root_inside_the_real_home_panics`; the panic takes no
    /// lock and cannot poison anything.
    #[test]
    fn writing_through_a_shadowed_env_is_refused_not_silently_lost() {
        let outer = TestEnv::new();
        outer.set("SHADOW_PROBE", "from-outer");
        let inner = TestEnv::new();

        for op in ["set", "unset"] {
            let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match op {
                "set" => outer.set("SHADOW_PROBE", "from-outer-while-shadowed"),
                _ => outer.unset("SHADOW_PROBE"),
            }))
            .expect_err("a write through the shadowed env must panic");
            let msg = payload.downcast_ref::<String>().cloned().unwrap_or_default();
            assert!(msg.contains("not installed on this thread"), "{op}: {msg}");
        }

        // The refused writes changed neither overlay: the inner one never held
        // the key, and the outer one still holds its original value.
        assert_eq!(crate::env::var_os("SHADOW_PROBE"), None);
        drop(inner);
        assert_eq!(
            crate::env::var("SHADOW_PROBE").as_deref(),
            Ok("from-outer"),
            "a refused write must not have modified the shadowed overlay either"
        );

        // ...and the guard does NOT fire on the shape that only *looks* like
        // shadowing: `enter()` on the installing thread pushes a second frame
        // over the SAME overlay, so a write through `outer` is still the write
        // the reads see. A frame-identity check would wrongly refuse this.
        let entered = outer.handle().enter();
        outer.set("SHADOW_PROBE", "still-visible");
        assert_eq!(
            crate::env::var("SHADOW_PROBE").as_deref(),
            Ok("still-visible"),
            "a second frame over the same overlay is not shadowing"
        );
        drop(entered);
    }

    /// Every quadrant of the rule, including the one this machine cannot
    /// produce: a temp root that lives under `$HOME`.
    #[test]
    fn the_home_rule_exempts_only_the_system_temp_root() {
        let home = Path::new("/home/u");
        let temp = Path::new("/tmp");
        assert!(is_forbidden_root(
            Path::new("/home/u/.local/share"),
            home,
            temp
        ));
        assert!(!is_forbidden_root(Path::new("/tmp/x/data"), home, temp));
        assert!(!is_forbidden_root(Path::new("/var/x/data"), home, temp));

        // TMPDIR under $HOME — the case the exemption exists for.
        let temp_in_home = Path::new("/home/u/tmp");
        assert!(!is_forbidden_root(
            Path::new("/home/u/tmp/x/data"),
            home,
            temp_in_home
        ));
        assert!(is_forbidden_root(
            Path::new("/home/u/.local/share"),
            home,
            temp_in_home
        ));
    }

    /// The seeded roots do not exist yet, so the check compares paths
    /// `fs::canonicalize` refuses outright. Resolving through the deepest
    /// existing parent is what keeps both sides comparable.
    /// A relative value is resolved the way the OS will resolve it.
    ///
    /// Left relative, it could never `starts_with` an absolute `$HOME`, so the
    /// home check would pass it unconditionally — a hole exactly as wide as
    /// "the value has not been created yet", which every fresh scratch root is.
    #[test]
    fn a_relative_root_is_resolved_against_the_working_directory() {
        let cwd = std::env::current_dir().expect("a working directory");
        assert_eq!(
            canonical_ish(Path::new("drovr_no_such_dir_here/data")),
            canonical_ish(&cwd).join("drovr_no_such_dir_here/data"),
        );
    }

    /// ...and a guarded key refuses one outright, whatever it resolves to today.
    ///
    /// Checking a relative value against the working directory and then storing
    /// it relative would be a time-of-check/time-of-use hole: `set` in `/tmp`,
    /// `chdir` to `$HOME`, and the same stored value now names the live data
    /// root while the check that cleared it looked somewhere else entirely.
    /// Refusing the shape closes that without needing to reason about it.
    #[test]
    fn a_relative_root_is_refused_outright() {
        for key in [DATA_KEY, CONFIG_KEY, HOME_KEY] {
            let env = TestEnv::new();
            let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                env.set(key, "drovr_relative_probe/data");
            }))
            .expect_err("a relative root must be refused");
            let msg = payload.downcast_ref::<String>().cloned().unwrap_or_default();
            assert!(msg.contains("must be an absolute path"), "{key}: {msg}");
        }
    }

    /// A root cannot be taken away either, only pointed somewhere safe.
    ///
    /// `unset` would otherwise be a hole straight through the write guard, and
    /// the two guarded XDG keys fail differently when absent — which is what
    /// makes it worth closing rather than reasoning about:
    ///
    /// * `unset(XDG_DATA_HOME)` → `data_dir()`'s `$HOME` fallback reads absent
    ///   (the overlay never seeds `HOME`) and `.unwrap()` panics. Loud, safe.
    /// * `unset(XDG_CONFIG_HOME)` → `config_path()`'s fallback is
    ///   `unwrap_or_default()`, so an absent `HOME` yields a *relative*
    ///   `.config/drovr/config.toml`, which resolves against the working
    ///   directory — inside the real `$HOME` for any checkout under it. Silent,
    ///   and exactly the class of thing this module exists to stop.
    #[test]
    fn a_guarded_key_cannot_be_unset() {
        for key in GUARDED_KEYS {
            let env = TestEnv::new();
            let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                env.unset(key);
            }))
            .expect_err("unsetting a root must be refused");
            let msg = payload.downcast_ref::<String>().cloned().unwrap_or_default();
            assert!(msg.contains("cannot be unset"), "{key}: {msg}");
        }
    }

    /// `HOME` is a root-naming key too, so it is guarded like the XDG pair.
    ///
    /// `data_dir()` falls back to `$HOME/.local/share` when `XDG_DATA_HOME` is
    /// absent, and config resolution has the same fallback. Guarding only the
    /// XDG pair would leave `unset(XDG_DATA_HOME)` + `set(HOME, <real home>)`
    /// as an unguarded route to exactly the directory this guard exists for.
    #[test]
    fn setting_home_to_the_real_home_is_refused() {
        let Some(home) = real_home() else {
            eprintln!("skipped: HOME is unset, so there is no real home to protect");
            return;
        };
        let env = TestEnv::new();
        let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            env.set(HOME_KEY, &home);
        }))
        .expect_err("naming the real home as HOME must be refused");
        let msg = payload.downcast_ref::<String>().cloned().unwrap_or_default();
        assert!(msg.contains("inside the real home directory"), "{msg}");
    }

    /// Each guard uninstalls the frame IT pushed, not merely one that happens
    /// to carry the same overlay.
    ///
    /// `entered` re-installs `outer`'s overlay ON TOP of `inner`, so the stack
    /// holds that overlay twice. Dropping `outer` must remove `outer`'s own
    /// frame — the bottom one — and leave the guard's on top; matching by
    /// overlay identity alone would pop the guard's frame and silently switch
    /// reads to `inner` while the guard is still live.
    #[test]
    fn a_guard_uninstalls_its_own_frame_not_a_twin() {
        let outer = TestEnv::new();
        outer.set(PROBE, "outer");
        let inner = TestEnv::new();
        inner.set(PROBE, "inner");
        let entered = outer.handle().enter();
        assert_eq!(crate::env::var(PROBE).as_deref(), Ok("outer"));

        drop(outer);
        assert_eq!(
            crate::env::var(PROBE).as_deref(),
            Ok("outer"),
            "the still-live entered guard keeps its own overlay on top",
        );

        drop(entered);
        assert_eq!(crate::env::var(PROBE).as_deref(), Ok("inner"));
        drop(inner);
        assert!(current().is_none());
    }

    #[test]
    fn a_not_yet_created_path_canonicalises_through_its_deepest_existing_parent() {
        let tmp = tempfile::TempDir::new().expect("scratch dir");
        let real = std::fs::canonicalize(tmp.path()).expect("tempdir exists");
        assert_eq!(canonical_ish(&tmp.path().join("a/b/c")), real.join("a/b/c"));
    }
}
