use std::io;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::process::Command;
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
    fn agent_read(&self, target: &str) -> io::Result<String>;
    /// The pane's `agent_status` (`idle|working|blocked|done|unknown`) as reported
    /// by herdr, or `None` if it cannot be read/parsed. READ-ONLY: it must never
    /// move focus (it shells `herdr pane get`, which does not focus), so
    /// `phase_wait` can poll it every iteration without disturbing the user.
    fn agent_status(&self, pane_id: &str) -> Option<String>;
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

    fn agent_read(&self, target: &str) -> io::Result<String> {
        // 0.7.5 socket API: agent.read returns the recent transcript in `text`.
        let result = self.socket_call(
            "agent.read",
            json!({ "target": target, "source": "recent" }),
        )?;
        let text = result
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        Ok(text)
    }

    fn agent_status(&self, pane_id: &str) -> Option<String> {
        // `pane get` is a read-only query and does not move focus, so this is safe
        // to poll from `phase_wait` on every iteration.
        let out = self.run(&["pane", "get", pane_id]).ok()?;
        if !out.status.success() {
            return None;
        }
        let stdout = String::from_utf8_lossy(&out.stdout);
        parse_agent_status(&stdout)
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

/// Parse a pane's `agent_status` from herdr's `pane get` / `pane list` JSON.
/// The status may live directly on `result` (`pane get`) or on a pane object
/// inside `result.panes` (`pane list`); this walks the value recursively and
/// returns the first `"agent_status"` string it finds. Returns `None` when the
/// field is absent or the JSON does not parse (a best-effort read must never
/// panic the poll loop).
fn parse_agent_status(json: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    find_agent_status(&v)
}

/// Recursively search a JSON value for the first `"agent_status"` string.
fn find_agent_status(v: &serde_json::Value) -> Option<String> {
    match v {
        serde_json::Value::Object(map) => {
            if let Some(serde_json::Value::String(s)) = map.get("agent_status") {
                return Some(s.clone());
            }
            for child in map.values() {
                if let Some(found) = find_agent_status(child) {
                    return Some(found);
                }
            }
            None
        }
        serde_json::Value::Array(arr) => arr.iter().find_map(find_agent_status),
        _ => None,
    }
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
    /// Queued return values for agent_status (FIFO). `None` entries model a pane
    /// whose status could not be read/parsed; an empty queue also yields `None`.
    status_queue: RefCell<VecDeque<Option<String>>>,
    /// When true, the next `pane_run` returns an error (tests the failure path).
    fail_pane_run: RefCell<bool>,
}

#[cfg(test)]
impl FakeHerdr {
    pub fn new() -> Self {
        Self {
            calls: RefCell::new(Vec::new()),
            counter: RefCell::new(0),
            read_queue: RefCell::new(VecDeque::new()),
            status_queue: RefCell::new(VecDeque::new()),
            fail_pane_run: RefCell::new(false),
        }
    }

    pub fn calls(&self) -> Vec<String> {
        self.calls.borrow().clone()
    }

    /// Queue a string to be returned by the next `agent_read` call.
    pub fn push_read(&self, text: impl Into<String>) {
        self.read_queue.borrow_mut().push_back(text.into());
    }

    /// Queue a value to be returned by the next `agent_status` call. Pass
    /// `Some("blocked")` to model a blocked pane, or `None` to model an
    /// unreadable status. Mirrors `push_read`.
    pub fn push_status(&self, status: Option<impl Into<String>>) {
        self.status_queue
            .borrow_mut()
            .push_back(status.map(Into::into));
    }

    /// Make the next `pane_run` fail, so a caller's error handling can be tested.
    pub fn fail_pane_run(&self) {
        *self.fail_pane_run.borrow_mut() = true;
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

    fn agent_read(&self, target: &str) -> io::Result<String> {
        self.record(format!("agent_read target={target}"));
        let text = self.read_queue.borrow_mut().pop_front().unwrap_or_default();
        Ok(text)
    }

    fn agent_status(&self, pane_id: &str) -> Option<String> {
        self.record(format!("agent_status target={pane_id}"));
        // A scripted status (pushed via `push_status`) is consumed FIFO. When the
        // queue is empty the fake models a booted, ready agent parked at its
        // composer — `Some("idle")` — so a test that does not care about status
        // (the common case) sails through `phase_send`'s readiness gate instead of
        // waiting out its timeout. Tests that need a different status (blocked,
        // done, or an unreadable `None`) push it explicitly.
        match self.status_queue.borrow_mut().pop_front() {
            Some(scripted) => scripted,
            None => Some("idle".to_string()),
        }
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

    #[test]
    fn parse_agent_status_from_pane_get() {
        // `pane get` shape: status sits on the `result` object.
        let json = r#"{"result":{"pane_id":"w1:p1","agent_status":"blocked"}}"#;
        assert_eq!(parse_agent_status(json).as_deref(), Some("blocked"));
    }

    #[test]
    fn parse_agent_status_from_pane_list() {
        // `pane list` shape: status sits on a pane object nested in an array.
        let json = r#"{"result":{"panes":[{"pane_id":"w1:p1","agent_status":"idle"},{"pane_id":"w1:p2","agent_status":"working"}]}}"#;
        // Returns the first agent_status found while walking the value.
        assert_eq!(parse_agent_status(json).as_deref(), Some("idle"));
    }

    #[test]
    fn parse_agent_status_missing_or_bad_json_returns_none() {
        assert!(parse_agent_status(r#"{"result":{"pane_id":"w1:p1"}}"#).is_none());
        assert!(parse_agent_status("not json at all").is_none());
    }

    #[test]
    fn fake_status_queue() {
        let h = FakeHerdr::new();
        // Empty queue → a ready, idle agent (so phase_send's readiness gate does
        // not wait out its timeout when a test does not script status).
        assert_eq!(h.agent_status("pane-1").as_deref(), Some("idle"));
        h.push_status(Some("blocked"));
        h.push_status(None::<String>);
        // Scripted values are consumed FIFO, including an explicit unreadable None.
        assert_eq!(h.agent_status("pane-1").as_deref(), Some("blocked"));
        assert_eq!(h.agent_status("pane-1"), None);
        // Queue drained → back to the idle default.
        assert_eq!(h.agent_status("pane-1").as_deref(), Some("idle"));
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
