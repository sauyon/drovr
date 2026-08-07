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
    use std::path::Path;

    /// A raw `std::env::var*` read that is allowed to stay raw, and the reason.
    ///
    /// `file` is a filename under `cli/src/`; `line` is the source line with
    /// its leading indentation stripped, matched exactly so that editing the
    /// site re-opens the question. `why` is here to be read by whoever is
    /// tempted to add the next entry.
    struct RawException {
        file: &'static str,
        line: &'static str,
        why: &'static str,
    }

    /// The complete list. Adding to it is a design decision, not a fix for a
    /// failing test — a raw read is invisible to the overlay and therefore
    /// still races.
    const RAW_EXCEPTIONS: &[RawException] = &[RawException {
        file: "run.rs",
        line: r#"let home = match std::env::var("HOME") {"#,
        why: "refuse_home_data_root must see the REAL $HOME. The cfg(test) shim answers \
              only from the overlay, which never seeds HOME, so a shimmed read would \
              return NotPresent and the guard would pass unconditionally for every \
              migrated test — silently inert for exactly the sweep it backstops. This \
              exception dies with the function it belongs to.",
    }];

    /// `env.rs` is the door itself; its own `std::env` calls are the point.
    const THE_DOOR: &str = "env.rs";

    fn src_dir() -> &'static Path {
        Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/src"))
    }

    /// Source lines that are commented out are prose, not reads.
    ///
    /// This is a line-comment check only — a `std::env::var` buried in a block
    /// comment would be reported. Nothing in the crate does that, and a false
    /// positive here is a visible test failure rather than a silent hole.
    fn is_comment(line: &str) -> bool {
        line.trim_start().starts_with("//")
    }

    /// Every environment read in `src/` goes through this module.
    ///
    /// The chokepoint is the whole point of the module: a read that bypasses it
    /// cannot be redirected by a test's overlay, so it keeps resolving whatever
    /// the process-global environment happens to say at that instant. Grepping
    /// for that by hand once, at the moment the sweep lands, protects nothing
    /// from the tasks that come after it.
    #[test]
    fn crate_reads_the_environment_only_through_the_shim() {
        let mut offenders: Vec<String> = Vec::new();
        let mut matched = vec![false; RAW_EXCEPTIONS.len()];

        let mut files: Vec<_> = std::fs::read_dir(src_dir())
            .expect("cli/src must be readable")
            .map(|e| e.expect("cli/src entry must be readable").path())
            .filter(|p| p.extension().is_some_and(|x| x == "rs"))
            .collect();
        files.sort();
        assert!(!files.is_empty(), "found no .rs files in {:?}", src_dir());

        for path in &files {
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .expect("source file names are UTF-8");
            if name == THE_DOOR {
                continue;
            }
            let text = std::fs::read_to_string(path)
                .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
            for (i, line) in text.lines().enumerate() {
                if !line.contains("std::env::var") || is_comment(line) {
                    continue;
                }
                let trimmed = line.trim();
                match RAW_EXCEPTIONS
                    .iter()
                    .position(|x| x.file == name && x.line == trimmed)
                {
                    Some(idx) => matched[idx] = true,
                    None => offenders.push(format!("{}:{}: {trimmed}", name, i + 1)),
                }
            }
        }

        assert!(
            offenders.is_empty(),
            "{} environment read(s) bypass crate::env. A raw std::env::var* cannot be \
             redirected by a test's overlay, so it still resolves the process-global \
             environment and still races. Route each through crate::env::var / \
             crate::env::var_os, or — if it genuinely must stay raw — add it to \
             RAW_EXCEPTIONS in cli/src/env.rs with the reason:\n  {}",
            offenders.len(),
            offenders.join("\n  "),
        );

        for (i, ok) in matched.iter().enumerate() {
            assert!(
                ok,
                "RAW_EXCEPTIONS[{i}] no longer matches any source line ({}: {}). The site \
                 moved or went away; delete the exception rather than leaving a standing \
                 permission nobody is using. It was granted because: {}",
                RAW_EXCEPTIONS[i].file, RAW_EXCEPTIONS[i].line, RAW_EXCEPTIONS[i].why,
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
