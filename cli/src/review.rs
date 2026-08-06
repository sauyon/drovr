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
    /// Per-run cache of the blocked-agent scan, on the same shape as `cells`:
    /// the outer lock is held only long enough to hand out the run's cell, and
    /// the scan itself — which talks to herdr — happens under that run's own
    /// lock, so one wedged run cannot stall the session list.
    blocked: Mutex<HashMap<String, Arc<Mutex<BlockedCache>>>>,
    /// `host:port` values this server will answer state-changing requests for.
    /// Empty means "reject every write" — fail closed, so a construction path
    /// that forgets to populate it cannot silently disable the guard.
    allowed_hosts: Vec<String>,
    /// Set when bound to a wildcard address (`0.0.0.0`/`::`), where the reachable
    /// addresses cannot be enumerated ahead of time. Then any `Host` that is an
    /// IP *literal* on this port is accepted. That keeps a LAN/Tailscale reviewer
    /// working while still defeating rebinding, which fundamentally needs a DNS
    /// *name* — `evil.example:8791` is not an IP literal and stays refused.
    wildcard_port: Option<u16>,
}

/// How long a blocked-agent scan stands in for the live answer.
///
/// The page polls every 2s and a scan costs a herdr round-trip per live pane, so
/// scanning per poll would put a permanent load on herdr for a fact that changes
/// on human timescales. Five seconds decouples the two: the browser keeps its 2s
/// rhythm, herdr sees at most one scan per run per 5s however many tabs are
/// open, and the worst case is that a badge appears 5s after the agent stopped.
const BLOCKED_TTL: Duration = Duration::from_secs(5);

/// How long an INCONCLUSIVE sweep stands in — shorter, because it is not an
/// answer, but not zero.
///
/// Not caching it at all was the first fix, and it has a failure mode of its
/// own: while herdr is down or hung, every poll from every tab re-sweeps every
/// live run, each sweep waiting out its own socket, and the requests stack on
/// the server. A short floor bounds that to one attempt per run per second while
/// still noticing herdr's return promptly.
///
/// It costs nothing in honesty, and that is why it is safe: an inconclusive
/// sweep is now REPORTED as inconclusive (`blocked.inconclusive` on the wire),
/// so what is being cached for a second is "we do not know", never "you are
/// fine".
const BLOCKED_RETRY_TTL: Duration = Duration::from_secs(1);

/// One run's last blocked sweep, and when it was taken. How long it stands in
/// depends on what it was: [`BLOCKED_TTL`] for an answer, the much shorter
/// [`BLOCKED_RETRY_TTL`] for a sweep that learned nothing.
///
/// The whole [`crate::blocked::RunScan`] is kept, not a projection of it: the
/// blocked list means one thing when the sweep reached the run's panes and
/// another when it did not, and a cache that stored only the list is exactly how
/// a herdr outage came to render as a clean row.
struct BlockedCache {
    at: Option<Instant>,
    scan: crate::blocked::RunScan,
}

impl Ctx {
    fn new(runs_root: PathBuf, allowed_hosts: Vec<String>) -> Self {
        Ctx {
            runs_root,
            cells: Mutex::new(HashMap::new()),
            blocked: Mutex::new(HashMap::new()),
            allowed_hosts,
            wildcard_port: None,
        }
    }

    /// This run's blocked agents, re-scanned at most once per [`BLOCKED_TTL`].
    ///
    /// Takes the herdr client rather than making one so a test can drive it with
    /// a `FakeHerdr`; the client is never stored, only the plain-data result is.
    fn blocked_of<H: Herdr>(&self, h: &H, run: &str, state: &RunState) -> crate::blocked::RunScan {
        let cell = {
            let mut map = self.blocked.lock().unwrap_or_else(|e| e.into_inner());
            map.entry(run.to_string())
                .or_insert_with(|| {
                    Arc::new(Mutex::new(BlockedCache {
                        at: None,
                        scan: crate::blocked::RunScan::default(),
                    }))
                })
                .clone()
        };
        let mut cache = cell.lock().unwrap_or_else(|e| e.into_inner());
        // A sweep that reached no pane holds for a second, not for five: it is
        // not an answer, so the badge must not keep repeating it after herdr
        // comes back — but the retry floor keeps a hung herdr from being swept
        // once per request per tab. Both halves matter, and which TTL applies is
        // decided by the CACHED sweep, not the incoming one.
        let ttl = if cache.scan.inconclusive() {
            BLOCKED_RETRY_TTL
        } else {
            BLOCKED_TTL
        };
        let fresh = cache.at.is_some_and(|at| at.elapsed() < ttl);
        if !fresh {
            cache.scan = crate::blocked::scan_run(h, state);
            cache.at = Some(Instant::now());
        }
        cache.scan.clone()
    }

    fn with_wildcard_port(mut self, port: Option<u16>) -> Self {
        self.wildcard_port = port;
        self
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
///
/// `pub(crate)` because the review panel interpolates the SAME recorded file into the
/// same shape of git command (`code_review::base_sha`) and must not grow a second,
/// drifting opinion about what a base may contain. One validator, both call sites.
pub(crate) fn safe_sha(sha: &str) -> bool {
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

/// Read a request header case-insensitively.
fn header_of(req: &Request, name: &'static str) -> Option<String> {
    req.headers()
        .iter()
        .find(|h| h.field.equiv(name))
        .map(|h| h.value.as_str().to_string())
}

/// Whether a state-changing request may proceed.
///
/// Two checks, and the FIRST is the load-bearing one:
///
/// 1. **`Host` must be an address this server actually serves.** Comparing
///    `Origin` against `Host` alone is not enough, because a browser derives both
///    from the same URL the page was loaded from — so they agree by construction
///    even when that URL is the attacker's. That is DNS rebinding: a page served
///    from `evil.example:8791` whose DNS is then re-pointed at `127.0.0.1` sends
///    `Origin: http://evil.example:8791` and `Host: evil.example:8791`, matching
///    each other perfectly while the connection lands here. Checking `Host`
///    against the addresses we bound breaks that: the forged name is not one of
///    them. This matters most for `/send` and `/keys`, which type into a live
///    agent's pane — remote command injection, not a flag flip.
/// 2. **A present `Origin` must be same-origin.** Catches the ordinary
///    cross-origin POST. The opaque `null` (sandboxed iframe, `file://` page)
///    fails this: it can never legitimately be this server.
///
/// A missing `Origin` is not a browser cross-origin write at all — curl and
/// drovr's own CLI send none — so check 1 alone governs those.
fn write_allowed(req: &Request, allowed_hosts: &[String], wildcard_port: Option<u16>) -> bool {
    let Some(host) = header_of(req, "Host") else {
        return false;
    };
    let host = host.to_ascii_lowercase();
    if !allowed_hosts.iter().any(|h| h == &host) && !wildcard_ip_host(&host, wildcard_port) {
        return false;
    }
    match header_of(req, "Origin") {
        None => true,
        // Lowercased on both sides: `host` was normalised above, so comparing a
        // raw origin against it would reject on nothing but casing. Bound to a
        // local rather than chained, so nothing borrows from a temporary.
        Some(origin) => {
            let origin = origin.to_ascii_lowercase();
            origin.split("://").nth(1) == Some(host.as_str())
        }
    }
}

/// Whether `host` is an IP literal on the wildcard bind's port.
///
/// Only reachable when the server bound `0.0.0.0`/`::`, where the set of
/// addresses a reviewer might legitimately use is not knowable up front. An IP
/// literal is safe to accept because DNS rebinding needs a *name*: the attacker
/// controls `evil.example`'s resolution, not the literal `192.168.1.5`, and a
/// page served from a bare IP has no rebinding lever at all.
fn wildcard_ip_host(host: &str, wildcard_port: Option<u16>) -> bool {
    let Some(port) = wildcard_port else {
        return false;
    };
    let Some((h, p)) = host.rsplit_once(':') else {
        return false;
    };
    if p.parse::<u16>() != Ok(port) {
        return false;
    }
    h.trim_start_matches('[')
        .trim_end_matches(']')
        .parse::<std::net::IpAddr>()
        .is_ok()
}

/// Whether `host` names a wildcard bind ("listen on every interface").
///
/// Parsed rather than string-matched: `serve_host` is a free-form String with
/// only a non-empty check (`cli/src/config.rs`), so a user can legitimately write
/// `::0`, `[::0]` or `0:0:0:0:0:0:0:0`. A spelling this misses is not a security
/// hole — it fails closed — but it is a silent lockout: the page loads and every
/// button 403s, which is a miserable thing to debug.
fn is_wildcard_host(host: &str) -> bool {
    host.trim_start_matches('[')
        .trim_end_matches(']')
        .parse::<std::net::IpAddr>()
        .is_ok_and(|ip| ip.is_unspecified())
}

/// The `host:port` values a server bound to `host:port` legitimately answers to.
/// Loopback aliases are included because the reviewer may reach the UI by any of
/// them; a forged name is excluded precisely because it is not in this list.
fn allowed_hosts_for(host: &str, port: u16) -> Vec<String> {
    // Bracket a bare IPv6 literal: browsers send `Host: [::1]:8791`, never `::1:8791`.
    let display = if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.to_string()
    };
    let mut out = vec![format!("{display}:{port}").to_ascii_lowercase()];
    // Parsed, not string-matched, for the same reason as `is_wildcard_host`:
    // `serve_host` is free-form, so `::1`, `[::1]` and `0:0:0:0:0:0:0:1` are all
    // spellings of loopback a user could reasonably write.
    let parsed = host
        .trim_start_matches('[')
        .trim_end_matches(']')
        .parse::<std::net::IpAddr>();
    let loopback_ish = host.eq_ignore_ascii_case("localhost")
        || parsed.is_ok_and(|ip| ip.is_loopback() || ip.is_unspecified());
    if loopback_ish {
        for alias in ["127.0.0.1", "localhost", "[::1]"] {
            let candidate = format!("{alias}:{port}").to_ascii_lowercase();
            if !out.contains(&candidate) {
                out.push(candidate);
            }
        }
    }
    out
}

fn handle(req: Request, ctx: &Arc<Ctx>) {
    let method = req.method().clone();
    let url = req.url().to_string();
    let path = url.split('?').next().unwrap_or("/").to_string();

    // Cross-origin writes are refused. This server has no authentication, so
    // without it any page the user happens to visit while it is running can POST
    // here from their browser: `fetch(url, {mode:'no-cors', ...})` sends a simple
    // request with no preflight, and CORS only stops the attacker READING the
    // reply — the side effect has already happened. That now includes closing a
    // herdr workspace and killing a live agent's panes (`/archive`), typing into
    // a live pane (`/send`), and approving a spec (`/submit`).
    //
    // Checking `Origin` is enough and costs nothing: browsers always attach it to
    // a cross-origin request and cannot be talked out of it from script, while
    // curl and drovr's own CLI send no `Origin` at all and are unaffected.
    if method == Method::Post && !write_allowed(&req, &ctx.allowed_hosts, ctx.wildcard_port) {
        eprintln!("drovr: refused an untrusted POST to {path}");
        respond_str(req, 403, "text/plain", "untrusted write refused".into());
        return;
    }

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
        // One herdr call answers liveness for every row (see `workspace_list`).
        let h = crate::herdr::SystemHerdr::new();
        let live = h.workspace_list();
        respond_str(
            req,
            200,
            "application/json",
            list_runs_json(ctx, &h, live.as_deref()),
        );
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

        // POST archive — file a run away (or restore it). Body: {"archived":bool}.
        //
        // Archiving is the browser's equivalent of `drovr cleanup`: it closes the
        // run's herdr workspace and sets the flag. Restoring only clears the flag
        // — closed panes cannot be reopened, and the run resumes via
        // `drovr phase start` with a fresh agent seeded from its handoff.
        (Method::Post, "archive") => handle_archive(req, ctx, run),

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
        (Method::Get, "agents") => handle_get_agents(req, ctx, run, &p),

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
/// `POST /api/runs/<run>/archive` — body `{"archived":true|false}`.
///
/// The browser-side twin of `drovr cleanup`, minus the worktree pruning: that
/// path can squash-commit and delete branches, which is not something a button
/// should do without a git-aware conversation. Panes and the flag are enough to
/// get a dead run out of the way; `drovr cleanup --purge` remains the way to
/// reclaim the worktree.
///
/// Restore clears the flag, puts the row back in the active list, and the run is
/// runnable again: archiving destroys the workspace, but `phase::ensure_workspace`
/// re-provisions one on the next `phase_start` (in the run's `project_dir`) and
/// records the new ids. What it cannot restore is the *agents* — every pane died
/// with the workspace, so a phase that was `Running` when it was archived comes
/// back `Failed` and has to be restarted. See docs/known-issues.md, "Restoring an
/// archived run does not make it runnable again — FIXED 2026-08-02".
/// Close `state`'s herdr workspace when archiving; report whether it closed.
///
/// Split out of [`handle_archive`] and generic over [`Herdr`] so the destructive
/// half — the part that kills panes — can be driven by `FakeHerdr` in a test.
/// The handler itself is bound to `Request`/`SystemHerdr` and cannot be.
///
/// Restoring never touches herdr: closed panes cannot be reopened, so a restore
/// is purely a flag change.
fn close_for_archive<H: Herdr>(h: &H, state: &RunState, archived: bool) -> bool {
    if !archived {
        return false;
    }
    let Some(ws) = state.workspace.as_deref() else {
        return false;
    };
    // The SAME teardown `drovr cleanup` performs — deliberately, not incidentally.
    // This used to call `workspace_close` outright, which killed every pane in the
    // workspace including the shell or editor the reviewer had open in it. Cleanup
    // was hardened against exactly that; the button is one click and must not be
    // the careless path to the same destruction.
    //
    // A `false` return now means "the workspace is still standing", which covers
    // both a failed close and one deliberately withheld because the human's panes
    // are in there. Both deserve the page's warning: in each case the run may
    // still have something live attached to it.
    crate::close_run_panes(state, ws, h)
}

fn handle_archive(mut req: Request, ctx: &Arc<Ctx>, run: &str) {
    let body = read_body(&mut req);
    let want = serde_json::from_str::<serde_json::Value>(&body)
        .ok()
        .and_then(|v| v.get("archived").and_then(|a| a.as_bool()));
    let Some(archived) = want else {
        respond_str(req, 400, "text/plain", "body must be {\"archived\":bool}".into());
        return;
    };

    let dir = ctx.runs_root.join(run);
    let Some(state) = load_run_state(&dir) else {
        // "Not there" and "there but unreadable" are different answers, and the
        // reviewer can tell them apart: `list_runs_in` lists any run whose
        // state.json merely EXISTS, so an unparseable one is visible on the page
        // with a working-looking Archive button. Answering 404 to a click on a row
        // they can plainly see reads as a bug in the page.
        let (code, msg): (u16, &str) = if dir.join("state.json").is_file() {
            (409, "run state is unreadable; fix or remove its state.json")
        } else {
            (404, "no such run")
        };
        respond_str(req, code, "text/plain", msg.into());
        return;
    };

    // Close the workspace BEFORE flipping the flag, and only when archiving. If
    // the close fails for a reason other than "already gone" we still archive:
    // the flag is about how the run is filed, and refusing to file it would
    // leave the reviewer with a row they cannot clear from the browser.
    let workspace_closed = close_for_archive(&crate::herdr::SystemHerdr::new(), &state, archived);

    // Re-read before writing. `save_in` rewrites the WHOLE file, and the close
    // above is a blocking round-trip to the herdr daemon — a phase agent can
    // land its own `state.json` write during it (recording a phase as Done, say),
    // which writing back the copy loaded above would silently revert. This server
    // is multi-threaded and this endpoint is a button a human can hit mid-phase,
    // so the window is far more reachable than the equivalent one in
    // `cmd_cleanup`. Re-reading narrows it to this function's own last two
    // statements; closing it completely needs locking in `RunState::save`
    // (docs/known-issues.md).
    let mut state = load_run_state(&dir).unwrap_or(state);
    state.archived = archived;
    if let Err(e) = state.save_in(&dir) {
        eprintln!("drovr archive: could not save run '{run}': {e}");
        respond_str(req, 500, "text/plain", "could not save run state".into());
        return;
    }
    respond_str(
        req,
        200,
        "application/json",
        serde_json::json!({
            "ok": true,
            "archived": archived,
            "workspace_closed": workspace_closed,
        })
        .to_string(),
    );
}

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
/// * **409** — the phase is not in a state to be brought back: it still holds a
///   pane, it has never run (`drovr new` pre-seeds `Pending` placeholders, and
///   starting one is `phase start`'s job), it never got an agent, or it is a
///   reviewer. Every one of them is [`RunState::rehydratable`], read from the
///   same `state.json` the CLI reads, so the status code and the CLI's refusal
///   cannot disagree.
///
/// And one non-refusal worth its own line:
///
/// * **200 with `complete: false`** — child exit 2. The pane IS back, but the
///   agent in it was not confirmed to have the phase's context. Neither a 500
///   (which would claim nothing happened) nor a plain `ok: true` (which would
///   let a caller checking only the status treat it as fully recovered).
///
/// # ⚠️ This endpoint is unauthenticated and it starts agent processes
///
/// Recorded here because it is a real question with a considered answer, not
/// an oversight — and because the next person to read this handler will ask it.
///
/// **It is not a new capability class.** This server is unauthenticated by
/// design and already offers arbitrary code execution: `POST /send` types into
/// a live claude pane, and a claude pane runs bash. Nothing here widens what a
/// caller who can reach the port may do.
///
/// **What it does add is process creation** — talking to an existing agent
/// versus starting a new one — so the question is resource consumption, and
/// the answer is that it is bounded by the run's own shape rather than by the
/// caller's persistence:
///
/// 1. A caller cannot invent a target. Rehydrate NEVER appends a phase (unlike
///    `phase_start`), so the 404 above confines it to phases already in
///    `state.json`, and the `NeverStarted` / `NoAgentEverRan` refusals confine
///    it further to phases that have actually run.
/// 2. A caller cannot multiply agents on one phase. `HoldsPane` refuses a phase
///    that already has a pane, and a successful rehydrate records one — so the
///    ceiling is one live agent per already-run phase, which is exactly the
///    steady state the run has when nothing is reaped.
/// 3. A flood cannot beat the check. `phase_rehydrate` takes an exclusive
///    per-run lock and re-reads `state.json` under it, so concurrent requests
///    serialize and every one after the first sees the pane the first recorded.
///
/// **The residual cost, accepted:** each request can occupy one server worker
/// for up to `SEND_READY_TIMEOUT` (30s) while the agent comes up, and a caller
/// may restart a reaped phase's agent once. The first is the same slow-handler
/// exposure `GET /pane` has against a slow herdr; the second is one process for
/// a phase that is supposed to have one. Neither justifies inventing an auth
/// scheme for a server whose front door is already open by design — and a
/// half-measure here would read as protection this endpoint does not have.
/// `serve_host` defaults to `127.0.0.1`, which is the actual boundary.
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
    // The SAME predicate `phase_rehydrate` gates on, off the same `state.json`
    // — not a re-derivation. Written separately, this path checked two of the
    // CLI's three refusals, so the button a human clicks was more permissive
    // than the command it shells out to.
    //
    // `NoSuchPhase` is the one arm that is a 404 (the name does not exist here
    // — and `safe_component` permits `:` and is a path check, NOT an
    // authorization one). Every other arm names a real phase drovr is refusing
    // to act on, which is a 409.
    if let Err(why) = state.rehydratable(&phase) {
        use crate::run::NotRehydratable as Why;
        let msg = match &why {
            Why::NoSuchPhase => "no such phase".to_string(),
            Why::Reviewer => format!(
                "phase '{phase}' is a review-panel agent — its findings channel cannot be \
                 re-attached to a resumed session; run the panel again instead"
            ),
            Why::HoldsPane(pane) => format!("phase '{phase}' still holds pane {pane}"),
            Why::NeverStarted => {
                format!("phase '{phase}' has never run — start it, don't rehydrate it")
            }
            Why::NoAgentEverRan => {
                format!("phase '{phase}' has no agent on record — start it again, with its seed")
            }
            Why::NoProjectDir => {
                format!("run '{run_name}' records no project_dir, so there is nowhere to launch")
            }
            Why::NoWorkspace => {
                format!("run '{run_name}' has no herdr workspace to open a tab in")
            }
        };
        let code = if why == Why::NoSuchPhase { 404 } else { 409 };
        respond_str(req, code, "text/plain", msg);
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
    // ⚠️ `--` before the positionals is LOAD-BEARING, not tidiness. The exit-2
    // arm below reads exit 2 as the CLI's "incomplete" outcome — but clap uses
    // exit 2 for its own usage errors, and neither `safe_component` here nor
    // `require_phase_name` in the CLI rejects a phase name starting with `-`.
    // Without the `--`, `?phase=-weird` would make clap fail to parse, exit 2,
    // and be reported as "the pane is back but incomplete" when in fact nothing
    // was ever created. `--` makes every positional a value, so exit 2 from the
    // child can only come from `phase_rehydrate`.
    match Command::new(&exe)
        .args(["phase", "rehydrate", "--", run_name, &phase])
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
        // ⚠️ Exit 2 is the CLI's "the pane is back, but the agent was NOT
        // CONFIRMED to have this phase's context". It must NOT flatten into
        // either bucket: a 500 would claim nothing happened (a pane really was
        // created and recorded), and a plain `ok: true` would let a caller
        // checking only the status treat an unconfirmed agent as fully
        // recovered. 200 with `complete: false`, and `detail` carries the CLI's
        // stderr note — which is what says WHICH of the five states it was, a
        // distinction this status code cannot make and must not appear to.
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

/// One blocked agent as the browser sees it: what it is blocked on, the pane
/// tail so the reviewer can read the prompt without attaching, and whether it
/// needs a human (a routine prompt does not — see
/// [`crate::phase::BlockedClass::needs_human`]).
fn blocked_json(a: &crate::blocked::BlockedAgent) -> serde_json::Value {
    serde_json::json!({
        "class": a.class.as_str(),
        "needs_human": a.class.needs_human(),
        "excerpt": a.excerpt,
        "pane_id": a.pane_id,
    })
}

/// The session list's one-line verdict on a run: `null` when nothing is blocked,
/// otherwise `{count, phase, class, human_phases}`.
///
/// The named phase is the first one NEEDING A HUMAN where there is one, because
/// that is the row the badge is asking someone to act on; it falls back to the
/// first blocked phase so a run held up only by routine prompts still says which
/// agent is sitting there.
///
/// `human_phases` lists EVERY phase needing a human rather than counting them,
/// and that is what the browser raises alarms from: an alarm is per phase (it
/// carries a phase's name and is cleared when that phase's block clears), so a
/// count would let a run's second simultaneous block go unannounced. It is also
/// why there is no `needs_human` field here — the per-node
/// [`blocked_json`] already spends that name on a bool, and the same name
/// holding a bool on one endpoint and a number on a neighbouring one is a shape
/// no typed client can share.
/// `scan` is `None` when no sweep was possible at all — the run's own
/// `state.json` would not parse, so drovr never learned which panes it has.
/// That is reported exactly like a sweep that reached nothing, because it is the
/// same fact: `inconclusive`, with nothing found.
///
/// Passing `None` rather than a fabricated `RunScan { unreadable: 1 }` keeps
/// `RunScan::unreadable` meaning what it says — PANES herdr would not answer for
/// — which is the same distinction `WatchScope::unparseable_runs` is named apart
/// for. A run-level failure is not one unreadable pane.
fn blocked_summary_json(scan: Option<&crate::blocked::RunScan>) -> serde_json::Value {
    let empty = crate::blocked::RunScan::default();
    let agents = &scan.unwrap_or(&empty).blocked;
    let inconclusive = scan.is_none_or(crate::blocked::RunScan::inconclusive);
    let human_phases: Vec<&str> = agents
        .iter()
        .filter(|a| a.class.needs_human())
        .map(|a| a.phase.as_str())
        .collect();
    let Some(lead) = agents
        .iter()
        .find(|a| a.class.needs_human())
        .or_else(|| agents.first())
    else {
        // Nothing blocked — but only say so when the sweep actually reached the
        // run's panes. `null` is what the browser reads as "this run is fine",
        // including as permission to CLEAR an alarm it already raised, so a
        // sweep that reached nothing must answer something else.
        return if inconclusive {
            serde_json::json!({
                "count": 0,
                "phase": serde_json::Value::Null,
                "class": serde_json::Value::Null,
                "human_phases": [],
                "inconclusive": true,
            })
        } else {
            serde_json::Value::Null
        };
    };
    serde_json::json!({
        "count": agents.len(),
        "phase": lead.phase,
        "class": lead.class.as_str(),
        "human_phases": human_phases,
        // Blocks WERE found, so the row has something true to say; `unknown`
        // still travels because some other pane of the run went unread and a
        // further block may be hiding behind it.
        "inconclusive": inconclusive,
    })
}

/// `GET /api/runs/<run>/agents` — the tree of spawned agents: each phase pane
/// with its per-task review panels nested beneath it. Only agents that actually
/// have a pane appear (unstarted placeholder phases are omitted).
fn handle_get_agents(req: Request, ctx: &Arc<Ctx>, run: &str, p: &RunPaths) {
    // A config that fails to load must not blank the tree — but it must not be
    // SILENT either. `resumable` is computed from the agent map, so a config
    // drovr could not read means every ⟳ on this page is decided by the
    // built-in backends rather than the user's, and the button then appears (or
    // vanishes) for a reason nothing on screen explains. Serve the tree, carry
    // the reason with it, and let the page say so.
    let (cfg, config_error) = match crate::config::load_config() {
        Ok(cfg) => (cfg, None),
        Err(e) => (
            crate::config::Config::default(),
            Some(format!(
                "drovr could not read your config ({e}), so the ⟳ buttons below are decided \
                 by the built-in agent map — a resume surface you configured yourself is not \
                 reflected here."
            )),
        ),
    };
    // `inconclusive` travels with the tree for the same reason it travels with a
    // session-list row, and it matters MORE here: on a run's page this endpoint
    // is the browser's only feed of blocked state (the list poll has stopped),
    // so without it a herdr blip would render every node `blocked: null` and the
    // page would clear an alarm it had already raised — reporting "all clear"
    // from a sweep that reached nothing.
    //
    // An unreadable `state.json` is the same class of non-answer: it is not a
    // run with no agents, it is a run we could not read.
    let tree = match load_run_state(&p.dir) {
        Some(state) => {
            let scan = ctx.blocked_of(&crate::herdr::SystemHerdr::new(), run, &state);
            let mut tree = build_agent_tree(&state, &cfg, config_error.as_deref(), &scan.blocked);
            tree["inconclusive"] = serde_json::Value::Bool(scan.inconclusive());
            tree
        }
        None => serde_json::json!({
            "workspace": serde_json::Value::Null,
            "nodes": [],
            "config_error": config_error,
            "inconclusive": true,
        }),
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

/// Whether a rehydrate would bring the CONVERSATION back, as opposed to
/// launching a fresh agent that re-reads the notes.
///
/// **This is not what gates the ⟳ button** — `rehydratable` is (a phase with no
/// session is still worth bringing back; it reseeds). This decides what the
/// button PROMISES, so the two are reported separately and each named for what
/// it means. Reporting only this under a name the button reads would put a ⟳ on
/// phases the CLI then refuses, and hide it from phases it would have recovered.
///
/// Both halves matter, and the second is easy to miss: codex ships with no
/// resume surface at all, so a codex phase can carry a perfectly good session id
/// and still only be relaunchable.
fn has_resumable_session(phase: &crate::run::Phase, cfg: &crate::config::Config) -> bool {
    phase
        .resume_target()
        .is_some_and(|target| cfg.resume_surface(target.backend()).is_some())
}

/// Build the agent tree for `run`: phases (started, or reaped) as top-level nodes,
/// with review panels (`review:<task>:<iter>:<angle>`) nested under the matching
/// `implement-<task>` phase. Reviews with no matching phase land in a trailing
/// group node so nothing is dropped.
///
/// `config_error` is why `cfg` is NOT the user's own config, when it is not. It
/// travels with the tree rather than being logged, because the poll that builds
/// this runs every two seconds — a log line would be either spam or invisible,
/// and the person it concerns is looking at the page, not the server's stderr.
fn build_agent_tree(
    run: &RunState,
    cfg: &crate::config::Config,
    config_error: Option<&str>,
    blocked: &[crate::blocked::BlockedAgent],
) -> serde_json::Value {
    use std::collections::BTreeMap;
    // Keyed by PHASE NAME, not pane id: the tree node knows its phase, and a
    // pane id can be recycled by a rehydrate between the scan and this render.
    let blocked_for = |name: &str| {
        blocked
            .iter()
            .find(|a| a.phase == name)
            .map(blocked_json)
            .unwrap_or(serde_json::Value::Null)
    };
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
                "reaped": rp.is_reaped(), "rehydratable": run.rehydratable(&rp.name).is_ok(),
                "resumable": has_resumable_session(rp, cfg),
                "blocked": blocked_for(&rp.name),
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
            "reaped": ph.is_reaped(), "rehydratable": run.rehydratable(&ph.name).is_ok(),
            "resumable": has_resumable_session(ph, cfg),
            "blocked": blocked_for(&ph.name),
            "children": children,
        }));
    }
    for (task, revs) in reviews_by_task {
        nodes.push(serde_json::json!({
            "name": format!("reviews: {task}"), "kind": "group",
            "status": "", "pane_id": serde_json::Value::Null, "children": revs,
        }));
    }
    serde_json::json!({
        "workspace": run.workspace,
        "nodes": nodes,
        "config_error": config_error,
    })
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
///
/// `live_workspaces` is the set of workspace ids herdr currently has open, or
/// `None` when herdr could not be reached. Each row reports `live` as
/// `true`/`false`/`null` accordingly — `null` is "unknown", and the UI must treat
/// it with the same caution as `true` (it gates a workspace-closing archive).
///
/// `h` is the herdr client the blocked scan runs on. Passed in rather than made
/// here so a test can drive the scan with a `FakeHerdr`, and so the ONE
/// `workspace_list` the caller already made and this scan come from the same
/// client.
fn list_runs_json<H: Herdr>(ctx: &Arc<Ctx>, h: &H, live_workspaces: Option<&[String]>) -> String {
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
        let live = match (live_workspaces, run_state.as_ref()) {
            // herdr could not be reached: unknown for every run.
            (None, _) => None,
            // The run's own state.json did not parse, so we never learned its
            // workspace id. That is UNKNOWN, not "no panes" — the ambiguity is
            // this run's file rather than herdr, but the answer is the same, and
            // claiming `false` asserts a fact we have no basis for. `list_runs_in`
            // only checks state.json EXISTS, so such runs really are listed.
            (Some(_), None) => None,
            (Some(ids), Some(st)) => match st.workspace.as_deref() {
                // Parsed, and genuinely has no workspace recorded.
                None => Some(false),
                Some(ws) => Some(ids.iter().any(|i| i == ws)),
            },
        };
        // Liveness gates the ARCHIVED case only, and nothing else.
        //
        // Archiving sets the flag even when closing the workspace failed, so an
        // archived run with an open workspace is a zombie: filed away while an
        // agent still runs in panes we believe we shut. That one stays in the
        // active list, because a fold is exactly where it must not go.
        //
        // Finishing every phase is different. Nothing closes a workspace on
        // completion — only `cleanup` and this endpoint ever call
        // `workspace_close` — so a normally-finished run keeps its workspace open
        // indefinitely. Gating on liveness there stranded EVERY finished run in
        // the active list, which is the clutter this feature exists to remove.
        let archived = run_state.as_ref().is_some_and(|s| s.archived);
        // A zombie is specifically an ARCHIVED run whose workspace is still open:
        // the human asked to close it, the close failed, and nothing else reports
        // that. Surfaced regardless of phase progress — the anomaly is that an
        // explicit request did not take effect, which is worth seeing whether or
        // not the pipeline happened to finish.
        //
        // Going through `is_complete()` rather than recomputing keeps its
        // empty-phases guard: a run whose state.json will not parse stays visible
        // instead of being hidden as finished.
        //
        // `Some(true)` deliberately, NOT `!= Some(false)`. The asymmetry with the
        // archive confirm — which DOES treat unknown as live — is the point:
        //
        // * The confirm gates a destructive act. Unknown must warn, because being
        //   wrong there means killing a live agent.
        // * This decides whether to assert "panes still live" on a row. Unknown
        //   asserting it would stamp that warning on EVERY archived run whenever
        //   `herdr workspace list` blips — false alarms on a claim we cannot
        //   support, which is exactly how a warning stops being read.
        //
        // The cost is that a genuine zombie collapses while herdr is unreachable.
        // That is transient and self-healing — the next successful poll surfaces
        // it again — and `live: null` on the row lets the UI tell the reviewer
        // liveness is unknown rather than let them read the grouping as fact.
        let zombie = archived && live == Some(true);
        let complete = (run_state.as_ref().is_some_and(|s| s.is_complete())
            || rs.state == LoopState::Cancelled)
            && !zombie;
        // Blocked agents, for the row's badge. The scan is TTL-cached
        // (`Ctx::blocked_of`), so the 2s list poll costs at most one herdr sweep
        // per run per `BLOCKED_TTL`.
        //
        // **Liveness is the ONLY gate**, and the two things it is deliberately
        // not are both cases where an agent can be stuck:
        //
        // * `complete` — `is_complete()` walks `phases` only, so a run whose
        //   pipeline finished while a REVIEW PANEL is still up reads as
        //   complete, and a reviewer on a destructive prompt is exactly the
        //   block nobody would otherwise notice.
        // * `archived` — the zombie two lines above IS an archived run whose
        //   panes are still live, kept in the active list precisely because
        //   something is running in panes we believe we closed. Skipping it
        //   would blind the one row the list singles out as needing attention.
        //
        // `live == None` (herdr unreachable) still scans, and the sweep reports
        // its own uncertainty (`unknown` below) rather than fabricating a clean
        // answer.
        let blocked = match run_state.as_ref() {
            Some(st) if live != Some(false) => {
                blocked_summary_json(Some(&ctx.blocked_of(h, &name, st)))
            }
            // The run's own `state.json` would not parse, so we never learned
            // which panes it has — no sweep was possible at all. That is not
            // "nothing is blocked", and `null` is read as exactly that,
            // including as permission to clear an alarm already raised.
            // `list_runs_in` only checks the file EXISTS, so such rows really
            // are listed.
            None => blocked_summary_json(None),
            // Workspace confirmed gone: its panes went with it, and there is
            // genuinely nothing that can be blocked.
            _ => serde_json::Value::Null,
        };
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
                "archived": archived,
                // Whether the run's herdr workspace is still open. `null` when
                // herdr could not be asked — never coerced to `false`, because
                // archiving closes panes and "unknown" must not read as "safe".
                "live": match live {
                    None => serde_json::Value::Null,
                    Some(b) => serde_json::Value::Bool(b),
                },
                // `null` when a sweep confirmed no agent of this run is parked
                // on a prompt. Otherwise `{count, phase, class, human_phases,
                // unknown}` — and it is `human_phases`, not `count`, that earns
                // the alarm: a routine permission dialog is answered by whatever
                // driver is waiting. `unknown` says the sweep could not reach
                // the run's panes, which is neither "blocked" nor "fine".
                "blocked": blocked,
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

    // Built from the BOUND port, never the requested one: `--port 0` asks the OS
    // to pick, so `port` is still 0 here while every real request carries the
    // assigned port. Using the requested value would put `host:0` in the
    // allowlist and reject every write — locking the user out of their own UI.
    let bound_port = server
        .server_addr()
        .to_ip()
        .map(|a| a.port())
        .unwrap_or(port);
    // A wildcard bind cannot know which address a reviewer will actually use, so
    // it falls back to "any IP literal on this port" (see `wildcard_ip_host`).
    // Without this, binding 0.0.0.0 — the whole point of which is reaching the UI
    // from another machine — serves a readable page whose every button 403s.
    let wildcard = is_wildcard_host(host).then_some(bound_port);
    let ctx =
        Arc::new(Ctx::new(root, allowed_hosts_for(host, bound_port)).with_wildcard_port(wildcard));
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
        let bound_port = server.server_addr().to_ip().expect("ip addr").port();
        let ctx = Arc::new(Ctx::new(runs_root, allowed_hosts_for("127.0.0.1", bound_port)));
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

    /// `/api/runs` as parsed rows, swept by a herdr that reports every pane
    /// `idle` — the shape every list test below wants, where the run's agents
    /// are healthy and nothing is blocked. Tests about the blocked column script
    /// their own `FakeHerdr` and call `list_runs_json` directly.
    fn rows_of(ctx: &Arc<Ctx>, live: Option<&[String]>) -> Vec<serde_json::Value> {
        serde_json::from_str(&list_runs_json(ctx, &crate::herdr::FakeHerdr::new(), live)).unwrap()
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

        let ctx = Arc::new(Ctx::new(tmp.clone(), vec![]));
        let rows: Vec<serde_json::Value> = rows_of(&ctx, Some(&[]));

        // An unreadable state.json means the run's workspace id was never read, so
        // liveness is UNKNOWN. Reporting `false` asserts "no live panes", which is
        // a claim we have no basis for — and the row IS listed (`list_runs_in`
        // only checks the file exists), so the reviewer sees and acts on it.
        assert!(
            row_for(&rows, "broken")["live"].is_null(),
            "an unparseable state.json makes liveness unknown, not false"
        );
        // ...while a run that parsed and genuinely records no workspace is not live.
        assert_eq!(
            row_for(&rows, "midflight")["live"],
            false,
            "a parsed run with no workspace really is not live"
        );

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
    fn cross_origin_writes_are_refused_but_same_origin_and_cli_are_not() {
        use crate::run::PhaseStatus::{Pending, Running};
        let tmp = std::env::temp_dir().join(format!("drovr-csrf-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        make_run_with_phases(&tmp, "guarded", &[Running, Pending], false);
        let addr = start_server(tmp.clone());

        let archived = || -> bool {
            let s: RunState = serde_json::from_str(
                &fs::read_to_string(tmp.join("guarded").join("state.json")).unwrap(),
            )
            .unwrap();
            s.archived
        };

        // A page on another origin: the drive-by case. Refused, and — the part
        // that actually matters — the side effect must not have happened.
        let (code, _) = http_post_origin(
            &addr,
            "/api/runs/guarded/archive",
            r#"{"archived":true}"#,
            Some("http://evil.example"),
        );
        assert_eq!(code, 403, "a cross-origin POST must be refused");
        assert!(!archived(), "a refused request must not archive the run");

        // An opaque origin (sandboxed iframe, file://) is not 'absent' — refuse.
        let (code, _) = http_post_origin(
            &addr,
            "/api/runs/guarded/archive",
            r#"{"archived":true}"#,
            Some("null"),
        );
        assert_eq!(code, 403, "the opaque `null` origin must be refused");
        assert!(!archived());

        // DNS REBINDING: the attacker's page is served from evil.example, whose
        // DNS is then re-pointed at this server. The browser derives BOTH headers
        // from that one URL, so Origin == Host and an Origin-vs-Host check waves
        // it through. Only the Host allowlist stops this.
        let (code, _) = http_post_full(
            &addr,
            "/api/runs/guarded/archive",
            r#"{"archived":true}"#,
            Some("http://evil.example"),
            Some("evil.example"),
        );
        assert_eq!(
            code, 403,
            "a rebound Host must be refused even though Origin matches it"
        );
        assert!(!archived(), "a rebinding attempt must not archive the run");

        // Same, with no Origin — the shape a plain cross-site <form> POST takes.
        let (code, _) = http_post_full(
            &addr,
            "/api/runs/guarded/archive",
            r#"{"archived":true}"#,
            None,
            Some("evil.example"),
        );
        assert_eq!(code, 403, "an unknown Host is refused with or without Origin");
        assert!(!archived());

        // Casing must not decide the outcome: `Host` is normalised, so `Origin`
        // has to be too, or an uppercase spelling would 403 for no good reason.
        let (code, _) = http_post_full(
            &addr,
            "/api/runs/guarded/archive",
            r#"{"archived":true}"#,
            Some(&format!("HTTP://{}", addr.to_uppercase())),
            Some(&addr.to_uppercase()),
        );
        assert_eq!(code, 200, "an upper-cased Host/Origin pair is still same-origin");
        assert!(archived());
        let (code, _) = http_post_origin(
            &addr,
            "/api/runs/guarded/archive",
            r#"{"archived":false}"#,
            Some(&format!("http://{addr}")),
        );
        assert_eq!(code, 200);
        assert!(!archived());

        // drovr's own UI: Origin matches Host.
        let (code, _) = http_post_origin(
            &addr,
            "/api/runs/guarded/archive",
            r#"{"archived":true}"#,
            Some(&format!("http://{addr}")),
        );
        assert_eq!(code, 200, "the review UI's own origin must be allowed");
        assert!(archived());

        // curl / the drovr CLI send no Origin at all and must keep working.
        let (code, _) = http_post_origin(
            &addr,
            "/api/runs/guarded/archive",
            r#"{"archived":false}"#,
            None,
        );
        assert_eq!(code, 200, "a request with no Origin is not a browser write");
        assert!(!archived());

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn allowed_hosts_cover_the_ways_a_reviewer_actually_reaches_the_ui() {
        // Loopback: all three spellings work, because the reviewer may use any.
        let lo = allowed_hosts_for("127.0.0.1", 8791);
        for expected in ["127.0.0.1:8791", "localhost:8791", "[::1]:8791"] {
            assert!(lo.contains(&expected.to_string()), "missing {expected} in {lo:?}");
        }
        // A bare IPv6 literal must be bracketed — browsers send `Host: [::1]:80`.
        assert!(allowed_hosts_for("::1", 8791).contains(&"[::1]:8791".to_string()));
        // A specific non-loopback bind allows exactly itself.
        let tail = allowed_hosts_for("100.71.4.9", 8791);
        assert_eq!(tail, vec!["100.71.4.9:8791".to_string()]);
    }

    #[test]
    fn every_spelling_of_a_wildcard_bind_is_recognised() {
        // `serve_host` is a free-form String, so all of these are things a user can
        // actually write. A spelling missed here is not a hole — it fails closed —
        // but it is a silent lockout: page loads, every button 403s.
        for h in ["0.0.0.0", "::", "[::]", "::0", "[::0]", "0:0:0:0:0:0:0:0"] {
            assert!(is_wildcard_host(h), "{h} is a wildcard bind");
        }
        for h in ["127.0.0.1", "::1", "localhost", "192.168.1.5", "evil.example", ""] {
            assert!(!is_wildcard_host(h), "{h} is NOT a wildcard bind");
        }
        // A wildcard bind still offers the loopback aliases, since the machine
        // running the server reaches it that way.
        let ws = allowed_hosts_for("::0", 8791);
        assert!(ws.contains(&"localhost:8791".to_string()), "{ws:?}");
    }

    /// Give an existing fixture run a workspace id, so liveness is computable.
    /// The default fixture leaves `workspace: None`, which pins `live` to
    /// `Some(false)` and makes the live combinations untestable — the gap that
    /// let a regression stranding every finished run reach a green suite.
    fn set_workspace(dir: &Path, ws: &str) {
        let mut s: RunState =
            serde_json::from_str(&fs::read_to_string(dir.join("state.json")).unwrap()).unwrap();
        s.workspace = Some(ws.to_string());
        fs::write(dir.join("state.json"), serde_json::to_string(&s).unwrap()).unwrap();
    }

    #[test]
    fn a_finished_run_is_complete_even_though_its_workspace_is_still_open() {
        use crate::run::PhaseStatus::Done;
        let tmp = std::env::temp_dir().join(format!("drovr-fin-live-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        // The ordinary end of a pipeline: all four phases Done, nobody has run
        // `cleanup` yet — so the herdr workspace is still open. Nothing closes it
        // on completion, so this is the COMMON state of a finished run, not an
        // edge case, and it must still collapse into "Completed".
        let dir = make_run_with_phases(&tmp, "finished", &[Done, Done, Done, Done], false);
        set_workspace(&dir, "wAG");

        let ctx = Arc::new(Ctx::new(tmp.clone(), vec![]));
        let live = vec!["wAG".to_string()];
        let rows: Vec<serde_json::Value> =
            rows_of(&ctx, Some(&live));
        let row = row_for(&rows, "finished");
        assert_eq!(row["live"], true, "precondition: the workspace really is open");
        assert_eq!(row["archived"], false);
        assert_eq!(
            row["complete"], true,
            "a run that finished every phase belongs in Completed even with its \
             workspace open — gating this on liveness strands every finished run \
             in the active list, which is the clutter the group exists to remove"
        );

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn a_run_whose_panes_are_still_live_is_never_filed_as_complete() {
        use crate::run::PhaseStatus::{Done, Pending, Running};
        let tmp = std::env::temp_dir().join(format!("drovr-zombie-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        // Archived, but its workspace close failed — the agent is still running.
        let dir = make_run_with_phases(&tmp, "zombie", &[Running, Pending], true);
        let mut s: RunState =
            serde_json::from_str(&fs::read_to_string(dir.join("state.json")).unwrap()).unwrap();
        s.workspace = Some("wAG".into());
        fs::write(dir.join("state.json"), serde_json::to_string(&s).unwrap()).unwrap();

        let ctx = Arc::new(Ctx::new(tmp.clone(), vec![]));
        let live = vec!["wAG".to_string()];
        let rows: Vec<serde_json::Value> =
            rows_of(&ctx, Some(&live));
        let row = row_for(&rows, "zombie");
        assert_eq!(row["archived"], true);
        assert_eq!(row["live"], true);
        assert_eq!(
            row["complete"], false,
            "an archived run with a LIVE workspace must stay in the active list — \
             filing it under Completed hides the one row that needs attention"
        );

        // Once the workspace really is gone, it files away normally.
        let rows: Vec<serde_json::Value> =
            rows_of(&ctx, Some(&[]));
        assert_eq!(row_for(&rows, "zombie")["complete"], true);

        // Herdr unreachable: liveness is unknown, and the row does NOT claim
        // "panes still live". A deliberate asymmetry with the archive confirm,
        // which treats unknown as live because it gates a destructive act —
        // asserting it here would stamp the warning on every archived run on any
        // herdr blip. `live: null` is what tells the UI to say liveness is
        // unknown instead of presenting the grouping as verified.
        let rows: Vec<serde_json::Value> =
            rows_of(&ctx, None);
        let row = row_for(&rows, "zombie");
        assert!(row["live"].is_null(), "herdr unreachable is unknown, not false");
        assert_eq!(
            row["complete"], true,
            "with liveness unknown the row collapses rather than asserting a \
             claim we cannot support; transient and self-healing on the next poll"
        );

        // Finishing every phase does NOT excuse a failed close. The anomaly is
        // that an explicit archive request didn't take effect, which is worth
        // surfacing whether or not the pipeline happened to finish.
        let dir = make_run_with_phases(&tmp, "done-zombie", &[Done, Done], true);
        set_workspace(&dir, "wAG");
        let rows: Vec<serde_json::Value> =
            rows_of(&ctx, Some(&live));
        assert_eq!(
            row_for(&rows, "done-zombie")["complete"],
            false,
            "an archived run with an open workspace stays visible even with all phases Done"
        );

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn wildcard_bind_accepts_ip_literals_but_never_a_rebound_name() {
        // Binding 0.0.0.0 exists to be reached from another machine, so the LAN or
        // Tailscale address a reviewer types must work...
        assert!(wildcard_ip_host("192.168.1.5:8791", Some(8791)));
        assert!(wildcard_ip_host("100.71.4.9:8791", Some(8791)));
        assert!(wildcard_ip_host("[fd7a::1]:8791", Some(8791)));
        // ...while a DNS name never does. Rebinding needs a name it controls; an
        // IP literal gives the attacker no lever, which is what makes this safe.
        assert!(!wildcard_ip_host("evil.example:8791", Some(8791)));
        assert!(!wildcard_ip_host("localhost:8791", Some(8791)));
        // Wrong port, and the non-wildcard case, stay closed.
        assert!(!wildcard_ip_host("192.168.1.5:9999", Some(8791)));
        assert!(!wildcard_ip_host("192.168.1.5:8791", None));
        assert!(!wildcard_ip_host("192.168.1.5", Some(8791)));
    }

    #[test]
    fn archiving_never_closes_a_workspace_holding_the_humans_own_panes() {
        use crate::herdr::FakeHerdr;
        let mut state = tree_run(vec![], vec![]);
        state.workspace = Some("wAG".into());
        state.root_pane = Some("wAG:p1".into());

        let h = FakeHerdr::new();
        // The reviewer's own shell/editor sits in the run's workspace alongside
        // drovr's pane. `drovr cleanup` is explicitly hardened to spare it
        // (`cleanup_keeps_a_workspace_holding_panes_drovr_did_not_create`); the
        // Archive button is one click and must not be the careless path.
        h.push_workspace_panes("wAG", ["wAG:p1", "wAG:p9"]);

        let closed = close_for_archive(&h, &state, true);

        let calls = h.calls();
        assert!(
            !calls.iter().any(|c| c.contains("workspace_close")),
            "archiving must not destroy a workspace holding panes drovr did not \
             create — that is the human's work: {calls:?}"
        );
        assert!(
            calls.iter().any(|c| c == "pane_close pane=wAG:p1"),
            "drovr's own pane is still closed: {calls:?}"
        );
        assert!(
            !calls.iter().any(|c| c == "pane_close pane=wAG:p9"),
            "the human's pane must be left alone: {calls:?}"
        );
        assert!(
            !closed,
            "the workspace is still standing, so the page must not be told it closed"
        );
    }

    #[test]
    fn archiving_closes_the_workspace_and_restoring_never_does() {
        use crate::herdr::FakeHerdr;
        let mut state = tree_run(vec![], vec![]);
        state.workspace = Some("wAG".into());

        let h = FakeHerdr::new();
        assert!(
            close_for_archive(&h, &state, true),
            "archiving closes the run's workspace"
        );
        assert!(h.calls().iter().any(|c| c == "workspace_close id=wAG"));

        // Restoring must not touch herdr at all — there is nothing to reopen, and
        // a stray close here would kill panes on what is meant to be an undo.
        let h = FakeHerdr::new();
        assert!(!close_for_archive(&h, &state, false));
        assert!(
            h.calls().is_empty(),
            "restore must issue no herdr calls: {:?}",
            h.calls()
        );

        // An already-gone workspace is the common case for a stale run: report
        // "not closed" but never fail, or the row could never be filed away.
        let h = FakeHerdr::new();
        h.set_fail_workspace_close(true);
        assert!(!close_for_archive(&h, &state, true));

        // A run that never recorded a workspace has nothing to close.
        let mut no_ws = tree_run(vec![], vec![]);
        no_ws.workspace = None;
        let h = FakeHerdr::new();
        assert!(!close_for_archive(&h, &no_ws, true));
        assert!(h.calls().is_empty());
    }

    #[test]
    fn fake_reports_configured_live_workspaces() {
        use crate::herdr::FakeHerdr;
        let h = FakeHerdr::new();
        h.set_live_workspaces(Some(vec!["w1".into()]));
        assert_eq!(h.workspace_list(), Some(vec!["w1".to_string()]));
        h.set_live_workspaces(None);
        assert_eq!(h.workspace_list(), None, "unreachable herdr stays unknown");
    }

    #[test]
    fn list_runs_json_reports_workspace_liveness() {
        use crate::run::PhaseStatus::{Pending, Running};
        let tmp = std::env::temp_dir().join(format!("drovr-live-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let mk = |name: &str, ws: Option<&str>| {
            let dir = make_run_with_phases(&tmp, name, &[Running, Pending], false);
            let mut s: RunState =
                serde_json::from_str(&fs::read_to_string(dir.join("state.json")).unwrap()).unwrap();
            s.workspace = ws.map(|w| w.to_string());
            fs::write(dir.join("state.json"), serde_json::to_string(&s).unwrap()).unwrap();
        };
        mk("alive", Some("wAG"));
        mk("dead", Some("wZZ"));
        mk("no-workspace", None);

        let ctx = Arc::new(Ctx::new(tmp.clone(), vec![]));
        let live = vec!["w1".to_string(), "wAG".to_string()];
        let rows: Vec<serde_json::Value> =
            rows_of(&ctx, Some(&live));
        assert_eq!(row_for(&rows, "alive")["live"], true);
        assert_eq!(row_for(&rows, "dead")["live"], false);
        assert_eq!(
            row_for(&rows, "no-workspace")["live"],
            false,
            "a run that never recorded a workspace has nothing live to close"
        );

        // herdr unreachable: liveness is UNKNOWN, never silently "false" — the UI
        // gates a pane-closing archive on this.
        let rows: Vec<serde_json::Value> =
            rows_of(&ctx, None);
        assert!(
            row_for(&rows, "alive")["live"].is_null(),
            "unknown liveness must not be reported as not-live"
        );

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn archive_endpoint_separates_missing_from_unreadable() {
        let tmp = std::env::temp_dir().join(format!("drovr-arch404-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        // Present on the page (listing only requires state.json to EXIST), but its
        // contents do not parse.
        let broken = tmp.join("broken");
        fs::create_dir_all(&broken).unwrap();
        fs::write(broken.join("state.json"), b"{ not json").unwrap();

        let addr = start_server(tmp.clone());

        let (code, _) = http_post(
            &addr,
            "/api/runs/broken/archive",
            "application/json",
            r#"{"archived":true}"#,
        );
        assert_eq!(
            code, 409,
            "a run that is listed but unreadable must not answer 'no such run' — \
             the reviewer can see the row, so 404 reads as a bug in the page"
        );

        let (code, _) = http_post(
            &addr,
            "/api/runs/ghost/archive",
            "application/json",
            r#"{"archived":true}"#,
        );
        assert_eq!(code, 404, "a genuinely absent run is still 404");
    }

    #[test]
    fn archive_endpoint_sets_and_clears_the_flag() {
        use crate::run::PhaseStatus::{Pending, Running};
        let tmp = std::env::temp_dir().join(format!("drovr-archep-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        make_run_with_phases(&tmp, "filed", &[Running, Pending], false);

        let addr = start_server(tmp.clone());
        let archived_on_disk = || -> bool {
            let s: RunState = serde_json::from_str(
                &fs::read_to_string(tmp.join("filed").join("state.json")).unwrap(),
            )
            .unwrap();
            s.archived
        };

        assert!(!archived_on_disk(), "precondition");
        let (code, body) = http_post(&addr, "/api/runs/filed/archive", "application/json", r#"{"archived":true}"#);
        assert_eq!(code, 200, "body: {body}");
        assert!(archived_on_disk(), "archive must persist to state.json");

        // Archiving sets ONE field. Everything else must survive byte-for-byte:
        // the handler rewrites the whole file, so a stale or partially-populated
        // struct here would quietly erase a concurrent phase-status write.
        let after: RunState = serde_json::from_str(
            &fs::read_to_string(tmp.join("filed").join("state.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(after.task, "t");
        assert_eq!(after.phases.len(), 2, "phases must not be dropped");
        assert_eq!(after.phases[0].status, crate::run::PhaseStatus::Running);
        assert_eq!(after.gate, "spec");

        // The write must land in the server's runs_root, not the ambient data dir.
        assert!(
            tmp.join("filed").join("state.json").is_file(),
            "state.json must be written under the server's runs_root"
        );

        let (code, _) = http_post(&addr, "/api/runs/filed/archive", "application/json", r#"{"archived":false}"#);
        assert_eq!(code, 200);
        assert!(!archived_on_disk(), "restore must clear the flag");

        // Malformed and unknown-run requests are rejected, not silently ignored.
        let (code, _) = http_post(&addr, "/api/runs/filed/archive", "application/json", r#"{"nope":1}"#);
        assert_eq!(code, 400);
        let (code, _) = http_post(&addr, "/api/runs/ghost/archive", "application/json", r#"{"archived":true}"#);
        assert_eq!(code, 404);

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

        let ctx = Arc::new(Ctx::new(tmp.clone(), vec![]));
        let rows: Vec<serde_json::Value> = rows_of(&ctx, Some(&[]));
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

    /// POST with an optional `Origin` header, for exercising the cross-origin
    /// write guard. `None` models curl / the drovr CLI, which send none.
    fn http_post_origin(
        addr: &str,
        path: &str,
        body: &str,
        origin: Option<&str>,
    ) -> (u16, String) {
        http_post_full(addr, path, body, origin, None)
    }

    /// POST with an overridable `Host` as well as `Origin`. Forging `Host` is how
    /// a DNS-rebinding request actually looks on the wire, so the guard cannot be
    /// tested honestly without it. `host: None` sends the real address.
    fn http_post_full(
        addr: &str,
        path: &str,
        body: &str,
        origin: Option<&str>,
        host: Option<&str>,
    ) -> (u16, String) {
        let mut stream = TcpStream::connect(addr).expect("connect");
        let host_hdr = host.unwrap_or(addr);
        let origin_line = origin.map(|o| format!("Origin: {o}\r\n")).unwrap_or_default();
        write!(
            stream,
            "POST {path} HTTP/1.0\r\nHost: {host_hdr}\r\n{origin_line}Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
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
        let rb = resp.split_once("\r\n\r\n").map(|x| x.1).unwrap_or("").to_string();
        (status, rb)
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
        let rb = resp.split_once("\r\n\r\n").map(|x| x.1).unwrap_or("").to_string();
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
        let tree = build_agent_tree(&run, &crate::config::Config::default(), None, &[]);
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

    /// The mirror must not keep typing into a pane drovr has closed.
    ///
    /// `mark_reaped` clears `pane_id` in the same statement it sets the flag, so
    /// a reaped phase leaves the writable allow-list by construction rather than
    /// by a check written here — which is exactly why that pair is one mutator.
    /// Asserted end to end anyway: the browser holds a sticky `selectedPane` and
    /// will go on naming a pane the run has moved past, and 409 (gated, before
    /// herdr) is a different answer from 500 (gated in, failed at herdr).
    #[test]
    fn a_reaped_phases_pane_is_no_longer_writable() {
        let tmp = make_root("reaped-write-gate");
        let dir = tmp.path().join("r");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("spec.md"), b"# Spec").unwrap();
        let mut run = crate::run::RunState {
            name: "r".into(),
            task: "t".into(),
            agent: None,
            phases: vec![
                crate::run::Phase::new("brainstorm"),
                crate::run::Phase::new("implement"),
            ],
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
        // brainstorm ran on w:p1 and drovr reaped it; implement is live on w:p2.
        run.phases[0].status = crate::run::PhaseStatus::Done;
        run.phases[0].set_pane("w:p1");
        run.phases[0].mark_reaped();
        run.retire_pane("w:p1");
        run.phases[1].status = crate::run::PhaseStatus::Running;
        run.phases[1].set_pane("w:p2");
        fs::write(dir.join("state.json"), serde_json::to_string(&run).unwrap()).unwrap();
        let addr = start_server(tmp.path().to_path_buf());

        let (s, _) = http_post(&addr, "/api/runs/r/send?pane=w%3Ap1", "text/plain", "hi");
        assert_eq!(s, 409, "a reaped pane must be refused before herdr");
        // ⚠️ And retiring it did NOT make it writable again. `retired_panes` is
        // what `drovr cleanup` reads to prove a pane was drovr's; it is not a
        // list of places the mirror may type.
        assert!(
            run.retired_panes.contains(&"w:p1".to_string()),
            "precondition: the pane is retired, and still refused"
        );

        // Positive control: the live phase's pane passes the gate and fails at
        // herdr instead, so the 409 above is discrimination, not a broken state.
        let (s, _) = http_post(&addr, "/api/runs/r/send?pane=w%3Ap2", "text/plain", "hi");
        assert_eq!(s, 500, "the live phase's pane must still be writable");
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
        // A phase `phase_start` persisted as Running and then failed to launch:
        // has_run() is true, but no agent was ever recorded.
        let mut launch_failed = crate::run::Phase::new("launch-failed");
        launch_failed.status = crate::run::PhaseStatus::Running;
        let mut reviewer = crate::run::Phase::new("review:task-1:1:security");
        reviewer.status = crate::run::PhaseStatus::Done;
        reviewer.set_pane("w:p2");
        reviewer.record_launch("claude", None);
        reviewer.mark_reaped();
        run.review_phases = vec![reviewer];
        run.phases = vec![
            reaped,
            live,
            crate::run::Phase::new("placeholder"),
            launch_failed,
        ];
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

        // …and the third refusal, which this path used to omit entirely: a
        // phase that LOOKS started (`phase_start` persists `Running` before it
        // launches) but never got an agent. The CLI refuses it; so must the
        // button, or the endpoint a human clicks is more permissive than the
        // command it shells out to.
        let (s, body) = http_post(
            &addr,
            "/api/runs/r/rehydrate?phase=launch-failed",
            "text/plain",
            "",
        );
        assert_eq!(s, 409, "{body}");
        assert!(body.contains("no agent on record"), "{body}");

        // A reviewer → 409. Its findings channel cannot be re-attached to a
        // resumed session, so bringing it back would produce an agent unable to
        // do the one thing it exists for.
        let (s, body) = http_post(
            &addr,
            "/api/runs/r/rehydrate?phase=review%3Atask-1%3A1%3Asecurity",
            "text/plain",
            "",
        );
        assert_eq!(s, 409, "{body}");
        assert!(body.contains("review-panel agent"), "{body}");

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
        let tree = build_agent_tree(&run, &crate::config::Config::default(), None, &[]);
        let nodes = tree["nodes"].as_array().unwrap();
        assert_eq!(
            nodes.len(),
            3,
            "reaped phases appear; the placeholder does not: {tree}"
        );

        assert_eq!(nodes[0]["name"], "brainstorm");
        assert_eq!(nodes[0]["reaped"], true);
        assert_eq!(nodes[0]["resumable"], true);
        assert_eq!(nodes[0]["rehydratable"], true);
        assert!(nodes[0]["pane_id"].is_null(), "a reaped phase has no pane");

        assert_eq!(nodes[1]["name"], "plan");
        assert_eq!(nodes[1]["reaped"], true);
        // No session to resume — but STILL rehydratable: it reseeds, and the ⟳
        // is gated on that. The two fields answer different questions.
        assert_eq!(nodes[1]["resumable"], false);
        assert_eq!(nodes[1]["rehydratable"], true);

        assert_eq!(nodes[2]["name"], "implement-task-1");
        assert_eq!(nodes[2]["reaped"], false);
        assert_eq!(nodes[2]["pane_id"], "w:p3");
        assert_eq!(
            nodes[2]["rehydratable"], false,
            "a phase holding a live pane must not be offered a ⟳ the CLI refuses"
        );
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
        let tree = build_agent_tree(&run, &cfg, None, &[]);
        assert_eq!(tree["nodes"][0]["resumable"], false);

        // …and it flips the moment the user opts codex in.
        let mut cfg = cfg;
        cfg.agents.get_mut("codex").unwrap().resume =
            Some(crate::config::ResumeSpec::subcommand("resume").unwrap());
        let tree = build_agent_tree(&run, &cfg, None, &[]);
        assert_eq!(tree["nodes"][0]["resumable"], true);
    }

    #[test]
    fn the_tree_offers_no_rehydrate_the_cli_would_refuse() {
        // The ⟳ is gated on the SAME predicate the CLI and the handler ask, so
        // the run-level prerequisites have to reach the tree too — a button
        // rendered on a run with no workspace is a button that errors on click.
        // A reviewer never gets one at all: its findings channel cannot be
        // re-attached to a resumed session.
        use crate::run::PhaseStatus::Done;
        let reaped = |name: &str| {
            let mut p = ph(name, Done, Some("w:p1"));
            p.record_launch("claude", None);
            p.mark_reaped();
            p
        };
        let run = tree_run(
            vec![reaped("implement-task-1")],
            vec![reaped("review:task-1:1:security")],
        );
        let cfg = crate::config::Config::default();
        let tree = build_agent_tree(&run, &cfg, None, &[]);
        assert_eq!(tree["nodes"][0]["rehydratable"], true);
        assert_eq!(
            tree["nodes"][0]["children"][0]["rehydratable"],
            false,
            "a reviewer must never be offered a ⟳: {tree}"
        );
        // …and the reviewer is still SHOWN, dimmed — hiding it would make a
        // pane drovr closed look like one that never ran.
        assert_eq!(tree["nodes"][0]["children"][0]["reaped"], true);

        let mut no_ws = tree_run(vec![reaped("implement-task-1")], vec![]);
        no_ws.workspace = None;
        let tree = build_agent_tree(&no_ws, &cfg, None, &[]);
        assert_eq!(
            tree["nodes"][0]["rehydratable"], false,
            "no workspace means nowhere to open the tab: {tree}"
        );
    }

    #[test]
    fn the_tree_and_the_launcher_read_the_same_resume_surface() {
        // `has_resumable_session` used to reach into `cfg.agents` and test
        // `spec.resume` itself — a SECOND classifier of the fact
        // `Config::resume_launch` classifies when it decides between resuming
        // and reseeding. This branch has already paid twice for exactly that
        // shape (`Capture::from_poll` vs `PaneState::from_poll`). Pin that the
        // two agree for every backend, including the ones the config file adds.
        use crate::run::PhaseStatus::Done;
        let mut cfg = crate::config::Config::default();
        cfg.agents.get_mut("codex").unwrap().resume =
            Some(crate::config::ResumeSpec::subcommand("resume").unwrap());
        for backend in ["claude", "cursor", "codex", "not-in-the-config"] {
            let mut p = ph("plan", Done, Some("w:p1"));
            p.record_launch(backend, None);
            assert!(p.record_session(crate::herdr::SessionId::new("s-1".into()).unwrap()));
            let target = p.resume_target().unwrap();
            // `Ok(None)` is "no resume surface"; `Err` is "the config does not
            // know this backend at all", which is also not a resume.
            let launcher = cfg
                .resume_launch(&target, "/tmp/p", false)
                .ok()
                .flatten()
                .is_some();
            assert_eq!(
                has_resumable_session(&p, &cfg),
                launcher,
                "the ⟳'s promise and the launcher disagree about '{backend}'"
            );
        }
    }

    #[test]
    fn a_config_that_would_not_load_is_reported_rather_than_swallowed() {
        // The tree falls back to the BUILT-IN agent map when the user's config
        // cannot be read, so `resumable` is computed against the wrong backends
        // and the ⟳ appears (or vanishes) for a reason nothing on screen
        // explains. Serving the tree anyway is right — a config typo must not
        // blank the panel — but silently is not.
        use crate::run::PhaseStatus::Done;
        let run = tree_run(vec![ph("plan", Done, Some("w:p1"))], vec![]);
        let cfg = crate::config::Config::default();

        let clean = build_agent_tree(&run, &cfg, None, &[]);
        assert_eq!(clean["config_error"], serde_json::Value::Null);

        let broken = build_agent_tree(&run, &cfg, Some("expected a value at line 3"), &[]);
        assert!(
            broken["config_error"]
                .as_str()
                .is_some_and(|s| s.contains("expected a value at line 3")),
            "the reason has to reach the page: {broken}"
        );
        // …and the tree is still served, or a config typo blanks the panel.
        assert_eq!(broken["nodes"].as_array().unwrap().len(), 1);
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

        // Every failure here reports the path and what the file held, because this test
        // has flaked twice (2026-07-26, 2026-08-01) and neither sighting left enough to
        // diagnose. It has never reproduced on demand — 14 full-suite runs — and the two
        // obvious causes are ruled out: `make_root` is a unique `tempdir()`, so no other
        // process shares this path, and `try_take_lock` is `flock` on a distinct file.
        // If it fails again, the message below is the evidence to file.
        let whats_there = |when: &str| -> String {
            format!(
                "{when}: {} contains {:?}; this pid is {}",
                path.display(),
                fs::read_to_string(&path).unwrap_or_else(|e| format!("<unreadable: {e}>")),
                std::process::id()
            )
        };

        let held = try_take_lock(&path)
            .unwrap_or_else(|e| panic!("claim failed: {e}; {}", whats_there("at claim")))
            .unwrap_or_else(|| {
                panic!(
                    "a freshly created tempdir path was already locked. {}",
                    whats_there("at claim")
                )
            });
        assert_eq!(
            fs::read_to_string(&path)
                .ok()
                .and_then(|s| s.trim().parse().ok()),
            Some(std::process::id()),
            "the holder records its pid for humans to kill. {}",
            whats_there("after claim")
        );

        // Nothing has to prove the holder died for the lock to be free again.
        drop(held);
        let _held = try_take_lock(&path)
            .unwrap_or_else(|e| panic!("re-claim failed: {e}; {}", whats_there("after drop")))
            .unwrap_or_else(|| {
                panic!(
                    "a lock released by dropping its File was still held. {}",
                    whats_there("after drop")
                )
            });
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

    // -----------------------------------------------------------------------
    // Blocked agents on the session list and in the agent tree
    // -----------------------------------------------------------------------

    /// A fixture run whose first phase is Running in pane `w:p0`, with a live
    /// workspace. The shape a blocked agent actually occurs in.
    fn make_running_run(root: &Path, run: &str, pane: &str) -> PathBuf {
        use crate::run::PhaseStatus::{Pending, Running};
        let dir = make_run_with_phases(root, run, &[Running, Pending], false);
        let mut s: RunState =
            serde_json::from_str(&fs::read_to_string(dir.join("state.json")).unwrap()).unwrap();
        s.workspace = Some("wB".into());
        s.phases[0].set_pane(pane);
        fs::write(dir.join("state.json"), serde_json::to_string(&s).unwrap()).unwrap();
        dir
    }

    fn blocked_rows(ctx: &Arc<Ctx>, h: &crate::herdr::FakeHerdr) -> Vec<serde_json::Value> {
        let live = vec!["wB".to_string()];
        serde_json::from_str(&list_runs_json(ctx, h, Some(&live))).unwrap()
    }

    #[test]
    fn a_destructive_prompt_raises_the_session_list_badge() {
        let tmp = std::env::temp_dir().join(format!("drovr-blk-1-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        make_running_run(&tmp, "stuck", "w:p0");

        let h = crate::herdr::FakeHerdr::new();
        h.push_status_for("w:p0", Some("blocked"));
        h.push_read_for("w:p0", "Dangerous rm operation\n  rm -rf /\n1. Yes\n2. No");

        let ctx = Arc::new(Ctx::new(tmp.clone(), vec![]));
        let row = &blocked_rows(&ctx, &h)[0].clone();
        assert_eq!(row["blocked"]["count"], 1);
        assert_eq!(row["blocked"]["phase"], "phase0");
        assert_eq!(row["blocked"]["class"], "destructive");
        // Every phase needing a human is NAMED, not counted: the browser raises
        // one alarm per phase, so two simultaneous blocks in one run have to be
        // two entries or the second is never announced.
        assert_eq!(row["blocked"]["human_phases"], serde_json::json!(["phase0"]));

        let _ = fs::remove_dir_all(&tmp);
    }

    /// A routine permission dialog is reported but does NOT ask for a human:
    /// whatever driver is waiting on the phase answers it. A badge that fires on
    /// every file-edit prompt is a badge nobody reads.
    #[test]
    fn a_routine_prompt_is_reported_without_asking_for_a_human() {
        let tmp = std::env::temp_dir().join(format!("drovr-blk-2-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        make_running_run(&tmp, "polite", "w:p0");

        let h = crate::herdr::FakeHerdr::new();
        h.push_status_for("w:p0", Some("blocked"));
        h.push_read_for("w:p0", "Do you want to make this edit to lib.rs?\n1. Yes");

        let ctx = Arc::new(Ctx::new(tmp.clone(), vec![]));
        let row = &blocked_rows(&ctx, &h)[0].clone();
        assert_eq!(row["blocked"]["count"], 1);
        assert_eq!(row["blocked"]["class"], "routine");
        assert_eq!(
            row["blocked"]["human_phases"],
            serde_json::json!([]),
            "reported, but nobody is asked to act"
        );

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn a_run_with_nothing_blocked_reports_null() {
        let tmp = std::env::temp_dir().join(format!("drovr-blk-3-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        make_running_run(&tmp, "busy", "w:p0");

        let h = crate::herdr::FakeHerdr::new();
        h.push_status_for("w:p0", Some("working"));

        let ctx = Arc::new(Ctx::new(tmp.clone(), vec![]));
        let row = &blocked_rows(&ctx, &h)[0].clone();
        assert!(row["blocked"].is_null(), "{}", row["blocked"]);

        let _ = fs::remove_dir_all(&tmp);
    }

    /// The list poll runs every 2s against every run. Scanning a run that cannot
    /// have a working agent is pure herdr load for a foregone answer.
    #[test]
    fn a_finished_or_archived_run_is_never_scanned() {
        use crate::run::PhaseStatus::Done;
        let tmp = std::env::temp_dir().join(format!("drovr-blk-4-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let done = make_run_with_phases(&tmp, "done", &[Done, Done], false);
        set_workspace(&done, "wB");
        let filed = make_run_with_phases(&tmp, "filed", &[Done, Done], true);
        set_workspace(&filed, "wB");

        let h = crate::herdr::FakeHerdr::new();
        let ctx = Arc::new(Ctx::new(tmp.clone(), vec![]));
        let rows = blocked_rows(&ctx, &h);
        assert!(row_for(&rows, "done")["blocked"].is_null());
        assert!(row_for(&rows, "filed")["blocked"].is_null());
        let polls: Vec<String> = h
            .calls()
            .into_iter()
            .filter(|c| c.contains("agent_status"))
            .collect();
        assert!(polls.is_empty(), "herdr was polled anyway: {polls:?}");

        let _ = fs::remove_dir_all(&tmp);
    }

    /// `is_complete()` walks `phases` only, so a run whose pipeline finished
    /// while a REVIEW PANEL is still up reads as complete. Skipping those would
    /// hide a stuck reviewer — the block least likely to be noticed any other
    /// way, since a completed run is exactly what nobody is watching.
    #[test]
    fn a_completed_run_with_a_live_review_panel_is_still_scanned() {
        use crate::run::PhaseStatus::{Done, Running};
        let tmp = std::env::temp_dir().join(format!("drovr-blk-6-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let dir = make_run_with_phases(&tmp, "shipped", &[Done, Done], false);
        let mut s: RunState =
            serde_json::from_str(&fs::read_to_string(dir.join("state.json")).unwrap()).unwrap();
        s.workspace = Some("wB".into());
        let mut panel = crate::run::Phase::new("review:task-1:1:security");
        panel.status = Running;
        panel.set_pane("w:rev");
        s.review_phases = vec![panel];
        fs::write(dir.join("state.json"), serde_json::to_string(&s).unwrap()).unwrap();

        let h = crate::herdr::FakeHerdr::new();
        h.push_status_for("w:rev", Some("blocked"));
        h.push_read_for("w:rev", "Bash: git push --force\n1. Yes\n2. No");

        let ctx = Arc::new(Ctx::new(tmp.clone(), vec![]));
        let row = &blocked_rows(&ctx, &h)[0].clone();
        assert_eq!(row["complete"], true, "precondition: it reads as finished");
        assert_eq!(
            row["blocked"]["human_phases"],
            serde_json::json!(["review:task-1:1:security"])
        );

        let _ = fs::remove_dir_all(&tmp);
    }

    /// A zombie is an ARCHIVED run whose workspace is still open: the human asked
    /// to close it, the close failed, and an agent is running in panes drovr
    /// believes it shut. The session list keeps that row out of the Completed
    /// fold precisely because it needs attention — so it must be swept too.
    #[test]
    fn an_archived_run_whose_panes_are_still_live_is_still_scanned() {
        let tmp = std::env::temp_dir().join(format!("drovr-blk-8-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let dir = make_running_run(&tmp, "zombie", "w:p0");
        let mut s: RunState =
            serde_json::from_str(&fs::read_to_string(dir.join("state.json")).unwrap()).unwrap();
        s.archived = true;
        fs::write(dir.join("state.json"), serde_json::to_string(&s).unwrap()).unwrap();

        let h = crate::herdr::FakeHerdr::new();
        h.push_status_for("w:p0", Some("blocked"));
        h.push_read_for("w:p0", "Dangerous rm operation\n1. Yes");

        let ctx = Arc::new(Ctx::new(tmp.clone(), vec![]));
        let row = &blocked_rows(&ctx, &h)[0].clone();
        assert_eq!(row["archived"], true, "precondition: it was filed away");
        assert_eq!(row["live"], true, "precondition: but its panes are alive");
        assert_eq!(
            row["blocked"]["human_phases"],
            serde_json::json!(["phase0"]),
            "archiving is not evidence the panes closed — the failed close is \
             exactly why this row is still on screen"
        );

        let _ = fs::remove_dir_all(&tmp);
    }

    /// Two agents of one run can be stuck at once — a phase and its review
    /// panel. The browser raises one alarm per PHASE, so the wire has to name
    /// both or the second is never announced.
    #[test]
    fn two_simultaneous_blocks_in_one_run_are_both_named() {
        use crate::run::PhaseStatus::Running;
        let tmp = std::env::temp_dir().join(format!("drovr-blk-9-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        let dir = make_running_run(&tmp, "double", "w:p0");
        let mut s: RunState =
            serde_json::from_str(&fs::read_to_string(dir.join("state.json")).unwrap()).unwrap();
        let mut panel = crate::run::Phase::new("review:task-1:1:security");
        panel.status = Running;
        panel.set_pane("w:rev");
        s.review_phases = vec![panel];
        fs::write(dir.join("state.json"), serde_json::to_string(&s).unwrap()).unwrap();

        let h = crate::herdr::FakeHerdr::new();
        h.push_status_for("w:p0", Some("blocked"));
        h.push_read_for("w:p0", "Dangerous rm operation\n1. Yes");
        h.push_status_for("w:rev", Some("blocked"));
        h.push_read_for("w:rev", "A prompt drovr has never seen\n1. Yes");

        let ctx = Arc::new(Ctx::new(tmp.clone(), vec![]));
        let row = &blocked_rows(&ctx, &h)[0].clone();
        assert_eq!(row["blocked"]["count"], 2);
        assert_eq!(
            row["blocked"]["human_phases"],
            serde_json::json!(["phase0", "review:task-1:1:security"])
        );

        let _ = fs::remove_dir_all(&tmp);
    }

    /// A sweep that could not read a single pane learned nothing. It is held for
    /// [`BLOCKED_RETRY_TTL`] rather than the full [`BLOCKED_TTL`] — long enough
    /// that a hung herdr is not swept once per request per tab, short enough
    /// that the badge is right again shortly after herdr returns. What it must
    /// never do is claim the run is fine.
    #[test]
    fn a_sweep_that_learned_nothing_answers_unknown_and_retries_soon() {
        let tmp = std::env::temp_dir().join(format!("drovr-blk-7-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        make_running_run(&tmp, "flaky", "w:p0");

        let h = crate::herdr::FakeHerdr::new();
        h.fail_pane_info();
        let ctx = Arc::new(Ctx::new(tmp.clone(), vec![]));
        let row = &blocked_rows(&ctx, &h)[0].clone();
        assert_eq!(
            row["blocked"]["inconclusive"], true,
            "a sweep that reached nothing must not answer `null`, which the \
             browser reads as `this run is fine` — and as permission to clear \
             an alarm it already raised"
        );
        assert_eq!(row["blocked"]["count"], 0);

        // Inside the retry window a second poll is served from the cache rather
        // than sweeping again. This is what keeps a hung herdr from being swept
        // once per request per open tab.
        let polls = h.calls().iter().filter(|c| c.contains("agent_status")).count();
        let _ = blocked_rows(&ctx, &h);
        assert_eq!(
            h.calls().iter().filter(|c| c.contains("agent_status")).count(),
            polls,
            "an immediate re-poll must not re-sweep"
        );

        // Past the retry window, herdr's recovery is picked up — without waiting
        // out the full BLOCKED_TTL a conclusive answer would have earned.
        std::thread::sleep(BLOCKED_RETRY_TTL + Duration::from_millis(100));
        let back = crate::herdr::FakeHerdr::new();
        back.push_status_for("w:p0", Some("blocked"));
        back.push_read_for("w:p0", "Dangerous rm operation\n1. Yes");
        assert_eq!(
            blocked_rows(&ctx, &back)[0]["blocked"]["human_phases"],
            serde_json::json!(["phase0"])
        );

        let _ = fs::remove_dir_all(&tmp);
    }

    /// The whole reason the scan is affordable behind a 2s poll: the second poll
    /// inside `BLOCKED_TTL` is served from the cache, so herdr sees one sweep
    /// however many browser tabs are open.
    #[test]
    fn a_second_poll_inside_the_ttl_does_not_re_scan() {
        let tmp = std::env::temp_dir().join(format!("drovr-blk-5-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        make_running_run(&tmp, "cached", "w:p0");

        let h = crate::herdr::FakeHerdr::new();
        h.push_status_for("w:p0", Some("blocked"));
        h.push_read_for("w:p0", "Dangerous rm operation\n1. Yes");

        let ctx = Arc::new(Ctx::new(tmp.clone(), vec![]));
        let first = &blocked_rows(&ctx, &h)[0].clone();
        let polls_after_first = h.calls().iter().filter(|c| c.contains("agent_status")).count();
        let second = &blocked_rows(&ctx, &h)[0].clone();
        let polls_after_second = h.calls().iter().filter(|c| c.contains("agent_status")).count();

        assert_eq!(polls_after_first, 1, "the first poll scans");
        assert_eq!(
            polls_after_second, 1,
            "the second poll inside the TTL must reuse the first scan"
        );
        assert_eq!(
            first["blocked"], second["blocked"],
            "and it must answer the same thing"
        );

        let _ = fs::remove_dir_all(&tmp);
    }

    /// On a run's page the session-list poll has stopped, so `/agents` is the
    /// browser's only feed of blocked state. If it could not say that a sweep
    /// reached nothing, opening a run during a herdr blip would render every
    /// node clean and clear an alarm already raised.
    #[test]
    fn the_agent_tree_says_when_its_sweep_reached_nothing() {
        let tmp = std::env::temp_dir().join(format!("drovr-blk-10-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        make_running_run(&tmp, "blip", "w:p0");
        let addr = start_server(tmp.clone());

        // No herdr in the test environment, so the real `SystemHerdr` this
        // handler uses reaches nothing — which is precisely the state under
        // test: a sweep that answered for no pane.
        let (status, body) = http_get(&addr, "/api/runs/blip/agents");
        assert_eq!(status, 200);
        let tree: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(tree["inconclusive"], true, "{body}");

        // A run whose own state.json will not parse is the same class of
        // non-answer: not "no agents", but "we could not read it".
        let broken = tmp.join("broken");
        fs::create_dir_all(&broken).unwrap();
        fs::write(broken.join("state.json"), b"{ not json").unwrap();
        let (status, body) = http_get(&addr, "/api/runs/broken/agents");
        assert_eq!(status, 200);
        let tree: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(tree["inconclusive"], true, "{body}");
        assert_eq!(tree["nodes"], serde_json::json!([]));

        // And the session list says it too, for the same run.
        let (_, body) = http_get(&addr, "/api/runs");
        let rows: Vec<serde_json::Value> = serde_json::from_str(&body).unwrap();
        assert_eq!(row_for(&rows, "broken")["blocked"]["inconclusive"], true);

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn the_agent_tree_carries_the_prompt_the_pane_is_parked_on() {
        use crate::run::PhaseStatus::Running;
        let mut run = RunState {
            name: "r".into(),
            task: "t".into(),
            agent: Some("claude".into()),
            phases: vec![crate::run::Phase::new("implement-task-1")],
            review_phases: vec![crate::run::Phase::new("review:task-1:1:security")],
            gate: "spec".into(),
            cursor: 0,
            workspace: Some("w".into()),
            root_pane: None,
            project_dir: "/tmp/p".into(),
            worktree_path: None,
            worktree_branch: None,
            archived: false,
            retired_panes: vec![],
        };
        run.phases[0].status = Running;
        run.phases[0].set_pane("w:p1");
        run.review_phases[0].status = Running;
        run.review_phases[0].set_pane("w:p2");

        let blocked = vec![crate::blocked::BlockedAgent {
            phase: "review:task-1:1:security".into(),
            pane_id: "w:p2".into(),
            class: crate::phase::BlockedClass::Unknown,
            excerpt: "Trust the files in this folder?\n1. Yes".into(),
        }];
        let tree = build_agent_tree(&run, &crate::config::Config::default(), None, &blocked);
        let phase = &tree["nodes"][0];
        assert!(
            phase["blocked"].is_null(),
            "a phase nobody reported blocked stays null: {}",
            phase["blocked"]
        );
        let panel = &phase["children"][0];
        assert_eq!(panel["blocked"]["class"], "unknown");
        assert_eq!(panel["blocked"]["needs_human"], true);
        assert!(
            panel["blocked"]["excerpt"]
                .as_str()
                .unwrap()
                .contains("Trust the files"),
            "the tree quotes the prompt so the reviewer need not attach"
        );
    }
}
