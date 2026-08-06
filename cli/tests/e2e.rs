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
//!   • always-on `drovr serve` in background → `/api/runs/<run>/state` = `idle`.
//!   • `POST /api/runs/<run>/submit` (request-changes) → state = `waiting`.
//!   • `drovr review summary` (finds server via global addr) → state = `ready`.
//!   • `drovr cleanup --purge` → run dir removed.

use std::fs;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
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

/// Poll `GET /api/runs/<run>/state` until `field "state"` equals `expected`.
fn poll_state(addr: &str, run: &str, expected: &str, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Ok(mut stream) = TcpStream::connect(addr) {
            let req = format!("GET /api/runs/{run}/state HTTP/1.0\r\nHost: {addr}\r\n\r\n");
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
    // The binary cargo built for this test run, whatever the profile (debug,
    // release, or a nix sandbox's release). `CARGO_BIN_EXE_<name>` is set by
    // cargo for integration tests and always points at the real artifact —
    // unlike a hardcoded `target/debug/drovr`, which is absent under `--release`.
    PathBuf::from(env!("CARGO_BIN_EXE_drovr"))
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

    // Point drovr at a throwaway (never-created) herdr socket so `drovr new` /
    // `drovr cleanup` exercise the graceful no-herdr path instead of creating and
    // tearing down REAL workspaces on the developer's *live* herdr daemon. Left
    // un-isolated, each test run churned the user's UI — `workspace.create` → root
    // shell pane spawns → `workspace.close` → pane exits code 1 → `PaneDied` — which
    // is exactly the "reader exits 1 in a throwaway workspace" churn we were chasing.
    let iso_sock = xdg.join("isolated-herdr.sock");
    // Helper: run drovr with our XDG_DATA_HOME set
    let base_cmd = || {
        let mut c = Command::new(&drovr);
        c.env("XDG_DATA_HOME", &xdg);
        c.env("DROVR_AGENT", "claude");
        c.env("HERDR_SOCKET_PATH", &iso_sock);
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
    // Isolation guard: with the throwaway socket, `drovr new` degrades gracefully and
    // records NO workspace (the field is `skip_serializing_if = None`, so it is absent
    // when workspace_create failed). If this ever contains an id, the test just created
    // a real workspace on a live herdr and churned it — the bug this test now guards.
    assert!(
        state.get("workspace").is_none(),
        "e2e must not create a real herdr workspace (would churn the dev's live herdr); got {:?}",
        state.get("workspace")
    );
    println!(
        "e2e: drovr new OK — state.json exists with {} phases, no live workspace",
        phases.len()
    );

    // ---- Step 2: always-on serve + review cycle ----------------------------

    let port = free_port();
    let addr = format!("127.0.0.1:{port}");

    // Start the always-on `drovr serve` (no run arg — it serves every run) in a
    // background child process on a test port.
    let serve_child = base_cmd()
        .args(["serve", "--host", "127.0.0.1", "--port", &port.to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("drovr serve");
    let _guard = KillOnDrop(serve_child);

    // Poll until the server is reachable and reports `idle` for our run.
    assert!(
        poll_state(&addr, run_name, "idle", Duration::from_secs(5)),
        "timed out waiting for serve to reach idle state at {addr}"
    );
    println!("e2e: drovr serve started — state=idle");

    // POST submit (request-changes + feedback) → state should become `waiting`
    let submit_payload = r#"{"decision":"request-changes","feedback":"please revise","answers":{},"annotations":[]}"#;
    let submit_path = format!("/api/runs/{run_name}/submit");
    let (status, body) = http_post(&addr, &submit_path, "application/json", submit_payload);
    assert_eq!(status, 200, "POST submit failed: {body}");
    assert!(
        body.contains(r#""state":"waiting""#),
        "expected waiting after submit, got: {body}"
    );
    println!("e2e: POST submit OK — state=waiting");

    // Verify GET state agrees
    let state_path = format!("/api/runs/{run_name}/state");
    let (_, state_body) = http_get(&addr, &state_path);
    assert!(
        state_body.contains(r#""state":"waiting""#),
        "GET state should be waiting: {state_body}"
    );

    // `drovr review summary` finds the always-on server via the global
    // `server.addr` (no per-run address file) → state should become `ready`.
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

    let (_, state_body) = http_get(&addr, &state_path);
    assert!(
        state_body.contains(r#""state":"ready""#),
        "GET state should be ready after summary: {state_body}"
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

/// End-to-end worktree isolation lifecycle through the real CLI:
/// `drovr new --worktree` in a git repo → worktree + branch created and recorded;
/// non-purge `cleanup` commits the run's work to the branch and prunes the
/// worktree while keeping the branch; `--purge` then deletes the branch.
///
/// Gated identically to `e2e_smoke` (herdr + claude + integration), since
/// `drovr new` creates a herdr workspace.
#[test]
fn e2e_worktree_lifecycle() {
    if !binary_on_path("herdr") {
        println!("skipping e2e worktree: `herdr` not found on PATH");
        return;
    }
    if !binary_on_path("claude") {
        println!("skipping e2e worktree: `claude` not found on PATH");
        return;
    }
    if !herdr_integration_installed() {
        println!("skipping e2e worktree: herdr claude integration not installed");
        return;
    }

    let tmp = tempfile::Builder::new()
        .prefix("drovr-e2e-wt-")
        .tempdir()
        .expect("tempdir");
    let xdg = tmp.path().join("xdg");
    let repo = tmp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();

    // Seed a git repo with one commit.
    let git = |args: &[&str]| {
        let out = Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(args)
            .output()
            .expect("git");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    };
    git(&["init", "-q"]);
    git(&["config", "user.email", "t@t.t"]);
    git(&["config", "user.name", "t"]);
    fs::write(repo.join("f.txt"), "hi").unwrap();
    git(&["add", "."]);
    git(&["commit", "-q", "-m", "init"]);

    let drovr = drovr_binary();
    // Throwaway herdr socket — see the note in `e2e_smoke`. Keeps `drovr new
    // --worktree` from creating/closing real workspaces on the developer's live herdr.
    let iso_sock = xdg.join("isolated-herdr.sock");
    let base_cmd = || {
        let mut c = Command::new(&drovr);
        c.env("XDG_DATA_HOME", &xdg);
        c.env("DROVR_AGENT", "claude");
        c.env("HERDR_SOCKET_PATH", &iso_sock);
        c
    };

    // Create a --worktree run and assert the worktree/branch/state are recorded.
    // Returns the worktree dir and the run dir. Independent run names keep the two
    // scenarios (keep-branch vs. --purge) from colliding on branch `drovr/<name>`.
    let make_run = |name: &str| -> (PathBuf, PathBuf) {
        let out = base_cmd()
            .args(["new", name, "--task", "isolate me", "--worktree"])
            .arg("--dir")
            .arg(&repo)
            .output()
            .expect("drovr new --worktree");
        assert!(
            out.status.success(),
            "drovr new --worktree failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let wt_dir = repo.join(".drovr/wt").join(name);
        let run_dir = xdg.join("drovr/runs").join(name);
        assert!(wt_dir.exists(), "worktree dir not created at {wt_dir:?}");
        assert!(
            wt_dir.join("f.txt").exists(),
            "worktree must check out HEAD"
        );
        let state: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(run_dir.join("state.json")).unwrap()).unwrap();
        assert_eq!(state["worktree_branch"], format!("drovr/{name}"));
        // project_dir was redirected into the worktree.
        assert_eq!(state["project_dir"], state["worktree_path"]);
        // Isolation guard: no real herdr workspace created (throwaway socket).
        assert!(
            state.get("workspace").is_none(),
            "e2e worktree must not create a real herdr workspace: {:?}",
            state.get("workspace")
        );
        let excl = fs::read_to_string(repo.join(".git/info/exclude")).unwrap();
        assert!(
            excl.lines().any(|l| l.trim() == ".drovr/wt/"),
            "exclude: {excl}"
        );
        // Simulate phase output landing in the worktree.
        fs::write(wt_dir.join("phase-output.txt"), "work from the run").unwrap();
        (wt_dir, run_dir)
    };

    // ---- Scenario A: non-purge cleanup commits work, prunes wt, keeps branch
    let (wt_a, run_a) = make_run("e2e-wt-keep");
    let out = base_cmd()
        .args(["cleanup", "e2e-wt-keep"])
        .output()
        .expect("cleanup");
    assert!(
        out.status.success(),
        "drovr cleanup failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!wt_a.exists(), "worktree must be pruned on cleanup");
    let branches = git(&["branch", "--list", "drovr/e2e-wt-keep"]);
    assert!(
        branches.contains("drovr/e2e-wt-keep"),
        "branch must survive cleanup"
    );
    let show = git(&["show", "--stat", "drovr/e2e-wt-keep"]);
    assert!(
        show.contains("phase-output.txt"),
        "branch tip must carry the committed run work: {show}"
    );
    assert!(run_a.exists(), "non-purge cleanup keeps the run dir");
    println!("e2e worktree: non-purge OK — work committed, worktree pruned, branch kept");

    // ---- Scenario B: --purge removes worktree, branch, and run dir ---------
    let (wt_b, run_b) = make_run("e2e-wt-purge");
    let out = base_cmd()
        .args(["cleanup", "e2e-wt-purge", "--purge"])
        .output()
        .expect("cleanup --purge");
    assert!(
        out.status.success(),
        "drovr cleanup --purge failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!wt_b.exists(), "--purge must remove the worktree");
    let branches = git(&["branch", "--list", "drovr/e2e-wt-purge"]);
    assert!(
        branches.is_empty(),
        "--purge must delete the branch: {branches}"
    );
    assert!(!run_b.exists(), "--purge removes the run dir");
    println!("e2e worktree: non-purge + purge scenarios OK");

    // ---- Scenario C: worktree manually deleted → --purge still completes ----
    // Regression for the review finding: a vanished worktree must not wedge the
    // run. --purge should tolerate the missing worktree and still remove the run
    // dir instead of aborting on a git error.
    let (wt_c, run_c) = make_run("e2e-wt-gone");
    std::fs::remove_dir_all(&wt_c).expect("simulate manual worktree deletion");
    let out = base_cmd()
        .args(["cleanup", "e2e-wt-gone", "--purge"])
        .output()
        .expect("cleanup --purge on vanished worktree");
    assert!(
        out.status.success(),
        "cleanup --purge must tolerate a vanished worktree: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        !run_c.exists(),
        "--purge removes the run dir even when worktree is gone"
    );
    println!("e2e worktree: all assertions passed");
}

// ---------------------------------------------------------------------------
// The cancel gate, end to end through the real binary
// ---------------------------------------------------------------------------

/// The signal the driver actually reads: `drovr review wait` must exit **5**
/// (not 0, and not 1) when the human cancels at the gate. Unlike `e2e_smoke`
/// this needs no herdr/claude — the run dir is built by hand — so it always
/// runs.
#[test]
fn e2e_cancel_gate_exits_5() {
    // ---- Prerequisite checks -----------------------------------------------
    // This test does not spawn a herdr agent, but it MUST share the same
    // herdr/claude-absent skip as the other e2e tests: that guard is the flake's
    // hermeticity contract (see flake.nix) — it is what stops the test from
    // binding a loopback server inside the Nix build sandbox, where it cannot
    // come up (the assertion below would fail the whole build). herdr+claude are
    // present locally, so the test still runs there.
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

    let tmp = tempfile::Builder::new()
        .prefix("drovr-e2e-cancel-")
        .tempdir()
        .expect("tempdir");
    let xdg = tmp.path().to_path_buf();
    let run_name = "e2e-cancel";
    let run_dir = xdg.join("drovr/runs").join(run_name);
    fs::create_dir_all(&run_dir).expect("run dir");
    fs::write(run_dir.join("spec.md"), b"# Spec").expect("spec");
    fs::write(
        run_dir.join("state.json"),
        format!(
            r#"{{"name":"{run_name}","task":"t","phases":[],"gate":"spec","cursor":0,"project_dir":""}}"#
        ),
    )
    .expect("state.json");

    let drovr = drovr_binary();
    assert!(drovr.exists(), "drovr binary missing; run `cargo build`");

    let port = free_port();
    let addr = format!("127.0.0.1:{port}");
    let serve_child = Command::new(&drovr)
        .env("XDG_DATA_HOME", &xdg)
        .args(["serve", "--host", "127.0.0.1", "--port", &port.to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("drovr serve");
    let _guard = KillOnDrop(serve_child);
    assert!(
        poll_state(&addr, run_name, "idle", Duration::from_secs(5)),
        "serve did not come up at {addr}"
    );

    // Driver blocks on the gate while the reviewer decides.
    let mut wait_child = Command::new(&drovr)
        .env("XDG_DATA_HOME", &xdg)
        .args(["review", "wait", run_name, "--timeout-ms", "20000"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("drovr review wait");
    thread::sleep(Duration::from_millis(300));
    assert!(
        wait_child.try_wait().expect("try_wait").is_none(),
        "review wait must still be blocking before any decision"
    );

    // The human hits Cancel in the browser.
    let (status, _) = http_post(
        &addr,
        &format!("/api/runs/{run_name}/submit"),
        "application/json",
        r#"{"decision":"cancel","feedback":"","answers":{},"annotations":[]}"#,
    );
    assert_eq!(status, 200, "cancel submit rejected");

    let out = wait_child.wait_with_output().expect("wait for review wait");
    assert_eq!(
        out.status.code(),
        Some(5),
        "review wait must exit 5 on cancel, got {:?}; stdout={}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.to_lowercase().contains("cancel"),
        "message must name the cancellation: {stdout}"
    );

    // The marker is on disk, so the outcome survives a server restart.
    assert!(
        run_dir.join("cancelled").exists(),
        "cancelled marker missing from the run dir"
    );
    println!("e2e cancel: review wait exited 5 and the marker is on disk");
}

// ---------------------------------------------------------------------------
// `drovr ask` / `drovr ask wait`, end to end through the real binary
// ---------------------------------------------------------------------------
//
// These need no herdr, no claude, and no server — so unlike the tests above they
// carry no skip guard and run everywhere, including the Nix build sandbox. That is
// possible because `ask` is a file append and `ask wait` is a file poller; the only
// network call in the pair is `ensure_server`, which `DROVR_NO_SPAWN` turns into a
// clean "server is down" instead of a spawned daemon (see `review::ensure_server`).
//
// What is under test is the EXIT-CODE CONTRACT: 0 answered, 2 timeout, 5 cancelled,
// 1 error. A timeout that reads as success is the exact silent failure recorded in
// `docs/known-issues.md`, *"Piping a `wait` command destroys its exit-code contract —
// a timeout reads as approval"*, so these assert the code, never the message alone.

/// An empty run dir under an isolated `XDG_DATA_HOME`. Returns the tempdir (kept
/// alive by the caller), the data home, and the run dir.
fn ask_fixture(tag: &str) -> (tempfile::TempDir, PathBuf, PathBuf) {
    let tmp = tempfile::Builder::new()
        .prefix(&format!("drovr-e2e-{tag}-"))
        .tempdir()
        .expect("tempdir");
    let xdg = tmp.path().to_path_buf();
    let run_dir = xdg.join("drovr/runs").join(tag);
    fs::create_dir_all(&run_dir).expect("run dir");
    (tmp, xdg, run_dir)
}

/// `drovr <args>` with the fixture's data home and no daemon spawning.
fn ask_cmd(xdg: &Path) -> Command {
    let mut c = Command::new(drovr_binary());
    c.env("XDG_DATA_HOME", xdg);
    c.env("DROVR_NO_SPAWN", "1");
    c
}

/// Post one question through the real CLI and assert the post itself did not block.
fn post_ask(xdg: &Path, run: &str, question: &str) {
    let started = Instant::now();
    let out = ask_cmd(xdg)
        .args(["ask", run, "--question", question])
        .output()
        .expect("drovr ask");
    let elapsed = started.elapsed();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        out.status.code(),
        Some(0),
        "ask must exit 0 even with the server down (the record is already on disk); \
         stdout={stdout} stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        stdout.contains("posted ask-"),
        "ask must name the id: {stdout}"
    );
    // The whole point of the design: `ask` returns immediately, with the question
    // still unanswered. If this ever starts waiting, a human who steps away turns
    // every ask into a dead call that takes its question with it.
    assert!(
        elapsed < Duration::from_secs(10),
        "ask must not wait for an answer; took {elapsed:?}"
    );
}

/// Append the answer record the server's `POST answer` will write in T4: exactly
/// `id`, `seq`, `answer`, `answered_at`, on a new line — the ask's own line is never
/// rewritten.
fn append_answer_line(run_dir: &Path, id: &str, seq: u64, answer: &str) {
    use std::fs::OpenOptions;
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(run_dir.join("interview.jsonl"))
        .expect("open interview.jsonl");
    writeln!(
        f,
        r#"{{"id":"{id}","seq":{seq},"answer":"{answer}","answered_at":"2026-08-06T12:00:00Z"}}"#
    )
    .expect("append answer");
}

#[test]
fn e2e_ask_wait_exits_0_when_answered() {
    let run = "e2e-ask-answered";
    let (_tmp, xdg, run_dir) = ask_fixture(run);
    post_ask(&xdg, run, "which arm ships?");

    let mut wait_child = ask_cmd(&xdg)
        .args(["ask", "wait", run, "--timeout-ms", "20000"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("drovr ask wait");
    thread::sleep(Duration::from_millis(300));
    assert!(
        wait_child.try_wait().expect("try_wait").is_none(),
        "ask wait must still be blocking while the question is unanswered"
    );

    append_answer_line(&run_dir, "ask-0", 0, "arm-b");

    let out = wait_child.wait_with_output().expect("wait for ask wait");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        out.status.code(),
        Some(0),
        "ask wait must exit 0 once answered; stdout={stdout}"
    );
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("stdout must be a JSON array: {e}; stdout={stdout}"));
    let arr = parsed.as_array().expect("JSON array");
    assert_eq!(arr.len(), 1, "one snapshotted ask: {stdout}");
    assert_eq!(arr[0]["id"], "ask-0");
    assert_eq!(arr[0]["question"], "which arm ships?");
    assert_eq!(arr[0]["answer"], "arm-b");
    assert_eq!(arr[0]["answered_at"], "2026-08-06T12:00:00Z");
    println!("e2e ask: wait exited 0 with the folded answer on stdout");
}

#[test]
fn e2e_ask_wait_exits_5_when_cancelled() {
    let run = "e2e-ask-cancelled";
    let (_tmp, xdg, run_dir) = ask_fixture(run);
    post_ask(&xdg, run, "which arm ships?");

    let mut wait_child = ask_cmd(&xdg)
        .args(["ask", "wait", run, "--timeout-ms", "20000"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("drovr ask wait");
    thread::sleep(Duration::from_millis(300));
    assert!(
        wait_child.try_wait().expect("try_wait").is_none(),
        "ask wait must still be blocking before the cancellation"
    );

    // The human hits Cancel; the server writes this marker (`review.rs`'s
    // `handle_post_submit`). `ask wait` reads the marker, not the server.
    fs::write(run_dir.join("cancelled"), b"cancelled\n").expect("cancelled marker");

    let out = wait_child.wait_with_output().expect("wait for ask wait");
    assert_eq!(
        out.status.code(),
        Some(5),
        "ask wait must exit 5 on cancel, not 0/1/2; stdout={} stderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.to_lowercase().contains("cancel"),
        "message must name the cancellation: {stderr}"
    );

    // And the other direction: a cancelled run refuses to take a NEW question, and
    // refuses it without appending anything.
    let before = fs::read_to_string(run_dir.join("interview.jsonl")).expect("log");
    let out = ask_cmd(&xdg)
        .args(["ask", run, "--question", "one more?"])
        .output()
        .expect("drovr ask on a cancelled run");
    assert_eq!(
        out.status.code(),
        Some(5),
        "ask must refuse a cancelled run with 5; stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let after = fs::read_to_string(run_dir.join("interview.jsonl")).expect("log");
    assert_eq!(before, after, "a refused ask must append nothing");
    println!("e2e ask: wait exited 5 on cancel, and a cancelled run refuses new asks");
}

#[test]
fn e2e_ask_wait_times_out_without_losing_the_question() {
    let run = "e2e-ask-timeout";
    let (_tmp, xdg, run_dir) = ask_fixture(run);
    post_ask(&xdg, run, "which arm ships?");
    let log = run_dir.join("interview.jsonl");
    let before = fs::read_to_string(&log).expect("log");

    // Two rounds: a timeout must be re-armable, and cost nothing each time. This is
    // what makes a non-blocking `ask` safe — the caller re-runs the wait instead of
    // holding a call open that could die with the question inside it.
    for round in 1..=2 {
        let out = ask_cmd(&xdg)
            .args(["ask", "wait", run, "--timeout-ms", "500"])
            .output()
            .expect("drovr ask wait");
        assert_eq!(
            out.status.code(),
            Some(2),
            "round {round}: timeout must exit 2, never 0; stdout={} stderr={}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains("re-run"),
            "round {round}: the message must say the wait is resumable: {stderr}"
        );
        assert!(
            String::from_utf8_lossy(&out.stdout).trim().is_empty(),
            "round {round}: a timeout must put nothing on stdout — stdout is the answer channel"
        );
        // The question survived the timeout, byte for byte, still unanswered.
        assert_eq!(
            fs::read_to_string(&log).expect("log"),
            before,
            "round {round}: a timed-out wait must not touch the log"
        );
    }
    println!("e2e ask: two timeouts, exit 2 each, question untouched on disk");
}

#[test]
fn e2e_ask_wait_still_yields_an_answer_that_landed_before_the_re_arm() {
    // The race the whole non-blocking design turns on. A wait times out; the human
    // answers in the seconds it takes the agent to wake and re-arm; the re-armed
    // wait now has nothing PENDING. If that case returned a bare `[]`, exit 0 would
    // mean "answered" while carrying no answer, and the caller would walk on without
    // it — a timeout that cost exactly the thing it was promised not to cost.
    //
    // So: with nothing pending, the wait returns the folded interview rather than an
    // empty array. An empty log still yields `[]`, because that IS its fold.
    let run = "e2e-ask-rearm";
    let (_tmp, xdg, run_dir) = ask_fixture(run);

    // Nothing asked yet — genuinely empty, and `[]` is the whole truth.
    let out = ask_cmd(&xdg)
        .args(["ask", "wait", run])
        .output()
        .expect("drovr ask wait");
    assert_eq!(out.status.code(), Some(0), "an empty interview exits 0");
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "[]",
        "no asks at all folds to []"
    );

    // Now: ask, time out, and let the answer land before the re-arm.
    post_ask(&xdg, run, "which arm ships?");
    let out = ask_cmd(&xdg)
        .args(["ask", "wait", run, "--timeout-ms", "300"])
        .output()
        .expect("drovr ask wait");
    assert_eq!(out.status.code(), Some(2), "still unanswered, so a timeout");
    append_answer_line(&run_dir, "ask-0", 0, "arm-b");

    let out = ask_cmd(&xdg)
        .args(["ask", "wait", run, "--timeout-ms", "300"])
        .output()
        .expect("drovr ask wait");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        out.status.code(),
        Some(0),
        "the re-arm must exit 0; stdout={stdout}"
    );
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("stdout must be JSON: {e}; stdout={stdout}"));
    let arr = parsed.as_array().expect("JSON array");
    assert_eq!(
        arr.len(),
        1,
        "the answer must survive the re-arm window: {stdout}"
    );
    assert_eq!(arr[0]["answer"], "arm-b");
    println!("e2e ask: an answer that landed between the timeout and the re-arm still comes back");
}

#[test]
fn e2e_ask_usage_errors_exit_1() {
    let run = "e2e-ask-usage";
    let (_tmp, xdg, run_dir) = ask_fixture(run);

    // 1 is the error code, distinct from every outcome: a malformed ask must never
    // be mistaken for a posted one (0) or a timeout (2).
    for (label, args) in [
        ("missing --question", vec!["ask", run]),
        (
            "--option without =",
            vec!["ask", run, "--question", "q?", "--option", "novalue"],
        ),
        (
            "--recommend naming no option",
            vec![
                "ask",
                run,
                "--question",
                "q?",
                "--option",
                "a=Arm A",
                "--recommend",
                "z",
            ],
        ),
        (
            "two --options sharing a value",
            vec![
                "ask",
                run,
                "--question",
                "q?",
                "--option",
                "a=Arm A",
                "--option",
                "a=Arm B",
            ],
        ),
        (
            "unknown run",
            vec!["ask", "no-such-run", "--question", "q?"],
        ),
        // The half `ask wait` must not get wrong: a run that does not exist reads,
        // through `interview::read`, exactly like a run whose questions are all
        // answered — both fold to an empty log. Without its own check the wait would
        // print `[]` and exit 0, which is "answered, nothing outstanding". A driver
        // that typo'd the run name, or whose run was torn down under it, would be
        // waved on. Same guard, same code, as the post half one function up.
        (
            "ask wait on an unknown run",
            vec!["ask", "wait", "no-such-run", "--timeout-ms", "500"],
        ),
    ] {
        let out = ask_cmd(&xdg).args(&args).output().expect("drovr ask");
        assert_eq!(
            out.status.code(),
            Some(1),
            "{label}: must exit 1; stdout={} stderr={}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            String::from_utf8_lossy(&out.stdout).trim().is_empty(),
            "{label}: an error must put nothing on stdout — stdout is the answer channel"
        );
    }
    assert!(
        !run_dir.join("interview.jsonl").exists(),
        "no usage error may create the log"
    );
    println!("e2e ask: every usage error exits 1 and writes nothing");
}
