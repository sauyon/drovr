//! Gated end-to-end smoke test for the `drovr` CLI.
//!
//! Prerequisites checked at test start:
//!   1. `herdr` is on PATH.
//!   2. `claude` is on PATH.
//!   3. The herdr claude integration hook is installed
//!      (`herdr integration status` prints a `claude:` line NOT containing
//!      "not installed").
//!
//! If any prerequisite is absent the test prints a clear message and returns
//! (skipped-but-passing). All assertions that require a live herdr agent
//! session (phase start / wait / compress) are additionally gated behind the
//! integration check and skipped cleanly here since spawning a real claude
//! session is not appropriate in CI.
//!
//! Runnable assertions (no integration agent required):
//!   • `drovr new`  → state.json exists with 4 seeded phases.
//!   • `drovr serve` in background → `/state` returns `idle`.
//!   • `POST /submit` (request-changes) → `/state` becomes `waiting`.
//!   • `drovr review summary` → `/state` becomes `ready`.
//!   • `drovr cleanup --purge` → run dir removed.

use std::fs;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::thread;
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// Prerequisite helpers
// ---------------------------------------------------------------------------

fn binary_on_path(name: &str) -> bool {
    Command::new("which")
        .arg(name)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn herdr_integration_installed() -> bool {
    let Ok(out) = Command::new("herdr")
        .args(["integration", "status"])
        .output()
    else {
        return false;
    };
    let stdout = String::from_utf8_lossy(&out.stdout);
    stdout
        .lines()
        .any(|l| l.starts_with("claude:") && !l.contains("not installed"))
}

// ---------------------------------------------------------------------------
// Network helpers (raw TCP, no extra deps)
// ---------------------------------------------------------------------------

fn http_get(addr: &str, path: &str) -> (u16, String) {
    let mut stream = TcpStream::connect(addr).expect("connect");
    write!(stream, "GET {path} HTTP/1.0\r\nHost: {addr}\r\n\r\n").unwrap();
    let mut resp = String::new();
    stream.read_to_string(&mut resp).unwrap();
    let status: u16 = resp
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let body = resp.splitn(2, "\r\n\r\n").nth(1).unwrap_or("").to_string();
    (status, body)
}

fn http_post(addr: &str, path: &str, content_type: &str, body: &str) -> (u16, String) {
    let mut stream = TcpStream::connect(addr).expect("connect");
    write!(
        stream,
        "POST {path} HTTP/1.0\r\nHost: {addr}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
    let mut resp = String::new();
    stream.read_to_string(&mut resp).unwrap();
    let status: u16 = resp
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let rb = resp.splitn(2, "\r\n\r\n").nth(1).unwrap_or("").to_string();
    (status, rb)
}

/// Poll `GET /state` until `field "state"` equals `expected` or timeout.
fn poll_state(addr: &str, expected: &str, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Ok(mut stream) = TcpStream::connect(addr) {
            let req = format!("GET /state HTTP/1.0\r\nHost: {addr}\r\n\r\n");
            let _ = stream.write_all(req.as_bytes());
            let mut resp = String::new();
            let _ = stream.read_to_string(&mut resp);
            let body = resp.splitn(2, "\r\n\r\n").nth(1).unwrap_or("");
            if body.contains(&format!(r#""state":"{expected}""#)) {
                return true;
            }
        }
        thread::sleep(Duration::from_millis(50));
    }
    false
}

/// Pick a free TCP port by binding to :0 and reading back the OS-assigned port.
fn free_port() -> u16 {
    use std::net::TcpListener;
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind for free port");
    listener.local_addr().unwrap().port()
}

// ---------------------------------------------------------------------------
// Locate the drovr binary
// ---------------------------------------------------------------------------

fn drovr_binary() -> PathBuf {
    // Prefer the binary produced by `cargo test` (same profile).
    // CARGO_TARGET_DIR may be set; fall back to conventional location.
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let bin = manifest.join("target/debug/drovr");
    if bin.exists() {
        return bin;
    }
    // Workspace target dir (one level up from cli/)
    let ws_bin = manifest
        .parent()
        .unwrap_or(&manifest)
        .join("target/debug/drovr");
    if ws_bin.exists() {
        return ws_bin;
    }
    // Last resort: look for it relative to OUT_DIR (set during `cargo test`)
    bin // will fail at runtime with a clear error if missing
}

// ---------------------------------------------------------------------------
// RAII guard: kill a child process when dropped
// ---------------------------------------------------------------------------

struct KillOnDrop(Child);
impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

// ---------------------------------------------------------------------------
// The test
// ---------------------------------------------------------------------------

#[test]
fn e2e_smoke() {
    // ---- Prerequisite checks -----------------------------------------------

    if !binary_on_path("herdr") {
        println!("skipping e2e: `herdr` not found on PATH");
        return;
    }
    if !binary_on_path("claude") {
        println!("skipping e2e: `claude` not found on PATH");
        return;
    }
    if !herdr_integration_installed() {
        println!(
            "skipping e2e: herdr claude integration not installed (run `herdr integration install claude`)"
        );
        return;
    }

    // ---- Setup: isolated XDG_DATA_HOME -------------------------------------

    let tmp = tempfile::Builder::new()
        .prefix("drovr-e2e-")
        .tempdir()
        .expect("tempdir");
    let xdg = tmp.path().to_path_buf();

    let run_name = "e2e-smoke";
    let run_dir: PathBuf = xdg.join("drovr/runs").join(run_name);

    let drovr = drovr_binary();
    assert!(
        drovr.exists(),
        "drovr binary not found at {:?}; run `cargo build` first",
        drovr
    );

    // Helper: run drovr with our XDG_DATA_HOME set
    let base_cmd = || {
        let mut c = Command::new(&drovr);
        c.env("XDG_DATA_HOME", &xdg);
        c.env("DROVR_AGENT", "claude");
        c
    };

    // ---- Step 1: drovr new -------------------------------------------------

    let out = base_cmd()
        .args(["new", run_name, "--task", "demo"])
        .output()
        .expect("drovr new");
    assert!(
        out.status.success(),
        "drovr new failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let state_path = run_dir.join("state.json");
    assert!(
        state_path.exists(),
        "state.json not created at {:?}",
        state_path
    );

    let state_json = fs::read_to_string(&state_path).expect("read state.json");
    let state: serde_json::Value = serde_json::from_str(&state_json).expect("parse state.json");
    assert_eq!(state["agent"], "claude");
    let phases = state["phases"].as_array().expect("phases array");
    assert_eq!(
        phases.len(),
        4,
        "expected 4 seeded phases, got {}",
        phases.len()
    );
    let phase_names: Vec<&str> = phases.iter().filter_map(|p| p["name"].as_str()).collect();
    assert_eq!(
        phase_names,
        ["brainstorm", "plan", "implement", "review"],
        "unexpected phase names: {:?}",
        phase_names
    );
    println!(
        "e2e: drovr new OK — state.json exists with {} phases",
        phases.len()
    );

    // ---- Step 2: drovr serve + review cycle --------------------------------

    let port = free_port();
    let addr = format!("127.0.0.1:{port}");

    // Start `drovr serve` in a background child process.
    let serve_child = base_cmd()
        .args([
            "serve",
            run_name,
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("drovr serve");
    let _guard = KillOnDrop(serve_child);

    // Poll until the server is reachable and reports `idle`.
    assert!(
        poll_state(&addr, "idle", Duration::from_secs(5)),
        "timed out waiting for serve to reach idle state at {addr}"
    );
    println!("e2e: drovr serve started — state=idle");

    // POST /submit (request-changes + feedback) → state should become `waiting`
    let submit_payload = r#"{"decision":"request-changes","feedback":"please revise","answers":{},"annotations":[]}"#;
    let (status, body) = http_post(&addr, "/submit", "application/json", submit_payload);
    assert_eq!(status, 200, "POST /submit failed: {body}");
    assert!(
        body.contains(r#""state":"waiting""#),
        "expected waiting after submit, got: {body}"
    );
    println!("e2e: POST /submit OK — state=waiting");

    // Verify GET /state agrees
    let (_, state_body) = http_get(&addr, "/state");
    assert!(
        state_body.contains(r#""state":"waiting""#),
        "GET /state should be waiting: {state_body}"
    );

    // `drovr review summary` → state should become `ready`
    let out = base_cmd()
        .args([
            "review",
            "summary",
            run_name,
            "agent completed the requested changes",
        ])
        .output()
        .expect("drovr review summary");
    assert!(
        out.status.success(),
        "drovr review summary failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let (_, state_body) = http_get(&addr, "/state");
    assert!(
        state_body.contains(r#""state":"ready""#),
        "GET /state should be ready after summary: {state_body}"
    );
    println!("e2e: drovr review summary OK — state=ready");

    // ---- Step 3: drovr cleanup --purge ------------------------------------

    // Drop the serve guard first to free the port cleanly.
    drop(_guard);

    let out = base_cmd()
        .args(["cleanup", run_name, "--purge"])
        .output()
        .expect("drovr cleanup");
    assert!(
        out.status.success(),
        "drovr cleanup failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        !run_dir.exists(),
        "run dir still exists after --purge: {:?}",
        run_dir
    );
    println!("e2e: drovr cleanup --purge OK — run dir removed");

    println!("e2e: all assertions passed");
}
