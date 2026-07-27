//! Review server — always-on, multi-run.
//!
//! A single long-lived HTTP server serves *every* run under the drovr data
//! dir. It presents a session-list landing view (`GET /api/runs`) and, per run,
//! the interactive spec-review surface: read the spec, diff it against the
//! prior version, answer MC questions, leave per-line annotations, and submit a
//! decision (approve / request-changes / cancel). The same run-scoped surface also
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
//!   * `server.pid`  — the daemon pid, and the file the single-server lock is on
//!
//! Exactly one server may serve a data dir: [`serve`] takes an exclusive lock on
//! `server.pid` for its lifetime ([`acquire_pid_lock`]) and a second invocation
//! refuses instead of starting, since two servers would fight over these two files
//! and each hold half the drivers. The lock is the whole test: a server that holds
//! no lock — one from a build predating it, or one whose lock file was deleted
//! underneath it — is not detected.
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
use std::fs::{File, OpenOptions, TryLockError};
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
    /// The human abandoned the run at the gate. Terminal, like `Approved`, but
    /// the opposite verdict — a driver must be able to tell them apart.
    Cancelled,
}

impl LoopState {
    fn as_str(&self) -> &'static str {
        match self {
            LoopState::Idle => "idle",
            LoopState::Waiting => "waiting",
            LoopState::Ready => "ready",
            LoopState::Approved => "approved",
            LoopState::Cancelled => "cancelled",
        }
    }

    fn from_str(s: &str) -> LoopState {
        match s {
            "waiting" => LoopState::Waiting,
            "ready" => LoopState::Ready,
            "approved" => LoopState::Approved,
            "cancelled" => LoopState::Cancelled,
            _ => LoopState::Idle,
        }
    }

    /// `Approved` and `Cancelled` are verdicts, not phases: once a human has
    /// decided, neither the agent nor a stray client may move the run off one.
    /// Without this a late `POST /summary` — very likely, since the agent is
    /// usually still mid-revision when the human cancels — would flip
    /// `cancelled` back to `ready` and silently erase the decision.
    fn is_terminal(&self) -> bool {
        matches!(self, LoopState::Approved | LoopState::Cancelled)
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
    fn cancelled(&self) -> PathBuf {
        self.dir.join("cancelled")
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

    // POST /api/runs — create a run and start its brainstorm agent (dogfood a
    // fresh drovr session straight from the browser).
    if method == Method::Post && path == "/api/runs" {
        handle_post_new_run(req);
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

        // GET agents — the tree of agents (phases + nested review panels).
        (Method::Get, "agents") => handle_get_agents(req, &p),

        // GET pane[?pane=<id>] — snapshot of a run agent's session (herdr read).
        (Method::Get, "pane") => handle_get_pane(req, &p, url),

        // POST send[?pane=<id>] — type text into a run agent's pane (herdr prompt).
        (Method::Post, "send") => handle_post_send(req, &p, url),

        // POST keys[?pane=<id>] — press keys in a run agent's pane, so the
        // browser can answer numbered/arrow menus that `send` cannot drive.
        (Method::Post, "keys") => handle_post_keys(req, &p, url),

        // POST rehydrate?phase=<name> — bring back a phase whose pane is gone.
        (Method::Post, "rehydrate") => handle_post_rehydrate(req, &p, run, url),

        _ => respond_404(req),
    }
}

// ---------------------------------------------------------------------------
// Live session mirror (herdr read / prompt)
// ---------------------------------------------------------------------------

/// The pane this run's live mirror follows — [`RunState::live_agent_pane`], the
/// same answer `drovr attach` gets. `None` when the run has no live agent pane,
/// which the endpoints render as 204 / "no live pane".
///
/// **Neither the workspace root pane nor an earlier phase's pane is a
/// fallback.** The root pane once was, but only reachably so before the first
/// `phase start`, because that phase then claimed it; phases now leave the idle
/// shell alone for the whole run, so falling back would point the mirror at an
/// `sh` prompt and present it as the run's agent. The earlier-phase fallback is
/// refused for the reason spelled out on `live_agent_pane`. An empty mirror is
/// honest; a stale one is not.
fn active_pane(run: &RunState) -> Option<String> {
    run.live_agent_pane().map(|(_, pane)| pane.to_owned())
}

/// What a request wants to do with the pane it names, which decides which
/// allow-list gates it — see [`run_writable_panes`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Access {
    Read,
    Write,
}

/// Every pane a run-scoped endpoint may READ: each phase pane, each reviewer
/// pane, and the workspace's idle root shell. The allow-list that stops
/// `?pane=<id>` from being used to reach an arbitrary herdr pane outside the run.
fn run_readable_panes(run: &RunState) -> std::collections::HashSet<String> {
    let mut set = run_writable_panes(run);
    if let Some(root) = &run.root_pane {
        set.insert(root.clone());
    }
    set
}

/// Every pane a run-scoped endpoint may WRITE (`/send`, `/keys`): the agent
/// panes only — the root shell is excluded.
///
/// **This is not a privilege boundary.** Typing into a live claude pane is
/// arbitrary code execution by design, so the unauthenticated server's reach is
/// exactly what it was. It is a footgun guard: the root shell is a bare `sh`
/// that now stays alive for the whole run, so a `/send` resolving there would
/// execute the user's *prose* as a shell command instead of prompting an agent.
fn run_writable_panes(run: &RunState) -> std::collections::HashSet<String> {
    let mut set = std::collections::HashSet::new();
    for ph in run.phases.iter().chain(run.review_phases.iter()) {
        if let Some(pane) = ph.pane_id() {
            set.insert(pane.to_owned());
        }
    }
    set
}

/// Resolve which pane a `/pane`, `/send` or `/keys` request targets. An explicit
/// `?pane=<id>` is honored only when it belongs to `run` under `access` (else
/// `None`); with no param, falls back to the run's [`active_pane`], which never
/// yields the root shell and so needs no further gating.
fn resolve_pane(run: &RunState, url: &str, access: Access) -> Option<String> {
    match query_param(url, "pane") {
        // Pane ids contain a `:` (`w16:p3`), which the browser's
        // `encodeURIComponent` sends as `%3A` — decode before matching, or the
        // allow-list never hits and every explicitly-selected pane 409s.
        Some(requested) => match access {
            Access::Read => run_readable_panes(run),
            Access::Write => run_writable_panes(run),
        }
        .take(&percent_decode(&requested)),
        None => active_pane(run),
    }
}

/// Minimal percent-decoder for a query-string value. `+` is left alone (these
/// are path-ish ids, not form encoding); an invalid escape is passed through
/// verbatim rather than dropped; and escapes that decode to non-UTF-8 bytes
/// yield the *original* undecoded string. Every one of those paths just means
/// the value fails the caller's allow-list — decoding widens nothing.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        // `i + 2 < len` because the escape needs both bytes[i+1] and bytes[i+2];
        // requiring ASCII hex digits keeps `from_str_radix` from accepting the
        // sign it otherwise would (`%+3` must stay literal, not become 0x03).
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && bytes[i + 1].is_ascii_hexdigit()
            && bytes[i + 2].is_ascii_hexdigit()
        {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
            if let Ok(b) = u8::from_str_radix(hex, 16) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_string())
}

/// `GET /api/runs/<run>/pane[?pane=<id>]` — the recent transcript of a run
/// agent's session, as plain text (204 when there is no such live pane).
fn handle_get_pane(req: Request, p: &RunPaths, url: &str) {
    let Some(pane) = load_run_state(&p.dir).and_then(|run| resolve_pane(&run, url, Access::Read))
    else {
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

/// `POST /api/runs/<run>/send[?pane=<id>]` — type the request body into a run
/// agent's pane (herdr submits it). 409 when there is no such live pane.
fn handle_post_send(mut req: Request, p: &RunPaths, url: &str) {
    let text = read_body(&mut req);
    let Some(pane) = load_run_state(&p.dir).and_then(|run| resolve_pane(&run, url, Access::Write))
    else {
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

/// Most keypresses one request may carry. A menu answer is 1–2 keys; a burst of
/// arrow-scrolling is a handful. The cap bounds the argv handed to herdr.
const MAX_KEYS: usize = 32;
/// Longest herdr key *name* (`enter`, `pagedown`, `ctrl+shift+k`). Anything
/// longer is free text, which belongs on `/send`.
const MAX_KEY_LEN: usize = 16;

/// Parse+validate a `POST /keys` body (`{"keys":["3","enter"]}`) into herdr key
/// names. Each key must be a short `[A-Za-z0-9+_-]` token that does not start
/// with `-`: these land in `herdr agent send-keys`'s argv, where a leading dash
/// would be parsed as an option rather than a key.
fn parse_keys(body: &str) -> Result<Vec<String>, &'static str> {
    let v: serde_json::Value = serde_json::from_str(body).map_err(|_| "invalid JSON")?;
    let arr = v.get("keys").and_then(|k| k.as_array()).ok_or("missing keys array")?;
    if arr.is_empty() {
        return Err("keys must not be empty");
    }
    if arr.len() > MAX_KEYS {
        return Err("too many keys");
    }
    let mut keys = Vec::with_capacity(arr.len());
    for item in arr {
        let k = item.as_str().ok_or("keys must be strings")?;
        if k.is_empty() || k.len() > MAX_KEY_LEN {
            return Err("invalid key name");
        }
        // Alphanumerics cover the names and digits; `+` joins a modifier chord
        // (`ctrl+c`). `-` is tolerated inside a name but never leading.
        if k.starts_with('-') || !k.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-')) {
            return Err("invalid key name");
        }
        keys.push(k.to_string());
    }
    Ok(keys)
}

/// `POST /api/runs/<run>/keys[?pane=<id>]` — press keys in a run agent's pane
/// (`herdr agent send-keys`), the only way to answer the numbered/arrow menus
/// (MCP-server approval, trust-dir prompt, model picker) that `/send`'s typed
/// prompt cannot drive. 400 on a bad body, 409 when there is no such live pane.
fn handle_post_keys(mut req: Request, p: &RunPaths, url: &str) {
    let body = read_body(&mut req);
    let keys = match parse_keys(&body) {
        Ok(keys) => keys,
        Err(msg) => {
            respond_str(req, 400, "text/plain", msg.into());
            return;
        }
    };
    let Some(pane) = load_run_state(&p.dir).and_then(|run| resolve_pane(&run, url, Access::Write))
    else {
        respond_str(req, 409, "text/plain", "no live pane for this run".into());
        return;
    };
    let h = crate::herdr::SystemHerdr::new();
    match h.agent_send_keys(&pane, &keys) {
        Ok(()) => respond_str(req, 200, "application/json", r#"{"ok":true}"#.into()),
        Err(e) => {
            eprintln!("drovr keys: to {pane} failed: {e}");
            respond_str(req, 500, "text/plain", "send-keys failed".into())
        }
    }
}

/// `POST /api/runs/<run>/rehydrate?phase=<name>` — bring back a phase whose
/// pane is gone, resuming its recorded session where the backend supports it.
///
/// **Shells out to `current_exe()`**, exactly as [`handle_post_new_run`] does,
/// so the CLI stays the sole writer of `state.json`. The server is a long-lived
/// daemon holding no run state; a second writer here would race the driver's own
/// `phase start` / `phase wait` for a whole-file save.
///
/// Three refusals, all BEFORE the shell-out:
///
/// * **400** — no `?phase=`, or a name that is not a safe filename component.
/// * **404** — a phase this run does not have. [`safe_component`] permits `:`
///   (reviewer names need it) and is a path check, **not an authorization
///   one** — and `phase_start` appends any name it is handed. Without the
///   membership test an unauthenticated caller could invent phases.
/// * **409** — the phase still holds a pane. "Holds a pane" is the same single
///   rule `phase_rehydrate` applies, read from the same `state.json`, so the
///   status code and the CLI's refusal can never disagree.
fn handle_post_rehydrate(req: Request, p: &RunPaths, run_name: &str, url: &str) {
    let Some(phase) = query_param(url, "phase").filter(|q| !q.is_empty()) else {
        respond_str(req, 400, "text/plain", "missing phase".into());
        return;
    };
    let phase = percent_decode(&phase);
    if !safe_component(&phase) {
        respond_str(req, 400, "text/plain", "invalid phase".into());
        return;
    }
    let Some(state) = load_run_state(&p.dir) else {
        respond_str(req, 404, "text/plain", "no such run".into());
        return;
    };
    let Some(target) = state.find_phase(&phase) else {
        respond_str(req, 404, "text/plain", "no such phase".into());
        return;
    };
    if let Some(pane) = target.pane_id() {
        respond_str(
            req,
            409,
            "text/plain",
            format!("phase '{phase}' still holds pane {pane}"),
        );
        return;
    }
    if !target.has_run() {
        respond_str(
            req,
            409,
            "text/plain",
            format!("phase '{phase}' has never run — start it, don't rehydrate it"),
        );
        return;
    }

    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(e) => {
            // JSON, like the other two 500s below: a client that parses the
            // body on failure must not hit one branch that throws instead.
            let msg = format!("cannot resolve drovr binary: {e}");
            eprintln!("drovr rehydrate: {msg}");
            respond_str(
                req,
                500,
                "application/json",
                serde_json::json!({ "ok": false, "error": msg }).to_string(),
            );
            return;
        }
    };
    match Command::new(&exe)
        .args(["phase", "rehydrate", run_name, &phase])
        .output()
    {
        Ok(o) if o.status.success() => respond_str(
            req,
            200,
            "application/json",
            serde_json::json!({
                "ok": true,
                "complete": true,
                "phase": phase,
                // The CLI's own line, which distinguishes "resumed with its
                // session" from "relaunched and reseeded" — the difference the
                // human actually cares about.
                "detail": String::from_utf8_lossy(&o.stdout).trim(),
            })
            .to_string(),
        ),
        // ⚠️ Exit 2 is the CLI's "the pane is back, but the agent was NOT given
        // this phase's context". It must NOT flatten into either bucket: a 500
        // would claim nothing happened (a pane really was created and recorded),
        // and a plain `ok: true` would let a caller checking only the status
        // treat an agent that never received its seed as fully recovered. 200
        // with `complete: false`, and the note on stderr says what to do.
        Ok(o) if o.status.code() == Some(2) => respond_str(
            req,
            200,
            "application/json",
            serde_json::json!({
                "ok": true,
                "complete": false,
                "phase": phase,
                "detail": String::from_utf8_lossy(&o.stderr).trim(),
            })
            .to_string(),
        ),
        Ok(o) => {
            let err = String::from_utf8_lossy(&o.stderr).trim().to_string();
            // Log it too. Every sibling handler does (`handle_post_send`,
            // `handle_post_keys`, `handle_get_pane`), and the browser tells the
            // user to "see the drovr server log" — which has to actually
            // contain something for that to be advice rather than a dead end.
            eprintln!("drovr rehydrate: {run_name}/{phase} failed: {err}");
            respond_str(
                req,
                500,
                "application/json",
                serde_json::json!({ "ok": false, "error": err }).to_string(),
            )
        }
        Err(e) => {
            let msg = format!("failed to run drovr phase rehydrate: {e}");
            eprintln!("drovr rehydrate: {msg}");
            respond_str(
                req,
                500,
                "application/json",
                serde_json::json!({ "ok": false, "error": msg }).to_string(),
            )
        }
    }
}

/// `GET /api/runs/<run>/agents` — the tree of spawned agents: each phase pane
/// with its per-task review panels nested beneath it. Only agents that actually
/// have a pane appear (unstarted placeholder phases are omitted).
fn handle_get_agents(req: Request, p: &RunPaths) {
    // A config that fails to load must not blank the tree: fall back to the
    // built-in agent map, which is what `resumable` is asking about anyway.
    let cfg = crate::config::load_config().unwrap_or_default();
    let tree = match load_run_state(&p.dir) {
        Some(run) => build_agent_tree(&run, &cfg),
        None => serde_json::json!({ "workspace": serde_json::Value::Null, "nodes": [] }),
    };
    respond_str(req, 200, "application/json", tree.to_string());
}

/// A `PhaseStatus` as its serialized string (`"Running"`, `"Done"`, …).
fn status_str(status: &crate::run::PhaseStatus) -> String {
    serde_json::to_value(status)
        .ok()
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_default()
}

/// Whether the ⟳ button should appear: is there a captured session AND a
/// backend that can be told to use it?
///
/// Both halves matter, and the second is the one that is easy to miss. codex
/// ships with no resume surface at all, so a codex phase can carry a perfectly
/// good session id and still only be relaunchable — and the button promises the
/// CONVERSATION back, not a fresh agent reading the notes. Advertising the one
/// as the other is how a human loses work they thought was recoverable.
fn is_resumable(phase: &crate::run::Phase, cfg: &crate::config::Config) -> bool {
    phase.resume_target().is_some_and(|target| {
        cfg.agents
            .get(target.backend())
            .is_some_and(|spec| spec.resume_surface().is_some())
    })
}

/// Build the agent tree for `run`: phases (started, or reaped) as top-level nodes,
/// with review panels (`review:<task>:<iter>:<angle>`) nested under the matching
/// `implement-<task>` phase. Reviews with no matching phase land in a trailing
/// group node so nothing is dropped.
fn build_agent_tree(run: &RunState, cfg: &crate::config::Config) -> serde_json::Value {
    use std::collections::BTreeMap;
    let mut reviews_by_task: BTreeMap<String, Vec<serde_json::Value>> = BTreeMap::new();
    for rp in &run.review_phases {
        // A placeholder is not an agent — see `Phase::has_run`, the same
        // predicate `phase_rehydrate` refuses on, so the tree never offers a ⟳
        // the CLI would then reject. A REAPED phase does show, dimmed: hiding
        // it would make a pane drovr closed look like one that never ran, which
        // is the opposite of what someone hunting for it needs.
        if !rp.has_run() {
            continue;
        }
        let parts: Vec<&str> = rp.name.split(':').collect();
        let task = parts.get(1).copied().unwrap_or("").to_string();
        let angle = parts.get(3).copied().unwrap_or("").to_string();
        reviews_by_task
            .entry(task)
            .or_default()
            .push(serde_json::json!({
                "name": rp.name, "kind": "review", "angle": angle,
                "status": status_str(&rp.status), "pane_id": rp.pane_id(),
                "reaped": rp.is_reaped(), "resumable": is_resumable(rp, cfg),
            }));
    }
    let mut nodes = Vec::new();
    for ph in &run.phases {
        if !ph.has_run() {
            continue;
        }
        let task_key = ph.name.strip_prefix("implement-").unwrap_or("");
        let children = reviews_by_task.remove(task_key).unwrap_or_default();
        nodes.push(serde_json::json!({
            "name": ph.name, "kind": "phase",
            "status": status_str(&ph.status), "pane_id": ph.pane_id(),
            "reaped": ph.is_reaped(), "resumable": is_resumable(ph, cfg),
            "children": children,
        }));
    }
    for (task, revs) in reviews_by_task {
        nodes.push(serde_json::json!({
            "name": format!("reviews: {task}"), "kind": "group",
            "status": "", "pane_id": serde_json::Value::Null, "children": revs,
        }));
    }
    serde_json::json!({ "workspace": run.workspace, "nodes": nodes })
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

/// `POST /api/runs/<run>/submit` — reviewer approve / cancel / request-changes.
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

    if rs.state.is_terminal() {
        let state = rs.state.as_str();
        drop(rs);
        respond_str(
            req,
            409,
            "application/json",
            format!(r#"{{"ok":false,"state":"{state}"}}"#),
        );
        return;
    }

    if decision == "approve" {
        // Approve is terminal, but not answer-free: the reviewer may have
        // answered the spec's open questions on the way to approving. Persist
        // the same feedback.json the request-changes path writes, so the driver
        // can read the selections instead of re-asking the human. The turn
        // advances for the same reason it does there — a driver reading
        // feedback.json must be able to tell this turn's answers from a stale
        // previous turn's.
        rs.turn += 1;
        let fb_json = serde_json::json!({
            "turn": rs.turn,
            "decision": "approve",
            "feedback": feedback,
            "answers": answers,
            "annotations": annotations,
        });
        let _ = fs::write(p.feedback(), fb_json.to_string());
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
    } else if decision == "cancel" {
        // Terminal like approve, but the opposite verdict: the human is
        // abandoning the run. The marker mirrors `approved` so a driver (or a
        // human poking at the run dir) can see the outcome without the server.
        let _ = fs::write(p.cancelled(), b"cancelled\n");
        rs.state = LoopState::Cancelled;
        let _ = rs.save(&p.review_state());
        drop(rs);
        respond_str(
            req,
            200,
            "application/json",
            r#"{"ok":true,"state":"cancelled"}"#.into(),
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

    // A decided run is closed: reject the summary rather than reviving it.
    if rs.state.is_terminal() {
        let state = rs.state.as_str();
        drop(rs);
        respond_str(
            req,
            409,
            "application/json",
            format!(r#"{{"ok":false,"state":"{state}"}}"#),
        );
        return;
    }

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
        // Pipeline progress, the axis the browser never had: `state` below is the
        // *gate* state, which says nothing about how far the run got. Note the
        // asymmetry — `approved` is set at the brainstorm gate, i.e. near the
        // START of a run, so it must never be read as "finished".
        let (done, total) = run_state.as_ref().map(|s| s.progress()).unwrap_or((0, 0));
        // A run is finished when its phases are all Done, when `drovr cleanup`
        // archived it, or when the human cancelled at the gate — the one terminal
        // verdict that ends a run without completing it. An unreadable state.json
        // is `None` here and stays visible rather than being hidden as complete.
        let complete = run_state.as_ref().is_some_and(|s| s.is_complete())
            || rs.state == LoopState::Cancelled;
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
                "complete": complete,
                "done": done,
                "total": total,
                // Lets the list say *why* a run is complete: "archived" (cleaned
                // up with phases outstanding) reads very differently from a run
                // that actually finished its pipeline.
                "archived": run_state.as_ref().is_some_and(|s| s.archived),
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
/// dir. Takes the `server.pid` lock, writes `server.addr` once the socket opens,
/// then blocks serving requests until the process exits.
///
/// Refuses to start a *second* server for the same data dir: one server owns
/// `server.addr` / `server.pid`, which is how every driver finds it, so a
/// duplicate would silently steal discovery from the live one and leave two
/// servers holding split in-memory run state. The lock (see [`acquire_pid_lock`])
/// is the whole test — it is held by the kernel, so it needs nothing to be judged
/// stale, and it catches a duplicate on *any* port, which the OS bind would not.
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

    // Single-writer for the discovery files: take the lock before touching the
    // socket or rewriting `server.addr`, so a live server keeps serving untouched.
    // Held until this function returns, which for a serving process means until it
    // exits; the kernel drops it either way.
    let _lock = acquire_pid_lock()?;

    let addr = format!("{host}:{port}");
    // Losing the bind means something foreign holds the port (a duplicate drovr
    // server was already turned away above); the error names which.
    let server =
        Server::http(&addr).map_err(|e| io::Error::other(format!("cannot bind {addr}: {e}")))?;

    let bound_addr = server
        .server_addr()
        .to_ip()
        .map(|a| a.to_string())
        .unwrap_or_else(|| addr.clone());
    fs::write(server_addr_file(), bound_addr.as_bytes())?;

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
    // `_lock` drops here (and on every error path above), so the next
    // `drovr serve` can take it the moment this one stops serving.
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

/// Take the single-server lock, held for as long as the returned file lives.
///
/// The lock is an advisory exclusive lock on `server.pid`, not the pid inside it:
/// the kernel holds it for this process and drops it however the process dies, so
/// a crashed server never leaves a claim anyone has to judge stale, and nothing
/// has to guess whether some pid is still a server. It is also the only check that
/// catches a duplicate on an *arbitrary* port, where the OS bind would not
/// serialize two starts at all.
///
/// The pid written inside is for humans (`kill $(cat server.pid)`) and for the
/// refusal message — never for the decision.
fn acquire_pid_lock() -> io::Result<File> {
    match try_take_lock(&server_pid_file())? {
        Some(lock) => Ok(lock),
        // Someone holds it: they are serving (or about to), so stand down.
        None => Err(duplicate_server_error(recorded_addr(), lock_holder())),
    }
}

/// Lock `path` and stamp this process's pid into it, or `Ok(None)` if another
/// process holds it. Takes the path so it is testable without the data dir (and
/// so without the process-global `XDG_DATA_HOME` other tests mutate).
///
/// The pid is written *after* the lock is taken, so for the moment between the two
/// the file still names the previous holder — which is why the pid only ever
/// informs the message, never a decision.
fn try_take_lock(path: &Path) -> io::Result<Option<File>> {
    let mut lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;

    match lock.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => return Ok(None),
        Err(TryLockError::Error(e)) => return Err(e),
    }

    lock.set_len(0)?;
    lock.write_all(std::process::id().to_string().as_bytes())?;
    lock.flush()?;
    Ok(Some(lock))
}

/// The pid recorded in `server.pid`, if it holds a readable one.
fn lock_holder() -> Option<u32> {
    fs::read_to_string(server_pid_file())
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok())
}

/// The refusal a second `drovr serve` exits with, naming the live server as
/// precisely as it can be named.
///
/// Both parts are conditional on what is known: a start that lost the lock race by
/// microseconds sees a holder that has neither written its pid nor bound yet.
fn duplicate_server_error(addr: Option<String>, pid: Option<u32>) -> io::Error {
    let where_ = match addr {
        Some(addr) => format!(" on http://{}", display_addr(&addr)),
        None => String::new(),
    };
    let (who, how) = match pid {
        Some(pid) => (
            format!(" (pid {pid})"),
            format!(", or stop it with: kill {pid}"),
        ),
        None => (String::new(), String::new()),
    };
    io::Error::new(
        io::ErrorKind::AddrInUse,
        format!(
            "a drovr review server is already running{where_}{who} — the server is global \
             and serves every run, so use that one{how}"
        ),
    )
}

/// The address in `server.addr`, unverified.
///
/// Only ever used to put a URL in the refusal above, never to decide anything —
/// which is why "unverified" is good enough. A holder that has taken the lock but
/// not yet bound leaves the *previous* server's address here, so the URL can be
/// stale for as long as a start takes; the refusal itself never depends on it.
fn recorded_addr() -> Option<String> {
    let addr = fs::read_to_string(server_addr_file()).ok()?;
    let addr = addr.trim().to_string();
    (!addr.is_empty()).then_some(addr)
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

/// `POST /api/runs` — create a run and start its brainstorm agent, so a fresh
/// drovr session can be launched (and then watched/driven) from the browser.
/// Body: `{ "name": "<run>", "task"?: "<text>", "dir"?: "<project dir>" }`.
/// Runs `drovr new` then `drovr phase start <run> brainstorm` via this same
/// binary, synchronously (both return quickly — `phase start` only spawns).
fn handle_post_new_run(mut req: Request) {
    let body = read_body(&mut req);
    let incoming: serde_json::Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(_) => {
            respond_str(req, 400, "text/plain", "invalid JSON".into());
            return;
        }
    };
    let name = incoming["name"].as_str().unwrap_or("").trim().to_string();
    if !safe_component(&name) {
        respond_str(req, 400, "text/plain", "invalid or missing run name".into());
        return;
    }
    let task = incoming["task"].as_str().unwrap_or("").to_string();
    let dir = incoming["dir"].as_str().unwrap_or("").to_string();

    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(e) => {
            respond_str(req, 500, "text/plain", format!("cannot resolve drovr binary: {e}"));
            return;
        }
    };

    let mut new_args = vec!["new".to_string(), name.clone()];
    if !task.is_empty() {
        new_args.push("--task".into());
        new_args.push(task);
    }
    if !dir.is_empty() {
        new_args.push("--dir".into());
        new_args.push(dir);
    }
    match Command::new(&exe).args(&new_args).output() {
        Ok(o) if o.status.success() => {}
        Ok(o) => {
            let err = String::from_utf8_lossy(&o.stderr).trim().to_string();
            respond_str(
                req,
                500,
                "application/json",
                serde_json::json!({ "ok": false, "error": err }).to_string(),
            );
            return;
        }
        Err(e) => {
            respond_str(req, 500, "text/plain", format!("failed to run drovr new: {e}"));
            return;
        }
    }

    // Best-effort: start the brainstorm agent so the run has a live session to
    // inspect. A failure here still leaves a created run the caller can drive.
    let started = Command::new(&exe)
        .args(["phase", "start", &name, "brainstorm"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    respond_str(
        req,
        200,
        "application/json",
        serde_json::json!({ "ok": true, "name": name, "started": started }).to_string(),
    );
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

/// Rewrite a wildcard bind address into one a human can actually click.
///
/// `serve --host 0.0.0.0` records `0.0.0.0:<port>` in `server.addr`. That is a
/// valid thing to bind to but not a valid destination, so a URL built from it
/// is useless in a browser. Any already-routable host — loopback, a LAN IP, the
/// Tailscale IP a `serve_host` config produces — passes through untouched.
pub fn display_addr(addr: &str) -> String {
    for wildcard in ["0.0.0.0:", "[::]:"] {
        if let Some(port) = addr.strip_prefix(wildcard) {
            return format!("127.0.0.1:{port}");
        }
    }
    addr.to_string()
}

/// POST summary text to the running review server for `run` (`drovr review
/// summary`). Ensures the server is up, then POSTs to
/// `/api/runs/<run>/summary`, flipping that run's state to `ready`.
///
/// Returns the server address on success so the caller can print the reviewer's
/// page URL and the matching `drovr review wait` invocation. This is the moment
/// the gate actually opens, so it is the only place that can reliably remind a
/// driver to start the watch — `drovr serve` is global and does not know which
/// run is being reviewed. See `docs/known-issues.md`, "Serving a spec doesn't
/// start a watcher".
pub fn review_summary(run: &str, text: &str) -> io::Result<String> {
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
    // 409 means the run already reached a terminal verdict. Name it, so the
    // agent learns *why* its revision was refused instead of retrying blindly.
    if response.contains(" 409 ") {
        let body = response.split_once("\r\n\r\n").map(|x| x.1).unwrap_or("");
        let parsed: serde_json::Value = serde_json::from_str(body.trim()).unwrap_or_default();
        let state = parsed["state"].as_str().unwrap_or("decided");
        return Err(io::Error::other(format!(
            "run '{run}' is already {state}; the review gate is closed — stop revising the spec"
        )));
    }
    if !response.starts_with("HTTP/1") || !response.contains(" 200 ") {
        return Err(io::Error::other(format!(
            "unexpected response from review server: {}",
            response.lines().next().unwrap_or("")
        )));
    }
    Ok(addr)
}

/// Terminal outcome of a [`review_wait`] blocking wait.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitOutcome {
    /// Reviewer approved — the `approved` marker is present.
    Approved,
    /// Reviewer requested changes — `feedback.json` holds this turn's feedback.
    ChangesRequested,
    /// Reviewer cancelled the run — the `cancelled` marker is present. Terminal:
    /// the driver should tear the run down, not revise the spec.
    Cancelled,
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
/// returns [`WaitOutcome::Approved`] once approved,
/// [`WaitOutcome::ChangesRequested`] once changes are requested
/// (`feedback.json` holds the turn), and [`WaitOutcome::Cancelled`] once the
/// reviewer cancels. On timeout returns [`WaitOutcome::Timeout`] — the wait is
/// resumable, so a driver just re-runs it.
pub fn review_wait(run: &str, timeout_ms: u64) -> io::Result<WaitOutcome> {
    let addr = ensure_server()?;

    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    loop {
        match fetch_state(&addr, run)?.as_str() {
            "approved" => return Ok(WaitOutcome::Approved),
            "cancelled" => return Ok(WaitOutcome::Cancelled),
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

    /// Create a run dir whose `state.json` carries real phases, so completion is
    /// computable. `statuses` is one `PhaseStatus` per pipeline phase.
    fn make_run_with_phases(
        root: &Path,
        run: &str,
        statuses: &[crate::run::PhaseStatus],
        archived: bool,
    ) -> PathBuf {
        let dir = root.join(run);
        fs::create_dir_all(&dir).unwrap();
        let phases: Vec<crate::run::Phase> = statuses
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let mut p = crate::run::Phase::new(&format!("phase{i}"));
                p.status = s.clone();
                p
            })
            .collect();
        let state = RunState {
            name: run.into(),
            task: "t".into(),
            agent: None,
            phases,
            review_phases: vec![],
            gate: "spec".into(),
            cursor: 0,
            workspace: None,
            root_pane: None,
            project_dir: String::new(),
            worktree_path: None,
            worktree_branch: None,
            archived,
            retired_panes: vec![],
        };
        fs::write(
            dir.join("state.json"),
            serde_json::to_string(&state).unwrap(),
        )
        .unwrap();
        dir
    }

    fn row_for<'a>(rows: &'a [serde_json::Value], name: &str) -> &'a serde_json::Value {
        rows.iter()
            .find(|r| r["name"] == name)
            .unwrap_or_else(|| panic!("run '{name}' missing from /api/runs"))
    }

    #[test]
    fn list_runs_json_reports_completion_and_progress() {
        use crate::run::PhaseStatus::{Done, Pending, Running};
        let tmp = std::env::temp_dir().join(format!("drovr-complete-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        make_run_with_phases(&tmp, "finished", &[Done, Done, Done, Done], false);
        make_run_with_phases(&tmp, "midflight", &[Done, Running, Pending, Pending], false);
        // The `mcp-endpoint` shape: cleaned up mid-brainstorm, phases frozen.
        make_run_with_phases(&tmp, "archived-early", &[Running, Pending], true);
        // Unreadable state.json → must NOT be reported complete (see is_complete).
        let broken = tmp.join("broken");
        fs::create_dir_all(&broken).unwrap();
        fs::write(broken.join("state.json"), b"{ not json").unwrap();

        let ctx = Arc::new(Ctx::new(tmp.clone()));
        let rows: Vec<serde_json::Value> = serde_json::from_str(&list_runs_json(&ctx)).unwrap();

        assert_eq!(row_for(&rows, "finished")["complete"], true);
        assert_eq!(row_for(&rows, "finished")["done"], 4);
        assert_eq!(row_for(&rows, "finished")["total"], 4);

        assert_eq!(row_for(&rows, "midflight")["complete"], false);
        assert_eq!(row_for(&rows, "midflight")["done"], 1);
        assert_eq!(row_for(&rows, "midflight")["total"], 4);

        assert_eq!(
            row_for(&rows, "archived-early")["complete"],
            true,
            "a cleaned-up run is complete even with phases left Pending"
        );
        assert_eq!(row_for(&rows, "archived-early")["archived"], true);
        assert_eq!(
            row_for(&rows, "finished")["archived"],
            false,
            "a run that finished its phases was never archived"
        );

        assert_eq!(
            row_for(&rows, "broken")["complete"],
            false,
            "a run whose state.json will not parse must stay visible, not be hidden as complete"
        );

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn list_runs_json_treats_a_cancelled_gate_as_complete() {
        use crate::run::PhaseStatus::{Pending, Running};
        let tmp = std::env::temp_dir().join(format!("drovr-cancelled-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let dir = make_run_with_phases(&tmp, "abandoned", &[Running, Pending], false);
        fs::write(
            dir.join("review.state.json"),
            br#"{"state":"cancelled","turn":2}"#,
        )
        .unwrap();

        let ctx = Arc::new(Ctx::new(tmp.clone()));
        let rows: Vec<serde_json::Value> = serde_json::from_str(&list_runs_json(&ctx)).unwrap();
        assert_eq!(
            row_for(&rows, "abandoned")["complete"],
            true,
            "cancelled is a terminal human verdict — the run is over"
        );

        let _ = fs::remove_dir_all(&tmp);
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

    /// The mirror's default target: the running phase, else the last phase that
    /// still holds a pane, else nothing.
    ///
    /// It used to fall back to `run.root_pane`, which was dead code while the
    /// first phase consumed that pane. Now that the root shell stays idle for
    /// the whole run, that fallback would silently point the UI at an empty
    /// shell — so the honest answer when no phase has a pane is `None` (204 /
    /// "no live pane"), not a shell prompt dressed up as an agent.
    #[test]
    fn active_pane_is_the_current_phase_or_nothing() {
        let mkphase = |name: &str, status, pane: Option<&str>| {
            let mut p = {
                let mut p = crate::run::Phase::new(name);
                p.status = status;
                p
            };
            if let Some(pane) = pane {
                p.set_pane(pane);
            }
            p
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
            archived: false,
            retired_panes: vec![],
        };
        // The phase the run is on.
        assert_eq!(active_pane(&run).as_deref(), Some("w:p2"));
        // Every phase Done → nothing to mirror. It must NOT fall back to that
        // phase's own pane once the run has moved past it, nor to an earlier
        // phase's: under reaping those are exactly the panes that get closed,
        // so a fallback would mirror a dead or recycled pane as if it were live.
        run.phases[1].status = crate::run::PhaseStatus::Done;
        assert_eq!(active_pane(&run), None);
        // Still nothing once the panes are actually gone — and still not the
        // idle root shell, which outlives every phase.
        run.phases[1].mark_reaped();
        run.phases[0].mark_reaped();
        assert!(
            run.root_pane.is_some(),
            "the root shell outlives the phases"
        );
        assert_eq!(active_pane(&run), None);
    }

    /// `POST /send` must never resolve to the workspace's idle root shell.
    ///
    /// Not a privilege boundary — sending into a live claude pane is arbitrary
    /// code execution by design, and that surface is unchanged. It is a
    /// footgun: the root shell is a bare `sh` alive for the whole run, so a
    /// `/send` landing there runs the user's PROSE as a shell command. Reading
    /// it is harmless and stays allowed.
    #[test]
    fn the_root_shell_is_readable_but_never_writable() {
        use crate::run::PhaseStatus::Running;
        let run = tree_run(vec![ph("implement", Running, Some("w:p3"))], vec![]);

        assert_eq!(
            resolve_pane(&run, "/x?pane=w:root", Access::Read).as_deref(),
            Some("w:root"),
            "mirroring the idle shell is fine"
        );
        assert_eq!(
            resolve_pane(&run, "/x?pane=w:root", Access::Write),
            None,
            "typing into the idle shell is not"
        );
        // A phase pane is writable, so the gate is about the root shell alone.
        assert_eq!(
            resolve_pane(&run, "/x?pane=w:p3", Access::Write).as_deref(),
            Some("w:p3")
        );
    }

    // A RunState with the given phases / review phases; other fields are inert.
    fn tree_run(phases: Vec<crate::run::Phase>, reviews: Vec<crate::run::Phase>) -> RunState {
        RunState {
            name: "r".into(),
            task: "t".into(),
            agent: Some("claude".into()),
            phases,
            review_phases: reviews,
            gate: "spec".into(),
            cursor: 0,
            workspace: Some("w".into()),
            root_pane: Some("w:root".into()),
            project_dir: "/tmp/p".into(),
            worktree_path: None,
            worktree_branch: None,
            archived: false,
            retired_panes: vec![],
        }
    }

    fn ph(name: &str, status: crate::run::PhaseStatus, pane: Option<&str>) -> crate::run::Phase {
        let mut p = crate::run::Phase::new(name);
        p.status = status;
        if let Some(pane) = pane {
            p.set_pane(pane);
        }
        p
    }

    #[test]
    fn agent_tree_nests_reviews_under_tasks() {
        use crate::run::PhaseStatus::*;
        let run = tree_run(
            vec![
                ph("brainstorm", Done, Some("w:p1")),
                ph("implement", Pending, None), // unstarted placeholder → omitted
                ph("implement-task-1", Running, Some("w:p3")),
            ],
            vec![
                ph("review:task-1:1:correctness", Running, Some("w:p4")),
                ph("review:task-1:1:security", Done, Some("w:p5")),
            ],
        );
        let tree = build_agent_tree(&run, &crate::config::Config::default());
        assert_eq!(tree["workspace"], "w");
        let nodes = tree["nodes"].as_array().unwrap();
        assert_eq!(nodes.len(), 2, "placeholder omitted: {tree}");
        assert_eq!(nodes[0]["name"], "brainstorm");
        assert_eq!(nodes[0]["children"].as_array().unwrap().len(), 0);
        let task1 = &nodes[1];
        assert_eq!(task1["name"], "implement-task-1");
        assert_eq!(task1["pane_id"], "w:p3");
        let reviews = task1["children"].as_array().unwrap();
        assert_eq!(reviews.len(), 2);
        assert_eq!(reviews[0]["kind"], "review");
        assert_eq!(reviews[0]["angle"], "correctness");
        assert_eq!(reviews[0]["pane_id"], "w:p4");
    }

    #[test]
    fn resolve_pane_gates_foreign_panes() {
        use crate::run::PhaseStatus::Running;
        let run = tree_run(
            vec![ph("brainstorm", Running, Some("w:p1")), ph("implement-task-1", Running, Some("w:p3"))],
            vec![],
        );
        // Explicit pane belonging to the run is honored.
        assert_eq!(
            resolve_pane(&run, "/x?pane=w:p3", Access::Write).as_deref(),
            Some("w:p3")
        );
        // The root pane is in the READABLE allow-list (see
        // `the_root_shell_is_readable_but_never_writable`).
        assert_eq!(
            resolve_pane(&run, "/x?pane=w:root", Access::Read).as_deref(),
            Some("w:root")
        );
        // A pane outside the run is rejected (no silent fallback), read or write.
        assert_eq!(resolve_pane(&run, "/x?pane=w9:p99", Access::Read), None);
        assert_eq!(resolve_pane(&run, "/x?pane=w9:p99", Access::Write), None);
        // No param → active_pane (first Running phase).
        assert_eq!(
            resolve_pane(&run, "/x", Access::Write).as_deref(),
            Some("w:p1")
        );
    }

    #[test]
    fn resolve_pane_decodes_percent_encoded_ids() {
        use crate::run::PhaseStatus::Running;
        let run = tree_run(vec![ph("implement", Running, Some("w:p3"))], vec![]);
        // The browser sends encodeURIComponent("w:p3") = "w%3Ap3"; without
        // decoding it misses the allow-list and every selected pane 409s.
        assert_eq!(
            resolve_pane(&run, "/x?pane=w%3Ap3", Access::Write).as_deref(),
            Some("w:p3")
        );
        // Decoding must not open a hole: a foreign pane is still rejected.
        assert_eq!(resolve_pane(&run, "/x?pane=w9%3Ap99", Access::Write), None);
        // Malformed escapes are passed through verbatim (and so simply miss).
        assert_eq!(resolve_pane(&run, "/x?pane=w%3", Access::Write), None);
    }

    #[test]
    fn percent_decode_handles_escapes_and_junk() {
        assert_eq!(percent_decode("w%3Ap3"), "w:p3");
        assert_eq!(percent_decode("w%3ap3"), "w:p3"); // lowercase hex
        assert_eq!(percent_decode("plain"), "plain");
        assert_eq!(percent_decode("a%zzb"), "a%zzb"); // not hex → literal
        assert_eq!(percent_decode("trailing%"), "trailing%");
        assert_eq!(percent_decode("a%2"), "a%2");
        assert_eq!(percent_decode("%+3"), "%+3"); // from_str_radix must not eat the sign
        // Escapes that decode to non-UTF-8 bytes fall back to the raw input;
        // either way the value misses the pane allow-list.
        assert_eq!(percent_decode("w%80p"), "w%80p");
        // Double-encoding decodes exactly once: `%253A` → `%3A`, never `:`.
        assert_eq!(percent_decode("w%253Ap3"), "w%3Ap3");
    }

    // -- send-keys -----------------------------------------------------------

    #[test]
    fn parse_keys_accepts_a_menu_answer() {
        assert_eq!(
            parse_keys(r#"{"keys":["3","enter"]}"#).unwrap(),
            vec!["3".to_string(), "enter".to_string()]
        );
        // The arrow/escape names the UI sends.
        assert_eq!(
            parse_keys(r#"{"keys":["down","up","esc","ctrl+c"]}"#).unwrap(),
            vec!["down", "up", "esc", "ctrl+c"]
        );
    }

    #[test]
    fn parse_keys_rejects_bad_bodies() {
        // Not JSON at all.
        assert!(parse_keys("enter").is_err());
        // Right shape, no keys — nothing to press.
        assert!(parse_keys(r#"{"keys":[]}"#).is_err());
        // Missing / wrongly-typed field.
        assert!(parse_keys(r#"{"text":"enter"}"#).is_err());
        assert!(parse_keys(r#"{"keys":"enter"}"#).is_err());
        assert!(parse_keys(r#"{"keys":[3]}"#).is_err());
        // Oversized list.
        let many = (0..MAX_KEYS + 1).map(|_| "\"a\"").collect::<Vec<_>>().join(",");
        assert!(parse_keys(&format!(r#"{{"keys":[{many}]}}"#)).is_err());
    }

    #[test]
    fn parse_keys_rejects_unsafe_key_names() {
        // A key is a herdr key *name*, never free text and never an argv flag:
        // `herdr agent send-keys` would otherwise parse a leading `-` as an option.
        assert!(parse_keys(r#"{"keys":["--help"]}"#).is_err());
        assert!(parse_keys(r#"{"keys":[""]}"#).is_err());
        assert!(parse_keys(r#"{"keys":["rm -rf /"]}"#).is_err());
        assert!(parse_keys(r#"{"keys":["ent;er"]}"#).is_err());
        assert!(parse_keys(r#"{"keys":["ent_er"]}"#).is_err());
        assert!(parse_keys(r#"{"keys":["a b"]}"#).is_err());
        let long = "a".repeat(MAX_KEY_LEN + 1);
        assert!(parse_keys(&format!(r#"{{"keys":["{long}"]}}"#)).is_err());
    }

    #[test]
    fn post_keys_validates_before_touching_a_pane() {
        // The run in `make_run` has no panes, so a *well-formed* request gets as
        // far as pane resolution (409) while a malformed one is rejected at the
        // body (400). That ordering is what makes the 400s observable without a
        // live herdr.
        let tmp = make_root("keys");
        make_run(tmp.path(), "r", b"# Spec");
        let addr = start_server(tmp.path().to_path_buf());

        let (s, _) = http_post(&addr, "/api/runs/r/keys", "application/json", r#"{"keys":["3","enter"]}"#);
        assert_eq!(s, 409, "valid body, no live pane → 409");

        for bad in [r#"{"keys":[]}"#, "not json", r#"{"keys":["--oops"]}"#, r#"{"keys":"enter"}"#] {
            let (s, _) = http_post(&addr, "/api/runs/r/keys", "application/json", bad);
            assert_eq!(s, 400, "body {bad} must be rejected");
        }
    }

    /// End-to-end proof that `/send` and `/keys` really use the WRITABLE
    /// allow-list, not just that the pure resolver can tell the two apart.
    ///
    /// A wrong `Access` at either call site is invisible to the unit tests: the
    /// handler would resolve the root pane, hand it to `SystemHerdr`, and fail
    /// there instead — so this asserts 409 (gated before herdr) rather than the
    /// 500 an ungated request would produce with no herdr running.
    ///
    /// The read side cannot be asserted the same way: `GET /pane` answers 204
    /// both when the pane is gated and when herdr cannot be reached, so the
    /// readable set is pinned by `the_root_shell_is_readable_but_never_writable`.
    #[test]
    fn the_root_shell_is_refused_by_the_write_endpoints_over_http() {
        let tmp = make_root("root-write-gate");
        let dir = tmp.path().join("r");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("spec.md"), b"# Spec").unwrap();
        // An idle root shell plus one phase pane. Both are needed: the phase
        // pane is the POSITIVE CONTROL that keeps this test from passing
        // vacuously. If the `state.json` below failed to parse, or `root_pane`
        // were dropped, every request would 409 for the wrong reason — the
        // phase pane's 500 is what proves the state loaded and the gate is
        // discriminating rather than refusing everything.
        let mut run = crate::run::RunState {
            name: "r".into(),
            task: "t".into(),
            agent: None,
            phases: vec![crate::run::Phase::new("implement")],
            review_phases: vec![],
            gate: "spec".into(),
            cursor: 0,
            workspace: Some("w".into()),
            root_pane: Some("w:root".into()),
            project_dir: String::new(),
            worktree_path: None,
            worktree_branch: None,
            archived: false,
            retired_panes: vec![],
        };
        run.phases[0].status = crate::run::PhaseStatus::Running;
        run.phases[0].set_pane("w:p1");
        fs::write(dir.join("state.json"), serde_json::to_string(&run).unwrap()).unwrap();
        let addr = start_server(tmp.path().to_path_buf());

        for (path, ctype, body) in [
            ("send?pane=w%3Aroot", "text/plain", "ls -la"),
            (
                "keys?pane=w%3Aroot",
                "application/json",
                r#"{"keys":["enter"]}"#,
            ),
        ] {
            let (s, _) = http_post(&addr, &format!("/api/runs/r/{path}"), ctype, body);
            assert_eq!(s, 409, "{path} must refuse the idle root shell");
        }

        // Positive control: the phase pane IS writable, so it gets past the gate
        // and fails at herdr instead (no daemon in the test environment).
        let (s, _) = http_post(&addr, "/api/runs/r/send?pane=w%3Ap1", "text/plain", "hi");
        assert_eq!(
            s, 500,
            "a phase pane must pass the gate and fail at herdr, not be refused"
        );
    }

    #[test]
    fn post_keys_honors_pane_gating() {
        // `?pane=` outside the run must never reach herdr: same allow-list as
        // /pane and /send (resolve_pane), so a foreign pane looks like no pane.
        let tmp = make_root("keys-gate");
        make_run(tmp.path(), "r", b"# Spec");
        let addr = start_server(tmp.path().to_path_buf());
        let (s, _) = http_post(
            &addr,
            "/api/runs/r/keys?pane=w9%3Ap99",
            "application/json",
            r#"{"keys":["enter"]}"#,
        );
        assert_eq!(s, 409);
    }

    #[test]
    fn post_rehydrate_refuses_before_it_can_shell_out() {
        // Every refusal short-circuits before `current_exe()`, so they are safe
        // to exercise in-process. The happy path shells out to the CLI — which
        // is the point: the CLI stays the sole writer of state.json — and is
        // covered by `phase::rehydrate_tests` plus manual verification.
        let tmp = make_root("rehydrate-http");
        let dir = make_run(tmp.path(), "r", b"# Spec");
        let mut run: RunState = serde_json::from_str(
            &fs::read_to_string(dir.join("state.json")).unwrap(),
        )
        .unwrap();
        let mut live = crate::run::Phase::new("plan");
        live.status = crate::run::PhaseStatus::Running;
        live.set_pane("w:p1");
        let mut reaped = crate::run::Phase::new("brainstorm");
        reaped.status = crate::run::PhaseStatus::Done;
        reaped.set_pane("w:p0");
        reaped.mark_reaped();
        run.phases = vec![reaped, live, crate::run::Phase::new("placeholder")];
        fs::write(dir.join("state.json"), serde_json::to_string(&run).unwrap()).unwrap();
        let before = fs::read_to_string(dir.join("state.json")).unwrap();
        let addr = start_server(tmp.path().to_path_buf());

        // No phase at all → 400, not a rehydrate of "".
        let (s, _) = http_post(&addr, "/api/runs/r/rehydrate", "text/plain", "");
        assert_eq!(s, 400);

        // A phase this run does not have → 404. `safe_component` permits `:` and
        // is a filename check, NOT an authorization one, and `phase_start`
        // happily appends unknown names — so the membership test is what stops
        // an unauthenticated caller inventing phases.
        let (s, _) = http_post(&addr, "/api/runs/r/rehydrate?phase=nope", "text/plain", "");
        assert_eq!(s, 404);
        // A traversal-shaped name is refused before anything reads a path.
        let (s, _) = http_post(
            &addr,
            "/api/runs/r/rehydrate?phase=..%2Fevil",
            "text/plain",
            "",
        );
        assert_eq!(s, 400);

        // A phase that still holds a pane → 409. Duplicating an agent into a
        // live conversation is exactly what rehydrate must not do.
        let (s, body) = http_post(&addr, "/api/runs/r/rehydrate?phase=plan", "text/plain", "");
        assert_eq!(s, 409, "{body}");

        // A phase that has never run → 409 as well, and for a reason worth
        // keeping separate: `drovr new` pre-seeds placeholders, so without this
        // an unauthenticated caller could START one out of pipeline order.
        let (s, body) = http_post(
            &addr,
            "/api/runs/r/rehydrate?phase=placeholder",
            "text/plain",
            "",
        );
        assert_eq!(s, 409, "{body}");
        assert!(body.contains("never run"), "{body}");

        assert_eq!(
            fs::read_to_string(dir.join("state.json")).unwrap(),
            before,
            "a refused rehydrate must not write state.json"
        );
    }

    #[test]
    fn agent_tree_carries_reaped_phases_but_not_placeholders() {
        use crate::run::PhaseStatus::*;
        let mut reaped = ph("brainstorm", Done, Some("w:p1"));
        reaped.record_launch("claude", None);
        assert!(reaped.record_session(
            crate::herdr::SessionId::new("sess-b".into()).unwrap()
        ));
        reaped.mark_reaped();
        // Reaped, but nothing was ever captured for it — the UI must not offer
        // a ⟳ that would land on a reseed the human did not ask for.
        let mut sessionless = ph("plan", Done, Some("w:p2"));
        sessionless.record_launch("claude", None);
        sessionless.mark_reaped();

        let run = tree_run(
            vec![
                reaped,
                sessionless,
                ph("implement", Pending, None), // unstarted placeholder → STILL omitted
                ph("implement-task-1", Running, Some("w:p3")),
            ],
            vec![],
        );
        let tree = build_agent_tree(&run, &crate::config::Config::default());
        let nodes = tree["nodes"].as_array().unwrap();
        assert_eq!(
            nodes.len(),
            3,
            "reaped phases appear; the placeholder does not: {tree}"
        );

        assert_eq!(nodes[0]["name"], "brainstorm");
        assert_eq!(nodes[0]["reaped"], true);
        assert_eq!(nodes[0]["resumable"], true);
        assert!(nodes[0]["pane_id"].is_null(), "a reaped phase has no pane");

        assert_eq!(nodes[1]["name"], "plan");
        assert_eq!(nodes[1]["reaped"], true);
        assert_eq!(nodes[1]["resumable"], false);

        assert_eq!(nodes[2]["name"], "implement-task-1");
        assert_eq!(nodes[2]["reaped"], false);
        assert_eq!(nodes[2]["pane_id"], "w:p3");
    }

    #[test]
    fn a_backend_with_no_resume_surface_is_not_advertised_as_resumable() {
        // The ⟳ button promises the CONVERSATION back. codex ships with no
        // resume field, so a captured session id is not enough — clicking it
        // would silently reseed.
        use crate::run::PhaseStatus::Done;
        let mut p = ph("plan", Done, Some("w:p1"));
        p.record_launch("codex", None);
        assert!(p.record_session(crate::herdr::SessionId::new("sess-c".into()).unwrap()));
        p.mark_reaped();
        let run = tree_run(vec![p], vec![]);

        let cfg = crate::config::Config::default();
        let tree = build_agent_tree(&run, &cfg);
        assert_eq!(tree["nodes"][0]["resumable"], false);

        // …and it flips the moment the user opts codex in.
        let mut cfg = cfg;
        cfg.agents.get_mut("codex").unwrap().resume_subcommand = Some("resume".into());
        let tree = build_agent_tree(&run, &cfg);
        assert_eq!(tree["nodes"][0]["resumable"], true);
    }

    #[test]
    fn post_new_run_rejects_bad_input() {
        // The reject paths short-circuit before spawning `drovr new`, so they are
        // safe to exercise in-process (the happy path shells out and is covered
        // by manual/e2e testing).
        let tmp = make_root("newrun");
        let addr = start_server(tmp.path().to_path_buf());
        // Missing name.
        let (s, _) = http_post(&addr, "/api/runs", "application/json", r#"{"task":"x"}"#);
        assert_eq!(s, 400);
        // Path-traversal name.
        let (s, _) = http_post(&addr, "/api/runs", "application/json", r#"{"name":"../evil"}"#);
        assert_eq!(s, 400);
        // Malformed JSON.
        let (s, _) = http_post(&addr, "/api/runs", "application/json", "not json");
        assert_eq!(s, 400);
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
    fn submit_approve_persists_question_answers() {
        // Approving is the common path for a spec whose open questions the
        // reviewer just answered. If the answers only survive `request-changes`,
        // every approved run silently drops them and the next phase has to
        // re-ask the human.
        let tmp = make_root("approve-answers");
        let dir = make_run(tmp.path(), "r", b"# Done");
        let addr = start_server(tmp.path().to_path_buf());

        let payload = r#"{"decision":"approve","feedback":"ship it",
            "answers":{"q1":"redis","q2":"a custom typed answer"},
            "annotations":[{"line":3,"note":"n"}]}"#;
        let (status, body) = http_post(&addr, "/api/runs/r/submit", "application/json", payload);
        assert_eq!(status, 200, "body={body}");

        let fb = fs::read_to_string(dir.join("feedback.json")).expect("feedback.json on approve");
        let v: serde_json::Value = serde_json::from_str(&fb).unwrap();
        assert_eq!(v["decision"], "approve");
        assert_eq!(v["feedback"], "ship it");
        assert_eq!(v["answers"]["q1"], "redis");
        assert_eq!(v["answers"]["q2"], "a custom typed answer");
        assert_eq!(v["annotations"][0]["note"], "n");
        assert_eq!(v["turn"], 1, "turn must advance so the driver sees a fresh turn");
    }

    #[test]
    fn submit_cancel_writes_marker_and_flips_cancelled() {
        let tmp = make_root("cancel");
        let dir = make_run(tmp.path(), "r", b"# Spec");
        let addr = start_server(tmp.path().to_path_buf());

        let payload = r#"{"decision":"cancel","feedback":"","answers":{},"annotations":[]}"#;
        let (status, body) = http_post(&addr, "/api/runs/r/submit", "application/json", payload);
        assert_eq!(status, 200, "body={body}");
        assert!(body.contains(r#""state":"cancelled""#), "body={body}");

        let marker = fs::read(dir.join("cancelled")).expect("cancelled marker");
        assert_eq!(marker, b"cancelled\n");

        let (_, state_body) = http_get(&addr, "/api/runs/r/state");
        assert!(
            state_body.contains(r#""state":"cancelled""#),
            "{state_body}"
        );

        // Persisted, so a restarted server still reports cancelled.
        let persisted = fs::read_to_string(dir.join("review.state.json")).unwrap();
        assert!(persisted.contains(r#""state":"cancelled""#), "{persisted}");
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
        // The cancel control and its terminal-state overlay must ship with the
        // embedded page — without them the server-side signal is unreachable.
        assert!(body.contains("cancel-btn"), "cancel control missing");
        assert!(
            body.contains("cancelled-overlay"),
            "cancelled overlay missing"
        );
    }

    /// markdown-it 14.1.0, verified against jsDelivr + unpkg. Kept in lockstep
    /// with cli/web/vendor/PROVENANCE.toml. See `vendor_integrity_*` tests below.
    const MARKDOWN_IT_SHA256: &str =
        "38c70a1e7ca91ab40e2d9e6e60129851a717ed1c7d4acbbdd41bf9503791cf68";

    /// Supply-chain tamper detection: the bytes embedded into the binary must
    /// match the pinned digest. A malicious or accidental edit to the vendored
    /// file changes its hash and fails here before it can ship to a reviewer's
    /// browser. If you intentionally update the asset, update the pin AND
    /// PROVENANCE.toml together (see PROVENANCE.toml for the procedure).
    #[test]
    fn vendor_integrity_matches_pin() {
        assert_eq!(
            crate::sha256::hex(MARKDOWN_IT_JS),
            MARKDOWN_IT_SHA256,
            "embedded markdown-it.min.js does not match its pinned SHA-256 — \
             the vendored file was modified without updating the pin"
        );
    }

    /// The provenance record must not silently drift from the enforced pin:
    /// PROVENANCE.toml is what a human audits, the pin is what the test enforces.
    #[test]
    fn vendor_integrity_provenance_in_sync() {
        let manifest = concat!(env!("CARGO_MANIFEST_DIR"), "/web/vendor/PROVENANCE.toml");
        let text = fs::read_to_string(manifest).expect("read PROVENANCE.toml");
        assert!(
            text.contains(MARKDOWN_IT_SHA256),
            "PROVENANCE.toml does not record the pinned markdown-it digest {MARKDOWN_IT_SHA256}"
        );
    }

    // -- the single-server lock ------------------------------------------------

    /// Exclusivity *between processes* is what matters, and it is pinned by
    /// `tests/serve_single.rs` (a second `drovr serve` is refused, and six racing
    /// starts leave one server). Taking the same lock twice inside one process is
    /// explicitly unspecified in std, so this covers the rest of the contract: the
    /// pid lands in the file for humans, and dropping releases the claim.
    ///
    /// Deliberately path-based rather than data-dir based: nothing here touches
    /// `XDG_DATA_HOME`, so it cannot be knocked over by (or knock over) the tests
    /// that do — see "Test suite flakes under parallel `cargo test`" in
    /// `docs/known-issues.md`.
    #[test]
    fn lock_records_our_pid_and_releases_on_drop() {
        let tmp = make_root("lock-claim");
        let path = tmp.path().join("server.pid");

        let held = try_take_lock(&path).expect("claim").expect("uncontended");
        assert_eq!(
            fs::read_to_string(&path).ok().and_then(|s| s.trim().parse().ok()),
            Some(std::process::id()),
            "the holder records its pid for humans to kill"
        );

        // Nothing has to prove the holder died for the lock to be free again.
        drop(held);
        let _held = try_take_lock(&path)
            .expect("claim after release")
            .expect("released lock must be free");
    }

    /// A server that was killed leaves the file behind with its pid in it. The
    /// kernel released the lock when it died, so the file must not wedge a start.
    #[test]
    fn lock_ignores_a_stale_pid_in_the_file() {
        let tmp = make_root("lock-stale");
        let path = tmp.path().join("server.pid");
        fs::write(&path, b"999999").expect("stale pid file");

        let _held = try_take_lock(&path)
            .expect("claim")
            .expect("an unlocked file must be claimable");
        assert_eq!(
            fs::read_to_string(&path).ok().and_then(|s| s.trim().parse().ok()),
            Some(std::process::id()),
            "claiming replaces the dead server's pid with ours"
        );
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

    /// The caller needs the bound address back so it can print the reviewer's
    /// page URL and the matching `drovr review wait` command — opening the gate
    /// is the only run-scoped moment that can remind a driver to start the
    /// watch. Regression guard for "serving a spec doesn't start a watcher".
    #[test]
    fn review_summary_returns_server_addr_for_the_watch_hint() {
        let _guard = crate::test_util::ENV_LOCK.lock().unwrap();
        let (addr, run, _tmp) = global_fixture("rev-summary-addr");

        let returned = review_summary(&run, "Agent summary text.").expect("review_summary");

        assert_eq!(returned, addr, "must return the address it posted to");
        // The CLI interpolates this into `http://{addr}/#/runs/{run}`, so it has
        // to be a bare host:port — a returned URL would produce `http://http://…`.
        assert!(
            !returned.contains("://"),
            "addr must be host:port, not a URL: {returned}"
        );
        assert!(
            returned.rsplit(':').next().is_some_and(|p| p.parse::<u16>().is_ok()),
            "addr must end in a port: {returned}"
        );
    }

    #[test]
    fn display_addr_rewrites_wildcard_binds_only() {
        // `serve --host 0.0.0.0` records a bind target, not a destination.
        assert_eq!(display_addr("0.0.0.0:8791"), "127.0.0.1:8791");
        assert_eq!(display_addr("[::]:8791"), "127.0.0.1:8791");
        // Anything already routable passes through untouched — notably the
        // Tailscale IP a `serve_host` config produces.
        assert_eq!(display_addr("100.71.58.39:8795"), "100.71.58.39:8795");
        assert_eq!(display_addr("127.0.0.1:8791"), "127.0.0.1:8791");
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

    #[test]
    fn summary_on_a_cancelled_run_errors_clearly() {
        // The agent's own exit path: it posts a summary, the run is already
        // cancelled, and the message must name that — not "unexpected response".
        let _guard = crate::test_util::ENV_LOCK.lock().unwrap();
        let (addr, run, _tmp) = global_fixture("summary-cancelled");
        http_post(
            &addr,
            &format!("/api/runs/{run}/submit"),
            "application/json",
            r#"{"decision":"cancel","feedback":"","answers":{},"annotations":[]}"#,
        );

        let err = review_summary(&run, "late revision").expect_err("must reject");
        let msg = err.to_string();
        assert!(msg.contains("cancelled"), "message must name the state: {msg}");
    }

    #[test]
    fn wait_returns_cancelled() {
        let _guard = crate::test_util::ENV_LOCK.lock().unwrap();
        let (addr, run, tmp) = global_fixture("cancel");
        http_post(&addr, &format!("/api/runs/{run}/summary"), "text/plain", "go");

        let run_t = run.clone();
        let handle = thread::spawn(move || review_wait(&run_t, 10_000));
        thread::sleep(Duration::from_millis(200));
        assert!(!handle.is_finished(), "wait must block while `ready`");

        http_post(
            &addr,
            &format!("/api/runs/{run}/submit"),
            "application/json",
            r#"{"decision":"cancel","feedback":"","answers":{},"annotations":[]}"#,
        );
        assert_eq!(
            handle.join().unwrap().expect("wait"),
            WaitOutcome::Cancelled
        );

        assert!(
            tmp.path()
                .join("drovr/runs")
                .join(&run)
                .join("cancelled")
                .exists(),
            "cancelled marker must be on disk"
        );
    }

    #[test]
    fn cancelled_is_terminal_against_a_late_summary() {
        // The realistic race: the human cancels while the agent is still
        // mid-revision, then the agent (unaware) posts its summary. If that
        // flipped the run back to `ready`, the cancelled signal would be lost
        // and the driver would resume waiting — exactly the bug this fixes.
        let tmp = make_root("cancel-terminal-summary");
        let dir = make_run(tmp.path(), "r", b"# Spec");
        let addr = start_server(tmp.path().to_path_buf());

        let payload = r#"{"decision":"cancel","feedback":"","answers":{},"annotations":[]}"#;
        assert_eq!(
            http_post(&addr, "/api/runs/r/submit", "application/json", payload).0,
            200
        );

        let (status, _) = http_post(&addr, "/api/runs/r/summary", "text/plain", "late revision");
        assert_eq!(status, 409, "a cancelled run must reject a late summary");

        let (_, state_body) = http_get(&addr, "/api/runs/r/state");
        assert!(state_body.contains(r#""state":"cancelled""#), "{state_body}");
        let persisted = fs::read_to_string(dir.join("review.state.json")).unwrap();
        assert!(persisted.contains(r#""state":"cancelled""#), "{persisted}");
    }

    #[test]
    fn cancelled_is_terminal_against_a_late_submit() {
        let tmp = make_root("cancel-terminal-submit");
        make_run(tmp.path(), "r", b"# Spec");
        let addr = start_server(tmp.path().to_path_buf());

        let cancel = r#"{"decision":"cancel","feedback":"","answers":{},"annotations":[]}"#;
        assert_eq!(
            http_post(&addr, "/api/runs/r/submit", "application/json", cancel).0,
            200
        );

        // Any later decision — including an approve — must not un-cancel.
        let approve = r#"{"decision":"approve","feedback":"","answers":{},"annotations":[]}"#;
        let (status, _) = http_post(&addr, "/api/runs/r/submit", "application/json", approve);
        assert_eq!(status, 409, "a cancelled run must reject a late submit");

        let (_, state_body) = http_get(&addr, "/api/runs/r/state");
        assert!(state_body.contains(r#""state":"cancelled""#), "{state_body}");
    }

    #[test]
    fn approved_is_terminal_against_a_late_summary() {
        // Same guarantee for the other terminal state: approval is a decision,
        // not a phase the agent can walk back.
        let tmp = make_root("approve-terminal");
        make_run(tmp.path(), "r", b"# Spec");
        let addr = start_server(tmp.path().to_path_buf());

        let payload = r#"{"decision":"approve","feedback":"","answers":{},"annotations":[]}"#;
        assert_eq!(
            http_post(&addr, "/api/runs/r/submit", "application/json", payload).0,
            200
        );

        let (status, _) = http_post(&addr, "/api/runs/r/summary", "text/plain", "late revision");
        assert_eq!(status, 409, "an approved run must reject a late summary");

        let (_, state_body) = http_get(&addr, "/api/runs/r/state");
        assert!(state_body.contains(r#""state":"approved""#), "{state_body}");
    }

    #[test]
    fn cancelled_state_round_trips() {
        // The wire/persistence spelling the driver and the UI both key off.
        assert_eq!(LoopState::Cancelled.as_str(), "cancelled");
        assert_eq!(LoopState::from_str("cancelled"), LoopState::Cancelled);
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
