//! The crate's only door to the process environment.
//!
//! Every environment *read* in `src/` goes through [`var`] or [`var_os`]. That
//! is not a convention: [`tests::crate_reads_the_environment_only_through_the_shim`]
//! scans the source and fails on any raw `std::env::var*` that has not declared
//! itself an exception.
//!
//! Outside `cfg(test)` these are verbatim `std::env` forwards. The shipped
//! binary resolves its environment exactly as it did before this module
//! existed; the indirection buys nothing at runtime and is not meant to.
//!
//! Under `cfg(test)` this is the seam that makes test isolation authoritative.
//! A test installs an overlay on its own thread and the shim answers only from
//! it, so two tests can no longer race each other through one process-global
//! `XDG_DATA_HOME`. There is deliberately **no per-key fallthrough** once an
//! overlay is installed: a key the overlay does not hold reads as absent, which
//! is what turns a test that still writes the process environment into a loud
//! failure rather than a silent one.
//!
//! There is likewise no fallthrough for the *absent overlay*. A read on a thread
//! with no `TestEnv` (`cli/src/test_env.rs`) installed panics rather than answering
//! from the process. That is what makes the isolation authoritative instead of
//! conventional: a test cannot opt out of declaring its environment, because
//! there is no longer anything to opt out *to*.
//!
//! The shim is READ-ONLY by design: there are two doors here and both of them
//! read. Process-global writes were the mechanism this removed, so there is no
//! forwarding wrapper for them — that would be the same capability re-exported
//! behind a nicer name. A test names its variables through
//! `TestEnv`, which writes only its own thread's overlay.

use std::env::VarError;
use std::ffi::OsString;

/// Read an environment variable as UTF-8, with `std::env::var`'s exact error
/// semantics.
#[cfg(not(test))]
pub fn var(key: &str) -> Result<String, VarError> {
    std::env::var(key)
}

/// Read an environment variable as raw OS bytes.
#[cfg(not(test))]
pub fn var_os(key: &str) -> Option<OsString> {
    std::env::var_os(key)
}

/// Read an environment variable as UTF-8, with `std::env::var`'s exact error
/// semantics — but only from this thread's overlay.
///
/// # Panics
///
/// If no overlay is installed on the calling thread. See [`no_overlay`].
#[cfg(test)]
pub fn var(key: &str) -> Result<String, VarError> {
    let Some(overlay) = crate::test_env::current() else {
        no_overlay(key)
    };
    // Converted here rather than in the overlay, so that `std::env::var`'s
    // error semantics stay in one place: absent is NotPresent, non-UTF-8 is
    // NotUnicode carrying the original bytes.
    match overlay.get_os(key) {
        None => Err(VarError::NotPresent),
        Some(v) => v.into_string().map_err(VarError::NotUnicode),
    }
}

/// Read an environment variable as raw OS bytes, from this thread's overlay.
///
/// # Panics
///
/// If no overlay is installed on the calling thread. See [`no_overlay`].
#[cfg(test)]
pub fn var_os(key: &str) -> Option<OsString> {
    let Some(overlay) = crate::test_env::current() else {
        no_overlay(key)
    };
    overlay.get_os(key)
}

/// The refusal both `cfg(test)` readers share.
///
/// Panicking is the whole point, and the alternatives are each a way of being
/// silently wrong. Falling through to the process environment is what this run
/// removed — it is how a test resolved another test's `XDG_DATA_HOME`, and how
/// `cargo test` twice deleted the live `~/.local/share/drovr`. Returning
/// `NotPresent` instead would be quieter and worse: `data_dir()` would take its
/// `$HOME/.local/share` fallback with `HOME` also absent, `unwrap()` on it, and
/// every caller would report a confusing failure some distance from the test
/// that forgot to declare an environment.
///
/// So the refusal names the thread as well as the key. The two ways to reach
/// here are a test that installed nothing, and — much more easily missed — a
/// thread the test *spawned*, which does not inherit the parent's overlay
/// because the overlay is thread-scoped by design.
#[cfg(test)]
fn no_overlay(key: &str) -> ! {
    panic!(
        "drovr test guard: read of {key} with no TestEnv installed on this thread. Under \
         cfg(test) drovr does not read the process environment at all — construct a \
         `TestEnv` (cli/src/test_env.rs) at the top of the test, or `EnvHandle::enter()` \
         if this is a thread the test spawned."
    )
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    /// A raw `std::env` access that is allowed to stay raw, and the reason.
    ///
    /// `file` is the path under `cli/src/`, `/`-separated. `tag` is the label
    /// written in an `ENV-SHIM-RAW-OK: <tag>` comment immediately above the
    /// site: the exception is keyed on that, not on the source line's text, so
    /// reformatting the call does not read as a policy violation while moving
    /// it out from under its comment does. `why` is here to be read by whoever
    /// is tempted to add the next entry.
    struct RawException {
        file: &'static str,
        tag: &'static str,
        why: &'static str,
    }

    /// The complete list. Adding to it is a design decision, not a fix for a
    /// failing test — a raw read is invisible to the overlay and therefore
    /// still races.
    const RAW_EXCEPTIONS: &[RawException] = &[
        RawException {
            file: "test_env.rs",
            tag: "bootstrap-home",
            why: "TestEnv's home check protects the REAL home, so it must read the real \
                  HOME. Reading it through the overlay would let a test disable the check \
                  by naming a different home — the bypass shape this module exists to \
                  remove. Permanent: it outlives the transitional shim.",
        },
        RawException {
            file: "test_env.rs",
            tag: "bootstrap-path",
            why: "PATH is copied verbatim INTO each new overlay so command_available keeps \
                  finding the agent binaries. A read cannot go through the overlay it is \
                  itself seeding. Permanent, and deliberately the only key treated this \
                  way — a hermetic PATH is a separate want.",
        },
    ];

    /// The comment that claims an exception, e.g. `// ENV-SHIM-RAW-OK: <tag>`.
    const MARKER: &str = "ENV-SHIM-RAW-OK:";

    /// `env.rs` is the door itself; its own `std::env` calls are the point.
    const THE_DOOR: &str = "env.rs";

    fn src_dir() -> &'static Path {
        Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/src"))
    }

    /// Every `.rs` file under `cli/src/`, at any depth.
    ///
    /// Recursive on purpose: a guard that stops looking the day someone splits
    /// a module into a directory is the silent hole this whole module exists to
    /// close.
    fn rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
        let entries =
            std::fs::read_dir(dir).unwrap_or_else(|e| panic!("reading {}: {e}", dir.display()));
        for entry in entries {
            let path = entry
                .unwrap_or_else(|e| panic!("entry under {}: {e}", dir.display()))
                .path();
            if path.is_dir() {
                rs_files(&path, out);
            } else if path.extension().is_some_and(|x| x == "rs") {
                out.push(path);
            }
        }
    }

    /// Source lines that are commented out are prose, not code.
    ///
    /// This is a line-comment check only — a `std::env::var` buried in a block
    /// comment would be reported. Nothing in the crate does that, and a false
    /// positive here is a visible test failure rather than a silent hole.
    fn is_comment(line: &str) -> bool {
        line.trim_start().starts_with("//")
    }

    /// What this line does that only [`super::var`]/[`super::var_os`] should.
    ///
    /// Two shapes, because catching only the first leaves the second as an
    /// escape hatch: `use std::env::var;` followed by a bare `var("HOME")` is a
    /// process-global read that no substring search for `std::env::var` would
    /// ever see. No file but `env.rs` needs to name `std::env` in a `use` at
    /// all, so the rule is simply that none may.
    ///
    /// `std::env::vars`/`vars_os` are caught by the same substring as `var`.
    fn violation(line: &str) -> Option<&'static str> {
        if line.contains("std::env::var") {
            Some("raw environment read")
        } else if line.contains("use std::env") {
            Some("import of std::env, which hides raw reads from this scan")
        } else {
            None
        }
    }

    /// Every environment read in `src/` goes through this module.
    ///
    /// The chokepoint is the whole point of the module: a read that bypasses it
    /// cannot be redirected by a test's overlay, so it keeps resolving whatever
    /// the process-global environment happens to say at that instant. Grepping
    /// for that by hand once, at the moment the sweep lands, protects nothing
    /// from the tasks that come after it.
    ///
    /// Scope is `cli/src/`. The integration tests under `cli/tests/` drive the
    /// *built binary*, compiled without `cfg(test)`, so there is no overlay for
    /// them to bypass; they pin the child's environment themselves.
    #[test]
    fn crate_reads_the_environment_only_through_the_shim() {
        let mut offenders: Vec<String> = Vec::new();
        let mut matched = vec![false; RAW_EXCEPTIONS.len()];

        let mut files = Vec::new();
        rs_files(src_dir(), &mut files);
        files.sort();
        assert!(!files.is_empty(), "found no .rs files in {:?}", src_dir());

        for path in &files {
            let name = path
                .strip_prefix(src_dir())
                .expect("rs_files only yields paths under src/")
                .to_str()
                .expect("source file names are UTF-8")
                .replace(std::path::MAIN_SEPARATOR, "/");
            if name == THE_DOOR {
                continue;
            }
            let text = std::fs::read_to_string(path)
                .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));

            // A marker comment claims the next line of code, so that the reason
            // travels with the site instead of living only in a table far away.
            let mut claimed: Option<&str> = None;
            for (i, line) in text.lines().enumerate() {
                if line.trim().is_empty() {
                    continue;
                }
                if is_comment(line) {
                    if let Some((_, tag)) = line.split_once(MARKER) {
                        claimed = Some(tag.trim());
                    }
                    continue;
                }
                let tag = claimed.take();
                let Some(what) = violation(line) else {
                    continue;
                };
                match tag.and_then(|t| {
                    RAW_EXCEPTIONS
                        .iter()
                        .position(|x| x.file == name && x.tag == t)
                }) {
                    Some(idx) => matched[idx] = true,
                    None => offenders.push(format!("{}:{}: {what}: {}", name, i + 1, line.trim())),
                }
            }
        }

        assert!(
            offenders.is_empty(),
            "{} site(s) reach the process environment around crate::env. A raw read \
             cannot be redirected by a test's overlay, so it still resolves the \
             process-global environment and still races. Route each through \
             crate::env::var / crate::env::var_os, or — if it genuinely must stay raw — \
             mark it with a `// {MARKER} <tag>` comment on the line above and declare \
             that tag in RAW_EXCEPTIONS in cli/src/env.rs with the reason:\n  {}",
            offenders.len(),
            offenders.join("\n  "),
        );

        for (i, ok) in matched.iter().enumerate() {
            assert!(
                ok,
                "RAW_EXCEPTIONS[{i}] matches no marked site ({}, tag {}). The site moved, \
                 lost its `{MARKER}` comment, or went away; delete the exception rather \
                 than leaving a standing permission nobody is using. It was granted \
                 because: {}",
                RAW_EXCEPTIONS[i].file, RAW_EXCEPTIONS[i].tag, RAW_EXCEPTIONS[i].why,
            );
        }
    }

    /// With no overlay installed the shim refuses, rather than answering from
    /// the process.
    ///
    /// This replaces the transitional `without_an_overlay_the_shim_is_std_env`,
    /// which asserted the opposite equality — and it is the single assertion
    /// that makes the isolation authoritative rather than conventional. While
    /// the fallthrough existed, "every test declares its environment" was a
    /// convention: a test that declared nothing still got an answer, just not a
    /// reproducible one. `PATH` is deliberately the key, because it is the one
    /// the process certainly *does* hold — a refusal on an absent key would
    /// prove nothing about fallthrough.
    #[test]
    #[should_panic(expected = "no TestEnv installed on this thread")]
    fn without_an_overlay_the_shim_refuses_to_read() {
        assert!(
            crate::test_env::current().is_none(),
            "this test asserts the NO-overlay branch; an overlay installed on \
             this thread would answer the read below instead of refusing it",
        );
        let _ = super::var("PATH");
    }

    /// `var_os` refuses on the same terms as [`var`](super::var).
    ///
    /// Separate test rather than a second line in the one above: `should_panic`
    /// stops at the first panic, so a single test would assert only whichever
    /// door it reached first and the other could quietly regain a fallthrough.
    #[test]
    #[should_panic(expected = "no TestEnv installed on this thread")]
    fn without_an_overlay_var_os_refuses_too() {
        assert!(crate::test_env::current().is_none());
        let _ = super::var_os("PATH");
    }

    /// With an overlay installed the shim answers from it, and ONLY from it.
    ///
    /// The second half is the load-bearing one. There is no per-key
    /// fallthrough: a key the overlay does not hold reads as absent even when
    /// the process environment has it. That is what turns a test which still
    /// writes the process environment into a loud failure instead of a silent
    /// pass on somebody else's value.
    #[test]
    fn with_an_overlay_the_shim_answers_only_from_it() {
        const PROBE: &str = "DROVR_ENV_SHIM_PROBE";
        let env = crate::test_env::TestEnv::new();
        env.set(PROBE, "from the overlay");
        assert_eq!(super::var(PROBE).as_deref(), Ok("from the overlay"));
        assert_eq!(
            super::var_os(PROBE),
            Some(std::ffi::OsString::from("from the overlay")),
        );

        // `HOME` is set in every environment this suite runs in and the overlay
        // deliberately never seeds it. Guarded rather than asserted, because a
        // machine without `HOME` has nothing to prove here.
        if std::env::var_os("HOME").is_some() {
            assert_eq!(super::var("HOME"), Err(std::env::VarError::NotPresent));
            assert_eq!(super::var_os("HOME"), None);
        }
    }
}
