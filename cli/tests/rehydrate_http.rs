//! `POST /api/runs/<run>/rehydrate` end to end, against the REAL binary.
//!
//! The handler deliberately shells out to `std::env::current_exe()` so the CLI
//! stays the sole writer of `state.json` — which is exactly what an in-process
//! unit test cannot exercise: under `cargo test` the current exe is the TEST
//! binary, so a unit test that reached the spawn would re-enter the suite. The
//! in-process test (`review::tests::post_rehydrate_refuses_before_it_can_shell_out`)
//! therefore covers only the refusals that short-circuit BEFORE the spawn.
//!
//! This covers the other side: the request really reaches `drovr phase
//! rehydrate`, and the CLI's own diagnosis comes back to the caller. It runs
//! `drovr serve` as a subprocess, so `current_exe()` is `drovr`.
//!
//! The 200 path is NOT reachable here: a successful rehydrate needs a live
//! herdr workspace to create a tab in. What is proved is the wiring, the JSON
//! shape, and that a failure is reported rather than swallowed.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use std::{fs, thread};

struct KillOnDrop(Child);
impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn free_port() -> u16 {
    use std::net::TcpListener;
    TcpListener::bind("127.0.0.1:0")
        .expect("bind for free port")
        .local_addr()
        .unwrap()
        .port()
}

fn wait_for_listener(addr: &str, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if TcpStream::connect(addr).is_ok() {
            return true;
        }
        thread::sleep(Duration::from_millis(50));
    }
    false
}

fn http_post(addr: &str, path: &str) -> (u16, String) {
    let mut s = TcpStream::connect(addr).expect("connect");
    write!(
        s,
        "POST {path} HTTP/1.0\r\nHost: {addr}\r\nContent-Length: 0\r\n\r\n"
    )
    .expect("write request");
    let mut resp = String::new();
    let _ = s.read_to_string(&mut resp);
    let status = resp
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    let body = resp
        .split_once("\r\n\r\n")
        .map(|(_, b)| b)
        .unwrap_or("")
        .to_string();
    (status, body)
}

#[test]
fn rehydrate_over_http_really_runs_the_cli_and_reports_its_failure() {
    let tmp = tempfile::Builder::new()
        .prefix("drovr-rehydrate-http-")
        .tempdir()
        .expect("tempdir");
    let xdg = tmp.path().join("xdg");
    let run_dir = xdg.join("drovr").join("runs").join("rh");
    fs::create_dir_all(&run_dir).expect("run dir");

    // A phase that HAS run and holds no pane — i.e. one every pre-spawn refusal
    // lets through. `project_dir` is empty, so the CLI itself refuses with a
    // message no HTTP-layer check produces; seeing that exact text come back is
    // what proves the request reached the CLI rather than being answered here.
    fs::write(
        run_dir.join("state.json"),
        r#"{"name":"rh","task":"t","gate":"spec","cursor":0,"project_dir":"","phases":[
{"name":"brainstorm","status":"Done","handoff_doc":null,"herdr_session":null,"pane_id":null,
 "reaped":true,"pane_agent":{"backend":"claude","session":"sess-http"}}]}"#,
    )
    .expect("state.json");

    let port = free_port();
    let addr = format!("127.0.0.1:{port}");
    // --host is explicit: `serve` otherwise takes serve_host from the
    // developer's own config, which may be an address this test cannot reach.
    let _server = KillOnDrop(
        Command::new(PathBuf::from(env!("CARGO_BIN_EXE_drovr")))
            .args(["serve", "--host", "127.0.0.1", "--port", &port.to_string()])
            .env("XDG_DATA_HOME", &xdg)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn drovr serve"),
    );
    assert!(
        wait_for_listener(&addr, Duration::from_secs(15)),
        "drovr serve never bound {addr}"
    );

    let (status, body) = http_post(&addr, "/api/runs/rh/rehydrate?phase=brainstorm");
    assert_eq!(status, 500, "body: {body}");
    let json: serde_json::Value = serde_json::from_str(&body)
        .unwrap_or_else(|e| panic!("a 500 must carry JSON, got {body:?} ({e})"));
    assert_eq!(json["ok"], false, "{body}");
    let err = json["error"].as_str().unwrap_or("");
    // The CLI's own words, not the handler's: only `phase_rehydrate` knows the
    // run has no project_dir. If the handler ever answered on its own, this
    // string would change.
    assert!(
        err.contains("project_dir"),
        "the CLI's diagnosis must reach the caller: {body}"
    );
    assert!(err.contains("phase rehydrate failed"), "{body}");

    // A refusal that short-circuits before the spawn still answers, over the
    // same real server — so the two paths are wired to one endpoint.
    let (status, _) = http_post(&addr, "/api/runs/rh/rehydrate?phase=nope");
    assert_eq!(status, 404);

    // …and nothing the request did wrote state.json: the CLI refused before
    // mutating, and the handler never writes at all.
    let after = fs::read_to_string(run_dir.join("state.json")).expect("state.json still there");
    assert!(after.contains("\"reaped\":true"), "{after}");
}

/// A rehydrate that cannot finish must not exit 0, end to end through the real
/// binary.
///
/// **Scope, stated honestly:** this drives the *refusal* path (no herdr to make
/// a tab in), not the `Relaunched` one — reaching that needs a live herdr and a
/// real agent that fails to become ready, which no test here can arrange. The
/// outcome→exit mapping itself is pinned by
/// `main::tests::an_incomplete_rehydrate_never_reports_as_success`, which is
/// where a mutation of the exit code dies. What this adds is that the mapping is
/// actually reached: the real CLI, spawned as a subprocess, does exit non-zero
/// and leaves the phase alone.
#[test]
fn a_rehydrate_that_cannot_finish_does_not_exit_zero() {
    let tmp = tempfile::Builder::new()
        .prefix("drovr-rehydrate-exit-")
        .tempdir()
        .expect("tempdir");
    let xdg = tmp.path().join("xdg");
    let run_dir = xdg.join("drovr").join("runs").join("rh");
    fs::create_dir_all(&run_dir).expect("run dir");
    // A reaped phase with a workspace but NO herdr running: `tab_create` fails,
    // so this exercises the refusal/exit plumbing rather than a real launch.
    // What matters here is only that a NON-completing outcome never exits 0.
    fs::write(
        run_dir.join("state.json"),
        r#"{"name":"rh","task":"t","gate":"spec","cursor":0,"project_dir":"/tmp",
"phases":[{"name":"brainstorm","status":"Done","handoff_doc":null,"herdr_session":null,
"pane_id":null,"reaped":true,"pane_agent":{"backend":"claude"}}]}"#,
    )
    .expect("state.json");

    let out = Command::new(PathBuf::from(env!("CARGO_BIN_EXE_drovr")))
        .args(["phase", "rehydrate", "rh", "brainstorm"])
        .env("XDG_DATA_HOME", &xdg)
        .env("HERDR_SOCKET_PATH", xdg.join("no-such-herdr.sock"))
        .output()
        .expect("run drovr phase rehydrate");

    assert_ne!(
        out.status.code(),
        Some(0),
        "a rehydrate that did not finish the job must never look like success. \
         stdout={:?} stderr={:?}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    // And the phase is still reaped, so a retry starts from the same place.
    // Parsed, not substring-matched: the CLI rewrites `state.json` pretty-printed
    // (it persists the new pass before it touches herdr), so the compact spelling
    // this fixture was written with is not what comes back.
    let after: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(run_dir.join("state.json")).expect("state.json"))
            .expect("state.json stays valid JSON");
    assert_eq!(after["phases"][0]["reaped"], true, "{after}");
    assert!(after["phases"][0]["pane_id"].is_null(), "{after}");
}
