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
    let complaint = format!(
        "XDG_DATA_HOME is {value:?}, which is not this repo's forced scratch root. Either \
         the repo-root .cargo/config.toml is missing/shadowed, or something overrode it — \
         in both cases this test run may be pointed at real drovr data.",
    );

    // Checked against THIS repo's path, not just the "target/cargo-xdg-data" suffix: an
    // exported XDG_DATA_HOME that happens to end that way would otherwise satisfy a suffix
    // match with the config absent, which is precisely the state this test exists to catch.
    //
    // `relative = true` anchors the value at the parent of `.cargo` — the repo root, which
    // is /build/source in the nix sandbox and a worktree path locally, and either may be
    // reached through a symlink. So the two leaf components are matched literally and the
    // root is compared canonically. Only the root is canonicalized: it always exists,
    // whereas target/cargo-xdg-data is created lazily by the first process that writes.
    assert_eq!(value.file_name(), Some("cargo-xdg-data".as_ref()), "{complaint}");
    let target = value.parent().expect("scratch root has a parent");
    assert_eq!(target.file_name(), Some("target".as_ref()), "{complaint}");

    let anchor = target.parent().expect("target/ has a parent");
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("cli/ has a parent");
    assert_eq!(
        anchor.canonicalize().ok(),
        repo_root.canonicalize().ok(),
        "{complaint} It is anchored at {anchor:?}, not at this repo ({repo_root:?}).",
    );
}
