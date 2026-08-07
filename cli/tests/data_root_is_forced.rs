//! The repo-root `.cargo/config.toml` is a safety interlock: it forces `XDG_DATA_HOME` at a
//! scratch path so no cargo-launched process can reach the real `~/.local/share/drovr`.
//!
//! Nothing else in the suite fails if that file is deleted, moved, or shadowed by a
//! `cli/.cargo/config.toml` (cargo resolves nearest-to-cwd first) — the suite would simply go
//! back to running against whatever `XDG_DATA_HOME` the developer exports, which is how the
//! real data root got destroyed twice. This test is that missing signal.
//!
//! The expected path is read out of that file rather than restated here, so the two cannot
//! drift apart: this asserts what the config DECLARES is what the test process actually GOT.

use std::path::{Path, PathBuf};

/// `<repo>/.cargo/config.toml`, and the repo root it is anchored at.
fn interlock() -> (PathBuf, PathBuf) {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("cli/ has a parent")
        .to_path_buf();
    (repo_root.join(".cargo/config.toml"), repo_root)
}

#[test]
fn cargo_forces_the_data_root_at_the_repo_scratch_path() {
    let (config_path, repo_root) = interlock();
    let raw_config = std::fs::read_to_string(&config_path).unwrap_or_else(|e| {
        panic!(
            "the test-isolation interlock {config_path:?} could not be read: {e}. Without it \
             every cargo-launched process in this repo inherits your real XDG_DATA_HOME."
        )
    });
    let parsed: toml::Value = raw_config
        .parse()
        .unwrap_or_else(|e| panic!("{config_path:?} is not valid TOML: {e}"));

    let forced = parsed
        .get("env")
        .and_then(|env| env.get("XDG_DATA_HOME"))
        .unwrap_or_else(|| panic!("{config_path:?} no longer sets [env].XDG_DATA_HOME"));

    // Both flags are load-bearing, so both are asserted: without `force` an exported
    // XDG_DATA_HOME wins over the config, and without `relative` the value is taken
    // literally rather than being anchored at the repo.
    for flag in ["relative", "force"] {
        assert_eq!(
            forced.get(flag).and_then(toml::Value::as_bool),
            Some(true),
            "{config_path:?} must keep XDG_DATA_HOME's `{flag} = true`",
        );
    }
    let declared = forced
        .get("value")
        .and_then(toml::Value::as_str)
        .unwrap_or_else(|| panic!("{config_path:?}'s XDG_DATA_HOME has no string `value`"));

    let raw_actual = std::env::var_os("XDG_DATA_HOME").expect(
        "XDG_DATA_HOME is unset even though the config forces it. Cargo discovers that file \
         by walking up from the current directory — run the suite from inside the repo, and \
         check it is not shadowed by a cli/.cargo/config.toml.",
    );
    let actual = Path::new(&raw_actual);
    let complaint = format!(
        "XDG_DATA_HOME is {actual:?}, but {config_path:?} forces {declared:?} relative to \
         the repo. Something overrode the interlock, so this test run may be pointed at real \
         drovr data.",
    );

    // The declared components are matched literally off the tail and the remaining anchor is
    // compared canonically. `relative = true` anchors the value at the parent of `.cargo`,
    // which is /build/source in the nix sandbox and a worktree path locally, either possibly
    // reached through a symlink — but only the anchor can be canonicalized, since the scratch
    // dir itself is created lazily by the first process that writes to it.
    let mut anchor = actual;
    for component in Path::new(declared).iter().collect::<Vec<_>>().iter().rev() {
        assert_eq!(anchor.file_name(), Some(*component), "{complaint}");
        anchor = anchor.parent().expect("forced path outran its components");
    }

    // Resolved separately rather than compared as `canonicalize().ok()` pairs: two failing
    // lookups both yield None and would compare EQUAL, passing the test on an anchor that
    // does not exist. A path this test cannot resolve is a failure, not a match.
    let repo_root = repo_root
        .canonicalize()
        .expect("this repo's root must resolve");
    let resolved = anchor.canonicalize().unwrap_or_else(|e| {
        panic!("{complaint} Its anchor {anchor:?} could not be resolved: {e}")
    });
    assert_eq!(
        resolved, repo_root,
        "{complaint} It is anchored at {resolved:?}, not at this repo.",
    );
}
