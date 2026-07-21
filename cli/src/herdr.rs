use std::cell::RefCell;
use std::collections::VecDeque;
use std::io;
use std::process::Command;

pub trait Herdr {
    fn agent_start(&self, name: &str, cwd: &str, argv: &[String]) -> io::Result<String>;
    fn agent_send(&self, target: &str, text: &str) -> io::Result<()>;
    fn agent_wait_done(&self, target: &str, timeout_ms: u64) -> io::Result<bool>;
    fn agent_read(&self, target: &str) -> io::Result<String>;
    fn session_stop(&self, name: &str) -> io::Result<()>;
    fn integration_present(&self) -> bool;
}

// ---------------------------------------------------------------------------
// SystemHerdr — shells the real `herdr` binary
// ---------------------------------------------------------------------------

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
    fn agent_start(&self, name: &str, cwd: &str, argv: &[String]) -> io::Result<String> {
        let mut args: Vec<&str> = vec!["agent", "start", name, "--cwd", cwd, "--"];
        let argv_refs: Vec<&str> = argv.iter().map(|s| s.as_str()).collect();
        args.extend(argv_refs.iter());
        let out = Command::new("herdr").args(&args).output()?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(io::Error::new(
                io::ErrorKind::Other,
                format!("herdr agent start failed: {stderr}"),
            ));
        }
        let stdout = String::from_utf8_lossy(&out.stdout);
        // herdr agent start emits JSON; extract pane_id from the result
        parse_pane_id(&stdout).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::Other,
                format!("herdr agent start: could not parse pane_id from: {stdout}"),
            )
        })
    }

    fn agent_send(&self, target: &str, text: &str) -> io::Result<()> {
        let out = self.run(&["agent", "send", target, text])?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(io::Error::new(
                io::ErrorKind::Other,
                format!("herdr agent send failed: {stderr}"),
            ));
        }
        Ok(())
    }

    fn agent_wait_done(&self, target: &str, timeout_ms: u64) -> io::Result<bool> {
        let ms_str = timeout_ms.to_string();
        let out = self.run(&[
            "agent",
            "wait",
            target,
            "--status",
            "idle",
            "--timeout",
            &ms_str,
        ])?;
        // exit 0 = condition met (done/idle), non-zero = timeout or error
        // treat any non-zero as timeout (Ok(false)) unless stderr suggests a hard error
        if out.status.success() {
            Ok(true)
        } else {
            let code = out.status.code().unwrap_or(1);
            // timeout exits with a specific code; treat non-zero as timeout unless stderr
            // contains something that looks like a hard failure
            let stderr = String::from_utf8_lossy(&out.stderr);
            if !stderr.is_empty() && code != 1 {
                Err(io::Error::new(
                    io::ErrorKind::Other,
                    format!("herdr agent wait error: {stderr}"),
                ))
            } else {
                Ok(false)
            }
        }
    }

    fn agent_read(&self, target: &str) -> io::Result<String> {
        let out = self.run(&["agent", "read", target, "--source", "recent"])?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(io::Error::new(
                io::ErrorKind::Other,
                format!("herdr agent read failed: {stderr}"),
            ));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    fn session_stop(&self, name: &str) -> io::Result<()> {
        let out = self.run(&["session", "stop", name])?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(io::Error::new(
                io::ErrorKind::Other,
                format!("herdr session stop failed: {stderr}"),
            ));
        }
        Ok(())
    }

    fn integration_present(&self) -> bool {
        let Ok(out) = self.run(&["integration", "status"]) else {
            return false;
        };
        let stdout = String::from_utf8_lossy(&out.stdout);
        // Look for a line starting with "claude:" that does NOT contain "not installed"
        stdout.lines().any(|line| {
            line.starts_with("claude:") && !line.contains("not installed")
        })
    }
}

/// Parse `pane_id` from herdr's JSON output.
/// Looks for `"pane_id":"<value>"` defensively.
fn parse_pane_id(json: &str) -> Option<String> {
    // Simple substring search — avoids a serde dependency for one field.
    let key = "\"pane_id\":\"";
    let start = json.find(key)? + key.len();
    let end = json[start..].find('"')? + start;
    let id = &json[start..end];
    if id.is_empty() { None } else { Some(id.to_owned()) }
}

// ---------------------------------------------------------------------------
// FakeHerdr — records calls; scripted return values for tests
// ---------------------------------------------------------------------------

pub struct FakeHerdr {
    calls: RefCell<Vec<String>>,
    counter: RefCell<u32>,
    /// Queued return strings for agent_read (FIFO)
    read_queue: RefCell<VecDeque<String>>,
    /// If set, agent_wait_done returns this value instead of true
    wait_result: RefCell<Option<io::Result<bool>>>,
}

impl FakeHerdr {
    pub fn new() -> Self {
        Self {
            calls: RefCell::new(Vec::new()),
            counter: RefCell::new(0),
            read_queue: RefCell::new(VecDeque::new()),
            wait_result: RefCell::new(None),
        }
    }

    pub fn calls(&self) -> Vec<String> {
        self.calls.borrow().clone()
    }

    /// Queue a string to be returned by the next `agent_read` call.
    pub fn push_read(&self, text: impl Into<String>) {
        self.read_queue.borrow_mut().push_back(text.into());
    }

    /// Script the next `agent_wait_done` return value.
    pub fn set_wait_result(&self, result: io::Result<bool>) {
        *self.wait_result.borrow_mut() = Some(result);
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

impl Herdr for FakeHerdr {
    fn agent_start(&self, name: &str, cwd: &str, argv: &[String]) -> io::Result<String> {
        let id = self.next_id();
        self.record(format!(
            "agent_start name={name} cwd={cwd} argv={argv:?} -> {id}"
        ));
        Ok(id)
    }

    fn agent_send(&self, target: &str, text: &str) -> io::Result<()> {
        self.record(format!("agent_send target={target} text={text:?}"));
        Ok(())
    }

    fn agent_wait_done(&self, target: &str, timeout_ms: u64) -> io::Result<bool> {
        self.record(format!(
            "agent_wait_done target={target} timeout_ms={timeout_ms}"
        ));
        let scripted = self.wait_result.borrow_mut().take();
        Ok(scripted.and_then(|r| r.ok()).unwrap_or(true))
    }

    fn agent_read(&self, target: &str) -> io::Result<String> {
        self.record(format!("agent_read target={target}"));
        let text = self
            .read_queue
            .borrow_mut()
            .pop_front()
            .unwrap_or_default();
        Ok(text)
    }

    fn session_stop(&self, name: &str) -> io::Result<()> {
        self.record(format!("session_stop name={name}"));
        Ok(())
    }

    fn integration_present(&self) -> bool {
        self.record("integration_present".to_string());
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
        let id = h.agent_start("brainstorm", "/tmp", &["claude".into()]).unwrap();
        assert!(!id.is_empty());
        h.agent_send(&id, "hello").unwrap();
        assert_eq!(h.calls().len(), 2);
        assert!(h.calls()[1].contains("send"));
    }

    #[test]
    fn fake_sequential_ids() {
        let h = FakeHerdr::new();
        let id1 = h.agent_start("a", "/tmp", &[]).unwrap();
        let id2 = h.agent_start("b", "/tmp", &[]).unwrap();
        assert_ne!(id1, id2);
    }

    #[test]
    fn fake_read_queue() {
        let h = FakeHerdr::new();
        h.push_read("output text");
        let id = h.agent_start("x", "/", &[]).unwrap();
        let text = h.agent_read(&id).unwrap();
        assert_eq!(text, "output text");
    }

    #[test]
    fn fake_wait_scripted_false() {
        let h = FakeHerdr::new();
        h.set_wait_result(Ok(false));
        let done = h.agent_wait_done("pane-1", 1000).unwrap();
        assert!(!done);
    }

    #[test]
    fn integration_present_recorded() {
        let h = FakeHerdr::new();
        assert!(h.integration_present());
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
}
