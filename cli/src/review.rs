//! Review server — always-on, multi-run.
//!
//! A single long-lived HTTP server serves *every* run under the drovr data
//! dir. It presents a session-list landing view (`GET /api/runs`) and, per run,
//! the interactive spec-review surface: read the spec, diff it against the
//! prior version, answer MC questions, leave per-line annotations, and submit a
//! decision (approve / request-changes). The same run-scoped surface also
//! serves the code-review panel (`/api/runs/<run>/review/{findings,diff}`).
//!
//! The agent counterpart calls [`review_summary`] to POST the summary text for
//! its run, which flips that run's state from `waiting` → `ready`. The driver
//! calls [`review_wait`] to block until the reviewer acts.
//!
//! ## Discovery & lifecycle
//!
//! The server binds a fixed port (default 8791) and writes two global files in
//! the drovr data dir:
//!   * `server.addr` — the bound `host:port`
//!   * `server.pid`  — the daemon pid
//!
//! [`ensure_server`] reads `server.addr`; if it is missing or nothing is
//! listening, it spawns `drovr serve` as a detached background daemon and waits
//! for the socket to come up. [`review_summary`] / [`review_wait`] call it, so
//! the human never has to start the server by hand.
//!
//! ## Per-run state
//!
//! Each run's `{state, turn}` is persisted to `<run_dir>/review.state.json` so
//! the server is restart-safe and can serve many runs. The server keeps an
//! in-memory cache keyed by run name, lazily loaded from disk on first touch.

use std::collections::HashMap;
use std::fs;
use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

use crate::herdr::Herdr;
use crate::run::{RunState, data_dir, list_runs_in, runs_dir};

/// How often [`review_wait`] polls the live server for a reviewer decision.
/// Mirrors `phase::POLL_INTERVAL` — a filesystem/state poll, not a hot loop.
const POLL_INTERVAL: Duration = Duration::from_millis(500);

/// Default port for the always-on server. (The default *host* lives in config
/// as `serve_host`, resolved by `main::cmd_serve`.)
pub const DEFAULT_PORT: u16 = 8791;

/// Worker threads sharing the listening socket. `/review/diff` shells out to
/// git, so a single thread would head-of-line-block browsing; a small pool
/// keeps the list view responsive while a diff renders.
const WORKERS: usize = 4;

// ---------------------------------------------------------------------------
// Embedded assets (single-binary shape)
// ---------------------------------------------------------------------------

const INDEX_HTML: &str = include_str!("../web/index.html");
const MARKDOWN_IT_JS: &[u8] = include_bytes!("../web/vendor/markdown-it.min.js");
const PIERRE_DIFFS_JS: &[u8] = include_bytes!("../web/vendor/pierre-diffs.js");

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
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

    fn from_str(s: &str) -> LoopState {
        match s {
            "waiting" => LoopState::Waiting,
            "ready" => LoopState::Ready,
            "approved" => LoopState::Approved,
            _ => LoopState::Idle,
        }
    }
}

/// A run's durable review state, persisted to `<run_dir>/review.state.json`.
#[derive(Debug, Clone, Copy)]
struct ReviewState {
    state: LoopState,
    turn: u32,
}

impl ReviewState {
    fn idle() -> Self {
        ReviewState {
            state: LoopState::Idle,
            turn: 0,
        }
    }

    /// Load from `review.state.json`; a missing/garbled file is treated as a
    /// fresh `idle` run (the server may start after `drovr new` but before the
    /// run ever enters review).
    fn load(path: &Path) -> Self {
        match fs::read_to_string(path) {
            Ok(s) => {
                let v: serde_json::Value = serde_json::from_str(&s).unwrap_or_default();
                let state = LoopState::from_str(v["state"].as_str().unwrap_or("idle"));
                let turn = v["turn"].as_u64().unwrap_or(0) as u32;
                ReviewState { state, turn }
            }
            Err(_) => ReviewState::idle(),
        }
    }

    fn save(&self, path: &Path) -> io::Result<()> {
        fs::write(
            path,
            format!(
                r#"{{"state":"{}","turn":{}}}"#,
                self.state.as_str(),
                self.turn
            ),
        )
    }
}

/// The set of paths for one run's review artifacts, rooted at its run dir.
struct RunPaths {
    dir: PathBuf,
}

impl RunPaths {
    fn new(dir: PathBuf) -> Self {
        RunPaths { dir }
    }
    fn spec(&self) -> PathBuf {
        self.dir.join("spec.md")
    }
    fn feedback(&self) -> PathBuf {
        self.dir.join("feedback.json")
    }
    fn prior(&self) -> PathBuf {
        self.dir.join("prior.md")
    }
    /// See the per-revision re-baseline note in [`handle_post_summary`].
    fn last_summarized(&self) -> PathBuf {
        self.dir.join("last_summarized.md")
    }
    fn summary(&self) -> PathBuf {
        self.dir.join("summary.txt")
    }
    fn approved(&self) -> PathBuf {
        self.dir.join("approved")
    }
    fn questions(&self) -> PathBuf {
        self.dir.join("questions.json")
    }
    fn review_state(&self) -> PathBuf {
        self.dir.join("review.state.json")
    }
}

/// Server context shared across worker threads. Holds the runs root and a map
/// of **per-run** state cells. The outer `Mutex` guards only the map (held
/// briefly); each run's read-modify-write — including its disk writes — happens
/// under that run's OWN inner `Mutex`, so a slow write on run A never blocks
/// reads of run B. Locks recover from poisoning (a prior panic) rather than
/// propagating it: an always-on server must not wedge on one bad request.
struct Ctx {
    runs_root: PathBuf,
    cells: Mutex<HashMap<String, Arc<Mutex<ReviewState>>>>,
}

impl Ctx {
    fn new(runs_root: PathBuf) -> Self {
        Ctx {
            runs_root,
            cells: Mutex::new(HashMap::new()),
        }
    }

    fn paths(&self, run: &str) -> RunPaths {
        RunPaths::new(self.runs_root.join(run))
    }

    /// The per-run state cell, lazily loaded from `review.state.json` on first
    /// touch. The brief outer-map lock recovers from poisoning.
    fn cell(&self, run: &str, p: &RunPaths) -> Arc<Mutex<ReviewState>> {
        let mut map = self.cells.lock().unwrap_or_else(|e| e.into_inner());
        map.entry(run.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(ReviewState::load(&p.review_state()))))
            .clone()
    }

    /// Current review state for `run` (cache hit, else lazy-load from disk).
    fn state_of(&self, run: &str) -> ReviewState {
        let p = self.paths(run);
        let cell = self.cell(run, &p);
        let guard = cell.lock().unwrap_or_else(|e| e.into_inner());
        *guard
    }
}

/// Parse a run's `state.json` from an explicit run dir (dir-relative, unlike
/// `RunState::load` which hardcodes the global run dir). Returns `None` if the
/// file is absent or unparseable.
fn load_run_state(dir: &Path) -> Option<RunState> {
    let s = fs::read_to_string(dir.join("state.json")).ok()?;
    serde_json::from_str(&s).ok()
}

// ---------------------------------------------------------------------------
// Discovery-file paths
// ---------------------------------------------------------------------------

fn server_addr_file() -> PathBuf {
    data_dir().join("server.addr")
}

fn server_pid_file() -> PathBuf {
    data_dir().join("server.pid")
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

/// Like `respond_bytes` but marks the body immutable/cacheable. Used for the
/// embedded vendor assets (identical for the life of the binary) so the browser
/// doesn't re-download them — notably `pierre-diffs.js` is multi-MB and
/// `no-store` would re-fetch it on every page load.
fn respond_bytes_cached(req: Request, content_type: &str, body: Vec<u8>) {
    let resp = Response::from_data(body)
        .with_status_code(StatusCode(200))
        .with_header(header("Content-Type", content_type))
        .with_header(header("Cache-Control", "public, max-age=31536000, immutable"));
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

/// Cap on a POST body. tiny_http bounds by Content-Length, but a chunked body
/// with no length would otherwise read unbounded → OOM. Summaries/decisions are
/// tiny; 16 MiB is comfortably generous.
const MAX_BODY_BYTES: u64 = 16 * 1024 * 1024;

fn read_body(req: &mut Request) -> String {
    let mut buf = String::new();
    let _ = req.as_reader().take(MAX_BODY_BYTES).read_to_string(&mut buf);
    buf
}

/// Extract a query-string parameter by key from a raw request URL
/// (e.g. `/…/findings?task=t` → `task` → `"t"`). No percent-decoding: task
/// labels are plain filename components (validated by [`safe_component`]).
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

/// A `run` or `task` is safe as a single filename component: non-empty and free
/// of path separators, traversal, or a null byte. Mirrors `main::validate_label`.
fn safe_component(s: &str) -> bool {
    !s.is_empty()
        && s != "."
        && !s.contains('/')
        && !s.contains('\\')
        && !s.contains("..")
        && !s.contains('\0')
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

/// Split `/api/runs/<run>/<sub...>` into `(run, sub)`. `sub` is `""` for the
/// bare `/api/runs/<run>`. Returns `None` when the path isn't run-scoped.
fn parse_run_path(path: &str) -> Option<(&str, &str)> {
    let rest = path.strip_prefix("/api/runs/")?;
    Some(match rest.split_once('/') {
        Some((run, sub)) => (run, sub),
        None => (rest, ""),
    })
}

fn handle(req: Request, ctx: &Arc<Ctx>) {
    let method = req.method().clone();
    let url = req.url().to_string();
    let path = url.split('?').next().unwrap_or("/").to_string();

    // GET / — serve embedded index.html (the SPA: list view + run detail)
    if method == Method::Get && path == "/" {
        respond_bytes(
            req,
            200,
            "text/html; charset=utf-8",
            INDEX_HTML.as_bytes().to_vec(),
        );
        return;
    }

    // GET /health — liveness probe for `ensure_server`.
    if method == Method::Get && path == "/health" {
        respond_str(req, 200, "text/plain", "ok".into());
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
                respond_bytes_cached(
                    req,
                    "application/javascript; charset=utf-8",
                    MARKDOWN_IT_JS.to_vec(),
                );
            }
            "vendor/pierre-diffs.js" => {
                respond_bytes_cached(
                    req,
                    "application/javascript; charset=utf-8",
                    PIERRE_DIFFS_JS.to_vec(),
                );
            }
            other => {
                let ct = content_type_for(other);
                respond_str(req, 404, ct, "not found".into());
            }
        }
        return;
    }

    // GET /api/runs — the session list view.
    if method == Method::Get && path == "/api/runs" {
        respond_str(req, 200, "application/json", list_runs_json(ctx));
        return;
    }

    // Everything else is run-scoped: /api/runs/<run>/<sub>.
    if let Some((run, sub)) = parse_run_path(&path) {
        if !safe_component(run) {
            respond_str(req, 400, "text/plain", "invalid run".into());
            return;
        }
        handle_run(req, ctx, method, &url, run, sub);
        return;
    }

    respond_404(req);
}

/// Dispatch a run-scoped request. `sub` is the path after `/api/runs/<run>/`.
fn handle_run(req: Request, ctx: &Arc<Ctx>, method: Method, url: &str, run: &str, sub: &str) {
    let p = ctx.paths(run);

    match (&method, sub) {
        // GET state — JSON {state, turn}
        (Method::Get, "state") => {
            let rs = ctx.state_of(run);
            respond_str(
                req,
                200,
                "application/json",
                format!(r#"{{"state":"{}","turn":{}}}"#, rs.state.as_str(), rs.turn),
            );
        }

        // GET doc — raw spec markdown (graceful fallback if absent)
        (Method::Get, "doc") => match fs::read(p.spec()) {
            Ok(bytes) => respond_bytes(req, 200, "text/markdown; charset=utf-8", bytes),
            Err(_) => respond_str(req, 200, "text/markdown; charset=utf-8", String::new()),
        },

        // GET prior — raw prior.md or 204 if none
        (Method::Get, "prior") => match fs::read(p.prior()) {
            Ok(bytes) if !bytes.is_empty() => {
                respond_bytes(req, 200, "text/markdown; charset=utf-8", bytes)
            }
            _ => respond_empty(req, 204),
        },

        // GET summary — raw summary.txt (or empty string)
        (Method::Get, "summary") => {
            let text = fs::read_to_string(p.summary()).unwrap_or_default();
            respond_str(req, 200, "text/plain; charset=utf-8", text);
        }

        // GET questions — questions.json (or empty array)
        (Method::Get, "questions") => {
            let body = fs::read_to_string(p.questions()).unwrap_or_else(|_| "[]".into());
            respond_str(req, 200, "application/json", body);
        }

        // GET review/findings?task=<task> — merged <task>-review.json (or {}).
        (Method::Get, "review/findings") => {
            let task = query_param(url, "task").unwrap_or_default();
            if !safe_component(&task) {
                respond_str(req, 400, "text/plain", "invalid task".into());
                return;
            }
            let file = p.dir.join(format!("{task}-review.json"));
            let body = fs::read_to_string(&file).unwrap_or_else(|_| "{}".into());
            respond_str(req, 200, "application/json", body);
        }

        // GET review/diff?task=<task> — unified `git diff <base>..HEAD`.
        (Method::Get, "review/diff") => handle_review_diff(req, &p, url),

        // POST submit — reviewer decision.
        (Method::Post, "submit") => handle_post_submit(req, ctx, run, &p),

        // POST summary — agent posts a summary; flips state → ready.
        (Method::Post, "summary") => handle_post_summary(req, ctx, run, &p),

        // GET pane — a snapshot of the run's live agent session (herdr read).
        (Method::Get, "pane") => handle_get_pane(req, &p),

        // POST send — type text into the run's live agent pane (herdr prompt).
        (Method::Post, "send") => handle_post_send(req, &p),

        _ => respond_404(req),
    }
}

// ---------------------------------------------------------------------------
// Live session mirror (herdr read / prompt)
// ---------------------------------------------------------------------------

/// The pane that is this run's live agent session: the first phase still
/// `Running` (with a pane), else the workspace root pane. `None` when the run
/// has no live pane to mirror.
fn active_pane(run: &RunState) -> Option<String> {
    run.phases
        .iter()
        .find(|ph| ph.status == crate::run::PhaseStatus::Running && ph.pane_id.is_some())
        .and_then(|ph| ph.pane_id.clone())
        .or_else(|| run.root_pane.clone())
}

/// `GET /api/runs/<run>/pane` — the recent transcript of the run's live agent
/// session, as plain text (204 when there is no live pane to read).
fn handle_get_pane(req: Request, p: &RunPaths) {
    let Some(run) = load_run_state(&p.dir) else {
        respond_empty(req, 204);
        return;
    };
    let Some(pane) = active_pane(&run) else {
        respond_empty(req, 204);
        return;
    };
    let h = crate::herdr::SystemHerdr::new();
    match h.agent_read(&pane) {
        Ok(text) => respond_str(req, 200, "text/plain; charset=utf-8", text),
        Err(e) => {
            eprintln!("drovr pane: read of {pane} failed: {e}");
            respond_empty(req, 204);
        }
    }
}

/// `POST /api/runs/<run>/send` — type the request body into the run's live
/// agent pane (herdr submits it). 409 when there is no live pane.
fn handle_post_send(mut req: Request, p: &RunPaths) {
    let text = read_body(&mut req);
    let Some(run) = load_run_state(&p.dir) else {
        respond_str(req, 409, "text/plain", "no run".into());
        return;
    };
    let Some(pane) = active_pane(&run) else {
        respond_str(req, 409, "text/plain", "no live pane for this run".into());
        return;
    };
    let h = crate::herdr::SystemHerdr::new();
    match h.agent_send(&pane, &text) {
        Ok(()) => respond_str(req, 200, "application/json", r#"{"ok":true}"#.into()),
        Err(e) => {
            eprintln!("drovr send: to {pane} failed: {e}");
            respond_str(req, 500, "text/plain", "send failed".into())
        }
    }
}

/// `GET /api/runs/<run>/review/diff?task=<task>`: unified `git diff
/// <base>..HEAD`, base from `<run_dir>/<task>-base.sha`, run against the run's
/// recorded `project_dir`. 204 when the base SHA / project dir are unavailable.
fn handle_review_diff(req: Request, p: &RunPaths, url: &str) {
    let task = query_param(url, "task").unwrap_or_default();
    if !safe_component(&task) {
        respond_str(req, 400, "text/plain", "invalid task".into());
        return;
    }
    let base_file = p.dir.join(format!("{task}-base.sha"));
    let base = match fs::read_to_string(&base_file) {
        Ok(s) if safe_sha(s.trim()) => s.trim().to_string(),
        // Absent, empty, or a non-SHA value (a compromised/buggy writer could
        // slip a git rev-arg or flag in) → no diff rather than trusting it.
        _ => {
            respond_empty(req, 204);
            return;
        }
    };
    let project_dir = load_run_state(&p.dir)
        .map(|s| s.project_dir)
        .unwrap_or_default();
    if project_dir.is_empty() {
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
}

/// `POST /api/runs/<run>/submit` — reviewer approve / request-changes.
///
/// Holds this run's OWN state lock across the handler so its read-modify-write
/// of state + the prior.md snapshot are atomic w.r.t. a concurrent `POST
/// summary` on the same run. Other runs are unaffected (their cells are
/// independent). `feedback.json` is written before the state flips to
/// `waiting`, so a driver that observes `waiting` always finds the turn.
fn handle_post_submit(mut req: Request, ctx: &Arc<Ctx>, run: &str, p: &RunPaths) {
    let body = read_body(&mut req);
    let parsed: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
    let decision = parsed["decision"].as_str().unwrap_or("");
    let feedback = parsed["feedback"].as_str().unwrap_or("").to_string();
    let answers = parsed
        .get("answers")
        .cloned()
        .unwrap_or(serde_json::json!({}));
    let annotations = parsed
        .get("annotations")
        .cloned()
        .unwrap_or(serde_json::json!([]));

    let cell = ctx.cell(run, p);
    let mut rs = cell.lock().unwrap_or_else(|e| e.into_inner());

    if decision == "approve" {
        let _ = fs::write(p.approved(), b"approved\n");
        rs.state = LoopState::Approved;
        let _ = rs.save(&p.review_state());
        drop(rs);
        respond_str(
            req,
            200,
            "application/json",
            r#"{"ok":true,"state":"approved"}"#.into(),
        );
    } else {
        // Snapshot the submitted spec as BOTH prior.md and last_summarized.md.
        // The reviewer's next turn diffs from the version they just acted on, so
        // prior = current spec. We also re-anchor last_summarized to the same
        // value: without it, the next `/summary` would promote a stale
        // last_summarized over this prior.
        let spec_bytes = fs::read(p.spec()).unwrap_or_default();
        if let Err(e) = fs::write(p.prior(), &spec_bytes) {
            eprintln!("drovr review: failed to snapshot prior.md: {e}");
        }
        if let Err(e) = fs::write(p.last_summarized(), &spec_bytes) {
            eprintln!("drovr review: failed to snapshot last_summarized.md: {e}");
        }

        rs.turn += 1;
        let turn = rs.turn;

        let fb_json = serde_json::json!({
            "turn": turn,
            "decision": decision,
            "feedback": feedback,
            "answers": answers,
            "annotations": annotations,
        });
        let _ = fs::write(p.feedback(), fb_json.to_string());
        rs.state = LoopState::Waiting;
        let _ = rs.save(&p.review_state());
        drop(rs);
        respond_str(
            req,
            200,
            "application/json",
            r#"{"ok":true,"state":"waiting"}"#.into(),
        );
    }
}

/// `POST /api/runs/<run>/summary` — agent posts a change summary; state → ready.
fn handle_post_summary(mut req: Request, ctx: &Arc<Ctx>, run: &str, p: &RunPaths) {
    let body = read_body(&mut req);

    let cell = ctx.cell(run, p);
    let mut rs = cell.lock().unwrap_or_else(|e| e.into_inner());

    // Re-baseline the diff per revision. The reviewer's diff is (current spec)
    // vs prior.md; we want each revision to diff against the *previous*
    // revision, not the accumulated change since the last reviewer submit. The
    // agent overwrites spec.md before calling `review summary`, so we can't read
    // the pre-revision spec here — instead the server keeps a rolling copy in
    // last_summarized.md: promote it to prior.md, then re-snapshot the
    // now-current spec as the baseline for the next revision.
    match fs::read(p.last_summarized()) {
        Ok(prev) if !prev.is_empty() => {
            if let Err(e) = fs::write(p.prior(), &prev) {
                eprintln!("drovr review: failed to write prior.md: {e}");
            }
        }
        _ => {}
    }
    // spec unreadable → keep the existing last_summarized baseline.
    if let Ok(current) = fs::read(p.spec()) {
        let refreshed = fs::write(p.last_summarized(), &current);
        if let Err(e) = refreshed {
            eprintln!("drovr review: failed to write last_summarized.md: {e}");
        }
    }

    let _ = fs::write(p.summary(), body.as_bytes());
    rs.state = LoopState::Ready;
    let _ = rs.save(&p.review_state());
    drop(rs);
    respond_str(
        req,
        200,
        "application/json",
        r#"{"ok":true,"state":"ready"}"#.into(),
    );
}

/// Build the `GET /api/runs` JSON payload: one object per run, newest first.
fn list_runs_json(ctx: &Arc<Ctx>) -> String {
    let mut rows: Vec<(u64, serde_json::Value)> = Vec::new();
    for name in list_runs_in(&ctx.runs_root) {
        let dir = ctx.runs_root.join(&name);
        let rs = ctx.state_of(&name);
        let run_state = load_run_state(&dir);
        let (task, gate) = run_state
            .as_ref()
            .map(|s| (s.task.clone(), s.gate.clone()))
            .unwrap_or_default();
        // Sort key: most-recently-touched review artifact (fall back to 0).
        let updated = fs::metadata(dir.join("review.state.json"))
            .or_else(|_| fs::metadata(dir.join("state.json")))
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        rows.push((
            updated,
            serde_json::json!({
                "name": name,
                "task": task,
                "gate": gate,
                "state": rs.state.as_str(),
                "turn": rs.turn,
                "updated": updated,
            }),
        ));
    }
    rows.sort_by_key(|r| std::cmp::Reverse(r.0));
    let arr: Vec<serde_json::Value> = rows.into_iter().map(|(_, v)| v).collect();
    serde_json::Value::Array(arr).to_string()
}

// ---------------------------------------------------------------------------
// Public API — the daemon
// ---------------------------------------------------------------------------

/// Start the always-on review server, serving every run under the drovr data
/// dir. Writes `server.addr` and `server.pid` immediately after the socket
/// opens, then blocks serving requests until the process exits.
pub fn serve(host: &str, port: u16) -> io::Result<()> {
    let root = runs_dir();
    fs::create_dir_all(&root)?;
    // Ensure the data dir exists for the discovery files even before any run.
    fs::create_dir_all(data_dir())?;
    // Tighten perms: the data dir holds every run's spec/diff/feedback and the
    // discovery files. 0700 keeps other local users off (loopback is shared).
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(data_dir(), fs::Permissions::from_mode(0o700));
    }

    let addr = format!("{host}:{port}");
    let server = Server::http(&addr).map_err(|e| io::Error::other(e.to_string()))?;

    let bound_addr = server
        .server_addr()
        .to_ip()
        .map(|a| a.to_string())
        .unwrap_or_else(|| addr.clone());
    fs::write(server_addr_file(), bound_addr.as_bytes())?;
    fs::write(server_pid_file(), std::process::id().to_string().as_bytes())?;

    eprintln!("drovr review server listening on http://{bound_addr}");
    eprintln!("  runs: {root:?}");

    let ctx = Arc::new(Ctx::new(root));
    let server = Arc::new(server);

    let mut handles = Vec::with_capacity(WORKERS);
    for _ in 0..WORKERS {
        let server = Arc::clone(&server);
        let ctx = Arc::clone(&ctx);
        handles.push(thread::spawn(move || {
            // recv() is multi-consumer safe on a shared tiny_http server.
            while let Ok(req) = server.recv() {
                // Isolate a panicking request: drop the connection and keep the
                // worker alive rather than shrinking the pool. Poisoned per-run
                // locks are separately recovered in `Ctx::cell`.
                let ctx = &ctx;
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    handle(req, ctx);
                }));
            }
        }));
    }
    for h in handles {
        let _ = h.join();
    }
    Ok(())
}

/// Ensure the always-on server is running and return its `host:port`.
///
/// Reuses a live server (from `server.addr`) if one is reachable; otherwise
/// spawns `drovr serve` as a detached background daemon and waits (bounded) for
/// it to come up. This is what lets [`review_summary`] / [`review_wait`] work
/// without the human starting a server by hand.
pub fn ensure_server() -> io::Result<String> {
    if let Some(addr) = live_server_addr() {
        return Ok(addr);
    }
    // Test seam: with DROVR_NO_SPAWN set, don't fork a daemon (spawning the test
    // binary with a `serve` arg would re-enter the harness). Just report down.
    if std::env::var_os("DROVR_NO_SPAWN").is_some() {
        return Err(io::Error::other("drovr review server is not running"));
    }
    spawn_daemon()?;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(addr) = live_server_addr() {
            return Ok(addr);
        }
        if Instant::now() >= deadline {
            return Err(io::Error::other(
                "timed out waiting for `drovr serve` to start",
            ));
        }
        thread::sleep(Duration::from_millis(50));
    }
}

/// The bound address if `server.addr` names a reachable server, else `None`.
///
/// Resolves via `ToSocketAddrs` so a `serve_host` like `localhost` (which
/// tiny_http writes to `server.addr` verbatim) still connects, not just bare
/// IP literals.
fn live_server_addr() -> Option<String> {
    use std::net::ToSocketAddrs;
    let addr = fs::read_to_string(server_addr_file()).ok()?;
    let addr = addr.trim().to_string();
    if addr.is_empty() {
        return None;
    }
    let sockaddr = addr.to_socket_addrs().ok()?.next()?;
    TcpStream::connect_timeout(&sockaddr, Duration::from_millis(500))
        .ok()
        .map(|_| addr)
}

/// Spawn `drovr serve` detached, so it outlives the invoking CLI process.
fn spawn_daemon() -> io::Result<()> {
    let exe = std::env::current_exe()?;
    let mut cmd = Command::new(exe);
    cmd.arg("serve")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // New process group → survives the parent CLI exiting / its terminal.
        cmd.process_group(0);
    }
    cmd.spawn()?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Public API — agent/driver coordination
// ---------------------------------------------------------------------------

/// POST summary text to the running review server for `run` (`drovr review
/// summary`). Ensures the server is up, then POSTs to
/// `/api/runs/<run>/summary`, flipping that run's state to `ready`.
pub fn review_summary(run: &str, text: &str) -> io::Result<()> {
    let addr = ensure_server()?;

    let body = text.as_bytes();
    let path = format!("/api/runs/{run}/summary");
    let request = format!(
        "POST {path} HTTP/1.0\r\nHost: {addr}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\n\r\n",
        body.len()
    );

    let mut stream = TcpStream::connect(&addr).map_err(|e| {
        io::Error::new(
            e.kind(),
            format!("could not connect to review server at {addr}: {e}"),
        )
    })?;
    stream.write_all(request.as_bytes())?;
    stream.write_all(body)?;

    let mut response = String::new();
    let _ = stream.read_to_string(&mut response);
    if !response.starts_with("HTTP/1") || !response.contains(" 200 ") {
        return Err(io::Error::other(format!(
            "unexpected response from review server: {}",
            response.lines().next().unwrap_or("")
        )));
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

/// GET `/api/runs/<run>/state` from the live server, returning the `state` str.
fn fetch_state(addr: &str, run: &str) -> io::Result<String> {
    let mut stream = TcpStream::connect(addr).map_err(|e| {
        io::Error::new(
            e.kind(),
            format!("could not connect to review server at {addr}: {e}"),
        )
    })?;
    write!(
        stream,
        "GET /api/runs/{run}/state HTTP/1.0\r\nHost: {addr}\r\n\r\n"
    )?;
    let mut resp = String::new();
    stream.read_to_string(&mut resp)?;
    let body = resp.split_once("\r\n\r\n").map(|x| x.1).unwrap_or("");
    let parsed: serde_json::Value = serde_json::from_str(body.trim()).unwrap_or_default();
    parsed["state"]
        .as_str()
        .map(|s| s.to_owned())
        .ok_or_else(|| io::Error::other(format!("malformed /state response from {addr}: {body:?}")))
}

/// Block until the reviewer acts on `run`'s spec gate, then return the outcome.
///
/// Ensures the server is up, then polls the authoritative `GET
/// /api/runs/<run>/state` at [`POLL_INTERVAL`] until either the reviewer
/// submits or `timeout_ms` elapses. Blocks while state is `idle`/`ready`;
/// returns [`WaitOutcome::Approved`] once approved and
/// [`WaitOutcome::ChangesRequested`] once changes are requested
/// (`feedback.json` holds the turn). On timeout returns [`WaitOutcome::Timeout`]
/// — the wait is resumable, so a driver just re-runs it.
pub fn review_wait(run: &str, timeout_ms: u64) -> io::Result<WaitOutcome> {
    let addr = ensure_server()?;

    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    loop {
        match fetch_state(&addr, run)?.as_str() {
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

    /// Start an always-on server on 127.0.0.1:0 in a background thread, rooted
    /// at `runs_root`. Returns the bound address string.
    fn start_server(runs_root: PathBuf) -> String {
        let server = Server::http("127.0.0.1:0").expect("bind");
        let bound = server.server_addr().to_ip().expect("ip addr").to_string();
        let ctx = Arc::new(Ctx::new(runs_root));
        let server = Arc::new(server);
        for _ in 0..2 {
            let server = Arc::clone(&server);
            let ctx = Arc::clone(&ctx);
            thread::spawn(move || {
                while let Ok(req) = server.recv() {
                    handle(req, &ctx);
                }
            });
        }
        thread::sleep(Duration::from_millis(10));
        bound
    }

    /// Create a run dir under `root`; return its path. Writes a minimal
    /// `state.json` so the run is discoverable by `/api/runs`.
    fn make_run(root: &Path, run: &str, spec: &[u8]) -> PathBuf {
        let dir = root.join(run);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("spec.md"), spec).unwrap();
        fs::write(
            dir.join("state.json"),
            format!(
                r#"{{"name":"{run}","task":"t","phases":[],"gate":"spec","cursor":0,"project_dir":""}}"#
            ),
        )
        .unwrap();
        dir
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

    fn make_root(suffix: &str) -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix(&format!("drovr-review-test-{suffix}"))
            .tempdir()
            .expect("tempdir")
    }

    #[test]
    fn state_starts_idle() {
        let tmp = make_root("idle");
        make_run(tmp.path(), "r", b"# Spec");
        let addr = start_server(tmp.path().to_path_buf());

        let (status, body) = http_get(&addr, "/api/runs/r/state");
        assert_eq!(status, 200);
        assert!(body.contains(r#""state":"idle""#), "body={body}");
        assert!(body.contains(r#""turn":0"#), "body={body}");
    }

    #[test]
    fn api_runs_lists_runs() {
        let tmp = make_root("list");
        make_run(tmp.path(), "alpha", b"# A");
        make_run(tmp.path(), "beta", b"# B");
        let addr = start_server(tmp.path().to_path_buf());

        let (status, body) = http_get(&addr, "/api/runs");
        assert_eq!(status, 200);
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        let names: Vec<&str> = v
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"alpha"), "body={body}");
        assert!(names.contains(&"beta"), "body={body}");
        assert_eq!(v[0]["state"], "idle");
    }

    #[test]
    fn active_pane_prefers_running_phase_then_root() {
        let mkphase = |name: &str, status, pane: Option<&str>| crate::run::Phase {
            name: name.into(),
            status,
            handoff_doc: None,
            herdr_session: None,
            pane_id: pane.map(|s| s.to_string()),
        };
        let mut run = RunState {
            name: "r".into(),
            task: "t".into(),
            agent: Some("claude".into()),
            phases: vec![
                mkphase("brainstorm", crate::run::PhaseStatus::Done, Some("w:p1")),
                mkphase("implement", crate::run::PhaseStatus::Running, Some("w:p2")),
            ],
            review_phases: vec![],
            gate: "spec".into(),
            cursor: 0,
            workspace: Some("w".into()),
            root_pane: Some("w:root".into()),
            project_dir: "/tmp/p".into(),
            worktree_path: None,
            worktree_branch: None,
        };
        // Running phase wins.
        assert_eq!(active_pane(&run).as_deref(), Some("w:p2"));
        // No running phase → falls back to the workspace root pane.
        run.phases[1].status = crate::run::PhaseStatus::Done;
        assert_eq!(active_pane(&run).as_deref(), Some("w:root"));
        // Neither → None.
        run.root_pane = None;
        assert_eq!(active_pane(&run), None);
    }

    #[test]
    fn runs_are_isolated() {
        // The central always-on claim: a POST to run A must not touch run B.
        let tmp = make_root("isolation");
        make_run(tmp.path(), "a", b"# A");
        make_run(tmp.path(), "b", b"# B");
        let addr = start_server(tmp.path().to_path_buf());

        let (s, _) = http_post(&addr, "/api/runs/a/summary", "text/plain", "changed A");
        assert_eq!(s, 200);

        let (_, sa) = http_get(&addr, "/api/runs/a/state");
        let (_, sb) = http_get(&addr, "/api/runs/b/state");
        assert!(sa.contains(r#""state":"ready""#), "A should be ready: {sa}");
        assert!(sb.contains(r#""state":"idle""#), "B must stay idle: {sb}");
    }

    #[test]
    fn api_runs_sorted_newest_first() {
        let tmp = make_root("sort");
        let a = make_run(tmp.path(), "alpha", b"# A");
        let b = make_run(tmp.path(), "beta", b"# B");
        // Give each a review.state.json with explicit, distinct mtimes so the
        // sort-by-most-recent is deterministic (second granularity otherwise ties).
        fs::write(a.join("review.state.json"), r#"{"state":"idle","turn":0}"#).unwrap();
        fs::write(b.join("review.state.json"), r#"{"state":"idle","turn":0}"#).unwrap();
        let older = std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        let newer = std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(2_000_000);
        fs::File::options()
            .write(true)
            .open(a.join("review.state.json"))
            .unwrap()
            .set_modified(older)
            .unwrap();
        fs::File::options()
            .write(true)
            .open(b.join("review.state.json"))
            .unwrap()
            .set_modified(newer)
            .unwrap();

        let addr = start_server(tmp.path().to_path_buf());
        let (status, body) = http_get(&addr, "/api/runs");
        assert_eq!(status, 200);
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v[0]["name"], "beta", "newest first: {body}");
        assert_eq!(v[1]["name"], "alpha", "oldest last: {body}");
    }

    #[test]
    fn post_summary_flips_to_ready_and_persists() {
        let tmp = make_root("summary");
        let dir = make_run(tmp.path(), "r", b"# Spec");
        let addr = start_server(tmp.path().to_path_buf());

        let (status, body) =
            http_post(&addr, "/api/runs/r/summary", "text/plain", "Initial summary.");
        assert_eq!(status, 200, "body={body}");
        assert!(body.contains(r#""state":"ready""#), "body={body}");

        let (_, body2) = http_get(&addr, "/api/runs/r/state");
        assert!(body2.contains(r#""state":"ready""#), "body2={body2}");

        // State is persisted to disk (restart-safe).
        let persisted = fs::read_to_string(dir.join("review.state.json")).unwrap();
        assert!(persisted.contains(r#""state":"ready""#), "{persisted}");
    }

    #[test]
    fn state_reloads_from_disk() {
        // A ReviewState written by a prior server instance is picked up by a new
        // Ctx (lazy-load) — proves the always-on server is restart-safe.
        let tmp = make_root("reload");
        let dir = make_run(tmp.path(), "r", b"# Spec");
        fs::write(
            dir.join("review.state.json"),
            r#"{"state":"waiting","turn":4}"#,
        )
        .unwrap();
        let addr = start_server(tmp.path().to_path_buf());

        let (status, body) = http_get(&addr, "/api/runs/r/state");
        assert_eq!(status, 200);
        assert!(body.contains(r#""state":"waiting""#), "body={body}");
        assert!(body.contains(r#""turn":4"#), "body={body}");
    }

    #[test]
    fn summary_rebaselines_prior_per_revision() {
        let tmp = make_root("rebaseline");
        let dir = make_run(tmp.path(), "r", b"v1");
        let addr = start_server(tmp.path().to_path_buf());

        let (s1, _) = http_post(&addr, "/api/runs/r/summary", "text/plain", "to v1");
        assert_eq!(s1, 200);
        let (ps, _) = http_get(&addr, "/api/runs/r/prior");
        assert_eq!(ps, 204, "no prior before a second revision exists");

        fs::write(dir.join("spec.md"), b"v2").unwrap();
        let (s2, _) = http_post(&addr, "/api/runs/r/summary", "text/plain", "to v2");
        assert_eq!(s2, 200);
        let (ps2, body2) = http_get(&addr, "/api/runs/r/prior");
        assert_eq!(ps2, 200);
        assert_eq!(body2, "v1", "prior after v2 must be v1");

        fs::write(dir.join("spec.md"), b"v3").unwrap();
        let (s3, _) = http_post(&addr, "/api/runs/r/summary", "text/plain", "to v3");
        assert_eq!(s3, 200);
        let (ps3, body3) = http_get(&addr, "/api/runs/r/prior");
        assert_eq!(ps3, 200);
        assert_eq!(body3, "v2", "prior after v3 must advance to v2");
    }

    #[test]
    fn submit_reanchors_baseline_for_next_revision() {
        let tmp = make_root("interleave");
        let dir = make_run(tmp.path(), "r", b"v1");
        let addr = start_server(tmp.path().to_path_buf());
        assert_eq!(
            http_post(&addr, "/api/runs/r/summary", "text/plain", "v1").0,
            200
        );

        fs::write(dir.join("spec.md"), b"v2").unwrap();
        assert_eq!(
            http_post(&addr, "/api/runs/r/summary", "text/plain", "v2").0,
            200
        );

        let payload =
            r#"{"decision":"request-changes","feedback":"x","answers":{},"annotations":[]}"#;
        assert_eq!(
            http_post(&addr, "/api/runs/r/submit", "application/json", payload).0,
            200
        );
        let prior_after_submit = fs::read(dir.join("prior.md")).unwrap();
        assert_eq!(prior_after_submit, b"v2");

        fs::write(dir.join("spec.md"), b"v3").unwrap();
        assert_eq!(
            http_post(&addr, "/api/runs/r/summary", "text/plain", "v3").0,
            200
        );
        let (ps, body) = http_get(&addr, "/api/runs/r/prior");
        assert_eq!(ps, 200);
        assert_eq!(body, "v2", "post-submit revision must diff against v2");
    }

    #[test]
    fn questions_empty_when_no_file() {
        let tmp = make_root("questions");
        make_run(tmp.path(), "r", b"# Spec");
        let addr = start_server(tmp.path().to_path_buf());

        let (status, body) = http_get(&addr, "/api/runs/r/questions");
        assert_eq!(status, 200);
        assert_eq!(body.trim(), "[]");
    }

    #[test]
    fn questions_served_when_file_present() {
        let tmp = make_root("questions-present");
        let dir = make_run(tmp.path(), "r", b"# Spec");
        let q_json = r#"[{"id":"q1","prompt":"Which?","options":[{"value":"a","label":"A"}]}]"#;
        fs::write(dir.join("questions.json"), q_json).unwrap();
        let addr = start_server(tmp.path().to_path_buf());

        let (status, body) = http_get(&addr, "/api/runs/r/questions");
        assert_eq!(status, 200);
        assert!(body.contains("q1"), "body={body}");
    }

    #[test]
    fn submit_request_changes_flips_waiting_and_writes_files() {
        let tmp = make_root("submit");
        let dir = make_run(tmp.path(), "r", b"# Spec\nSome content.");
        let addr = start_server(tmp.path().to_path_buf());

        let payload = r#"{"decision":"request-changes","feedback":"needs work","answers":{},"annotations":[]}"#;
        let (status, body) = http_post(&addr, "/api/runs/r/submit", "application/json", payload);
        assert_eq!(status, 200, "body={body}");
        assert!(body.contains(r#""state":"waiting""#), "body={body}");

        let (_, state_body) = http_get(&addr, "/api/runs/r/state");
        assert!(state_body.contains(r#""state":"waiting""#));
        assert!(state_body.contains(r#""turn":1"#));

        let prior = fs::read(dir.join("prior.md")).expect("prior.md");
        assert_eq!(prior, b"# Spec\nSome content.");

        let fb = fs::read_to_string(dir.join("feedback.json")).expect("feedback.json");
        let v: serde_json::Value = serde_json::from_str(&fb).unwrap();
        assert_eq!(v["turn"], 1);
        assert_eq!(v["decision"], "request-changes");
        assert_eq!(v["feedback"], "needs work");
    }

    #[test]
    fn submit_approve_writes_marker_and_flips_approved() {
        let tmp = make_root("approve");
        let dir = make_run(tmp.path(), "r", b"# Done");
        let addr = start_server(tmp.path().to_path_buf());

        let payload = r#"{"decision":"approve","feedback":"","answers":{},"annotations":[]}"#;
        let (status, body) = http_post(&addr, "/api/runs/r/submit", "application/json", payload);
        assert_eq!(status, 200, "body={body}");
        assert!(body.contains(r#""state":"approved""#), "body={body}");

        let approved = fs::read(dir.join("approved")).expect("approved marker");
        assert_eq!(approved, b"approved\n");

        let (_, state_body) = http_get(&addr, "/api/runs/r/state");
        assert!(state_body.contains(r#""state":"approved""#));
    }

    #[test]
    fn doc_graceful_when_spec_absent() {
        let tmp = make_root("no-spec");
        let dir = tmp.path().join("r");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("state.json"),
            r#"{"name":"r","task":"t","phases":[],"gate":"spec","cursor":0,"project_dir":""}"#,
        )
        .unwrap();
        let addr = start_server(tmp.path().to_path_buf());

        let (status, body) = http_get(&addr, "/api/runs/r/doc");
        assert_eq!(status, 200);
        assert_eq!(body.trim(), "");
    }

    #[test]
    fn prior_returns_204_when_absent() {
        let tmp = make_root("no-prior");
        make_run(tmp.path(), "r", b"# Spec");
        let addr = start_server(tmp.path().to_path_buf());

        let (status, _) = http_get(&addr, "/api/runs/r/prior");
        assert_eq!(status, 204);
    }

    #[test]
    fn index_html_served() {
        let tmp = make_root("index");
        make_run(tmp.path(), "r", b"# Spec");
        let addr = start_server(tmp.path().to_path_buf());

        let (status, body) = http_get(&addr, "/");
        assert_eq!(status, 200);
        assert!(body.contains("Drovr Review"), "title missing");
    }

    #[test]
    fn health_served() {
        let tmp = make_root("health");
        let addr = start_server(tmp.path().to_path_buf());
        let (status, body) = http_get(&addr, "/health");
        assert_eq!(status, 200);
        assert_eq!(body.trim(), "ok");
    }

    #[test]
    fn vendor_js_served() {
        let tmp = make_root("vendor");
        let addr = start_server(tmp.path().to_path_buf());

        let (status, _) = http_get(&addr, "/web/vendor/markdown-it.min.js");
        assert_eq!(status, 200);

        // The pierre-diffs bundle (client-side diff renderer) is served too.
        let (status, _) = http_get(&addr, "/web/vendor/pierre-diffs.js");
        assert_eq!(status, 200);
    }

    #[test]
    fn path_traversal_rejected() {
        let tmp = make_root("traversal");
        let addr = start_server(tmp.path().to_path_buf());

        let (status, _) = http_get(&addr, "/web/../etc/passwd");
        assert_eq!(status, 404);
    }

    #[test]
    fn invalid_run_rejected() {
        let tmp = make_root("bad-run");
        let addr = start_server(tmp.path().to_path_buf());
        // A backslash in the run component is rejected before any fs access.
        let (status, _) = http_get(&addr, r"/api/runs/a\b/state");
        assert_eq!(status, 400);
        // A bare "." (which would resolve to the runs root) is also rejected.
        let (status_dot, _) = http_get(&addr, "/api/runs/./state");
        assert_eq!(status_dot, 400);
    }

    // -- review_summary / review_wait over the global server -----------------

    /// Spin a server rooted at `<XDG_DATA_HOME>/drovr/runs`, write the global
    /// `server.addr`, and return (addr, run, tempdir). Caller holds ENV_LOCK.
    fn global_fixture(suffix: &str) -> (String, String, tempfile::TempDir) {
        let tmp = make_root(suffix);
        let base = tmp.path().to_path_buf();
        unsafe {
            std::env::set_var("XDG_DATA_HOME", base.to_str().unwrap());
        }
        let runs_root = base.join("drovr/runs");
        let run = format!("run-{suffix}");
        make_run(&runs_root, &run, b"# Spec");
        let addr = start_server(runs_root);
        fs::write(data_dir().join("server.addr"), addr.as_bytes()).unwrap();
        (addr, run, tmp)
    }

    #[test]
    fn review_summary_posts_to_server() {
        let _guard = crate::test_util::ENV_LOCK.lock().unwrap();
        let (_addr, run, tmp) = global_fixture("rev-summary");

        review_summary(&run, "Agent summary text.").expect("review_summary");

        let summary =
            fs::read_to_string(tmp.path().join("drovr/runs").join(&run).join("summary.txt"))
                .expect("summary.txt");
        assert_eq!(summary, "Agent summary text.");
    }

    #[test]
    fn wait_missing_server_errors() {
        let _guard = crate::test_util::ENV_LOCK.lock().unwrap();
        let tmp = make_root("wait-no-server");
        unsafe {
            std::env::set_var("XDG_DATA_HOME", tmp.path().to_str().unwrap());
            // Don't fork the test binary as a daemon; just prove "down" errors.
            std::env::set_var("DROVR_NO_SPAWN", "1");
        }
        let res = review_wait("nope", 100);
        unsafe {
            std::env::remove_var("DROVR_NO_SPAWN");
        }
        assert!(res.is_err(), "missing server must error");
    }

    #[test]
    fn wait_times_out_while_idle() {
        let _guard = crate::test_util::ENV_LOCK.lock().unwrap();
        let (_addr, run, _tmp) = global_fixture("idle-timeout");
        let outcome = review_wait(&run, 60).expect("wait");
        assert_eq!(outcome, WaitOutcome::Timeout);
    }

    #[test]
    fn wait_returns_approved() {
        let _guard = crate::test_util::ENV_LOCK.lock().unwrap();
        let (addr, run, _tmp) = global_fixture("approve");
        http_post(&addr, &format!("/api/runs/{run}/summary"), "text/plain", "go");

        let run_t = run.clone();
        let handle = thread::spawn(move || review_wait(&run_t, 10_000));
        thread::sleep(Duration::from_millis(200));
        assert!(!handle.is_finished(), "wait must block while `ready`");

        http_post(
            &addr,
            &format!("/api/runs/{run}/submit"),
            "application/json",
            r#"{"decision":"approve","feedback":"","answers":{},"annotations":[]}"#,
        );
        assert_eq!(handle.join().unwrap().expect("wait"), WaitOutcome::Approved);
    }

    #[test]
    fn wait_returns_changes_requested() {
        let _guard = crate::test_util::ENV_LOCK.lock().unwrap();
        let (addr, run, tmp) = global_fixture("changes");
        http_post(&addr, &format!("/api/runs/{run}/summary"), "text/plain", "go");

        let run_t = run.clone();
        let handle = thread::spawn(move || review_wait(&run_t, 10_000));
        thread::sleep(Duration::from_millis(200));
        assert!(!handle.is_finished(), "wait must block while `ready`");

        http_post(
            &addr,
            &format!("/api/runs/{run}/submit"),
            "application/json",
            r#"{"decision":"request-changes","feedback":"needs work","answers":{},"annotations":[]}"#,
        );
        assert_eq!(
            handle.join().unwrap().expect("wait"),
            WaitOutcome::ChangesRequested
        );

        let fb = fs::read_to_string(tmp.path().join("drovr/runs").join(&run).join("feedback.json"))
            .expect("feedback.json");
        assert!(fb.contains("needs work"), "feedback.json: {fb}");
    }

    // -- code-review surface (/review/findings, /review/diff) ----------------

    #[test]
    fn findings_served_when_file_present() {
        let tmp = make_root("findings-present");
        let dir = make_run(tmp.path(), "r", b"# Spec");
        let merged = r#"{"verdict":"changes","findings":[{"file":"a.rs","line":3,"severity":"important","angle":"correctness","summary":"off by one","rationale":"loop bound"}]}"#;
        fs::write(dir.join("t-review.json"), merged).unwrap();
        let addr = start_server(tmp.path().to_path_buf());

        let (status, body) = http_get(&addr, "/api/runs/r/review/findings?task=t");
        assert_eq!(status, 200);
        assert!(body.contains("off by one"), "body={body}");
    }

    #[test]
    fn findings_empty_when_absent() {
        let tmp = make_root("findings-absent");
        make_run(tmp.path(), "r", b"# Spec");
        let addr = start_server(tmp.path().to_path_buf());

        let (status, body) = http_get(&addr, "/api/runs/r/review/findings?task=t");
        assert_eq!(status, 200);
        assert_eq!(body.trim(), "{}");
    }

    #[test]
    fn findings_rejects_traversal_task() {
        let tmp = make_root("findings-traversal");
        make_run(tmp.path(), "r", b"# Spec");
        let addr = start_server(tmp.path().to_path_buf());

        let (status, _) = http_get(&addr, "/api/runs/r/review/findings?task=../etc/passwd");
        assert_eq!(status, 400);
    }

    #[test]
    fn diff_204_when_no_base() {
        let tmp = make_root("diff-no-base");
        make_run(tmp.path(), "r", b"# Spec");
        let addr = start_server(tmp.path().to_path_buf());

        let (status, _) = http_get(&addr, "/api/runs/r/review/diff?task=t");
        assert_eq!(status, 204);
    }

    #[test]
    fn diff_served_with_base_and_git_repo() {
        let tmp = make_root("diff-git");
        // A throwaway git repo as the run's project_dir.
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

        // Run dir with a state.json pointing project_dir at the repo.
        let runs_root = tmp.path().join("runs");
        let dir = runs_root.join("r");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("spec.md"), b"# Spec").unwrap();
        fs::write(
            dir.join("state.json"),
            format!(
                r#"{{"name":"r","task":"t","phases":[],"gate":"spec","cursor":0,"project_dir":"{}"}}"#,
                repo.display()
            ),
        )
        .unwrap();
        fs::write(dir.join("t-base.sha"), format!("{base}\n")).unwrap();

        let addr = start_server(runs_root);
        let (status, body) = http_get(&addr, "/api/runs/r/review/diff?task=t");
        assert_eq!(status, 200, "body={body}");
        assert!(body.contains("+two"), "diff should show added line: {body}");
    }

    #[test]
    fn diff_204_when_base_is_not_a_sha() {
        let tmp = make_root("diff-bad-sha");
        let dir = make_run(tmp.path(), "r", b"# Spec");
        fs::write(dir.join("t-base.sha"), "--no-index\n").unwrap();
        let addr = start_server(tmp.path().to_path_buf());

        let (status, _) = http_get(&addr, "/api/runs/r/review/diff?task=t");
        assert_eq!(status, 204);
    }

    #[test]
    fn diff_204_when_git_fails() {
        // A well-formed but unknown SHA → git exits non-zero → 204, not empty 200.
        let tmp = make_root("diff-git-fail");
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

        let runs_root = tmp.path().join("runs");
        let dir = runs_root.join("r");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("spec.md"), b"# Spec").unwrap();
        fs::write(
            dir.join("state.json"),
            format!(
                r#"{{"name":"r","task":"t","phases":[],"gate":"spec","cursor":0,"project_dir":"{}"}}"#,
                repo.display()
            ),
        )
        .unwrap();
        // 40 hex chars that are (almost certainly) not a real object → git errors.
        fs::write(dir.join("t-base.sha"), "0".repeat(40)).unwrap();

        let addr = start_server(runs_root);
        let (status, _) = http_get(&addr, "/api/runs/r/review/diff?task=t");
        assert_eq!(status, 204);
    }
}
