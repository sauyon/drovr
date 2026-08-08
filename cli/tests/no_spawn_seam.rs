//! `DROVR_NO_SPAWN` is the human's escape hatch on `ensure_server`: with it set,
//! a `drovr review` command whose server is down says so instead of forking a
//! `drovr serve` daemon.
//!
//! **This has to live here, not in the unit suite.** Under `cfg(test)`
//! `ensure_server` now refuses to fork unconditionally (`NO_DAEMON_UNDER_TEST`),
//! and that wall sits ABOVE the `DROVR_NO_SPAWN` check — so no unit test can
//! reach the seam any more. `review::tests::wait_missing_server_errors` used to
//! be its only coverage, and after the wall landed it was covering the wall
//! instead. This binary drives the real, non-`cfg(test)` build, which is the
//! only configuration where the seam is live at all — and therefore the only
//! place it can be pinned.
//!
//! If this test ever fails by actually forking, it leaves a `drovr serve` bound
//! to a port and pointed at the scratch root below. That is contained (never the
//! live `~/.local/share/drovr`), but it is a process, so a failure here is worth
//! looking at rather than retrying.

use std::process::Command;

#[test]
fn no_spawn_reports_a_missing_server_instead_of_forking_a_daemon() {
    let xdg = tempfile::tempdir().expect("scratch XDG_DATA_HOME");
    let out = Command::new(env!("CARGO_BIN_EXE_drovr"))
        .args(["review", "wait", "no-such-run", "--timeout-ms", "1000"])
        .env("XDG_DATA_HOME", xdg.path())
        .env("DROVR_NO_SPAWN", "1")
        .output()
        .expect("run drovr review wait");

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(1),
        "a down server is exit 1; stderr: {stderr}"
    );
    assert!(
        stderr.contains("drovr review server is not running"),
        "stderr: {stderr}"
    );
    // And it really did not start one. A daemon writes `server.addr` into the
    // data root on its way up, so its absence is the evidence that the early
    // return was taken rather than merely that the command failed.
    assert!(
        !xdg.path().join("drovr/server.addr").exists(),
        "DROVR_NO_SPAWN must not leave a daemon behind"
    );
}
