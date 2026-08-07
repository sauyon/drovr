//! The repo-root `.cargo/config.toml` is a safety interlock: it forces `XDG_DATA_HOME` at a
//! scratch path so no cargo-launched process can reach the real `~/.local/share/drovr`.
//!
//! Nothing else in the suite fails if that file is deleted, moved, or shadowed by a
//! `cli/.cargo/config.toml` (cargo resolves nearest-to-cwd first) — the suite would simply go
//! back to running against whatever `XDG_DATA_HOME` the developer exports, which is how the
//! real data root got destroyed twice. This test is that missing signal.

use std::path::Path;

#[test]
fn cargo_forces_the_data_root_at_the_repo_scratch_path() {
    let raw = std::env::var_os("XDG_DATA_HOME").expect(
        "XDG_DATA_HOME is unset: the repo-root .cargo/config.toml did not apply. Cargo \
         discovers it by walking up from the current directory — run the suite from inside \
         the repo, and check the file still exists and is not shadowed by cli/.cargo/.",
    );
    let value = Path::new(&raw);

    // Matched by suffix, not by equality: `relative = true` anchors the value at the parent
    // of `.cargo`, which is the repo root under a different absolute path in the nix sandbox
    // (/build/source) than it is locally, and either may be reached through a symlink.
    assert!(
        value.ends_with("target/cargo-xdg-data"),
        "XDG_DATA_HOME is {value:?}, which is not the forced scratch root. Either the \
         repo-root .cargo/config.toml is missing/shadowed, or something overrode it — in \
         both cases this test run may be pointed at real drovr data.",
    );
}
