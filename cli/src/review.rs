//! Review server — interactive spec-review loop.
//!
//! Serves a `tiny_http` HTTP server that lets a human reviewer read the
//! spec, diff it against the prior version, answer MC questions, leave
//! per-line annotations, and submit a decision (approve / request-changes).
//!
//! The agent counterpart calls [`review_summary`] to POST the summary text
//! to the running server, which flips state from `waiting` → `ready`.
//!
//! ## Port discovery
//!
//! [`serve`] writes the bound address to `<run_dir>/review.addr` (e.g.
//! `127.0.0.1:8791`) immediately after the socket is open. [`review_summary`]
//! reads that file to find the server.

use std::fs;
use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

use crate::run::{RunState, run_dir};

/// How often [`review_wait`] polls the live server for a reviewer decision.
/// Mirrors `phase::POLL_INTERVAL` — a filesystem/state poll, not a hot loop.
const POLL_INTERVAL: Duration = Duration::from_millis(500);

// ---------------------------------------------------------------------------
// Embedded assets (single-binary shape)
// ---------------------------------------------------------------------------

const INDEX_HTML: &str = include_str!("../web/index.html");
const MARKDOWN_IT_JS: &[u8] = include_bytes!("../web/vendor/markdown-it.min.js");

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum LoopState {
    Idle,
    Waiting,
    Ready,
    Approved,
}

impl LoopState {
    fn as_str(&self) -> &'static str {
        match self {
            LoopState::Idle => "idle",
            LoopState::Waiting => "waiting",
            LoopState::Ready => "ready",
            LoopState::Approved => "approved",
        }
    }
}

struct AppState {
    state: LoopState,
    turn: u32,
    spec_path: PathBuf,
    workdir: PathBuf,
    /// Git repo to diff for the code-review surface (`GET /review/diff`).
    /// Empty when the run could not be loaded — the diff endpoint then 204s.
    project_dir: PathBuf,
}

impl AppState {
    fn new(spec_path: PathBuf, workdir: PathBuf, project_dir: PathBuf) -> Self {
        AppState {
            state: LoopState::Idle,
            turn: 0,
            spec_path,
            workdir,
            project_dir,
        }
    }

    fn feedback_path(&self) -> PathBuf {
        self.workdir.join("feedback.json")
    }

    fn prior_path(&self) -> PathBuf {
        self.workdir.join("prior.md")
    }

    fn summary_path(&self) -> PathBuf {
        self.workdir.join("summary.txt")
    }

    fn approved_path(&self) -> PathBuf {
        self.workdir.join("approved")
    }

    fn questions_path(&self) -> PathBuf {
        self.workdir.join("questions.json")
    }
}

// ---------------------------------------------------------------------------
// HTTP helpers
// ---------------------------------------------------------------------------

fn header(name: &str, value: &str) -> Header {
    Header::from_bytes(name.as_bytes(), value.as_bytes()).expect("valid header")
}

fn respond_str(req: Request, status: u16, content_type: &str, body: String) {
    let resp = Response::from_string(body)
        .with_status_code(StatusCode(status))
        .with_header(header("Content-Type", content_type))
        .with_header(header("Cache-Control", "no-store"));
    let _ = req.respond(resp);
}

fn respond_bytes(req: Request, status: u16, content_type: &str, body: Vec<u8>) {
    let resp = Response::from_data(body)
        .with_status_code(StatusCode(status))
        .with_header(header("Content-Type", content_type))
        .with_header(header("Cache-Control", "no-store"));
    let _ = req.respond(resp);
}

fn respond_404(req: Request) {
    respond_str(req, 404, "text/plain", "not found".into());
}

fn respond_empty(req: Request, status: u16) {
    let resp = Response::from_data(Vec::<u8>::new())
        .with_status_code(StatusCode(status))
        .with_header(header("Cache-Control", "no-store"));
    let _ = req.respond(resp);
}

fn read_body(req: &mut Request) -> String {
    let mut buf = String::new();
    let _ = req.as_reader().read_to_string(&mut buf);
    buf
}

/// Extract a query-string parameter by key from a raw request URL
/// (e.g. `/review/findings?task=t` → `task` → `"t"`). No percent-decoding:
/// task labels are plain filename components (validated by [`safe_task`]).
fn query_param(url: &str, key: &str) -> Option<String> {
    let query = url.split('?').nth(1)?;
    for pair in query.split('&') {
        let mut it = pair.splitn(2, '=');
        if it.next() == Some(key) {
            return Some(it.next().unwrap_or("").to_string());
        }
    }
    None
}

/// A `task` is safe as a single filename component: non-empty and free of path
/// separators, traversal, or a null byte. Mirrors `main::validate_label`.
fn safe_task(task: &str) -> bool {
    !task.is_empty()
        && !task.contains('/')
        && !task.contains('\\')
        && !task.contains("..")
        && !task.contains('\0')
}

/// A recorded base is a bare git object id (hex, ≤64 chars for SHA-256). Reject
/// anything else so it can never be interpreted as a git rev-arg or flag when
/// interpolated into `git diff <base>..HEAD`.
fn safe_sha(sha: &str) -> bool {
    !sha.is_empty() && sha.len() <= 64 && sha.chars().all(|c| c.is_ascii_hexdigit())
}

fn content_type_for(path: &str) -> &'static str {
    if path.ends_with(".js") {
        "application/javascript; charset=utf-8"
    } else if path.ends_with(".css") {
        "text/css; charset=utf-8"
    } else if path.ends_with(".html") {
        "text/html; charset=utf-8"
    } else {
        "application/octet-stream"
    }
}

// ---------------------------------------------------------------------------
// Request handler
// ---------------------------------------------------------------------------

fn handle(mut req: Request, shared: &Arc<Mutex<AppState>>) {
    let method = req.method().clone();
    let url = req.url().to_string();
    let path = url.split('?').next().unwrap_or("/");

    // GET / — serve embedded index.html
    if method == Method::Get && path == "/" {
        respond_bytes(req, 200, "text/html; charset=utf-8", INDEX_HTML.as_bytes().to_vec());
        return;
    }

    // GET /web/<file> — serve embedded vendor assets (no path traversal)
    if method == Method::Get && path.starts_with("/web/") {
        let rel = path.strip_prefix("/web/").unwrap_or("");
        if rel.contains("..") {
            respond_404(req);
            return;
        }
        match rel {
            "vendor/markdown-it.min.js" => {
                respond_bytes(req, 200, "application/javascript; charset=utf-8", MARKDOWN_IT_JS.to_vec());
            }
            other => {
                let ct = content_type_for(other);
                respond_str(req, 404, ct, "not found".into());
            }
        }
        return;
    }

    // GET /doc — raw spec markdown (graceful fallback if absent)
    if method == Method::Get && path == "/doc" {
        let st = shared.lock().unwrap();
        let spec_path = st.spec_path.clone();
        drop(st);
        match fs::read(&spec_path) {
            Ok(bytes) => respond_bytes(req, 200, "text/markdown; charset=utf-8", bytes),
            Err(_) => respond_str(req, 200, "text/markdown; charset=utf-8", String::new()),
        }
        return;
    }

    // GET /prior — raw prior.md or 204 if none
    if method == Method::Get && path == "/prior" {
        let st = shared.lock().unwrap();
        let prior = st.prior_path();
        drop(st);
        match fs::read(&prior) {
            Ok(bytes) if !bytes.is_empty() => {
                respond_bytes(req, 200, "text/markdown; charset=utf-8", bytes)
            }
            _ => respond_empty(req, 204),
        }
        return;
    }

    // GET /summary — raw summary.txt (or empty string)
    if method == Method::Get && path == "/summary" {
        let st = shared.lock().unwrap();
        let summary_path = st.summary_path();
        drop(st);
        let text = fs::read_to_string(&summary_path).unwrap_or_default();
        respond_str(req, 200, "text/plain; charset=utf-8", text);
        return;
    }

    // GET /state — JSON {state, turn}
    if method == Method::Get && path == "/state" {
        let st = shared.lock().unwrap();
        let body = format!(
            r#"{{"state":"{}","turn":{}}}"#,
            st.state.as_str(),
            st.turn
        );
        drop(st);
        respond_str(req, 200, "application/json", body);
        return;
    }

    // GET /questions — workdir/questions.json (or empty array)
    if method == Method::Get && path == "/questions" {
        let st = shared.lock().unwrap();
        let qpath = st.questions_path();
        drop(st);
        let body = fs::read_to_string(&qpath).unwrap_or_else(|_| "[]".into());
        respond_str(req, 200, "application/json", body);
        return;
    }

    // GET /review/findings?task=<task> — merged <task>-review.json (or {} if absent).
    if method == Method::Get && path == "/review/findings" {
        let task = query_param(&url, "task").unwrap_or_default();
        if !safe_task(&task) {
            respond_str(req, 400, "text/plain", "invalid task".into());
            return;
        }
        let st = shared.lock().unwrap();
        let file = st.workdir.join(format!("{task}-review.json"));
        drop(st);
        let body = fs::read_to_string(&file).unwrap_or_else(|_| "{}".into());
        respond_str(req, 200, "application/json", body);
        return;
    }

    // GET /review/diff?task=<task> — unified `git diff <base>..HEAD`, base from
    // <workdir>/<task>-base.sha. 204 when the base SHA has not been recorded.
    if method == Method::Get && path == "/review/diff" {
        let task = query_param(&url, "task").unwrap_or_default();
        if !safe_task(&task) {
            respond_str(req, 400, "text/plain", "invalid task".into());
            return;
        }
        let st = shared.lock().unwrap();
        let base_file = st.workdir.join(format!("{task}-base.sha"));
        let project_dir = st.project_dir.clone();
        drop(st);

        let base = match fs::read_to_string(&base_file) {
            Ok(s) if safe_sha(s.trim()) => s.trim().to_string(),
            // Absent, empty, or a non-SHA value (a compromised/buggy writer could
            // slip a git rev-arg or flag in) → no diff rather than trusting it.
            _ => {
                respond_empty(req, 204);
                return;
            }
        };
        if project_dir.as_os_str().is_empty() {
            respond_empty(req, 204);
            return;
        }
        match Command::new("git")
            .arg("-C")
            .arg(&project_dir)
            .arg("diff")
            .arg(format!("{base}..HEAD"))
            .output()
        {
            // Non-zero git exit (e.g. base SHA not in the repo) → 204, not a
            // misleading empty 200. stderr is dropped; stdout carries the diff.
            Ok(out) if out.status.success() => {
                let body = String::from_utf8_lossy(&out.stdout).into_owned();
                respond_str(req, 200, "text/plain; charset=utf-8", body);
            }
            _ => respond_empty(req, 204),
        }
        return;
    }

    // POST /submit — JSON body: {decision, feedback, answers, annotations}
    if method == Method::Post && path == "/submit" {
        let body = read_body(&mut req);
        let parsed: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
        let decision = parsed["decision"].as_str().unwrap_or("");
        let feedback = parsed["feedback"].as_str().unwrap_or("").to_string();
        let answers = parsed.get("answers").cloned().unwrap_or(serde_json::json!({}));
        let annotations = parsed.get("annotations").cloned().unwrap_or(serde_json::json!([]));

        let mut st = shared.lock().unwrap();

        if decision == "approve" {
            let _ = fs::write(st.approved_path(), b"approved\n");
            st.state = LoopState::Approved;
            drop(st);
            respond_str(req, 200, "application/json", r#"{"ok":true,"state":"approved"}"#.into());
        } else {
            // Snapshot current spec → prior.md
            let spec_bytes = fs::read(&st.spec_path).unwrap_or_default();
            let _ = fs::write(st.prior_path(), &spec_bytes);

            st.turn += 1;
            let turn = st.turn;

            let fb_json = serde_json::json!({
                "turn": turn,
                "decision": decision,
                "feedback": feedback,
                "answers": answers,
                "annotations": annotations,
            });
            let _ = fs::write(st.feedback_path(), fb_json.to_string());
            st.state = LoopState::Waiting;
            drop(st);
            respond_str(req, 200, "application/json", r#"{"ok":true,"state":"waiting"}"#.into());
        }
        return;
    }

    // POST /summary — body = summary text; flips state → ready
    if method == Method::Post && path == "/summary" {
        let body = read_body(&mut req);
        let mut st = shared.lock().unwrap();
        let _ = fs::write(st.summary_path(), body.as_bytes());
        st.state = LoopState::Ready;
        drop(st);
        respond_str(req, 200, "application/json", r#"{"ok":true,"state":"ready"}"#.into());
        return;
    }

    respond_404(req);
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Start the review HTTP server.
///
/// The bound address is written to `<run_dir>/review.addr` immediately after
/// the socket opens, so `drovr review summary` can discover the port.
///
/// `spec_path` is `run_dir(run)/spec.md`; missing spec is handled gracefully
/// (`GET /doc` returns an empty body).
///
/// This function blocks until the process exits.
pub fn serve(run: &str, host: &str, port: u16) -> io::Result<()> {
    let workdir = run_dir(run);
    fs::create_dir_all(&workdir)?;

    let spec_path = workdir.join("spec.md");
    let addr_file = workdir.join("review.addr");

    let addr = format!("{}:{}", host, port);
    let server = Server::http(&addr)
        .map_err(|e| io::Error::other(e.to_string()))?;

    // Write the actual bound address (important when port=0 → OS-assigned).
    let bound_addr = server
        .server_addr()
        .to_ip()
        .map(|a| a.to_string())
        .unwrap_or_else(|| addr.clone());
    fs::write(&addr_file, bound_addr.as_bytes())?;

    eprintln!("drovr review listening on http://{}", bound_addr);
    eprintln!("  run:  {}", run);
    eprintln!("  spec: {:?}", spec_path);

    // Load the run to find the project dir to diff for the code-review surface.
    // On load failure, pass an empty path — `GET /review/diff` then 204s.
    let project_dir = RunState::load(run)
        .map(|s| PathBuf::from(s.project_dir))
        .unwrap_or_default();

    let shared = Arc::new(Mutex::new(AppState::new(spec_path, workdir, project_dir)));

    for req in server.incoming_requests() {
        let shared = Arc::clone(&shared);
        handle(req, &shared);
    }

    Ok(())
}

/// POST summary text to the running review server (`drovr review summary`).
///
/// Reads the bound address from `<run_dir>/review.addr` written by [`serve`].
pub fn review_summary(run: &str, text: &str) -> io::Result<()> {
    let addr_file = run_dir(run).join("review.addr");
    let addr = fs::read_to_string(&addr_file).map_err(|e| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("review server address not found ({}); is `drovr serve` running for run {:?}? ({})", addr_file.display(), run, e),
        )
    })?;
    let addr = addr.trim();

    // Send HTTP POST via raw TCP to avoid adding an HTTP client dep.
    let body = text.as_bytes();
    let request = format!(
        "POST /summary HTTP/1.0\r\nHost: {addr}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\n\r\n",
        body.len()
    );

    let mut stream = TcpStream::connect(addr)
        .map_err(|e| io::Error::new(e.kind(), format!("could not connect to review server at {addr}: {e}")))?;

    stream.write_all(request.as_bytes())?;
    stream.write_all(body)?;

    // Read response to confirm 200
    let mut response = String::new();
    let _ = stream.read_to_string(&mut response);
    if !response.starts_with("HTTP/1") || !response.contains(" 200 ") {
        return Err(io::Error::other(
            format!("unexpected response from review server: {}", response.lines().next().unwrap_or("")),
        ));
    }

    Ok(())
}

/// Terminal outcome of a [`review_wait`] blocking wait.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitOutcome {
    /// Reviewer approved — the `approved` marker is present.
    Approved,
    /// Reviewer requested changes — `feedback.json` holds this turn's feedback.
    ChangesRequested,
    /// No reviewer action within the timeout; the caller may re-run to resume.
    Timeout,
}

/// GET `/state` from the live review server, returning the `state` string.
fn fetch_state(addr: &str) -> io::Result<String> {
    let mut stream = TcpStream::connect(addr).map_err(|e| {
        io::Error::new(e.kind(), format!("could not connect to review server at {addr}: {e}"))
    })?;
    write!(stream, "GET /state HTTP/1.0\r\nHost: {addr}\r\n\r\n")?;
    let mut resp = String::new();
    stream.read_to_string(&mut resp)?;
    let body = resp.splitn(2, "\r\n\r\n").nth(1).unwrap_or("");
    let parsed: serde_json::Value = serde_json::from_str(body.trim()).unwrap_or_default();
    parsed["state"]
        .as_str()
        .map(|s| s.to_owned())
        .ok_or_else(|| io::Error::other(format!("malformed /state response from {addr}: {body:?}")))
}

/// Block until the reviewer acts on the spec gate, then return the outcome.
///
/// Reads `<run_dir>/review.addr` (written by [`serve`]) and polls the live
/// server's authoritative `GET /state` at [`POLL_INTERVAL`] until either the
/// reviewer submits or `timeout_ms` elapses. Blocks while state is `idle`/
/// `ready`; returns [`WaitOutcome::Approved`] once the `approved` marker lands
/// and [`WaitOutcome::ChangesRequested`] once the reviewer requests changes
/// (`feedback.json` holds the turn). On timeout it returns
/// [`WaitOutcome::Timeout`] — the wait is resumable, so a driver just re-runs
/// it. A missing `review.addr` or an unreachable server is an `Err`.
///
/// Mirrors [`crate::phase::phase_wait`]: a bounded poll with a deadline and no
/// hot loop, so a driver can background it and be woken on process exit.
pub fn review_wait(run: &str, timeout_ms: u64) -> io::Result<WaitOutcome> {
    let addr_file = run_dir(run).join("review.addr");
    let addr = fs::read_to_string(&addr_file).map_err(|e| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("review server address not found ({}); is `drovr serve` running for run {:?}? ({})", addr_file.display(), run, e),
        )
    })?;
    let addr = addr.trim().to_owned();

    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    loop {
        match fetch_state(&addr)?.as_str() {
            "approved" => return Ok(WaitOutcome::Approved),
            "waiting" => return Ok(WaitOutcome::ChangesRequested),
            // "idle" / "ready" — reviewer has not acted yet; keep blocking.
            _ => {}
        }
        let now = Instant::now();
        if now >= deadline {
            return Ok(WaitOutcome::Timeout);
        }
        thread::sleep(POLL_INTERVAL.min(deadline - now));
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpStream;

    /// Start a review server on 127.0.0.1:0 in a background thread.
    /// Returns the bound address string (e.g. "127.0.0.1:54321").
    fn start_server(workdir: PathBuf, spec_path: PathBuf) -> String {
        // Default project_dir to the workdir; the diff endpoint 204s without a
        // base.sha regardless. Use `start_server_pd` to diff a real git repo.
        let pd = workdir.clone();
        start_server_pd(workdir, spec_path, pd)
    }

    /// Like `start_server` but with an explicit `project_dir` for `/review/diff`.
    fn start_server_pd(workdir: PathBuf, spec_path: PathBuf, project_dir: PathBuf) -> String {
        let server = Server::http("127.0.0.1:0").expect("bind");
        let bound = server
            .server_addr()
            .to_ip()
            .expect("ip addr")
            .to_string();

        let shared = Arc::new(Mutex::new(AppState::new(spec_path, workdir, project_dir)));
        thread::spawn(move || {
            for req in server.incoming_requests() {
                let shared = Arc::clone(&shared);
                handle(req, &shared);
            }
        });

        // Give the thread a moment to start accepting.
        thread::sleep(Duration::from_millis(10));
        bound
    }

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

    fn make_workdir(suffix: &str) -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix(&format!("drovr-review-test-{suffix}"))
            .tempdir()
            .expect("tempdir")
    }

    #[test]
    fn state_starts_idle() {
        let tmp = make_workdir("idle");
        let spec = tmp.path().join("spec.md");
        fs::write(&spec, b"# Spec").unwrap();
        let addr = start_server(tmp.path().to_path_buf(), spec);

        let (status, body) = http_get(&addr, "/state");
        assert_eq!(status, 200);
        assert!(body.contains(r#""state":"idle""#), "body={body}");
        assert!(body.contains(r#""turn":0"#), "body={body}");
    }

    #[test]
    fn post_summary_flips_to_ready() {
        let tmp = make_workdir("summary");
        let spec = tmp.path().join("spec.md");
        fs::write(&spec, b"# Spec").unwrap();
        let addr = start_server(tmp.path().to_path_buf(), spec);

        let (status, body) = http_post(&addr, "/summary", "text/plain", "Initial summary text.");
        assert_eq!(status, 200, "body={body}");
        assert!(body.contains(r#""state":"ready""#), "body={body}");

        let (status2, body2) = http_get(&addr, "/state");
        assert_eq!(status2, 200);
        assert!(body2.contains(r#""state":"ready""#), "body2={body2}");
    }

    #[test]
    fn questions_empty_when_no_file() {
        let tmp = make_workdir("questions");
        let spec = tmp.path().join("spec.md");
        fs::write(&spec, b"# Spec").unwrap();
        let addr = start_server(tmp.path().to_path_buf(), spec);

        let (status, body) = http_get(&addr, "/questions");
        assert_eq!(status, 200);
        assert_eq!(body.trim(), "[]", "expected empty array, got: {body}");
    }

    #[test]
    fn questions_served_when_file_present() {
        let tmp = make_workdir("questions-present");
        let spec = tmp.path().join("spec.md");
        fs::write(&spec, b"# Spec").unwrap();
        let q_json = r#"[{"id":"q1","prompt":"Which?","options":[{"value":"a","label":"A"}]}]"#;
        fs::write(tmp.path().join("questions.json"), q_json).unwrap();
        let addr = start_server(tmp.path().to_path_buf(), spec);

        let (status, body) = http_get(&addr, "/questions");
        assert_eq!(status, 200);
        assert!(body.contains("q1"), "body={body}");
    }

    #[test]
    fn submit_request_changes_flips_waiting_and_writes_files() {
        let tmp = make_workdir("submit");
        let spec = tmp.path().join("spec.md");
        fs::write(&spec, b"# Spec\nSome content.").unwrap();
        let addr = start_server(tmp.path().to_path_buf(), spec.clone());

        let payload = r#"{"decision":"request-changes","feedback":"needs work","answers":{},"annotations":[]}"#;
        let (status, body) = http_post(&addr, "/submit", "application/json", payload);
        assert_eq!(status, 200, "body={body}");
        assert!(body.contains(r#""state":"waiting""#), "body={body}");

        let (_, state_body) = http_get(&addr, "/state");
        assert!(state_body.contains(r#""state":"waiting""#));
        assert!(state_body.contains(r#""turn":1"#));

        // prior.md should exist and match spec
        let prior = fs::read(tmp.path().join("prior.md")).expect("prior.md");
        assert_eq!(prior, b"# Spec\nSome content.");

        // feedback.json should be written
        let fb = fs::read_to_string(tmp.path().join("feedback.json")).expect("feedback.json");
        let v: serde_json::Value = serde_json::from_str(&fb).unwrap();
        assert_eq!(v["turn"], 1);
        assert_eq!(v["decision"], "request-changes");
        assert_eq!(v["feedback"], "needs work");
    }

    #[test]
    fn submit_approve_writes_marker_and_flips_approved() {
        let tmp = make_workdir("approve");
        let spec = tmp.path().join("spec.md");
        fs::write(&spec, b"# Done").unwrap();
        let addr = start_server(tmp.path().to_path_buf(), spec);

        let payload = r#"{"decision":"approve","feedback":"","answers":{},"annotations":[]}"#;
        let (status, body) = http_post(&addr, "/submit", "application/json", payload);
        assert_eq!(status, 200, "body={body}");
        assert!(body.contains(r#""state":"approved""#), "body={body}");

        let approved = fs::read(tmp.path().join("approved")).expect("approved marker");
        assert_eq!(approved, b"approved\n");

        let (_, state_body) = http_get(&addr, "/state");
        assert!(state_body.contains(r#""state":"approved""#));
    }

    #[test]
    fn doc_graceful_when_spec_absent() {
        let tmp = make_workdir("no-spec");
        // No spec.md written
        let spec = tmp.path().join("spec.md");
        let addr = start_server(tmp.path().to_path_buf(), spec);

        let (status, body) = http_get(&addr, "/doc");
        assert_eq!(status, 200);
        assert_eq!(body.trim(), "", "should return empty body: {body}");
    }

    #[test]
    fn prior_returns_204_when_absent() {
        let tmp = make_workdir("no-prior");
        let spec = tmp.path().join("spec.md");
        fs::write(&spec, b"# Spec").unwrap();
        let addr = start_server(tmp.path().to_path_buf(), spec);

        let (status, _) = http_get(&addr, "/prior");
        assert_eq!(status, 204);
    }

    #[test]
    fn index_html_served() {
        let tmp = make_workdir("index");
        let spec = tmp.path().join("spec.md");
        fs::write(&spec, b"# Spec").unwrap();
        let addr = start_server(tmp.path().to_path_buf(), spec);

        let (status, body) = http_get(&addr, "/");
        assert_eq!(status, 200);
        assert!(body.contains("Drovr Review"), "body should contain title: {}", &body[..200.min(body.len())]);
    }

    #[test]
    fn vendor_js_served() {
        let tmp = make_workdir("vendor");
        let spec = tmp.path().join("spec.md");
        fs::write(&spec, b"# Spec").unwrap();
        let addr = start_server(tmp.path().to_path_buf(), spec);

        let (status, _) = http_get(&addr, "/web/vendor/markdown-it.min.js");
        assert_eq!(status, 200);
    }

    #[test]
    fn path_traversal_rejected() {
        let tmp = make_workdir("traversal");
        let spec = tmp.path().join("spec.md");
        fs::write(&spec, b"# Spec").unwrap();
        let addr = start_server(tmp.path().to_path_buf(), spec);

        let (status, _) = http_get(&addr, "/web/../etc/passwd");
        assert_eq!(status, 404);
    }

    #[test]
    fn review_summary_fn_posts_to_server() {
        let _guard = crate::test_util::ENV_LOCK.lock().unwrap();
        // Use a unique run name and set XDG_DATA_HOME so run_dir() points to
        // our temp dir. The server must be started in that same run_dir so
        // review_summary() and the server agree on where to write summary.txt.
        let tmp = make_workdir("rev-summary");
        let run_name = "test-review-summary-fn";
        let fake_base = tmp.path().to_path_buf();
        let fake_run_dir = fake_base.join("drovr/runs").join(run_name);
        fs::create_dir_all(&fake_run_dir).unwrap();
        let spec = fake_run_dir.join("spec.md");
        fs::write(&spec, b"# Spec").unwrap();

        // Start server with the workdir that run_dir() will resolve to.
        let addr = start_server(fake_run_dir.clone(), spec);

        // Write review.addr into the run dir so review_summary can find it.
        fs::write(fake_run_dir.join("review.addr"), addr.as_bytes()).unwrap();

        unsafe { std::env::set_var("XDG_DATA_HOME", fake_base.to_str().unwrap()); }

        review_summary(run_name, "Agent summary text.").expect("review_summary");

        // Confirm summary.txt was written by the server into the run dir.
        let summary = fs::read_to_string(fake_run_dir.join("summary.txt")).expect("summary.txt");
        assert_eq!(summary, "Agent summary text.");
    }

    // -- review_wait ---------------------------------------------------------

    /// Spin a real review server in the run dir that `run_dir(run)` resolves to,
    /// write `review.addr`, and return `(addr, run_name, tempdir)`. Caller must
    /// already hold ENV_LOCK (this sets XDG_DATA_HOME).
    fn wait_fixture(suffix: &str) -> (String, String, tempfile::TempDir) {
        let tmp = make_workdir(suffix);
        let run_name = format!("wait-{suffix}");
        let fake_base = tmp.path().to_path_buf();
        let fake_run_dir = fake_base.join("drovr/runs").join(&run_name);
        fs::create_dir_all(&fake_run_dir).unwrap();
        let spec = fake_run_dir.join("spec.md");
        fs::write(&spec, b"# Spec").unwrap();

        let addr = start_server(fake_run_dir.clone(), spec);
        fs::write(fake_run_dir.join("review.addr"), addr.as_bytes()).unwrap();
        unsafe { std::env::set_var("XDG_DATA_HOME", fake_base.to_str().unwrap()); }
        (addr, run_name, tmp)
    }

    #[test]
    fn wait_missing_addr_errors() {
        let _guard = crate::test_util::ENV_LOCK.lock().unwrap();
        let tmp = make_workdir("wait-no-addr");
        unsafe { std::env::set_var("XDG_DATA_HOME", tmp.path().to_str().unwrap()); }
        // No review.addr written → unreachable server → error (exit 1 at CLI).
        let res = review_wait("does-not-exist", 100);
        assert!(res.is_err(), "missing review.addr must be an error");
    }

    #[test]
    fn wait_times_out_while_idle() {
        let _guard = crate::test_util::ENV_LOCK.lock().unwrap();
        let (_addr, run_name, _tmp) = wait_fixture("idle-timeout");
        // State is `idle` (no summary posted) → wait blocks then times out.
        let outcome = review_wait(&run_name, 60).expect("wait");
        assert_eq!(outcome, WaitOutcome::Timeout);
    }

    #[test]
    fn wait_returns_approved_when_reviewer_approves() {
        let _guard = crate::test_util::ENV_LOCK.lock().unwrap();
        let (addr, run_name, _tmp) = wait_fixture("approve");
        // Driver posted a summary → state `ready`; wait must BLOCK here.
        http_post(&addr, "/summary", "text/plain", "please review");

        let run_for_thread = run_name.clone();
        let handle = thread::spawn(move || review_wait(&run_for_thread, 10_000));

        // While the reviewer has not acted, the wait must not have returned.
        thread::sleep(Duration::from_millis(200));
        assert!(!handle.is_finished(), "wait must block while state is `ready`");

        // Reviewer approves → wait wakes and exits with Approved.
        http_post(
            &addr,
            "/submit",
            "application/json",
            r#"{"decision":"approve","feedback":"","answers":{},"annotations":[]}"#,
        );
        let outcome = handle.join().unwrap().expect("wait");
        assert_eq!(outcome, WaitOutcome::Approved);
    }

    #[test]
    fn wait_returns_changes_requested() {
        let _guard = crate::test_util::ENV_LOCK.lock().unwrap();
        let (addr, run_name, tmp) = wait_fixture("changes");
        http_post(&addr, "/summary", "text/plain", "please review");

        let run_for_thread = run_name.clone();
        let handle = thread::spawn(move || review_wait(&run_for_thread, 10_000));

        thread::sleep(Duration::from_millis(200));
        assert!(!handle.is_finished(), "wait must block while state is `ready`");

        // Reviewer requests changes → wait exits ChangesRequested (exit 3).
        http_post(
            &addr,
            "/submit",
            "application/json",
            r#"{"decision":"request-changes","feedback":"needs work","answers":{},"annotations":[]}"#,
        );
        let outcome = handle.join().unwrap().expect("wait");
        assert_eq!(outcome, WaitOutcome::ChangesRequested);

        // feedback.json holds this turn's feedback for the driver to forward.
        let run_dir = tmp.path().join("drovr/runs").join(&run_name);
        let fb = fs::read_to_string(run_dir.join("feedback.json")).expect("feedback.json");
        assert!(fb.contains("needs work"), "feedback.json must hold the turn: {fb}");
    }

    // -- code-review surface (/review/findings, /review/diff) ----------------

    #[test]
    fn findings_served_when_file_present() {
        let tmp = make_workdir("findings-present");
        let spec = tmp.path().join("spec.md");
        fs::write(&spec, b"# Spec").unwrap();
        let merged = r#"{"verdict":"changes","findings":[{"file":"a.rs","line":3,"severity":"important","angle":"correctness","summary":"off by one","rationale":"loop bound"}]}"#;
        fs::write(tmp.path().join("t-review.json"), merged).unwrap();
        let addr = start_server(tmp.path().to_path_buf(), spec);

        let (status, body) = http_get(&addr, "/review/findings?task=t");
        assert_eq!(status, 200);
        assert!(body.contains("off by one"), "body={body}");
        assert!(body.contains("correctness"), "body={body}");
    }

    #[test]
    fn findings_empty_when_absent() {
        let tmp = make_workdir("findings-absent");
        let spec = tmp.path().join("spec.md");
        fs::write(&spec, b"# Spec").unwrap();
        let addr = start_server(tmp.path().to_path_buf(), spec);

        let (status, body) = http_get(&addr, "/review/findings?task=t");
        assert_eq!(status, 200);
        assert_eq!(body.trim(), "{}", "expected empty object, got: {body}");
    }

    #[test]
    fn findings_rejects_traversal_task() {
        let tmp = make_workdir("findings-traversal");
        let spec = tmp.path().join("spec.md");
        fs::write(&spec, b"# Spec").unwrap();
        let addr = start_server(tmp.path().to_path_buf(), spec);

        let (status, _) = http_get(&addr, "/review/findings?task=../etc/passwd");
        assert_eq!(status, 400);
    }

    #[test]
    fn diff_204_when_no_base() {
        let tmp = make_workdir("diff-no-base");
        let spec = tmp.path().join("spec.md");
        fs::write(&spec, b"# Spec").unwrap();
        let addr = start_server(tmp.path().to_path_buf(), spec);

        let (status, _) = http_get(&addr, "/review/diff?task=t");
        assert_eq!(status, 204);
    }

    #[test]
    fn diff_served_with_base_and_git_repo() {
        let tmp = make_workdir("diff-git");
        let workdir = tmp.path().join("work");
        fs::create_dir_all(&workdir).unwrap();
        let spec = workdir.join("spec.md");
        fs::write(&spec, b"# Spec").unwrap();

        // A throwaway git repo as the project_dir: one commit (the base), then a
        // change so `git diff base..HEAD` is non-empty.
        let repo = tmp.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        let git = |args: &[&str]| {
            Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args(args)
                .output()
                .expect("git")
        };
        git(&["init", "-q"]);
        git(&["config", "user.email", "t@t"]);
        git(&["config", "user.name", "t"]);
        fs::write(repo.join("f.txt"), "one\n").unwrap();
        git(&["add", "."]);
        git(&["commit", "-q", "-m", "base"]);
        let base = String::from_utf8(git(&["rev-parse", "HEAD"]).stdout).unwrap();
        let base = base.trim().to_string();
        fs::write(repo.join("f.txt"), "one\ntwo\n").unwrap();
        git(&["add", "."]);
        git(&["commit", "-q", "-m", "change"]);

        // Record the base SHA where the diff endpoint reads it.
        fs::write(workdir.join("t-base.sha"), format!("{base}\n")).unwrap();

        let addr = start_server_pd(workdir.clone(), spec, repo.clone());
        let (status, body) = http_get(&addr, "/review/diff?task=t");
        assert_eq!(status, 200, "body={body}");
        assert!(body.contains("+two"), "diff body should show the added line: {body}");
    }

    #[test]
    fn diff_204_when_base_is_not_a_sha() {
        // A non-hex base value (e.g. a git flag) must never reach `git diff`.
        let tmp = make_workdir("diff-bad-sha");
        let spec = tmp.path().join("spec.md");
        fs::write(&spec, b"# Spec").unwrap();
        fs::write(tmp.path().join("t-base.sha"), "--no-index\n").unwrap();
        let addr = start_server(tmp.path().to_path_buf(), spec);

        let (status, _) = http_get(&addr, "/review/diff?task=t");
        assert_eq!(status, 204);
    }

    #[test]
    fn diff_204_when_git_fails() {
        // A well-formed but unknown SHA → git exits non-zero → 204, not empty 200.
        let tmp = make_workdir("diff-git-fail");
        let workdir = tmp.path().join("work");
        fs::create_dir_all(&workdir).unwrap();
        let spec = workdir.join("spec.md");
        fs::write(&spec, b"# Spec").unwrap();

        let repo = tmp.path().join("repo");
        fs::create_dir_all(&repo).unwrap();
        let git = |args: &[&str]| {
            Command::new("git").arg("-C").arg(&repo).args(args).output().expect("git")
        };
        git(&["init", "-q"]);
        git(&["config", "user.email", "t@t"]);
        git(&["config", "user.name", "t"]);
        fs::write(repo.join("f.txt"), "one\n").unwrap();
        git(&["add", "."]);
        git(&["commit", "-q", "-m", "base"]);

        // 40 hex chars that are (almost certainly) not a real object → git errors.
        fs::write(workdir.join("t-base.sha"), "0".repeat(40)).unwrap();
        let addr = start_server_pd(workdir, spec, repo);

        let (status, _) = http_get(&addr, "/review/diff?task=t");
        assert_eq!(status, 204);
    }
}
