//! Browser-driven checks for the review UI's keyboard navigation.
//!
//! The navigation lives entirely in `cli/web/index.html`, which no Rust test can
//! reach: the cursor, the filter, the question pickers and the accessibility
//! surfaces only exist once a browser has run the page. This boots the real
//! server against a throwaway `XDG_DATA_HOME`, points a headless chromium at it,
//! and runs `tests/web/nav.mjs` (a dependency-free CDP driver) against it.
//!
//! Prerequisites, checked at start — absent any of them the test prints why and
//! returns skipped-but-passing, matching `e2e.rs`:
//!   1. `node` on PATH (runs the driver; needs a global WebSocket, so node >= 22)
//!   2. a chromium binary on PATH
//!
//! Note that `cli/web/index.html` is `include_str!`'d into the binary, so this
//! exercises whatever HTML was compiled in — editing the file without a rebuild
//! tests the old page.

use std::io::Read;
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use std::{fs, thread};

fn binary_on_path(name: &str) -> bool {
    Command::new("which")
        .arg(name)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Chromium ships under several names depending on the distro / channel.
fn find_chromium() -> Option<&'static str> {
    ["chromium", "chromium-browser", "google-chrome", "google-chrome-stable"]
        .into_iter()
        .find(|b| binary_on_path(b))
}

fn free_port() -> u16 {
    use std::net::TcpListener;
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind for free port");
    listener.local_addr().unwrap().port()
}

struct KillOnDrop(Child);
impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Poll until `addr` accepts a connection, so we never race a not-yet-listening
/// server or a chromium that is still opening its debug port.
fn wait_for_listener(addr: &str, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if TcpStream::connect(addr).is_ok() {
            return true;
        }
        thread::sleep(Duration::from_millis(100));
    }
    false
}

fn http_get_ok(addr: &str, path: &str) -> bool {
    use std::io::Write;
    let Ok(mut s) = TcpStream::connect(addr) else {
        return false;
    };
    if write!(s, "GET {path} HTTP/1.0\r\nHost: {addr}\r\n\r\n").is_err() {
        return false;
    }
    let mut resp = String::new();
    let _ = s.read_to_string(&mut resp);
    resp.lines().next().is_some_and(|l| l.contains(" 200"))
}

/// The fixture the driver's assertions are written against: four runs so the
/// list has something to move over and re-sort, and one run carrying the three
/// open questions the detail-view checks answer.
fn seed_runs(runs_root: &PathBuf) {
    for run in ["alpha-deploy", "beta-cache", "gamma-review", "delta-idle"] {
        let dir = runs_root.join(run);
        fs::create_dir_all(&dir).expect("create run dir");
        fs::write(dir.join("spec.md"), format!("# Spec for {run}\n\nContent.\n")).unwrap();
        fs::write(
            dir.join("state.json"),
            format!(
                r#"{{"name":"{run}","task":"task for {run}","phases":[],"gate":"spec","cursor":0,"project_dir":""}}"#
            ),
        )
        .unwrap();
    }
    // alpha-deploy is the run the detail-view checks drive: put it in the state a
    // reviewer actually meets it in — `ready`, still on turn 0 (the counter only
    // moves when the reviewer submits), with the agent's summary posted.
    let alpha = runs_root.join("alpha-deploy");
    fs::write(alpha.join("summary.txt"), "spec drafted and ready for review").unwrap();
    fs::write(alpha.join("review.state.json"), r#"{"state":"ready","turn":0}"#).unwrap();
    fs::write(
        runs_root.join("alpha-deploy").join("questions.json"),
        r#"[{"id":"cache","prompt":"Which cache backend should the deploy use?","options":[{"value":"redis","label":"Redis","recommended":true},{"value":"memory","label":"In-memory"},{"value":"none","label":"No cache"}]},
 {"id":"retry","prompt":"Retry policy on a failed rollout?","options":[{"value":"exp","label":"Exponential backoff"},{"value":"fixed","label":"Fixed 5s"}]},
 {"id":"notes","prompt":"Anything else the plan phase should know?","options":[]}]"#,
    )
    .unwrap();
}

#[test]
fn web_keyboard_navigation() {
    if !binary_on_path("node") {
        println!("skipping web_nav: `node` not found on PATH");
        return;
    }
    let Some(chromium) = find_chromium() else {
        println!("skipping web_nav: no chromium binary found on PATH");
        return;
    };

    let tmp = tempfile::Builder::new()
        .prefix("drovr-webnav-")
        .tempdir()
        .expect("tempdir");
    let xdg = tmp.path().join("xdg");
    let runs_root = xdg.join("drovr").join("runs");
    seed_runs(&runs_root);

    // --host is explicit: `serve` otherwise takes serve_host from the developer's
    // own config, which may well be a LAN/Tailscale address the driver cannot
    // reach (and which this test has no business binding).
    let port = free_port();
    let addr = format!("127.0.0.1:{port}");
    let server = KillOnDrop(
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
    assert!(
        http_get_ok(&addr, "/api/runs"),
        "drovr serve did not answer /api/runs on {addr}"
    );

    let cdp_port = free_port();
    let cdp_addr = format!("127.0.0.1:{cdp_port}");
    let _browser = KillOnDrop(
        Command::new(chromium)
            .args([
                "--headless",
                "--disable-gpu",
                "--no-first-run",
                "--no-default-browser-check",
                &format!("--remote-debugging-port={cdp_port}"),
                "--remote-allow-origins=*",
                &format!("--user-data-dir={}", tmp.path().join("chrome").display()),
                "about:blank",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn chromium"),
    );
    assert!(
        wait_for_listener(&cdp_addr, Duration::from_secs(20)),
        "chromium never opened its debug port {cdp_addr}"
    );

    let driver = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("web")
        .join("nav.mjs");
    let out = Command::new("node")
        .arg(&driver)
        .env("DROVR_BASE", format!("http://{addr}"))
        .env("DROVR_CDP", format!("http://{cdp_addr}"))
        .output()
        .expect("run nav.mjs");

    println!("{}", String::from_utf8_lossy(&out.stdout));
    let stderr = String::from_utf8_lossy(&out.stderr);
    if !stderr.trim().is_empty() {
        println!("--- driver stderr ---\n{stderr}");
    }
    drop(server);
    assert!(out.status.success(), "web navigation checks failed");
}
