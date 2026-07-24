use std::io;
use std::process::Command;
use std::time::Duration;

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
// SystemHerdr — shells the real `herdr` binary
// ---------------------------------------------------------------------------

/// Pause between writing a message and sending the primary submit CR. A CR sent
/// immediately after a large `agent send` is swallowed by claude's
/// bracketed-paste handling and never submits; a CR sent after the paste
/// settles submits reliably (verified against the live claude TUI). Kept
/// generous because the failure is timing-dependent (racy) and multi-KB pastes
/// settle slowly.
const PASTE_SETTLE: Duration = Duration::from_millis(250);

/// Pause between the primary submit CR and the flush CR (see [`submit_handshake`]).
/// The flush CR is a bare carriage return sent late enough that the paste has
/// certainly settled, so it submits whatever the primary CR was absorbed into.
/// A CR on an already-submitted (empty) buffer is a harmless no-op, so the flush
/// is always safe. This bakes in the proven "second empty submit" workaround from
/// docs/known-issues.md.
const FLUSH_SETTLE: Duration = Duration::from_millis(900);

/// Submit a just-written message by sending carriage returns as separately-timed
/// keypresses. claude's TUI treats a large `agent send` as a bracketed paste; a
/// CR in the same burst is absorbed into the paste and never submits. So we
/// (1) wait `paste_settle` for the paste to land, then send a CR, and
/// (2) wait `flush_settle`, then send a second, bare CR. The first CR submits
/// when the paste settled in time; the second flushes any paste the first CR was
/// swallowed into. This relies on one observed claude-TUI property: a bare CR on
/// an already-submitted (empty) input box is a no-op, so the redundant flush never
/// double-submits. If a future claude build instead submits an empty message on a
/// bare CR, revisit this (poll the pane buffer before the single CR instead).
///
/// Durations are parameters so tests can drive it with `Duration::ZERO`.
fn submit_handshake<F>(
    mut send_cr: F,
    paste_settle: Duration,
    flush_settle: Duration,
) -> io::Result<()>
where
    F: FnMut() -> io::Result<()>,
{
    std::thread::sleep(paste_settle);
    send_cr()?;
    std::thread::sleep(flush_settle);
    send_cr()?;
    Ok(())
}

pub struct SystemHerdr;

impl SystemHerdr {
    pub fn new() -> Self {
        Self
    }

    fn run(&self, args: &[&str]) -> io::Result<std::process::Output> {
        Command::new("herdr").args(args).output()
    }
}

impl Herdr for SystemHerdr {
    fn workspace_create(&self, label: &str, cwd: &str) -> io::Result<Workspace> {
        let mut args: Vec<String> = vec![
            "workspace".into(),
            "create".into(),
            "--label".into(),
            label.into(),
            "--cwd".into(),
            cwd.into(),
            "--no-focus".into(),
        ];
        args.extend(spawn_env_flags());
        let args_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        let out = self.run(&args_refs)?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(io::Error::other(format!(
                "herdr workspace create failed: {stderr}"
            )));
        }
        let stdout = String::from_utf8_lossy(&out.stdout);
        let id = parse_workspace_id(&stdout).ok_or_else(|| {
            io::Error::other(format!(
                "herdr workspace create: could not parse workspace_id from: {stdout}"
            ))
        })?;
        // The output's first `pane_id` is `result.root_pane.pane_id` — the shell
        // pane the first phase will reuse.
        let root_pane = parse_pane_id(&stdout).ok_or_else(|| {
            io::Error::other(format!(
                "herdr workspace create: could not parse root_pane pane_id from: {stdout}"
            ))
        })?;
        Ok(Workspace { id, root_pane })
    }

    fn workspace_close(&self, id: &str) -> io::Result<()> {
        let out = self.run(&["workspace", "close", id])?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(io::Error::other(format!(
                "herdr workspace close failed: {stderr}"
            )));
        }
        Ok(())
    }

    fn tab_create(&self, workspace: &str, label: &str, cwd: &str) -> io::Result<String> {
        let mut args: Vec<String> = vec![
            "tab".into(),
            "create".into(),
            "--workspace".into(),
            workspace.into(),
            "--label".into(),
            label.into(),
            "--cwd".into(),
            cwd.into(),
            "--no-focus".into(),
        ];
        args.extend(spawn_env_flags());
        let args_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
        let out = self.run(&args_refs)?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(io::Error::other(format!(
                "herdr tab create failed: {stderr}"
            )));
        }
        let stdout = String::from_utf8_lossy(&out.stdout);
        // `tab create` returns `result.root_pane.pane_id` — the new tab's shell pane.
        parse_pane_id(&stdout).ok_or_else(|| {
            io::Error::other(format!(
                "herdr tab create: could not parse pane_id from: {stdout}"
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
        let out = self.run(&["agent", "send", target, text])?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(io::Error::other(format!(
                "herdr agent send failed: {stderr}"
            )));
        }
        // Submit the message. herdr writes the text into the pane's input box
        // without pressing Enter, so a CR is required to dispatch it. The CR must
        // be a separately-timed keypress, and we send two (see submit_handshake):
        // a large paste can swallow the first CR, and the delayed flush CR then
        // submits it reliably.
        submit_handshake(
            || {
                let cr_out = self.run(&["agent", "send", target, "\r"])?;
                if !cr_out.status.success() {
                    let stderr = String::from_utf8_lossy(&cr_out.stderr);
                    return Err(io::Error::other(format!(
                        "herdr agent send (CR) failed: {stderr}"
                    )));
                }
                Ok(())
            },
            PASTE_SETTLE,
            FLUSH_SETTLE,
        )
    }

    fn agent_read(&self, target: &str) -> io::Result<String> {
        let out = self.run(&["agent", "read", target, "--source", "recent"])?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(io::Error::other(format!(
                "herdr agent read failed: {stderr}"
            )));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
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

/// `--env KEY=VALUE` flags applied to every phase-agent pane drovr creates
/// (via `workspace create` / `tab create`). Two groups, both scoped to the
/// phase's `claude`:
///
///   * the claude auth vars set in this process's environment, so the spawned
///     `claude` inherits the caller's authenticated profile rather than the
///     default `~/.claude`. Secrets travel as herdr's `--env` argv, never
///     inlined into a `pane run` command string (which would echo into the
///     terminal buffer);
///   * `CLAUDE_CODE_NO_FLICKER=1`, which makes claude enable the fullscreen
///     renderer directly and SKIP the first-run "Try the new fullscreen
///     renderer?" upsell. A freshly-spawned interactive claude has no human to
///     answer that prompt, so without this it blocks first-run until
///     `phase wait` times out. (Verified: this flag preserves full transcript
///     reads via `agent read --source recent`, so `phase compress` still works;
///     `CLAUDE_CODE_DISABLE_ALTERNATE_SCREEN=1` does NOT and must not be used.)
fn spawn_env_flags() -> Vec<String> {
    let mut flags = Vec::new();
    for var in &["CLAUDE_CONFIG_DIR", "ANTHROPIC_API_KEY", "ANTHROPIC_MODEL"] {
        if let Ok(val) = std::env::var(var) {
            flags.push("--env".into());
            flags.push(format!("{var}={val}"));
        }
    }
    // Suppress claude's first-run renderer upsell so the spawned agent reaches
    // its composer instead of parking on an unanswerable prompt.
    flags.push("--env".into());
    flags.push("CLAUDE_CODE_NO_FLICKER=1".into());
    flags
}

/// Parse `pane_id` from herdr's JSON output.
/// Looks for `"pane_id":"<value>"` defensively.
fn parse_pane_id(json: &str) -> Option<String> {
    // Simple substring search — avoids a serde dependency for one field.
    let key = "\"pane_id\":\"";
    let start = json.find(key)? + key.len();
    let end = json[start..].find('"')? + start;
    let id = &json[start..end];
    if id.is_empty() {
        None
    } else {
        Some(id.to_owned())
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

/// Parse `workspace_id` from herdr's `workspace create` JSON output.
/// Looks for `"workspace_id":"<value>"` inside the top-level `workspace` object.
/// The output shape is: `{"result":{"workspace":{"workspace_id":"wN",...},...}}`.
fn parse_workspace_id(json: &str) -> Option<String> {
    let key = "\"workspace_id\":\"";
    let start = json.find(key)? + key.len();
    let end = json[start..].find('"')? + start;
    let id = &json[start..end];
    if id.is_empty() {
        None
    } else {
        Some(id.to_owned())
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
        // An empty queue models a pane with no reportable status (yields None),
        // so the default poll path (no scripted blocked status) keeps waiting.
        self.status_queue.borrow_mut().pop_front().flatten()
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
    fn parse_pane_id_extracts_correctly() {
        let json = r#"{"id":"cli:agent:start","result":{"pane_id":"w1:pXY","tab_id":"w1:tXY"}}"#;
        assert_eq!(parse_pane_id(json).as_deref(), Some("w1:pXY"));
    }

    #[test]
    fn parse_pane_id_missing_returns_none() {
        assert!(parse_pane_id(r#"{"no":"pane"}"#).is_none());
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
        // Empty queue → None (default "keep waiting" path).
        assert_eq!(h.agent_status("pane-1"), None);
        h.push_status(Some("blocked"));
        h.push_status(None::<String>);
        assert_eq!(h.agent_status("pane-1").as_deref(), Some("blocked"));
        assert_eq!(h.agent_status("pane-1"), None);
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

    // -- Fix 1: the submit CR is sent as a separately-timed keypress, not
    //    inline with the paste. The delay is what makes it a distinct keypress;
    //    guard that it is never zeroed out.
    #[test]
    fn paste_settle_is_nonzero() {
        assert!(
            PASTE_SETTLE > Duration::ZERO,
            "PASTE_SETTLE must be > 0 so the submit CR is a separate keypress, \
             not swallowed by claude's bracketed paste"
        );
    }

    // -- Fix 1 (issue 1): the submit handshake sends TWO carriage returns — a
    //    primary CR and a delayed flush CR — so a paste that swallows the first
    //    CR is still submitted by the second. FLUSH_SETTLE must be nonzero so the
    //    flush is a distinct keypress after the paste has settled.
    #[test]
    fn flush_settle_is_nonzero() {
        assert!(
            FLUSH_SETTLE > Duration::ZERO,
            "FLUSH_SETTLE must be > 0 so the flush CR is a distinct keypress"
        );
    }

    #[test]
    fn submit_handshake_sends_two_crs() {
        let mut count = 0;
        submit_handshake(
            || {
                count += 1;
                Ok(())
            },
            Duration::ZERO,
            Duration::ZERO,
        )
        .unwrap();
        assert_eq!(count, 2, "submit must send a primary CR and a flush CR");
    }

    #[test]
    fn submit_handshake_short_circuits_on_error() {
        let mut count = 0;
        let r = submit_handshake(
            || {
                count += 1;
                Err(io::Error::other("boom"))
            },
            Duration::ZERO,
            Duration::ZERO,
        );
        assert!(r.is_err(), "an error on the primary CR must propagate");
        assert_eq!(count, 1, "a failed primary CR must not attempt the flush");
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

    // parse_pane_id extracts `result.root_pane.pane_id` from a real
    // `workspace create` / `tab create` payload (root_pane is the first pane_id).
    #[test]
    fn parse_pane_id_extracts_root_pane_from_create_output() {
        let json = r#"{"result":{"root_pane":{"pane_id":"w9:p1","tab_id":"w9:t1","workspace_id":"w9"},"workspace":{"workspace_id":"w9"}}}"#;
        assert_eq!(parse_pane_id(json).as_deref(), Some("w9:p1"));
        assert_eq!(parse_workspace_id(json).as_deref(), Some("w9"));
    }

    // -- spawn_env_flags: secrets travel via herdr --env, never inlined --------
    #[test]
    fn spawn_env_flags_includes_set_vars_only() {
        use crate::test_util::ENV_LOCK;
        let _lock = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::set_var("CLAUDE_CONFIG_DIR", "/home/user/.config/claude-work");
            std::env::remove_var("ANTHROPIC_API_KEY");
            std::env::remove_var("ANTHROPIC_MODEL");
        }
        let joined = spawn_env_flags().join(" ");
        unsafe {
            std::env::remove_var("CLAUDE_CONFIG_DIR");
        }
        assert!(
            joined.contains("--env CLAUDE_CONFIG_DIR=/home/user/.config/claude-work"),
            "expected --env flag for set var: {joined}"
        );
        assert!(
            !joined.contains("ANTHROPIC_API_KEY"),
            "unset key must not appear: {joined}"
        );
        assert!(
            !joined.contains("ANTHROPIC_MODEL"),
            "unset model must not appear: {joined}"
        );
    }

    // Even with no auth vars set, the flicker-suppression flag is always emitted
    // so the spawned claude skips its first-run renderer upsell.
    #[test]
    fn spawn_env_flags_only_flicker_when_auth_unset() {
        use crate::test_util::ENV_LOCK;
        let _lock = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::remove_var("CLAUDE_CONFIG_DIR");
            std::env::remove_var("ANTHROPIC_API_KEY");
            std::env::remove_var("ANTHROPIC_MODEL");
        }
        let joined = spawn_env_flags().join(" ");
        assert!(
            joined.contains("--env CLAUDE_CODE_NO_FLICKER=1"),
            "flicker-suppression flag must always be present: {joined}"
        );
        assert!(
            !joined.contains("CLAUDE_CONFIG_DIR"),
            "no auth flags expected when unset: {joined}"
        );
        assert!(
            !joined.contains("ANTHROPIC_API_KEY"),
            "no auth flags expected when unset: {joined}"
        );
        assert!(
            !joined.contains("ANTHROPIC_MODEL"),
            "no auth flags expected when unset: {joined}"
        );
    }

    // The flicker flag must ride on every phase-agent pane. Without it a freshly
    // spawned interactive claude parks on "Try the new fullscreen renderer?" with
    // no human to answer, hanging the phase until `phase wait` times out.
    #[test]
    fn spawn_env_flags_always_suppresses_flicker_upsell() {
        use crate::test_util::ENV_LOCK;
        let _lock = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::set_var("CLAUDE_CONFIG_DIR", "/cfg");
            std::env::remove_var("ANTHROPIC_API_KEY");
            std::env::remove_var("ANTHROPIC_MODEL");
        }
        let joined = spawn_env_flags().join(" ");
        unsafe {
            std::env::remove_var("CLAUDE_CONFIG_DIR");
        }
        assert!(
            joined.contains("--env CLAUDE_CODE_NO_FLICKER=1"),
            "flicker-suppression flag must be present alongside auth flags: {joined}"
        );
        // We must NOT reach for the alternate-screen disable knob: it empties
        // `agent read --source recent` and would break `phase compress`.
        assert!(
            !joined.contains("CLAUDE_CODE_DISABLE_ALTERNATE_SCREEN"),
            "must not disable the alternate screen (breaks phase compress): {joined}"
        );
    }

    #[test]
    fn spawn_env_flags_includes_all_set_vars() {
        use crate::test_util::ENV_LOCK;
        let _lock = ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::set_var("CLAUDE_CONFIG_DIR", "/cfg");
            std::env::set_var("ANTHROPIC_API_KEY", "sk-test");
            std::env::set_var("ANTHROPIC_MODEL", "claude-opus-4-5");
        }
        let joined = spawn_env_flags().join(" ");
        unsafe {
            std::env::remove_var("CLAUDE_CONFIG_DIR");
            std::env::remove_var("ANTHROPIC_API_KEY");
            std::env::remove_var("ANTHROPIC_MODEL");
        }
        assert!(joined.contains("CLAUDE_CONFIG_DIR=/cfg"), "{joined}");
        assert!(joined.contains("ANTHROPIC_API_KEY=sk-test"), "{joined}");
        assert!(
            joined.contains("ANTHROPIC_MODEL=claude-opus-4-5"),
            "{joined}"
        );
        assert!(joined.contains("CLAUDE_CODE_NO_FLICKER=1"), "{joined}");
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
