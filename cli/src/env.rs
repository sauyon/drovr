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
//! The overlay type does not exist yet, so today the `cfg(test)` bodies are the
//! one transitional line marked below.
//!
//! The shim is READ-ONLY by design. `set_var`/`remove_var` are not forwarded:
//! process-global writes are the mechanism being removed, not a capability to
//! be re-exported behind a nicer name.

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
/// semantics.
#[cfg(test)]
pub fn var(key: &str) -> Result<String, VarError> {
    // TRANSITIONAL — deleted at T13, which is what makes the capability authoritative.
    // Until then an unmigrated test still reads the process env under ENV_LOCK.
    std::env::var(key)
}

/// Read an environment variable as raw OS bytes.
#[cfg(test)]
pub fn var_os(key: &str) -> Option<OsString> {
    // TRANSITIONAL — deleted at T13, which is what makes the capability authoritative.
    // Until then an unmigrated test still reads the process env under ENV_LOCK.
    std::env::var_os(key)
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
    const RAW_EXCEPTIONS: &[RawException] = &[RawException {
        file: "run.rs",
        tag: "refuse-home-data-root",
        why: "refuse_home_data_root must see the REAL $HOME. The cfg(test) shim answers \
              only from the overlay, which never seeds HOME, so a shimmed read would \
              return NotPresent and the guard would pass unconditionally for every \
              migrated test — silently inert for exactly the sweep it backstops. This \
              exception dies with the function it belongs to.",
    }];

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

    /// With no overlay installed the shim is `std::env`, exactly.
    ///
    /// This is the transitional behaviour every not-yet-migrated test still
    /// depends on. It is also the contract the overlay has to break on purpose:
    /// when `TestEnv` lands, this equality holds only while no overlay is
    /// installed on the calling thread.
    ///
    /// `PATH` is chosen because no test in the suite writes it, so the two
    /// reads cannot be separated by a concurrent `set_var`. A key that is
    /// absent covers the other branch.
    #[test]
    fn without_an_overlay_the_shim_is_std_env() {
        assert_eq!(super::var("PATH"), std::env::var("PATH"));
        assert_eq!(super::var_os("PATH"), std::env::var_os("PATH"));

        const ABSENT: &str = "DROVR_ENV_SHIM_KEY_THAT_IS_NEVER_SET";
        assert_eq!(super::var(ABSENT), Err(std::env::VarError::NotPresent));
        assert_eq!(super::var_os(ABSENT), None);
    }
}
