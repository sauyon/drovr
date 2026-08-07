//! A test's environment, scoped to the thread that installed it.
//!
//! [`TestEnv::new`] hands a test a fresh scratch root and installs an overlay
//! on the calling thread. From that moment [`crate::env::var`] and
//! [`crate::env::var_os`] — the crate's only environment reads — answer from
//! the overlay and nothing else, so two tests running concurrently can no
//! longer resolve each other's `XDG_DATA_HOME`.
//!
//! This replaces a convention with a capability. The old shape was a
//! process-global `set_var` under `ENV_LOCK` whose contract lived in a doc
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
//! * **Drop restores.** Nesting is LIFO and `--test-threads=1` cannot leak
//!   state forward, because sequential tests on one thread are separated only
//!   by [`Drop`]. `let _ = TestEnv::new()` uninstalls immediately, and the next
//!   `data_dir()` then fails rather than resolving anything real — fail-closed
//!   by construction, so no lint is needed to catch the mis-binding.
//!
//! # Two bootstrap reads
//!
//! `PATH` and `HOME` are read from the real process environment here, and
//! nowhere else in the crate ([`crate::env`]'s `RAW_EXCEPTIONS` names both).
//! They cannot go through the overlay they are themselves installing.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::sync::{Arc, PoisonError, RwLock};

/// The two keys the home check applies to — the only ones that name a root
/// drovr will go on to create and delete files under.
const DATA_KEY: &str = "XDG_DATA_HOME";
const CONFIG_KEY: &str = "XDG_CONFIG_HOME";

/// A scratch environment installed on THIS thread.
///
/// Restores the previous overlay on drop, so nesting is LIFO.
///
/// Deliberately `!Send`: moving an installed `TestEnv` to another thread would
/// make its `Drop` uninstall from the wrong thread's [`CURRENT`], silently
/// breaking the thread-scoping the whole design rests on. Cross a thread
/// boundary with [`TestEnv::handle`], which is the supported way.
pub struct TestEnv {
    prev: Option<Arc<Overlay>>,
    cur: Arc<Overlay>,
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
/// `!Send` for the same reason [`TestEnv`] is: its `Drop` restores
/// [`CURRENT`] on whichever thread it happens to run on, so it must not be the
/// wrong one.
pub struct EnteredEnv {
    prev: Option<Arc<Overlay>>,
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

thread_local! {
    /// The overlay installed on this thread, if any.
    static CURRENT: RefCell<Option<Arc<Overlay>>> = const { RefCell::new(None) };
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
        let prev = CURRENT.with(|c| c.borrow_mut().replace(Arc::clone(&cur)));
        TestEnv {
            prev,
            cur,
            _not_send: PhantomData,
        }
    }

    /// Overlay a variable for this thread.
    ///
    /// Takes `impl AsRef<OsStr>` so call sites can pass `&Path`, `PathBuf` or
    /// `&str` interchangeably, as they do today with `set_var`.
    ///
    /// # Panics
    ///
    /// If `key` is `XDG_DATA_HOME` or `XDG_CONFIG_HOME` and `val` resolves
    /// inside the real `$HOME` but outside the system temp root — see
    /// [`refuse_home_root`]. The check runs *before* the lock is taken, so a
    /// rejected write cannot poison the overlay.
    pub fn set(&self, key: &str, val: impl AsRef<OsStr>) {
        let val = val.as_ref();
        refuse_home_root(key, Path::new(val));
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
    pub fn unset(&self, key: &str) {
        self.cur
            .vars
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(key);
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
        let prev = self.prev.take();
        // `try_with`, not `with`: dropping during thread teardown, after the
        // thread-local has been destroyed, must not turn into a second panic.
        let _ = CURRENT.try_with(|c| *c.borrow_mut() = prev);
    }
}

impl EnvHandle {
    /// Install the same overlay on the calling thread; uninstalls on drop.
    pub fn enter(&self) -> EnteredEnv {
        let prev = CURRENT.with(|c| c.borrow_mut().replace(Arc::clone(&self.0)));
        EnteredEnv {
            prev,
            _not_send: PhantomData,
        }
    }
}

impl Drop for EnteredEnv {
    fn drop(&mut self) {
        let prev = self.prev.take();
        let _ = CURRENT.try_with(|c| *c.borrow_mut() = prev);
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

/// The overlay installed on this thread, if any. The shim's door.
pub(crate) fn current() -> Option<Arc<Overlay>> {
    CURRENT.try_with(|c| c.borrow().clone()).unwrap_or(None)
}

/// The real `$HOME`, ignoring the overlay.
///
/// A bootstrap read: the home check exists to protect the real home, so it has
/// to look at the real environment. An overlaid `HOME` would let a test
/// disable the check by naming a different home, which is the shape of bypass
/// this whole module exists to remove.
fn real_home() -> Option<PathBuf> {
    // ENV-SHIM-RAW-OK: bootstrap-home
    match std::env::var("HOME") {
        Ok(h) if !h.is_empty() => Some(PathBuf::from(h)),
        _ => None,
    }
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
fn canonical_ish(p: &Path) -> PathBuf {
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
/// the read path — where `run::refuse_home_data_root` still holds it, until it
/// is deleted — to the write path, which is now the single door through which a
/// root can be named. A precondition *on* the capability is the capability;
/// a guard sitting beside one would be a second, weaker mechanism.
///
/// `$HOME` unset ⇒ nothing to protect ⇒ no check.
fn refuse_home_root(key: &str, value: &Path) {
    if key != DATA_KEY && key != CONFIG_KEY {
        return;
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
    /// process-global `set_var` would have had them clobbering each other.
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
        assert_ne!(
            crate::env::var(DATA_KEY).map(PathBuf::from).ok(),
            Some(root),
            "the dropped overlay's data root is still visible",
        );
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
    /// here and takes no lock, so it cannot poison anything — see
    /// `docs/known-issues.md` → "A panicking test can poison `ENV_LOCK` for the
    /// whole suite". libtest still prints the caught panic; that is noise, not
    /// a failure.
    ///
    /// On a machine whose system temp root *contains* `$HOME` the exemption
    /// swallows this case and the assertion fails. That machine already fails
    /// the whole suite through `run::refuse_home_data_root`, which has no
    /// exemption at all; it is not a case this task can make green.
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
    #[test]
    fn a_not_yet_created_path_canonicalises_through_its_deepest_existing_parent() {
        let tmp = tempfile::TempDir::new().expect("scratch dir");
        let real = std::fs::canonicalize(tmp.path()).expect("tempdir exists");
        assert_eq!(canonical_ish(&tmp.path().join("a/b/c")), real.join("a/b/c"));
    }
}
