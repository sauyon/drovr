//! `drovr serve` must never bring up a *second* review server for one data dir.
//!
//! The server is global and always-on: it owns `server.addr` / `server.pid` in
//! the data dir, and every driver (`review summary`, `review wait`,
//! `ensure_server`) discovers it through those files. A second daemon silently
//! clobbers them, so half the CLI talks to one server and half to the other —
//! and the loser keeps its port and its stale in-memory run cells. These checks
//! pin the refusal: a duplicate exits non-zero, says where the live one is, and
//! binds nothing.
//!
//! A stale `server.addr` (server died without cleaning up) must NOT be mistaken
//! for a live one, so the last check starts a server on top of a dead pointer.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use std::{fs, thread};

fn drovr() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_drovr"))
}

/// Every port this binary has handed out. Cargo runs these tests in parallel
/// threads of one process, and an ephemeral port freed by one `free_ports` call
/// can be handed straight back to the next — two tests would then aim `drovr
/// serve` at the same port and one would lose the bind, failing for a reason that
/// has nothing to do with the lock under test.
static USED_PORTS: Mutex<Vec<u16>> = Mutex::new(Vec::new());

/// `n` distinct ports nothing is listening on. The scout listeners are all held
/// until every port is chosen, so one call never returns a port twice either.
fn free_ports(n: usize) -> Vec<u16> {
    let mut used = USED_PORTS.lock().expect("port registry");
    let mut scouts = Vec::with_capacity(n);
    let mut ports = Vec::with_capacity(n);
    while ports.len() < n {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind for free port");
        let port = listener.local_addr().unwrap().port();
        scouts.push(listener);
        if !used.contains(&port) {
            used.push(port);
            ports.push(port);
        }
    }
    drop(scouts);
    ports
}

fn free_port() -> u16 {
    free_ports(1)[0]
}

struct KillOnDrop(Child);
impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn wait_for_listener(addr: &str, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if TcpStream::connect(addr).is_ok() {
            return true;
        }
        thread::sleep(Duration::from_millis(50));
    }
    false
}

fn http_get_ok(addr: &str, path: &str) -> bool {
    let resp = http_get_raw(addr, path);
    resp.lines().next().is_some_and(|l| l.contains(" 200"))
}

/// Poll `GET /api/runs` until it answers 200. A bound socket is not yet a
/// serving one — `serve` binds before it spawns its workers — so a single shot
/// can catch the window and read an empty reply.
fn wait_until_serving(port: u16, timeout: Duration) -> bool {
    let addr = format!("127.0.0.1:{port}");
    let deadline = Instant::now() + timeout;
    loop {
        if http_get_ok(&addr, "/api/runs") {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        thread::sleep(Duration::from_millis(50));
    }
}

/// Assert `child` is serving on `port`, reporting how it died if it is not: an
/// empty reply from a live socket means the process went away mid-request, which
/// is unreadable without its exit status and stderr.
fn assert_serving(child: &mut KillOnDrop, port: u16, why: &str) {
    if wait_until_serving(port, Duration::from_secs(10)) {
        return;
    }
    let status = child.0.try_wait().expect("try_wait");
    let mut err = String::new();
    if let Some(pipe) = child.0.stderr.as_mut() {
        // The process is gone (or wedged): whatever it managed to say is here.
        let _ = pipe.read_to_string(&mut err);
    }
    panic!("{why}: no 200 from 127.0.0.1:{port}/api/runs; child status={status:?}; stderr={err}");
}

fn http_get_raw(addr: &str, path: &str) -> String {
    let Ok(mut s) = TcpStream::connect(addr) else {
        return format!("<connect failed: {addr}>");
    };
    if write!(s, "GET {path} HTTP/1.0\r\nHost: {addr}\r\n\r\n").is_err() {
        return "<write failed>".into();
    }
    let mut resp = String::new();
    let _ = s.read_to_string(&mut resp);
    resp
}

/// Start the always-on server against `xdg` on a fresh port and wait for it.
/// `--host` is explicit: `serve` otherwise takes `serve_host` from the
/// developer's own config, which may be a LAN address this test must not bind.
fn start_server(xdg: &Path) -> (KillOnDrop, u16) {
    let port = free_port();
    let mut child = KillOnDrop(
        Command::new(drovr())
            .args(["serve", "--host", "127.0.0.1", "--port", &port.to_string()])
            .env("XDG_DATA_HOME", xdg)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn drovr serve"),
    );
    assert_serving(&mut child, port, "drovr serve never came up");
    // Answering on the port is not proof it is *ours*: assert the child is the
    // live one, so a stray listener can never stand in for the server.
    assert!(
        child.0.try_wait().expect("try_wait").is_none(),
        "drovr serve exited during startup on 127.0.0.1:{port}"
    );
    (child, port)
}

/// Run `drovr serve` in the foreground and return (exit code, stderr). It only
/// returns at all because it is expected to refuse; a bounded wait keeps a
/// regression (a second server that *does* start) from hanging the suite.
fn try_serve(xdg: &Path, port: u16) -> (Option<i32>, String) {
    let mut child = Command::new(drovr())
        .args(["serve", "--host", "127.0.0.1", "--port", &port.to_string()])
        .env("XDG_DATA_HOME", xdg)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn duplicate drovr serve");

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match child.try_wait().expect("try_wait") {
            Some(status) => {
                let mut err = String::new();
                if let Some(mut pipe) = child.stderr.take() {
                    let _ = pipe.read_to_string(&mut err);
                }
                return (status.code(), err);
            }
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                panic!("duplicate `drovr serve` did not exit — it started a second server");
            }
            None => thread::sleep(Duration::from_millis(50)),
        }
    }
}

/// A minimal HTTP server that answers every request with `200 <body>`, used to
/// stand in for whatever else may hold a dead server's recorded address. Stops
/// when dropped.
struct Responder {
    addr: String,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Responder {
    fn spawn(body: &'static str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("responder listener");
        let addr = listener.local_addr().unwrap().to_string();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = Arc::clone(&stop);
        let thread = thread::spawn(move || {
            for conn in listener.incoming() {
                if stop_thread.load(Ordering::Relaxed) {
                    return;
                }
                let Ok(mut conn) = conn else { return };
                let mut buf = [0u8; 512];
                let _ = conn.read(&mut buf);
                let _ = conn.write_all(
                    format!(
                        "HTTP/1.0 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                );
            }
        });
        Responder {
            addr,
            stop,
            thread: Some(thread),
        }
    }
}

impl Drop for Responder {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        // The thread is parked in `accept`: one connection wakes it to see the flag.
        let _ = TcpStream::connect(&self.addr);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// The lock/discovery file for a test's data dir.
fn server_pid_file(xdg: &Path) -> PathBuf {
    xdg.join("drovr").join("server.pid")
}

fn tmp_xdg(prefix: &str) -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix(prefix)
        .tempdir()
        .expect("tempdir")
}

/// The bug: a duplicate on a *different* port used to start happily and take
/// over `server.addr`, leaving two live servers for one data dir.
#[test]
fn second_serve_on_another_port_is_refused() {
    let tmp = tmp_xdg("drovr-serve-dup-");
    let xdg = tmp.path().join("xdg");
    let (first, first_port) = start_server(&xdg);

    let second_port = free_port();
    let (code, err) = try_serve(&xdg, second_port);

    assert_eq!(code, Some(1), "duplicate serve must fail; stderr={err}");
    assert!(
        err.contains("already running"),
        "stderr must name the conflict, got: {err}"
    );
    assert!(
        err.contains(&first_port.to_string()),
        "stderr must point at the live server's address, got: {err}"
    );
    assert!(
        !wait_for_listener(
            &format!("127.0.0.1:{second_port}"),
            Duration::from_millis(300)
        ),
        "refused serve must not have bound 127.0.0.1:{second_port}"
    );

    // The live server is untouched: same address file, still answering.
    let addr_file = fs::read_to_string(xdg.join("drovr").join("server.addr")).expect("server.addr");
    assert_eq!(addr_file.trim(), format!("127.0.0.1:{first_port}"));
    let mut first = first;
    assert_serving(
        &mut first,
        first_port,
        "the original server must still serve",
    );
    drop(first);
}

/// Same port, where the OS bind would reject the duplicate anyway: the message
/// must still be the actionable one, because the refusal happens earlier (on the
/// lock and the address) and never reaches `Address already in use`.
#[test]
fn second_serve_on_same_port_is_refused() {
    let tmp = tmp_xdg("drovr-serve-same-");
    let xdg = tmp.path().join("xdg");
    let (first, first_port) = start_server(&xdg);

    let (code, err) = try_serve(&xdg, first_port);
    assert_eq!(code, Some(1), "duplicate serve must fail; stderr={err}");
    assert!(
        err.contains("already running"),
        "stderr must name the conflict, got: {err}"
    );
    let mut first = first;
    assert_serving(
        &mut first,
        first_port,
        "the original server must still serve",
    );
    drop(first);
}

/// The state a real data dir was found in: a live server, but a `server.pid`
/// that no longer describes it (deleted by hand, or written by an older drovr
/// that has since died). The address must still be enough to refuse — otherwise
/// the duplicate starts and takes `server.addr` off the server that is serving.
#[test]
fn live_server_without_a_lock_file_is_still_refused() {
    let tmp = tmp_xdg("drovr-serve-nolock-");
    let xdg = tmp.path().join("xdg");
    let (mut first, first_port) = start_server(&xdg);

    fs::remove_file(xdg.join("drovr").join("server.pid")).expect("drop the lock file");

    let (code, err) = try_serve(&xdg, free_port());
    assert_eq!(code, Some(1), "duplicate serve must fail; stderr={err}");
    assert!(
        err.contains("already running") && err.contains(&first_port.to_string()),
        "stderr must point at the live server, got: {err}"
    );
    // This server is unmistakably drovr, so the message must NOT offer the
    // delete-`server.addr` way out: with the lock file already gone, following that
    // advice is what would let a second server start.
    assert!(
        !err.contains("delete"),
        "a certain drovr server must not be offered as deletable, got: {err}"
    );
    assert_serving(
        &mut first,
        first_port,
        "the original server must still serve",
    );
    drop(first);
}

/// The narrow race the pid lock exists for: several servers starting at the same
/// instant on *different* ports, so the OS bind cannot serialize them. Exactly
/// one may survive — with a check-then-bind guard they all sailed through.
#[test]
fn concurrent_starts_leave_exactly_one_server() {
    const STARTERS: usize = 6;

    let tmp = tmp_xdg("drovr-serve-race-");
    let xdg = tmp.path().join("xdg");
    // Pre-create the data dir so the racers contend on the lock, not on mkdir.
    fs::create_dir_all(xdg.join("drovr")).expect("data dir");

    // Distinct ports: two racers sharing one would be serialized by the OS bind
    // instead of by the lock under test.
    let ports = free_ports(STARTERS);

    let mut children: Vec<(u16, KillOnDrop)> = ports
        .iter()
        .map(|&port| {
            let child = Command::new(drovr())
                .args(["serve", "--host", "127.0.0.1", "--port", &port.to_string()])
                .env("XDG_DATA_HOME", &xdg)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("spawn racing drovr serve");
            (port, KillOnDrop(child))
        })
        .collect();

    // Give the losers time to refuse and exit, and the winner time to bind.
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        let mut still_running = 0;
        for (_, child) in children.iter_mut() {
            if child.0.try_wait().expect("try_wait").is_none() {
                still_running += 1;
            }
        }
        if still_running <= 1 {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }

    let alive: Vec<u16> = children
        .iter()
        .filter(|(port, _)| TcpStream::connect(format!("127.0.0.1:{port}")).is_ok())
        .map(|(port, _)| *port)
        .collect();
    assert_eq!(
        alive.len(),
        1,
        "exactly one racing server may survive, got {alive:?}"
    );

    // Discovery must name the survivor, not a server that lost and exited.
    let addr = fs::read_to_string(xdg.join("drovr").join("server.addr")).expect("server.addr");
    assert_eq!(addr.trim(), format!("127.0.0.1:{}", alive[0]));
    assert!(
        wait_until_serving(alive[0], Duration::from_secs(10)),
        "the survivor named in server.addr must be serving"
    );
}

/// The lock is the kernel's, so a server that dies without cleaning up releases
/// it — including one that is SIGKILLed, which is how drovr servers usually stop.
/// The very next start must take it, with no stale-lock reasoning in between.
#[test]
fn killing_a_server_frees_the_lock_immediately() {
    let tmp = tmp_xdg("drovr-serve-kill-");
    let xdg = tmp.path().join("xdg");

    let (first, _) = start_server(&xdg);
    drop(first); // KillOnDrop: SIGKILL, no chance to tidy up
    assert!(
        server_pid_file(&xdg).exists(),
        "the killed server leaves its pid file behind — that must not matter"
    );

    let (second, port) = start_server(&xdg);
    assert!(
        wait_until_serving(port, Duration::from_secs(10)),
        "the next server must take the lock a killed one held"
    );
    drop(second);
}

/// A stale `server.addr` pointing at an unrelated service that answers 200 on
/// every path must not read as a live drovr server: the `/health` body has to be
/// drovr's, or the start is blocked by whatever else is on that port.
#[test]
fn an_unrelated_200_responder_does_not_block_startup() {
    let tmp = tmp_xdg("drovr-serve-200-");
    let xdg = tmp.path().join("xdg");
    let data = xdg.join("drovr");
    fs::create_dir_all(&data).expect("data dir");

    let responder = Responder::spawn("hello");
    fs::write(data.join("server.addr"), &responder.addr).expect("stale addr");

    let (mut server, port) = start_server(&xdg);
    assert_serving(
        &mut server,
        port,
        "an unrelated 200 on the stale address must not block a real start",
    );
    drop(server);
}

/// The unavoidable false positive: a service answering the bare `"ok"` that
/// pre-guard drovr servers answer *is* taken for a server, because a real one of
/// those holds no lock to be found by. The refusal must then say which file to
/// delete — a verdict drawn from a stale address cannot be a dead end.
#[test]
fn a_legacy_ok_responder_refuses_but_names_the_way_out() {
    let tmp = tmp_xdg("drovr-serve-legacy-");
    let xdg = tmp.path().join("xdg");
    let data = xdg.join("drovr");
    fs::create_dir_all(&data).expect("data dir");

    let responder = Responder::spawn("ok");
    fs::write(data.join("server.addr"), &responder.addr).expect("stale addr");

    let (code, err) = try_serve(&xdg, free_port());
    assert_eq!(code, Some(1), "must refuse; stderr={err}");
    assert!(
        err.contains("already running") && err.contains(&responder.addr),
        "stderr must name what it found, got: {err}"
    );
    assert!(
        err.contains("delete") && err.contains("server.addr"),
        "stderr must name the way out of a wrong verdict, got: {err}"
    );
}

/// The other way a start can fail: no drovr server anywhere, but something
/// foreign holds the requested port. That is a bind failure, not a duplicate — it
/// must say so, and must not leave the lock held behind it.
#[test]
fn a_foreign_process_on_the_requested_port_fails_to_bind() {
    let tmp = tmp_xdg("drovr-serve-bind-");
    let xdg = tmp.path().join("xdg");

    let squatter = TcpListener::bind("127.0.0.1:0").expect("squatter listener");
    let squatted = squatter.local_addr().unwrap().port();

    let (code, err) = try_serve(&xdg, squatted);
    assert_eq!(
        code,
        Some(1),
        "bind failure must exit non-zero; stderr={err}"
    );
    assert!(
        err.contains("cannot bind") && err.contains(&squatted.to_string()),
        "stderr must name the port it could not bind, got: {err}"
    );

    // The failed start released its lock, so a good one still works
    // (`start_server` asserts the server actually serves).
    drop(squatter);
    drop(start_server(&xdg).0);
}

/// A foreign process squatting on a dead server's recorded address must not be
/// mistaken for a live drovr server: the pid file is the authority, and its
/// holder is gone, so a fresh start is allowed.
#[test]
fn foreign_listener_on_stale_addr_does_not_block_startup() {
    let tmp = tmp_xdg("drovr-serve-foreign-");
    let xdg = tmp.path().join("xdg");
    let data = xdg.join("drovr");
    fs::create_dir_all(&data).expect("data dir");

    // Something that is emphatically not drovr, holding the recorded address.
    let squatter = TcpListener::bind("127.0.0.1:0").expect("squatter listener");
    let squatted = squatter.local_addr().unwrap().to_string();
    fs::write(data.join("server.addr"), &squatted).expect("stale addr");
    // A pid that cannot be running: pid 0 is never a live process.
    fs::write(data.join("server.pid"), "0").expect("stale pid");

    let (mut server, port) = start_server(&xdg);
    assert_serving(
        &mut server,
        port,
        "serve must come up despite a foreign listener on the stale address",
    );
    drop(server);
    drop(squatter);
}

/// A `server.addr` left behind by a dead server must not block a fresh start —
/// otherwise a crashed daemon wedges every later `drovr serve` for good.
#[test]
fn stale_addr_file_does_not_block_startup() {
    let tmp = tmp_xdg("drovr-serve-stale-");
    let xdg = tmp.path().join("xdg");
    let data = xdg.join("drovr");
    fs::create_dir_all(&data).expect("data dir");
    // Nothing listens here: the port was free at the moment we asked for it and
    // we never bound it.
    fs::write(
        data.join("server.addr"),
        format!("127.0.0.1:{}", free_port()),
    )
    .expect("stale addr");
    fs::write(data.join("server.pid"), "999999").expect("stale pid");

    let (mut server, port) = start_server(&xdg);
    assert_serving(
        &mut server,
        port,
        "serve must come up over a stale server.addr",
    );
    drop(server);
}
