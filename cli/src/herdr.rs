use std::io;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

#[cfg(test)]
use std::cell::RefCell;
#[cfg(test)]
use std::collections::VecDeque;

/// A freshly created herdr workspace: its id plus the id of its auto-created root
/// shell pane. drovr runs the first phase's `claude` *inside* `root_pane` (via
/// `pane_run`) rather than splitting a new pane beside it, so no empty shell is
/// left dangling. The root pane is never closed mid-run — closing any pane makes
/// herdr reassign focus and disturbs the user — and is torn down together with
/// every phase pane by the single `workspace_close` at `drovr cleanup`.
#[derive(Debug)]
pub struct Workspace {
    pub id: String,
    pub root_pane: String,
}

/// A herdr tab id, carrying its own proof: the inner string is private to this
/// module and only [`parse_pane_info`] builds one, so the only way to name a tab
/// is to have read the pane that lives in it.
///
/// [`Herdr::tab_close`] takes one of these, which makes "closed a tab by passing
/// a pane id" a compile error rather than a runtime surprise — and the surprise
/// would be severe: closing the wrong tab can take out the workspace root tab
/// and destroy the run.
///
/// Pane ids are deliberately NOT newtyped. They are persisted in `state.json`,
/// gated by the HTTP server's pane allowlists and threaded through herdr's JSON;
/// the ripple is out of proportion to the risk, and `tab_close` is the only
/// call where a mix-up is destructive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabId(String);

impl TabId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A resumable session id, carrying its own proof: the inner string is private
/// to this module, so one can only be built here — by parsing a `kind == "id"`
/// session.
///
/// It exists to make [`AgentSession`]'s guarantee structural rather than
/// conventional. An enum's variants are as public as the enum, so with every
/// variant holding a bare `String` a caller could merge them in one pattern —
/// `Id { value, .. } | Path { value } => value` — and walk off with a transcript
/// path where a session id was expected. Giving `Id` a payload type of its own
/// makes that or-pattern fail to type-check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionId(String);

// Capability only for now: nothing in drovr resumes a session yet, so the id is
// read by tests alone until task 5 composes `--resume`.
#[allow(dead_code)]
impl SessionId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A session that is safe to resume: a `kind == "id"` session whose owning agent
/// herdr told us. Both halves of the resume rule travel together, so a caller
/// cannot hold the id without also holding the backend to check it against —
/// see [`AgentSession::resumable`], the only thing that builds one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResumableSession<'a> {
    pub id: &'a SessionId,
    pub agent: &'a str,
}

/// The agent session herdr records on a pane (`agent_session`), keyed by herdr's
/// own `kind` discriminator.
///
/// Only a `kind == "id"` session may ever be interpolated into an agent's
/// `--resume` argument — a transcript path there would be read as a session
/// name. That rule lives in the TYPE rather than in every caller: the id is
/// reachable through [`AgentSession::resumable`], and only for an `Id` whose
/// owning agent herdr reported; the value it hands back is a [`SessionId`] no
/// other variant can produce. A `Path`'s value is still readable — diagnostics
/// need it — but only by naming `Path` explicitly, which is a deliberate,
/// greppable act.
///
/// Parsing stays FAITHFUL: an id session with no `agent` key is still parsed as
/// `Id { agent: None }`, because that is what herdr said. It is `resumable` that
/// refuses it — the safety judgement is a method, not a lie about the wire.
///
/// herdr DROPS this whole key once the pane's agent process exits (verified
/// against 0.7.5), so it must be captured while the agent is alive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentSession {
    /// A resumable session id.
    Id {
        value: SessionId,
        /// The agent that owns the session (`claude`, `cursor`, …). Optional —
        /// absent on some herdr versions — and a session id is only safe to
        /// resume with the backend that created it, so a caller that cannot
        /// confirm the backend must not resume.
        agent: Option<String>,
    },
    /// A transcript path. Never a resume operand.
    Path { value: String },
    /// A kind this drovr does not know. Preserved verbatim so it is visible in
    /// diagnostics, and never resumable.
    Other { kind: String, value: String },
}

// Capability only for now: nothing in drovr reads a session yet — capturing it
// onto `Phase` is a later step, and resuming from it later still.
#[allow(dead_code)]
impl AgentSession {
    /// The session — and ONLY when it is an id whose owning agent is known.
    /// A `kind:"path"` session, an unrecognised kind, and an id herdr did not
    /// attribute to an agent all yield `None`.
    ///
    /// This is the single chokepoint for the whole resume rule: never
    /// interpolate a path as a session id, AND never resume an id without being
    /// able to check it came from this run's backend. Without the agent that
    /// check is impossible, and resuming a claude session under cursor is not a
    /// recoverable mistake — so an agent-less id is not resumable at all.
    pub fn resumable(&self) -> Option<ResumableSession<'_>> {
        match self {
            AgentSession::Id {
                value,
                agent: Some(agent),
            } => Some(ResumableSession { id: value, agent }),
            _ => None,
        }
    }

    /// The agent that owns the session, when herdr reported one. Only an `Id`
    /// session carries it, because it is only ever consulted to decide whether
    /// a resume is safe.
    pub fn agent(&self) -> Option<&str> {
        match self {
            AgentSession::Id { agent, .. } => agent.as_deref(),
            _ => None,
        }
    }

    /// herdr's own `kind` string, for diagnostics and logging.
    pub fn kind(&self) -> &str {
        match self {
            AgentSession::Id { .. } => "id",
            AgentSession::Path { .. } => "path",
            AgentSession::Other { kind, .. } => kind,
        }
    }
}

/// A pane's agent status, as reported by herdr.
///
/// The vocabulary is `idle|working|blocked|done|unknown`, but it is herdr's to
/// extend, so anything else is preserved verbatim as [`AgentStatus::Other`]
/// rather than collapsed onto a known state. That matters more than it looks:
/// `Done` is the verdict that tears a pane down, and a future herdr state
/// silently reading as `Done` would close a live agent's pane.
///
/// `Unknown` is herdr's own literal `"unknown"` — the status of a pane whose
/// agent has exited — and is distinct from `Option::None`, which means herdr
/// reported no status field at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentStatus {
    Idle,
    Working,
    Blocked,
    Done,
    Unknown,
    Other(String),
}

impl AgentStatus {
    /// Classify a raw herdr status. Total: an unrecognised value becomes
    /// `Other`, never a known state.
    pub fn from_herdr(status: &str) -> AgentStatus {
        match status {
            "idle" => AgentStatus::Idle,
            "working" => AgentStatus::Working,
            "blocked" => AgentStatus::Blocked,
            "done" => AgentStatus::Done,
            "unknown" => AgentStatus::Unknown,
            other => AgentStatus::Other(other.to_string()),
        }
    }

    /// The raw herdr string this status came from.
    pub fn as_str(&self) -> &str {
        match self {
            AgentStatus::Idle => "idle",
            AgentStatus::Working => "working",
            AgentStatus::Blocked => "blocked",
            AgentStatus::Done => "done",
            AgentStatus::Unknown => "unknown",
            AgentStatus::Other(raw) => raw,
        }
    }
}

impl std::fmt::Display for AgentStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A snapshot of one pane, from herdr's `pane.get`. This is drovr's ONLY poll
/// primitive: status polling reads it, and reaping resolves a pane's tab through
/// it.
///
/// `tab_id` is required — a `PaneInfo` that cannot name the pane's tab is of no
/// use to either caller — while `agent_status` and `agent_session` are both
/// optional, because a pane whose agent has exited reports `agent_status:
/// "unknown"` and carries no session at all.
///
/// **The two `None`s are not the same and must never be collapsed:**
///
/// - `pane_info(..) == None` — the poll FAILED. herdr was unreachable, or the
///   pane is gone. It says nothing about the agent.
/// - `Some(PaneInfo { agent_status: Some(Unknown), agent_session: None, .. })` —
///   the poll SUCCEEDED and the agent has exited. The tab is still there.
///
/// Reaping turns on exactly this distinction: treating a transient poll failure
/// as "the agent exited" tears down a pane whose agent is alive and working. So
/// there is no projection helper on [`Herdr`] that could lose it — the only poll
/// returns the whole `PaneInfo`, and each caller narrows it in the open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneInfo {
    pub tab_id: TabId,
    pub agent_status: Option<AgentStatus>,
    pub agent_session: Option<AgentSession>,
}

pub trait Herdr {
    /// Create a new `--no-focus` herdr workspace (label + cwd); returns its id and
    /// its auto-created root shell pane id.
    fn workspace_create(&self, label: &str, cwd: &str) -> io::Result<Workspace>;
    /// Close a herdr workspace (closes all its panes). This is the only pane
    /// teardown drovr performs — once, at end-of-run.
    fn workspace_close(&self, id: &str) -> io::Result<()>;
    /// Create a new `--no-focus` tab in `workspace` (label + cwd); returns the
    /// tab's auto-created shell pane id. Every phase after the first gets its own
    /// tab so each phase agent occupies a full pane with no split.
    fn tab_create(&self, workspace: &str, label: &str, cwd: &str) -> io::Result<String>;
    /// Launch `command` inside an existing pane (`herdr pane run`). drovr runs
    /// `claude` in a tab's shell pane instead of splitting a second pane beside it.
    fn pane_run(&self, pane_id: &str, command: &str) -> io::Result<()>;
    /// Cosmetically label a pane with its phase name (best-effort).
    fn pane_rename(&self, pane_id: &str, label: &str) -> io::Result<()>;
    /// The currently-focused workspace id, if any. Captured before pane
    /// operations that can move focus so it can be restored afterward.
    fn focused_workspace(&self) -> Option<String>;
    /// Restore focus to a workspace (best-effort). `pane_run`/`pane_rename` have
    /// no `--no-focus` flag, so drovr captures focus before and restores it after.
    fn workspace_focus(&self, id: &str) -> io::Result<()>;
    fn agent_send(&self, target: &str, text: &str) -> io::Result<()>;
    /// Press keys in an agent's pane (`herdr agent send-keys`). Unlike
    /// [`Herdr::agent_send`], which types a prompt, this drives the agent's
    /// *menus* — the "New MCP server" approval, the trust-dir prompt, the model
    /// picker — which only answer to keypresses. `keys` are herdr key names
    /// (`enter`, `esc`, `up`, `down`, digits like `3`).
    fn agent_send_keys(&self, target: &str, keys: &[String]) -> io::Result<()>;
    fn agent_read(&self, target: &str) -> io::Result<String>;
    /// Read a pane's state (socket `pane.get`): its tab, its agent status and its
    /// agent session. `None` means the pane could not be read at all — NOT that
    /// its agent is gone (see [`PaneInfo`] for that distinction).
    ///
    /// READ-ONLY: `pane.get` is a query and does not move focus, so this is safe
    /// to poll every iteration without disturbing the user.
    ///
    /// It is drovr's ONLY poll. There is deliberately no `agent_status`
    /// projection beside it: such a helper returned `None` both when the poll
    /// failed and when herdr answered that the agent had exited, and reaping
    /// turns on telling those apart. Callers narrow this themselves —
    /// `pane_info(id).and_then(|info| info.agent_status)` where only the status
    /// matters — so a collapse is always written out loud at the site that wants
    /// it. **Do not re-add a status-only method to this trait.**
    fn pane_info(&self, pane_id: &str) -> Option<PaneInfo>;
    /// Close a tab and every pane in it (socket `tab.close`). The only pane
    /// teardown besides `workspace_close`, and it must never be aimed at the tab
    /// holding a run's root pane — a policy the CALLER enforces, since only it
    /// knows the run.
    ///
    /// Takes a [`TabId`], which only a `pane_info` read can produce, so a pane id
    /// cannot be passed here by mistake.
    // Capability only for now: nothing in drovr closes a pane yet — reaping is a
    // later step, and this landing on its own must not change any behavior.
    #[allow(dead_code)]
    fn tab_close(&self, tab_id: &TabId) -> io::Result<()>;
    fn integration_present(&self, agent: &str) -> bool;
}

// ---------------------------------------------------------------------------
// SystemHerdr — talks to the real herdr daemon over its Unix-socket JSON-RPC API
// ---------------------------------------------------------------------------

/// Read timeout for a single JSON-RPC request/response round-trip on the socket.
const SOCKET_READ_TIMEOUT: Duration = Duration::from_secs(5);

/// Claude auth env vars propagated to spawned agents so they use the caller's
/// authenticated profile rather than the default `~/.claude` dir.
const AGENT_ENV_VARS: &[&str] = &["CLAUDE_CONFIG_DIR", "ANTHROPIC_API_KEY", "ANTHROPIC_MODEL"];

pub struct SystemHerdr;

impl SystemHerdr {
    pub fn new() -> Self {
        Self
    }

    /// Shell out to the `herdr` binary (still used for `integration status` and
    /// `session stop`, which are unchanged in 0.7.5).
    fn run(&self, args: &[&str]) -> io::Result<std::process::Output> {
        Command::new("herdr").args(args).output()
    }

    /// Perform one JSON-RPC call over the herdr Unix socket. Writes a single
    /// request line and reads a single response line; returns the `result`
    /// value on success, or an `io::Error` carrying the error message.
    fn socket_call(&self, method: &str, params: Value) -> io::Result<Value> {
        let path = std::env::var("HERDR_SOCKET_PATH").map_err(|_| {
            io::Error::other("HERDR_SOCKET_PATH is not set; cannot reach herdr socket")
        })?;
        let id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos().to_string())
            .unwrap_or_else(|_| "0".to_string());

        let mut stream = UnixStream::connect(&path)?;
        stream.set_read_timeout(Some(SOCKET_READ_TIMEOUT))?;

        let request = json!({ "id": id, "method": method, "params": params });
        let mut line = serde_json::to_string(&request)?;
        line.push('\n');
        stream.write_all(line.as_bytes())?;
        stream.flush()?;

        let mut reader = BufReader::new(stream);
        let mut response = String::new();
        reader.read_line(&mut response)?;
        if response.trim().is_empty() {
            return Err(io::Error::other(format!(
                "herdr socket returned empty response for method {method}"
            )));
        }

        let value: Value = serde_json::from_str(response.trim())?;
        if let Some(err) = value.get("error") {
            let msg = err
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("unknown herdr error");
            return Err(io::Error::other(msg.to_string()));
        }
        Ok(value.get("result").cloned().unwrap_or(Value::Null))
    }

    /// Build the `env` object for `workspace.create` from the claude auth vars
    /// currently set in this process's environment, plus the flicker-suppression
    /// flag. Every pane later created in the workspace (root pane + each phase's
    /// tab shell) inherits this env, so the spawned `claude` uses the caller's
    /// authenticated profile (`CLAUDE_CONFIG_DIR`) instead of the default
    /// `~/.claude`, and skips its first-run fullscreen-renderer upsell (which a
    /// freshly-spawned interactive agent has no human to answer — see
    /// `spawn_env_flags`).
    fn agent_env(&self) -> Value {
        let mut map = serde_json::Map::new();
        for var in AGENT_ENV_VARS {
            if let Ok(val) = std::env::var(var) {
                map.insert((*var).to_string(), Value::String(val));
            }
        }
        // Always suppress the first-run renderer upsell (see spawn_env_flags).
        map.insert(
            "CLAUDE_CODE_NO_FLICKER".to_string(),
            Value::String("1".to_string()),
        );
        Value::Object(map)
    }
}

impl Herdr for SystemHerdr {
    fn workspace_create(&self, label: &str, cwd: &str) -> io::Result<Workspace> {
        // 0.7.5 socket API: create the workspace over the Unix socket rather than
        // shelling `herdr workspace create`. `focus:false` mirrors the old
        // `--no-focus` (never steal focus from the user); the auth env is set at
        // the WORKSPACE level so every pane later created in it (the root shell
        // pane and each phase's tab shell) inherits the caller's claude profile
        // (`CLAUDE_CONFIG_DIR`) instead of the default ~/.claude.
        let result = self.socket_call(
            "workspace.create",
            json!({ "label": label, "cwd": cwd, "focus": false, "env": self.agent_env() }),
        )?;
        let id = find_string_field(&result, "workspace_id").ok_or_else(|| {
            io::Error::other(format!(
                "workspace.create: could not find workspace_id in result: {result}"
            ))
        })?;
        // The result's `root_pane.pane_id` is the auto-created shell pane the
        // first phase will reuse (found by walking the result for `pane_id`).
        let root_pane = find_string_field(&result, "pane_id").ok_or_else(|| {
            io::Error::other(format!(
                "workspace.create: could not find root_pane pane_id in result: {result}"
            ))
        })?;
        Ok(Workspace { id, root_pane })
    }

    fn workspace_close(&self, id: &str) -> io::Result<()> {
        // 0.7.5 socket API: close over the socket rather than shelling
        // `herdr workspace close`.
        self.socket_call("workspace.close", json!({ "workspace_id": id }))?;
        Ok(())
    }

    fn tab_create(&self, workspace: &str, label: &str, cwd: &str) -> io::Result<String> {
        // Socket `tab.create`, carrying the auth env explicitly. A tab's shell
        // pane does NOT inherit the workspace-level env set at `workspace.create`
        // (verified empirically), so without this the spawned agent would miss
        // `CLAUDE_CONFIG_DIR` and fall back to the default `~/.claude` profile.
        // `focus:false` keeps the user's focus put. Returns the new tab's shell
        // pane id (`result.root_pane.pane_id`).
        let result = self.socket_call(
            "tab.create",
            json!({
                "workspace_id": workspace,
                "label": label,
                "cwd": cwd,
                "focus": false,
                "env": self.agent_env(),
            }),
        )?;
        find_string_field(&result, "pane_id").ok_or_else(|| {
            io::Error::other(format!(
                "tab.create: could not find pane_id in result: {result}"
            ))
        })
    }

    fn pane_run(&self, pane_id: &str, command: &str) -> io::Result<()> {
        let out = self.run(&["pane", "run", pane_id, command])?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(io::Error::other(format!("herdr pane run failed: {stderr}")));
        }
        Ok(())
    }

    fn pane_rename(&self, pane_id: &str, label: &str) -> io::Result<()> {
        let out = self.run(&["pane", "rename", pane_id, label])?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(io::Error::other(format!(
                "herdr pane rename failed: {stderr}"
            )));
        }
        Ok(())
    }

    fn focused_workspace(&self) -> Option<String> {
        let out = self.run(&["workspace", "list"]).ok()?;
        if !out.status.success() {
            return None;
        }
        let stdout = String::from_utf8_lossy(&out.stdout);
        let v: serde_json::Value = serde_json::from_str(&stdout).ok()?;
        let workspaces = v.get("result")?.get("workspaces")?.as_array()?;
        workspaces
            .iter()
            .find(|w| w.get("focused").and_then(|f| f.as_bool()).unwrap_or(false))
            .and_then(|w| w.get("workspace_id").and_then(|i| i.as_str()))
            .map(|s| s.to_owned())
    }

    fn workspace_focus(&self, id: &str) -> io::Result<()> {
        let out = self.run(&["workspace", "focus", id])?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(io::Error::other(format!(
                "herdr workspace focus failed: {stderr}"
            )));
        }
        Ok(())
    }

    fn agent_send(&self, target: &str, text: &str) -> io::Result<()> {
        // 0.7.5 socket API: agent.prompt types AND submits the prompt natively,
        // so the old PASTE_SETTLE/flush-CR handshake (0.7.3's `agent send`, which
        // only wrote text into the input box) is gone.
        self.socket_call(
            "agent.prompt",
            json!({ "target": target, "text": text }),
        )?;
        Ok(())
    }

    fn agent_send_keys(&self, target: &str, keys: &[String]) -> io::Result<()> {
        // No socket method for keypresses in 0.7.5 — shell the CLI, which takes
        // the key names as trailing positionals: `agent send-keys <target> 3 enter`.
        let mut args = vec!["agent", "send-keys", target];
        args.extend(keys.iter().map(String::as_str));
        let out = self.run(&args)?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(io::Error::other(format!(
                "herdr agent send-keys failed: {stderr}"
            )));
        }
        Ok(())
    }

    fn agent_read(&self, target: &str) -> io::Result<String> {
        // 0.7.5 socket API: agent.read nests the transcript at
        // `result.read.text` (result = {type:"pane_read", read:{…,text}}), so
        // dig for it rather than reading `result.text` (which is always absent).
        let result = self.socket_call(
            "agent.read",
            json!({ "target": target, "source": "recent" }),
        )?;
        Ok(find_string_field(&result, "text").unwrap_or_default())
    }

    fn pane_info(&self, pane_id: &str) -> Option<PaneInfo> {
        // Socket `pane.get` (params: `pane_id`) is a read-only query and does not
        // move focus, so this is safe to poll from `phase_wait` on every
        // iteration. Every failure — herdr unreachable, pane gone, unexpected
        // shape — collapses to `None`: a best-effort read must never break a
        // poll loop. But it must not collapse SILENTLY: both failures present
        // downstream as an unexplained readiness or wait timeout, so each is
        // reported once per process.
        let result = match self.socket_call("pane.get", json!({ "pane_id": pane_id })) {
            Ok(result) => result,
            Err(err) => {
                // Connection refused, read timeout, or a JSON-RPC error such as
                // an unknown pane — herdr's own message is the only clue, and
                // `.ok()?` used to drop it on the floor.
                if first_time(&PANE_GET_ERROR_WARNED) {
                    eprintln!("{}", pane_get_error_message(pane_id, &err.to_string()));
                }
                return None;
            }
        };
        let info = parse_pane_info(&result);
        if info.is_none() && first_time(&PANE_GET_SHAPE_WARNED) {
            // A closed/unknown pane comes back as a socket *error* handled
            // above, so an unparseable SUCCESS means herdr's response shape
            // moved under us. That degrades silently and totally (every poll
            // `None` → `phase_send` burns its readiness timeout on a healthy
            // agent, `blocked` is never detected early).
            eprintln!("{}", pane_get_shape_message(&result));
        }
        info
    }

    fn tab_close(&self, tab_id: &TabId) -> io::Result<()> {
        // Socket `tab.close` (params: `tab_id`) → `{"type":"ok"}`.
        self.socket_call("tab.close", json!({ "tab_id": tab_id.as_str() }))?;
        Ok(())
    }

    fn integration_present(&self, agent: &str) -> bool {
        let Ok(out) = self.run(&["integration", "status"]) else {
            return false;
        };
        let stdout = String::from_utf8_lossy(&out.stdout);
        let prefix = format!("{agent}:");
        stdout
            .lines()
            .any(|line| line.starts_with(&prefix) && !line.contains("not installed"))
    }
}

/// Recursively search a JSON value for a string field with the given key,
/// returning its (non-empty) value. Defensive against nesting: the socket
/// result shape may wrap the field in one or more objects/arrays.
fn find_string_field(value: &Value, key: &str) -> Option<String> {
    match value {
        Value::Object(map) => {
            if let Some(Value::String(s)) = map.get(key) {
                if !s.is_empty() {
                    return Some(s.clone());
                }
            }
            for v in map.values() {
                if let Some(found) = find_string_field(v, key) {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(items) => {
            for v in items {
                if let Some(found) = find_string_field(v, key) {
                    return Some(found);
                }
            }
            None
        }
        _ => None,
    }
}

/// Set once the first unreadable `pane.get` success has been reported, so a
/// 500 ms poll loop warns once per process rather than twice a second.
static PANE_GET_SHAPE_WARNED: AtomicBool = AtomicBool::new(false);

/// Set once the first socket-layer `pane.get` failure has been reported.
static PANE_GET_ERROR_WARNED: AtomicBool = AtomicBool::new(false);

/// `true` the first time it is called for `flag`, `false` forever after. Turns
/// a 500 ms poll loop's diagnostic into one line per process instead of two a
/// second.
fn first_time(flag: &AtomicBool) -> bool {
    !flag.swap(true, Ordering::Relaxed)
}

/// The diagnostic for a `pane.get` that succeeded with a shape
/// [`parse_pane_info`] does not recognise. Names the result's top-level keys
/// only, never their values: the payload carries cwds and terminal titles.
fn pane_get_shape_message(result: &Value) -> String {
    let keys = match result.as_object() {
        Some(map) => map.keys().cloned().collect::<Vec<_>>().join(", "),
        None => "<not an object>".to_string(),
    };
    format!(
        "drovr: herdr's pane.get returned a shape drovr cannot read \
         (expected a `pane` object with a `tab_id`; got keys: {keys}). \
         Agent status polling is degraded — phase sends will wait out their \
         readiness timeout. Check the herdr version."
    )
}

/// The diagnostic for a `pane.get` that failed at the socket layer.
fn pane_get_error_message(pane_id: &str, err: &str) -> String {
    format!(
        "drovr: herdr's pane.get failed for pane {pane_id}: {err}. \
         Agent status polling is degraded — phase sends and waits will run to \
         their timeouts with no other explanation. (A pane that has been closed \
         reports this too.)"
    )
}

/// A non-empty string field of `value`, or `None`. herdr writes `""` for some
/// absent fields, and an empty tab id or session value is as useless as a
/// missing one.
fn non_empty_string(value: &Value, key: &str) -> Option<String> {
    match value.get(key)?.as_str()? {
        "" => None,
        s => Some(s.to_string()),
    }
}

/// Parse the `result` of a `pane.get` call into a [`PaneInfo`].
///
/// herdr 0.7.5 nests the payload at `result.pane` (`{type:"pane_info", pane:{…}}`),
/// not on `result` itself. This reads it STRUCTURALLY rather than with
/// [`find_string_field`]'s recursive dig: `pane_id`, `tab_id`, `terminal_id` and
/// the session's `agent`/`kind`/`value`/`source` are all strings, so a
/// first-match walk would happily hand back a `pane_id` when asked for a
/// `tab_id`.
///
/// Returns `None` when the shape is not a single pane or carries no `tab_id` —
/// a `PaneInfo` that cannot name its tab serves neither caller.
fn parse_pane_info(result: &Value) -> Option<PaneInfo> {
    let pane = result.get("pane")?;
    Some(PaneInfo {
        tab_id: TabId(non_empty_string(pane, "tab_id")?),
        agent_status: non_empty_string(pane, "agent_status")
            .map(|raw| AgentStatus::from_herdr(&raw)),
        agent_session: pane.get("agent_session").and_then(parse_agent_session),
    })
}

/// Parse a pane's `agent_session` object. Both `kind` and `value` are required —
/// a session drovr cannot classify is one it must not resume — while `agent` is
/// optional. Note that a pane whose agent has exited has no `agent_session` key
/// at all, which is exactly `None` here.
fn parse_agent_session(value: &Value) -> Option<AgentSession> {
    let kind = non_empty_string(value, "kind")?;
    let session_value = non_empty_string(value, "value")?;
    Some(match kind.as_str() {
        "id" => AgentSession::Id {
            value: SessionId(session_value),
            agent: non_empty_string(value, "agent"),
        },
        "path" => AgentSession::Path {
            value: session_value,
        },
        _ => AgentSession::Other {
            kind,
            value: session_value,
        },
    })
}

// ---------------------------------------------------------------------------
// FakeHerdr — records calls; scripted return values for tests
// ---------------------------------------------------------------------------

#[cfg(test)]
pub struct FakeHerdr {
    calls: RefCell<Vec<String>>,
    counter: RefCell<u32>,
    /// Queued return strings for agent_read (FIFO)
    read_queue: RefCell<VecDeque<String>>,
    /// Queued statuses for the `pane_info` poll (FIFO), the shorthand for tests
    /// that only care about `agent_status`. `None` entries model a pane that
    /// could not be read at all.
    status_queue: RefCell<VecDeque<Option<String>>>,
    /// Queued whole `pane_info` results (FIFO). Takes precedence over
    /// `status_queue`, for tests that care about the tab id or the session.
    pane_info_queue: RefCell<VecDeque<Option<PaneInfo>>>,
    /// When true, the next `pane_run` returns an error (tests the failure path).
    fail_pane_run: RefCell<bool>,
    /// When true, every `pane_info` reads as unreadable (`None`).
    fail_pane_info: RefCell<bool>,
    /// When true, every `tab_close` returns an error — reaping is best-effort,
    /// so callers must survive it.
    fail_tab_close: RefCell<bool>,
}

#[cfg(test)]
impl FakeHerdr {
    pub fn new() -> Self {
        Self {
            calls: RefCell::new(Vec::new()),
            counter: RefCell::new(0),
            read_queue: RefCell::new(VecDeque::new()),
            status_queue: RefCell::new(VecDeque::new()),
            pane_info_queue: RefCell::new(VecDeque::new()),
            fail_pane_run: RefCell::new(false),
            fail_pane_info: RefCell::new(false),
            fail_tab_close: RefCell::new(false),
        }
    }

    /// The tab the fake reports for `pane_id` when a test has not scripted a
    /// whole `PaneInfo`. Exposed so tests can assert on `tab_close` without
    /// hard-coding the derivation.
    pub fn tab_id_for(pane_id: &str) -> TabId {
        TabId(format!("tab-of-{pane_id}"))
    }

    pub fn calls(&self) -> Vec<String> {
        self.calls.borrow().clone()
    }

    /// Queue a string to be returned by the next `agent_read` call.
    pub fn push_read(&self, text: impl Into<String>) {
        self.read_queue.borrow_mut().push_back(text.into());
    }

    /// Queue a status for the next `pane_info` poll (and so for the next
    /// `agent_status`). Pass `Some("blocked")` to model a blocked pane, or `None`
    /// to model a pane that cannot be read. Mirrors `push_read`.
    ///
    /// Takes the RAW herdr string, classified through [`AgentStatus::from_herdr`]
    /// exactly as a real response would be — so `push_status(Some("compacting"))`
    /// models a herdr state drovr has never seen.
    pub fn push_status(&self, status: Option<impl Into<String>>) {
        self.status_queue
            .borrow_mut()
            .push_back(status.map(Into::into));
    }

    /// Queue a whole `PaneInfo` for the next `pane_info` call — for tests that
    /// care about the tab id or the agent session. Takes precedence over
    /// `push_status`. `None` models a pane that could not be read.
    pub fn push_pane_info(&self, info: Option<PaneInfo>) {
        self.pane_info_queue.borrow_mut().push_back(info);
    }

    /// Make the next `pane_run` fail, so a caller's error handling can be tested.
    pub fn fail_pane_run(&self) {
        *self.fail_pane_run.borrow_mut() = true;
    }

    /// Make every `pane_info` read as unreadable (`None`), whatever is queued.
    pub fn fail_pane_info(&self) {
        *self.fail_pane_info.borrow_mut() = true;
    }

    /// Make every `tab_close` fail. Reaping is best-effort: a phase must survive
    /// this without erroring and without marking itself reaped.
    pub fn fail_tab_close(&self) {
        *self.fail_tab_close.borrow_mut() = true;
    }

    fn record(&self, call: String) {
        self.calls.borrow_mut().push(call);
    }

    fn next_id(&self) -> String {
        let mut c = self.counter.borrow_mut();
        *c += 1;
        format!("pane-{}", *c)
    }
}

#[cfg(test)]
impl Herdr for FakeHerdr {
    fn workspace_create(&self, label: &str, cwd: &str) -> io::Result<Workspace> {
        let mut c = self.counter.borrow_mut();
        *c += 1;
        let ws_id = format!("ws-{}", *c);
        drop(c);
        let root_pane = format!("{ws_id}:root");
        self.record(format!(
            "workspace_create label={label} cwd={cwd} -> {ws_id} root_pane={root_pane}"
        ));
        Ok(Workspace {
            id: ws_id,
            root_pane,
        })
    }

    fn workspace_close(&self, id: &str) -> io::Result<()> {
        self.record(format!("workspace_close id={id}"));
        Ok(())
    }

    fn tab_create(&self, workspace: &str, label: &str, cwd: &str) -> io::Result<String> {
        let id = self.next_id();
        self.record(format!(
            "tab_create workspace={workspace} label={label} cwd={cwd} -> {id}"
        ));
        Ok(id)
    }

    fn pane_run(&self, pane_id: &str, command: &str) -> io::Result<()> {
        self.record(format!("pane_run pane={pane_id} command={command:?}"));
        if *self.fail_pane_run.borrow() {
            return Err(io::Error::other("scripted pane_run failure"));
        }
        Ok(())
    }

    fn pane_rename(&self, pane_id: &str, label: &str) -> io::Result<()> {
        self.record(format!("pane_rename pane={pane_id} label={label}"));
        Ok(())
    }

    fn focused_workspace(&self) -> Option<String> {
        self.record("focused_workspace".to_string());
        Some("ws-focused".to_string())
    }

    fn workspace_focus(&self, id: &str) -> io::Result<()> {
        self.record(format!("workspace_focus id={id}"));
        Ok(())
    }

    fn agent_send(&self, target: &str, text: &str) -> io::Result<()> {
        self.record(format!("agent_send target={target} text={text:?}"));
        Ok(())
    }

    fn agent_send_keys(&self, target: &str, keys: &[String]) -> io::Result<()> {
        self.record(format!("agent_send_keys target={target} keys={keys:?}"));
        Ok(())
    }

    fn agent_read(&self, target: &str) -> io::Result<String> {
        self.record(format!("agent_read target={target}"));
        let text = self.read_queue.borrow_mut().pop_front().unwrap_or_default();
        Ok(text)
    }

    fn pane_info(&self, pane_id: &str) -> Option<PaneInfo> {
        // Resolution order: a scripted failure, then a whole scripted `PaneInfo`,
        // then a scripted status, then the default. A scripted status (pushed via
        // `push_status`) is consumed FIFO; when both queues are empty the fake
        // models a booted, ready agent parked at its composer — `Some("idle")` —
        // so a test that does not care about status (the common case) sails
        // through `phase_send`'s readiness gate instead of waiting out its
        // timeout. Tests that need a different status (blocked, done, or an
        // unreadable `None`) push it explicitly.
        let info = if *self.fail_pane_info.borrow() {
            None
        } else if let Some(scripted) = self.pane_info_queue.borrow_mut().pop_front() {
            scripted
        } else {
            let status = match self.status_queue.borrow_mut().pop_front() {
                Some(scripted) => scripted,
                None => Some("idle".to_string()),
            };
            // A scripted `None` status means the pane could not be read at all.
            status.map(|status| PaneInfo {
                tab_id: Self::tab_id_for(pane_id),
                agent_status: Some(AgentStatus::from_herdr(&status)),
                agent_session: None,
            })
        };
        // `pane_info` IS the status poll, so the recorded line names the status:
        // assertions that count (or forbid) status polls match on `agent_status`.
        let outcome = match &info {
            Some(i) => format!(
                "tab_id={} agent_status={:?} agent_session={:?}",
                i.tab_id.as_str(),
                i.agent_status,
                i.agent_session
            ),
            None => "unreadable agent_status=None".to_string(),
        };
        self.record(format!("pane_info pane={pane_id} -> {outcome}"));
        info
    }

    fn tab_close(&self, tab_id: &TabId) -> io::Result<()> {
        // Recorded before the scripted failure, so call-order assertions see the
        // attempt whether or not it succeeded.
        self.record(format!("tab_close tab={}", tab_id.as_str()));
        if *self.fail_tab_close.borrow() {
            return Err(io::Error::other("scripted tab_close failure"));
        }
        Ok(())
    }

    fn integration_present(&self, agent: &str) -> bool {
        self.record(format!("integration_present agent={agent}"));
        true
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_records_and_returns() {
        let h = FakeHerdr::new();
        let pane = h.tab_create("ws-1", "brainstorm", "/tmp").unwrap();
        assert!(!pane.is_empty());
        h.pane_run(&pane, "claude").unwrap();
        h.agent_send(&pane, "hello").unwrap();
        assert_eq!(h.calls().len(), 3);
        assert!(h.calls()[0].contains("tab_create"));
        assert!(h.calls()[1].contains("pane_run"));
        assert!(h.calls()[2].contains("send"));
    }

    #[test]
    fn fake_sequential_ids() {
        let h = FakeHerdr::new();
        let id1 = h.tab_create("ws-1", "a", "/tmp").unwrap();
        let id2 = h.tab_create("ws-1", "b", "/tmp").unwrap();
        assert_ne!(id1, id2);
    }

    #[test]
    fn fake_read_queue() {
        let h = FakeHerdr::new();
        h.push_read("output text");
        let pane = h.tab_create("ws-1", "x", "/").unwrap();
        h.pane_run(&pane, "claude").unwrap();
        let text = h.agent_read(&pane).unwrap();
        assert_eq!(text, "output text");
    }

    #[test]
    fn integration_present_recorded() {
        let h = FakeHerdr::new();
        assert!(h.integration_present("claude"));
        assert!(h.calls()[0].contains("integration_present"));
    }

    #[test]
    fn find_string_field_extracts_top_level() {
        let v: Value = serde_json::from_str(
            r#"{"pane_id":"w1:pXY","tab_id":"w1:tXY"}"#,
        )
        .unwrap();
        assert_eq!(find_string_field(&v, "pane_id").as_deref(), Some("w1:pXY"));
    }

    #[test]
    fn find_string_field_extracts_nested() {
        // workspace.create wraps the id inside a nested object.
        let v: Value = serde_json::from_str(
            r#"{"workspace":{"workspace_id":"w7","label":"drovr:demo"}}"#,
        )
        .unwrap();
        assert_eq!(find_string_field(&v, "workspace_id").as_deref(), Some("w7"));
    }

    #[test]
    fn find_string_field_missing_returns_none() {
        let v: Value = serde_json::from_str(r#"{"no":"pane"}"#).unwrap();
        assert!(find_string_field(&v, "pane_id").is_none());
    }

    #[test]
    fn find_string_field_ignores_empty() {
        let v: Value = serde_json::from_str(r#"{"pane_id":""}"#).unwrap();
        assert!(find_string_field(&v, "pane_id").is_none());
    }

    /// A verbatim `pane.get` response from herdr 0.7.5 for a pane whose claude
    /// agent is alive. Captured live by the driver; the payload nests at
    /// `result.pane`, NOT on `result` itself.
    const LIVE_PANE_GET: &str = r#"{"id":"cli:pane:get","result":{"pane":{"agent":"claude","agent_session":{"agent":"claude","kind":"id","source":"herdr:claude","value":"cca92f5b-3a8c-4008-a9f2-e2fa191395e5"},"agent_status":"working","cwd":"/home/sauyon/devel/drovr","display_agent":"claude","focused":false,"label":"implement-task-1","pane_id":"wAF:p1","revision":4,"tab_id":"wAF:t1","terminal_id":"term_abc","workspace_id":"wAF"},"type":"pane_info"}}"#;

    /// The same call against a pane whose agent has EXITED: `agent_status` is
    /// `"unknown"` and herdr drops the `agent` / `agent_session` keys entirely.
    /// The pane (and its `tab_id`) still exist.
    const EXITED_PANE_GET: &str = r#"{"id":"cli:pane:get","result":{"pane":{"agent_status":"unknown","cwd":"/home/sauyon/devel/drovr","focused":false,"label":"review-task-1","pane_id":"wAF:p2","revision":9,"tab_id":"wAF:t2","terminal_id":"term_def","workspace_id":"wAF"},"type":"pane_info"}}"#;

    /// `socket_call` hands `parse_pane_info` the `result` value, so tests peel
    /// the JSON-RPC envelope the same way.
    fn socket_result(json: &str) -> Value {
        serde_json::from_str::<Value>(json)
            .unwrap()
            .get("result")
            .cloned()
            .unwrap()
    }

    #[test]
    fn parse_pane_info_reads_live_pane_get() {
        let info = parse_pane_info(&socket_result(LIVE_PANE_GET)).unwrap();
        assert_eq!(info.tab_id.as_str(), "wAF:t1");
        assert_eq!(info.agent_status, Some(AgentStatus::Working));
        let session = info.agent_session.expect("live agent carries a session");
        assert_eq!(session.kind(), "id");
        assert_eq!(
            session.resumable().map(|r| r.id.as_str()),
            Some("cca92f5b-3a8c-4008-a9f2-e2fa191395e5")
        );
        assert_eq!(session.agent(), Some("claude"));
    }

    // A poll that fails at the SOCKET layer (connection refused, read timeout,
    // JSON-RPC "unknown pane") is invisible downstream — it presents as an
    // unexplained readiness or wait timeout. Say so once, naming the pane and
    // herdr's own message.
    #[test]
    fn pane_get_error_message_names_the_pane_and_the_cause() {
        let msg = pane_get_error_message("wAF:p1", "connection refused");
        assert!(msg.contains("wAF:p1"), "msg: {msg}");
        assert!(msg.contains("connection refused"), "msg: {msg}");
        assert!(msg.contains("pane.get"), "msg: {msg}");
    }

    // The shape diagnostic names the KEYS it got, never their values: the
    // payload carries cwds and terminal titles.
    #[test]
    fn pane_get_shape_message_lists_keys_not_values() {
        let msg = pane_get_shape_message(&socket_result(
            r#"{"result":{"panes":[{"pane_id":"w1:p1","cwd":"/home/someone/secret-project"}]}}"#,
        ));
        assert!(msg.contains("panes"), "msg: {msg}");
        assert!(
            !msg.contains("secret-project"),
            "values must never be printed: {msg}"
        );
        // A non-object result is reported, not swallowed.
        assert!(pane_get_shape_message(&Value::Null).contains("not an object"));
    }

    // Both diagnostics are gated so a 500 ms poll loop reports once per process
    // rather than twice a second.
    #[test]
    fn first_time_is_true_exactly_once() {
        let flag = AtomicBool::new(false);
        assert!(first_time(&flag), "the first call reports");
        assert!(!first_time(&flag), "and every later one is silent");
        assert!(!first_time(&flag));
    }

    #[test]
    fn agent_status_parses_herdrs_vocabulary() {
        for (raw, expected) in [
            ("idle", AgentStatus::Idle),
            ("working", AgentStatus::Working),
            ("blocked", AgentStatus::Blocked),
            ("done", AgentStatus::Done),
            ("unknown", AgentStatus::Unknown),
        ] {
            assert_eq!(AgentStatus::from_herdr(raw), expected, "raw: {raw}");
            assert_eq!(expected.as_str(), raw, "as_str must round-trip: {raw}");
        }
    }

    // A herdr state this drovr has never heard of must be PRESERVED, never
    // collapsed onto a known one. `Done` is what tears a pane down, so mistaking
    // a new herdr state for it would reap a live agent.
    #[test]
    fn an_unrecognised_status_is_preserved_not_collapsed() {
        let status = AgentStatus::from_herdr("compacting");
        assert_eq!(status, AgentStatus::Other("compacting".to_string()));
        assert_ne!(status, AgentStatus::Done);
        assert_ne!(status, AgentStatus::Idle);
        assert_ne!(status, AgentStatus::Unknown);
        assert_eq!(status.as_str(), "compacting", "the raw value survives");
        // And it reaches callers intact, straight off the wire.
        let v = socket_result(
            r#"{"result":{"pane":{"tab_id":"w1:t1","agent_status":"compacting"}}}"#,
        );
        assert_eq!(
            parse_pane_info(&v).unwrap().agent_status,
            Some(AgentStatus::Other("compacting".to_string()))
        );
    }

    // `kind == "id"` is the ONLY value that may ever be interpolated into an
    // agent's `--resume`. The type carries that rule so no caller has to.
    #[test]
    fn only_an_id_session_is_resumable() {
        let id = AgentSession::Id {
            value: SessionId("cca92f5b".to_string()),
            agent: Some("claude".to_string()),
        };
        let resumable = id.resumable().expect("an id session with a known agent");
        assert_eq!(resumable.id, &SessionId("cca92f5b".to_string()));
        assert_eq!(
            resumable.id.as_str(),
            "cca92f5b",
            "the raw id is still reachable, but only through a SessionId"
        );
        assert_eq!(
            resumable.agent, "claude",
            "the backend to check against comes WITH the id, not separately"
        );
        assert_eq!(id.agent(), Some("claude"));
        assert_eq!(id.kind(), "id");

        // An id whose owning agent herdr did not report is NOT resumable: with
        // no backend to compare against we cannot know the id belongs to this
        // run's agent, and resuming a claude session under cursor is not a
        // recoverable mistake.
        let agentless = AgentSession::Id {
            value: SessionId("cca92f5b".to_string()),
            agent: None,
        };
        assert!(
            agentless.resumable().is_none(),
            "half of the rule is not enough"
        );
        assert_eq!(agentless.kind(), "id", "but it is still an id session");

        let path = AgentSession::Path {
            value: "/tmp/transcript.jsonl".to_string(),
        };
        assert!(path.resumable().is_none(), "a path is never a session id");
        // …and a `Path`'s value cannot be passed off as one: only `Id` carries a
        // `SessionId`, so an or-pattern merging the two variants to lift out a
        // single `value` binding does not type-check.
        assert!(path.resumable().is_none());
        assert_eq!(path.kind(), "path");
        assert_eq!(path.agent(), None);

        let other = AgentSession::Other {
            kind: "handle".to_string(),
            value: "abc".to_string(),
        };
        assert!(
            other.resumable().is_none(),
            "an unrecognised kind is not resumable either"
        );
        assert_eq!(other.kind(), "handle");
    }

    // An exited agent must still yield `Some(PaneInfo)` — with `agent_session:
    // None` and a populated `tab_id`. Task 3 leans on this distinction: `None`
    // means "could not read the pane", which must never clear a captured
    // session, whereas `Some` with no session means "pane alive, agent gone".
    #[test]
    fn parse_pane_info_exited_agent_keeps_tab_id_and_drops_session() {
        let info = parse_pane_info(&socket_result(EXITED_PANE_GET))
            .expect("an exited agent still has a pane");
        assert_eq!(info.tab_id.as_str(), "wAF:t2");
        assert_eq!(info.agent_status, Some(AgentStatus::Unknown));
        assert!(info.agent_session.is_none());
    }

    // Regression guard for the reason this parser is structural: `pane_id`,
    // `tab_id`, `terminal_id` and the session's own fields are ALL strings, so
    // the recursive `find_string_field` dig would return whichever it met first.
    #[test]
    fn parse_pane_info_does_not_confuse_pane_id_with_tab_id() {
        let info = parse_pane_info(&socket_result(LIVE_PANE_GET)).unwrap();
        assert_ne!(info.tab_id.as_str(), "wAF:p1", "tab_id must not be the pane_id");
        // And a pane id cannot be handed to `tab_close` at all: it takes a
        // `TabId`, which only `pane_info`'s parser can build.
        assert_eq!(info.tab_id, TabId("wAF:t1".to_string()));
        assert_eq!(
            find_string_field(&socket_result(LIVE_PANE_GET), "agent").as_deref(),
            Some("claude"),
            "sanity: the recursive dig is still what tab_create/agent_read use"
        );
    }

    #[test]
    fn parse_pane_info_rejects_shapes_that_are_not_a_pane() {
        // `pane.list` shape — a pane *array*, not a single pane. Not a PaneInfo.
        let list = socket_result(
            r#"{"result":{"panes":[{"pane_id":"w1:p1","tab_id":"w1:t1","agent_status":"idle"}]}}"#,
        );
        assert!(parse_pane_info(&list).is_none());
        // No `tab_id` → nothing to close later, so not a usable PaneInfo.
        let no_tab = socket_result(r#"{"result":{"pane":{"pane_id":"w1:p1","agent_status":"idle"}}}"#);
        assert!(parse_pane_info(&no_tab).is_none());
        // An empty tab_id is as good as absent.
        let empty_tab =
            socket_result(r#"{"result":{"pane":{"pane_id":"w1:p1","tab_id":"","agent_status":"idle"}}}"#);
        assert!(parse_pane_info(&empty_tab).is_none());
    }

    // A `kind:"path"` session is still parsed and returned verbatim — task 5's
    // resume path is what refuses to interpolate it, and it can only refuse a
    // value it can see.
    #[test]
    fn parse_pane_info_keeps_non_id_session_kinds() {
        let v = socket_result(
            r#"{"result":{"pane":{"pane_id":"w1:p1","tab_id":"w1:t1","agent_status":"idle","agent_session":{"kind":"path","value":"/tmp/t.jsonl"}}}}"#,
        );
        let session = parse_pane_info(&v).unwrap().agent_session.unwrap();
        assert_eq!(
            session,
            AgentSession::Path {
                value: "/tmp/t.jsonl".to_string()
            }
        );
        assert!(session.resumable().is_none(), "a path is not a session id");
        // The `agent` key is optional, and an id session parses without it.
        let v = socket_result(
            r#"{"result":{"pane":{"tab_id":"w1:t1","agent_session":{"kind":"id","value":"abc"}}}}"#,
        );
        let session = parse_pane_info(&v).unwrap().agent_session.unwrap();
        assert_eq!(session.kind(), "id", "it is still parsed faithfully");
        assert_eq!(session.agent(), None);
        assert!(
            session.resumable().is_none(),
            "an id herdr did not attribute to an agent is not safely resumable"
        );
    }

    // `kind` and `value` are each independently required: a session missing
    // either one is no session. Asserted separately because a struct literal
    // short-circuits on the first `?`, so dropping both at once would only ever
    // prove the FIRST field is required.
    #[test]
    fn parse_pane_info_ignores_a_session_missing_kind_or_value() {
        let case = |session: &str| {
            let v = socket_result(&format!(
                r#"{{"result":{{"pane":{{"tab_id":"w1:t1","agent_session":{session}}}}}}}"#
            ));
            parse_pane_info(&v).expect("the pane itself is still readable")
        };
        // Neither field.
        let info = case(r#"{"agent":"claude","source":"herdr:claude"}"#);
        assert_eq!(info.tab_id.as_str(), "w1:t1");
        assert!(info.agent_session.is_none(), "an id-less session is no session");
        assert!(info.agent_status.is_none());
        // `kind` only — an unusable session id must not be resumable-looking.
        assert!(
            case(r#"{"kind":"id","agent":"claude"}"#).agent_session.is_none(),
            "a session with no value must not parse"
        );
        assert!(
            case(r#"{"kind":"id","value":"","agent":"claude"}"#)
                .agent_session
                .is_none(),
            "an empty value is as absent as a missing one"
        );
        // `value` only — a value drovr cannot classify must not be resumed.
        assert!(
            case(r#"{"value":"cca92f5b","agent":"claude"}"#)
                .agent_session
                .is_none(),
            "a session with no kind must not parse"
        );
    }

    // `agent_status` is now a provided trait method over the single `pane_info`
    // poll primitive: same contract (`idle|working|blocked|done|unknown` or
    // `None`), one socket round-trip.
    #[test]
    fn a_failed_poll_is_distinguishable_from_an_exited_agent() {
        let h = FakeHerdr::new();
        // herdr could not be reached / the pane is gone.
        h.push_pane_info(None);
        assert!(
            h.pane_info("w1:p1").is_none(),
            "a failed poll yields no PaneInfo at all"
        );
        // The pane is alive and its agent has exited: herdr answered, the tab is
        // still there, the status is its own `unknown`, and the session is gone.
        h.push_pane_info(Some(PaneInfo {
            tab_id: TabId("w1:t1".to_string()),
            agent_status: Some(AgentStatus::Unknown),
            agent_session: None,
        }));
        let exited = h.pane_info("w1:p1").expect("an exited agent still has a pane");
        assert_eq!(exited.tab_id.as_str(), "w1:t1");
        assert_eq!(exited.agent_status, Some(AgentStatus::Unknown));
        assert!(exited.agent_session.is_none());
        // The two cases must never collapse: a reaper that mistook the first for
        // the second would tear down a pane whose agent is alive and working.
        // There is no projection on the trait that can lose this — `pane_info` is
        // the only poll, and it returns the whole `PaneInfo`.
        h.push_pane_info(Some(PaneInfo {
            tab_id: TabId("w1:t1".to_string()),
            agent_status: Some(AgentStatus::Working),
            agent_session: None,
        }));
        let working = h.pane_info("w1:p1").unwrap();
        assert_ne!(working.agent_status, exited.agent_status);
    }

    // Task 6 asserts on CALL ORDER (agent_read before tab_close, focus captured
    // and restored around the close), so both new primitives must record an
    // unambiguous, argument-carrying line.
    #[test]
    fn fake_records_pane_info_and_tab_close_with_arguments() {
        let h = FakeHerdr::new();
        h.pane_info("w1:p9");
        h.tab_close(&TabId("w1:t9".to_string())).unwrap();
        let calls = h.calls();
        assert_eq!(calls.len(), 2, "one line per call: {calls:?}");
        assert!(calls[0].starts_with("pane_info pane=w1:p9"), "call: {}", calls[0]);
        assert_eq!(calls[1], "tab_close tab=w1:t9");
    }

    // `pane_info` is the status poll, so its recorded line carries the status —
    // that is what the "no status poll happened" assertions in phase.rs match on.
    #[test]
    fn fake_pane_info_records_the_status_it_returned() {
        let h = FakeHerdr::new();
        h.push_status(Some("working"));
        h.pane_info("w1:p1");
        assert!(
            h.calls()[0].contains("agent_status=Some(Working)"),
            "call: {}",
            h.calls()[0]
        );
    }

    // Scripted failures mirror `fail_pane_run`: reaping is best-effort, so task 6
    // needs both a pane whose info cannot be read and a close that fails.
    #[test]
    fn fake_scripted_failures_for_pane_info_and_tab_close() {
        let h = FakeHerdr::new();
        h.fail_pane_info();
        assert!(h.pane_info("w1:p1").is_none());
        assert!(h.pane_info("w1:p1").is_none(), "and stays unreadable");
        // A failed poll is STILL a poll: it must be recorded, and its line must
        // carry `agent_status` like every other, so "a poll happened but the
        // pane was unreadable" stays distinguishable from "no poll happened".
        let calls = h.calls();
        assert_eq!(calls.len(), 2, "both polls recorded: {calls:?}");
        for call in &calls {
            assert!(call.starts_with("pane_info pane=w1:p1"), "call: {call}");
            assert!(call.contains("agent_status=None"), "call: {call}");
        }

        let h = FakeHerdr::new();
        h.fail_tab_close();
        let err = h.tab_close(&TabId("w1:t1".to_string())).unwrap_err();
        assert!(err.to_string().contains("tab_close"), "err: {err}");
        assert_eq!(
            h.calls(),
            vec!["tab_close tab=w1:t1".to_string()],
            "a failed close is still recorded, so order assertions see it"
        );
    }

    // Unscripted, the fake models a normal live pane: an idle agent in a tab
    // derived from the pane id, so tests that only care about `tab_close` can
    // resolve a tab without scripting a full PaneInfo.
    #[test]
    fn fake_pane_info_defaults_to_an_idle_pane_with_a_derivable_tab() {
        let h = FakeHerdr::new();
        let info = h.pane_info("pane-1").unwrap();
        assert_eq!(info.tab_id, FakeHerdr::tab_id_for("pane-1"));
        assert_eq!(info.agent_status, Some(AgentStatus::Idle));
        assert!(info.agent_session.is_none());
    }

    #[test]
    fn fake_status_queue() {
        let h = FakeHerdr::new();
        // Empty queue → a ready, idle agent (so phase_send's readiness gate does
        // not wait out its timeout when a test does not script status).
        let status = |h: &FakeHerdr| h.pane_info("pane-1").and_then(|i| i.agent_status);
        assert_eq!(status(&h), Some(AgentStatus::Idle));
        h.push_status(Some("blocked"));
        h.push_status(None::<String>);
        // Scripted values are consumed FIFO, including an explicit unreadable None.
        assert_eq!(status(&h), Some(AgentStatus::Blocked));
        assert_eq!(h.pane_info("pane-1"), None, "an unreadable pane has no info");
        // Queue drained → back to the idle default.
        assert_eq!(status(&h), Some(AgentStatus::Idle));
        assert!(
            h.calls()
                .iter()
                .filter(|c| c.contains("agent_status"))
                .count()
                >= 3
        );
    }

    // -- Bug A: agent_send via FakeHerdr records the text (CR is a real-herdr detail)
    #[test]
    fn fake_agent_send_records_text_not_cr() {
        let h = FakeHerdr::new();
        h.agent_send("pane-1", "do the thing").unwrap();
        let calls = h.calls();
        assert_eq!(calls.len(), 1);
        assert!(calls[0].contains("do the thing"), "call: {}", calls[0]);
        // FakeHerdr does NOT inject a CR — that's a SystemHerdr submit detail
        assert!(
            !calls[0].contains("\\r"),
            "unexpected CR in fake call: {}",
            calls[0]
        );
    }

    // -- send-keys: the fake records the key *names*, never a text payload -----
    // Menus (the New MCP server approval, trust-dir prompt, model picker) are
    // answered by key presses, not by typing — so the recording must show the
    // keys verbatim and must not look like an `agent_send`.
    #[test]
    fn fake_agent_send_keys_records_keys_not_text() {
        let h = FakeHerdr::new();
        h.agent_send_keys("pane-1", &["3".to_string(), "enter".to_string()])
            .unwrap();
        let calls = h.calls();
        assert_eq!(calls.len(), 1);
        assert!(calls[0].contains("agent_send_keys"), "call: {}", calls[0]);
        assert!(calls[0].contains("target=pane-1"), "call: {}", calls[0]);
        assert!(calls[0].contains("keys=[\"3\", \"enter\"]"), "call: {}", calls[0]);
        // Must be distinguishable from a text send.
        assert!(!calls[0].contains("text="), "call: {}", calls[0]);
    }

    // workspace_create returns both the workspace id and its root shell pane id;
    // the first phase reuses the root pane rather than splitting a new one.
    #[test]
    fn fake_workspace_create_returns_id_and_root_pane() {
        let h = FakeHerdr::new();
        let ws = h.workspace_create("drovr:demo", "/proj").unwrap();
        assert!(!ws.id.is_empty());
        assert!(!ws.root_pane.is_empty(), "root_pane must be populated");
        let call = &h.calls()[0];
        assert!(call.contains("workspace_create"), "call: {call}");
        assert!(call.contains("cwd=/proj"), "cwd must be threaded: {call}");
    }

    // -- agent_env: auth vars propagate via the socket `env`, never inlined -----
    // Under 0.7.5 the caller's claude profile is passed as the `workspace.create`
    // `env` object (inherited by every pane in the workspace) rather than the old
    // 0.7.3 `--env` argv. Only vars that are actually set are forwarded, and the
    // flicker-suppression flag is always present so a freshly-spawned interactive
    // claude skips its first-run "Try the new fullscreen renderer?" upsell (which
    // it would otherwise park on, hanging the phase until `phase wait` times out).
    #[test]
    fn agent_env_includes_set_auth_vars_and_flicker() {
        use crate::test_util::ENV_LOCK;
        let _lock = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::set_var("CLAUDE_CONFIG_DIR", "/home/user/.config/claude-work");
            std::env::remove_var("ANTHROPIC_API_KEY");
            std::env::remove_var("ANTHROPIC_MODEL");
        }
        let env = SystemHerdr::new().agent_env();
        unsafe {
            std::env::remove_var("CLAUDE_CONFIG_DIR");
        }
        let map = env.as_object().expect("agent_env must be a JSON object");
        assert_eq!(
            map.get("CLAUDE_CONFIG_DIR").and_then(Value::as_str),
            Some("/home/user/.config/claude-work"),
            "set auth var must be forwarded: {env}"
        );
        assert!(
            !map.contains_key("ANTHROPIC_API_KEY"),
            "unset key must not appear: {env}"
        );
        assert!(
            !map.contains_key("ANTHROPIC_MODEL"),
            "unset model must not appear: {env}"
        );
        // Flicker suppression is unconditional; the alternate-screen disable knob
        // must NOT be used (it empties `agent read --source recent`, breaking the
        // pane readers — `diagnose_stuck_phase` / `triage_blocked_phase`).
        assert_eq!(
            map.get("CLAUDE_CODE_NO_FLICKER").and_then(Value::as_str),
            Some("1"),
            "flicker-suppression flag must always be present: {env}"
        );
        assert!(
            !map.contains_key("CLAUDE_CODE_DISABLE_ALTERNATE_SCREEN"),
            "must not disable the alternate screen (empties `agent read`, breaks pane readers): {env}"
        );
    }

    #[test]
    fn agent_env_flicker_only_when_auth_unset() {
        use crate::test_util::ENV_LOCK;
        let _lock = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::remove_var("CLAUDE_CONFIG_DIR");
            std::env::remove_var("ANTHROPIC_API_KEY");
            std::env::remove_var("ANTHROPIC_MODEL");
        }
        let env = SystemHerdr::new().agent_env();
        let map = env.as_object().expect("agent_env must be a JSON object");
        assert_eq!(
            map.get("CLAUDE_CODE_NO_FLICKER").and_then(Value::as_str),
            Some("1"),
            "flicker-suppression flag must always be present: {env}"
        );
        assert!(
            !map.contains_key("CLAUDE_CONFIG_DIR"),
            "no auth vars expected when unset: {env}"
        );
        assert!(
            !map.contains_key("ANTHROPIC_API_KEY"),
            "no auth vars expected when unset: {env}"
        );
        assert!(
            !map.contains_key("ANTHROPIC_MODEL"),
            "no auth vars expected when unset: {env}"
        );
    }

    #[test]
    fn agent_env_includes_all_set_vars() {
        use crate::test_util::ENV_LOCK;
        let _lock = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::set_var("CLAUDE_CONFIG_DIR", "/cfg");
            std::env::set_var("ANTHROPIC_API_KEY", "sk-test");
            std::env::set_var("ANTHROPIC_MODEL", "claude-opus-4-5");
        }
        let env = SystemHerdr::new().agent_env();
        unsafe {
            std::env::remove_var("CLAUDE_CONFIG_DIR");
            std::env::remove_var("ANTHROPIC_API_KEY");
            std::env::remove_var("ANTHROPIC_MODEL");
        }
        let map = env.as_object().expect("agent_env must be a JSON object");
        assert_eq!(map.get("CLAUDE_CONFIG_DIR").and_then(Value::as_str), Some("/cfg"), "{env}");
        assert_eq!(
            map.get("ANTHROPIC_API_KEY").and_then(Value::as_str),
            Some("sk-test"),
            "{env}"
        );
        assert_eq!(
            map.get("ANTHROPIC_MODEL").and_then(Value::as_str),
            Some("claude-opus-4-5"),
            "{env}"
        );
        assert_eq!(
            map.get("CLAUDE_CODE_NO_FLICKER").and_then(Value::as_str),
            Some("1"),
            "{env}"
        );
    }

    // Fake focus capture/restore primitives are recorded so phase_start can be
    // asserted to preserve focus around pane operations.
    #[test]
    fn fake_focus_primitives_recorded() {
        let h = FakeHerdr::new();
        let prev = h.focused_workspace();
        assert!(prev.is_some());
        h.workspace_focus(prev.as_deref().unwrap()).unwrap();
        let calls = h.calls();
        assert!(calls.iter().any(|c| c.contains("focused_workspace")));
        assert!(calls.iter().any(|c| c.contains("workspace_focus")));
    }
}
