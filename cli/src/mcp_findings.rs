//! A one-tool stdio MCP server: the review panel's findings channel.
//!
//! Reviewers run read-only (cursor `--mode plan`, claude `--permission-mode plan`), so
//! they cannot write their findings file themselves. Rather than widen their
//! permissions, drovr hands each reviewer a single tool — `submit_findings` — and
//! performs the write on its behalf.
//!
//! Verified on cursor `--mode plan`: an MCP tool call succeeds while a direct file
//! write is refused. The carve-out is therefore exactly one file, and the reviewer
//! stays unable to touch the project.
//!
//! # Why the path is not a parameter
//!
//! `dir`/`task`/`iter` come from argv, chosen by drovr when it spawns the reviewer,
//! and `angle` is validated against the panel's configured set. The tool takes only
//! the findings themselves. A reviewer cannot name the file it writes, so a confused
//! or hostile one cannot use its single write to reach a run's `state.json`, the
//! project, or anything else on disk. (It *can* name a panel-mate's angle — see the
//! note at the angle check in [`handle`]; that is a reviewed, deliberate tradeoff.)
//!
//! # Why validation happens here
//!
//! `submit_findings` parses the payload through [`parse_review`] before writing. A
//! malformed verdict is rejected with an error the reviewer can read and retry, while
//! it is still alive. The previous design could only discover a bad payload after the
//! reviewer had exited, which cost the whole review.

use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

use crate::findings::{Impact, Severity, Verdict, parse_review};

/// JSON-RPC + MCP wire constants.
const JSONRPC: &str = "2.0";
const DEFAULT_PROTOCOL: &str = "2025-06-18";

/// The key this server is registered under in the MCP config drovr writes, and the
/// one tool it serves.
///
/// Three places need to agree on these: the config drovr writes, the reviewer seed
/// (backends that namespace MCP tools show the qualified form), and claude's
/// `--allowedTools` carve-out. Derived from one definition so they cannot drift —
/// a mismatch would leave the reviewer unable to call the only tool it has.
pub(crate) const SERVER_NAME: &str = "drovr-findings";
pub(crate) const TOOL_NAME: &str = "submit_findings";

/// `mcp__<server>__<tool>` — how a namespacing backend lists this tool.
pub(crate) fn qualified_tool_name() -> String {
    format!("mcp__{SERVER_NAME}__{TOOL_NAME}")
}

/// The JSON Schema for a `Review` body — everything `submit_findings` takes EXCEPT
/// `angle`, which is drovr's routing argument rather than part of a review.
///
/// **This is the single source of truth for the findings shape.** It is what the MCP
/// tool advertises AND what `code_review::build_seed` renders into the reviewer's
/// brief, and its closed `enum`s are built from `findings::{Verdict, Impact,
/// Severity}::WIRE` — the same values `parse_review` accepts. Three copies of this
/// shape used to exist (tool schema, seed schema, Rust types) and could drift
/// independently; a drift there tells a reviewer to send something validation then
/// rejects, which reads exactly like a lazy reviewer.
pub(crate) fn review_schema() -> serde_json::Value {
    serde_json::json!({
        "verdict": {
            "type": "string",
            "enum": Verdict::WIRE,
            "description": "\"changes\" if you found any critical or important issue, else \"clean\"."
        },
        "findings": {
            "type": "array",
            "description": "One entry per issue. Empty for a clean review.",
            "items": {
                "type": "object",
                "properties": {
                    "file": {"type": "string"},
                    "line": {"type": "integer"},
                    "severity": {"type": "string", "enum": Severity::WIRE},
                    "summary": {"type": "string", "description": "one line: what is wrong"},
                    "rationale": {"type": "string", "description": "why it matters"}
                },
                "required": ["file", "severity", "summary"]
            }
        },
        "impact": {"type": "string", "enum": Impact::WIRE}
    })
}

/// The tool's JSON Schema: [`review_schema`] plus the `angle` drovr routes on.
/// `verdict` is required (its absence is what makes an arbitrary object fail
/// `parse_review`); `findings` defaults to empty so a clean review is one short call.
fn tool_def(angles: &[String]) -> serde_json::Value {
    let mut properties = review_schema();
    properties["angle"] = serde_json::json!({
        "type": "string",
        "enum": angles,
        "description": "The angle YOU were assigned. Submitting under another reviewer's angle overwrites their verdict."
    });
    serde_json::json!({
        "name": TOOL_NAME,
        "description":
            "Submit your review findings. This is the ONLY way to deliver them — your \
             pane output is never read. Call it exactly once, when your review is \
             complete. A clean review is verdict \"clean\" with an empty findings list.",
        "inputSchema": {
            "type": "object",
            "properties": properties,
            "required": ["angle", "verdict"]
        }
    })
}

/// Where this server's single write lands.
///
/// The **iteration** is part of the name, not decoration. Each review pass reviews a
/// different diff, so a verdict is only meaningful for the iteration that produced it.
/// With one shared name per angle, a reviewer that finished without ever calling this
/// tool would be credited with whatever the previous pass concluded — and a straggler
/// from a superseded iteration, submitting late, would land on top of a live panel.
/// Naming the iteration makes both impossible by construction rather than by
/// remembering to delete the file at every point a new panel can open.
pub(crate) fn findings_path(dir: &Path, task: &str, iter: u64, angle: &str) -> PathBuf {
    dir.join(format!("{task}-review-{iter}-{angle}.json"))
}

/// Outcome of handling one request: an optional reply (notifications get none).
type Reply = Option<serde_json::Value>;

fn ok(id: serde_json::Value, result: serde_json::Value) -> Reply {
    Some(serde_json::json!({"jsonrpc": JSONRPC, "id": id, "result": result}))
}

/// A tool-level error. Reported as a successful JSON-RPC response carrying
/// `isError: true` — the MCP convention that lets the MODEL see and act on the
/// message, rather than the client treating it as a transport fault.
fn tool_error(id: serde_json::Value, message: &str) -> Reply {
    Some(serde_json::json!({
        "jsonrpc": JSONRPC,
        "id": id,
        "result": {
            "content": [{"type": "text", "text": message}],
            "isError": true,
        }
    }))
}

/// Handle one decoded JSON-RPC request. Split from the IO loop so the protocol and the
/// write are unit-testable without spawning a process.
pub(crate) fn handle(
    req: &serde_json::Value,
    dir: &Path,
    task: &str,
    iter: u64,
    angles: &[String],
) -> Reply {
    let method = req.get("method").and_then(|m| m.as_str()).unwrap_or("");
    // A request without `id` is a notification: act, never reply.
    let id = match req.get("id") {
        Some(id) if !id.is_null() => id.clone(),
        _ => return None,
    };

    match method {
        "initialize" => {
            // Echo the client's protocol version when it offers one: an MCP client may
            // refuse a server that answers with a version it did not ask for.
            let proto = req
                .get("params")
                .and_then(|p| p.get("protocolVersion"))
                .and_then(|v| v.as_str())
                .unwrap_or(DEFAULT_PROTOCOL);
            ok(
                id,
                serde_json::json!({
                    "protocolVersion": proto,
                    "capabilities": {"tools": {}},
                    "serverInfo": {"name": SERVER_NAME, "version": env!("CARGO_PKG_VERSION")},
                }),
            )
        }
        "tools/list" => ok(id, serde_json::json!({"tools": [tool_def(angles)]})),
        "tools/call" => {
            let params = req.get("params");
            let name = params
                .and_then(|p| p.get("name"))
                .and_then(|n| n.as_str())
                .unwrap_or("");
            if name != TOOL_NAME {
                return tool_error(id, &format!("no such tool: '{name}'"));
            }
            let mut args = params
                .and_then(|p| p.get("arguments"))
                .cloned()
                .unwrap_or_else(|| serde_json::json!({}));

            // The angle decides which file is written, so it is checked against the
            // panel's configured angles — never used as a raw path component. An
            // unrecognised angle cannot create a file, and `..` or a separator can
            // never match a configured angle, so the path stays inside the run dir.
            //
            // # Why a reviewer CAN submit under a panel-mate's angle
            //
            // Deliberate, and reviewed: this is not an oversight to be hardened.
            // Nothing here binds a call to the reviewer that made it, so a confused
            // or hostile reviewer can name a sibling's angle and overwrite its
            // verdict. Binding per process is not achievable on the backend the panel
            // actually runs: cursor has no per-launch MCP scoping — no config flag
            // (servers come from one shared `.cursor/mcp.json`) and no environment
            // inheritance (probed with `DROVR_REVIEW_ANGLE`; it came back absent) —
            // and all four reviewers of a task share one worktree, so argv cannot
            // distinguish them either. The angle therefore HAS to be a tool argument.
            //
            // That leaves a bug-class risk inside drovr's own trust domain, not a
            // security boundary. The boundary is unaffected and still holds: the
            // reviewer names no path, so its one write cannot leave the run dir; it
            // cannot touch the project, `state.json`, or another run; and every
            // payload is validated before anything is written. The blast radius is
            // one angle's verdict within one task's own panel.
            let angle = match args.get("angle").and_then(|a| a.as_str()) {
                Some(a) if angles.iter().any(|c| c == a) => a.to_string(),
                Some(a) => {
                    return tool_error(
                        id,
                        &format!(
                            "unknown angle '{a}'. You must submit under the angle you were \
                             assigned, one of: {}.",
                            angles.join(", ")
                        ),
                    );
                }
                None => {
                    return tool_error(
                        id,
                        &format!(
                            "missing 'angle'. Submit under the angle you were assigned, one \
                             of: {}.",
                            angles.join(", ")
                        ),
                    );
                }
            };
            // Drop it before writing: the merge stamps each finding's angle from the
            // filename, so carrying it in the body would be a second source of truth.
            if let Some(o) = args.as_object_mut() {
                o.remove("angle");
            }

            // Validate BEFORE writing, and report failure to the model rather than the
            // transport, so the reviewer can correct itself while it is still running.
            let raw = match serde_json::to_string(&args) {
                Ok(b) => b,
                Err(e) => return tool_error(id, &format!("could not serialize arguments: {e}")),
            };
            let review = match parse_review(&raw) {
                Ok(r) => r,
                Err(e) => {
                    return tool_error(
                        id,
                        &format!(
                            "findings rejected ({e}). Required: verdict \"clean\"|\"changes\"; \
                             each finding needs file, severity (critical|important|nit) and \
                             summary. Fix the arguments and call submit_findings again."
                        ),
                    );
                }
            };
            // Persist the PARSED review, not the raw arguments. Serde ignores unknown
            // top-level keys when validating, so writing `args` back would let anything
            // a reviewer happened to pass — a stray `path`, a hallucinated field —
            // into the canonical artifact the merge and the web UI read. Round-tripping
            // through `Review` makes the file match the typed contract exactly.
            let body = match serde_json::to_string_pretty(&review) {
                Ok(b) => b,
                Err(e) => return tool_error(id, &format!("could not serialize review: {e}")),
            };

            let path = findings_path(dir, task, iter, &angle);
            if let Some(parent) = path.parent()
                && let Err(e) = std::fs::create_dir_all(parent)
            {
                return tool_error(id, &format!("could not create {}: {e}", parent.display()));
            }
            match write_atomically(&path, &body) {
                Ok(()) => ok(
                    id,
                    serde_json::json!({
                        "content": [{
                            "type": "text",
                            "text": format!("Findings recorded for angle '{angle}'. Your review is delivered; you may stop now."),
                        }],
                        "isError": false,
                    }),
                ),
                Err(e) => tool_error(id, &format!("could not write {}: {e}", path.display())),
            }
        }
        // Anything else (ping, logging/setLevel, …) gets a benign empty result rather
        // than an error: an unknown-method failure can abort a client's startup.
        _ => ok(id, serde_json::json!({})),
    }
}

/// Write `body` to `path` so that a concurrent reader sees either the previous file or
/// the complete new one — never a prefix of it.
///
/// The panel polls this path and treats a parseable file as proof the angle is finished,
/// so how the bytes land is now part of the contract. `fs::write` truncates and then
/// fills: a poll inside that window sees a prefix, and — worse for a RE-submission — an
/// empty file where a complete earlier verdict used to be. Parsing rejects both, so the
/// panel would keep waiting rather than bank nonsense, but that is luck rather than a
/// guarantee, and it makes a delivered review look undelivered for as long as the window
/// lasts.
///
/// So: write a sibling temp file, then `rename` onto the destination. POSIX rename within
/// one directory is atomic — a reader sees the old file or the new one. The temp name
/// carries the pid so two servers writing the same angle (possible: all of a task's
/// reviewers share one server config) cannot scribble into each other's temp file; the
/// rename then simply picks a winner.
fn write_atomically(path: &Path, body: &str) -> io::Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(format!(".tmp{}", std::process::id()));
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, body)?;
    if let Err(e) = std::fs::rename(&tmp, path) {
        // Do not leave the partial write lying next to the real file.
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

/// JSON-RPC "Parse error". Answered with `id: null`, per the spec: a line that did not
/// parse has no id to echo.
fn parse_error(detail: &str) -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": JSONRPC,
        "id": serde_json::Value::Null,
        "error": {"code": -32700, "message": format!("Parse error: {detail}")},
    })
}

/// Serve the tool on stdio until EOF. One line in, at most one line out.
pub fn serve(dir: &Path, task: &str, iter: u64, angles: &[String]) -> io::Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        // A malformed line is ANSWERED, not silently dropped, and never fatal. Dropping
        // it leaves the client with a request it will never see a response to, so it
        // waits out its own timeout — which looks to the panel exactly like a reviewer
        // that went quiet. Killing the server would be worse still: it strands the
        // reviewer with no way to deliver its findings at all.
        let reply = match serde_json::from_str::<serde_json::Value>(&line) {
            Ok(req) => handle(&req, dir, task, iter, angles),
            Err(e) => Some(parse_error(&e.to_string())),
        };
        if let Some(reply) = reply {
            writeln!(stdout, "{reply}")?;
            stdout.flush()?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn angles() -> Vec<String> {
        ["correctness", "security", "error-handling", "type-design"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    fn call(args: serde_json::Value) -> serde_json::Value {
        serde_json::json!({
            "jsonrpc": "2.0", "id": 7,
            "method": "tools/call",
            "params": {"name": "submit_findings", "arguments": args}
        })
    }

    fn is_error(reply: &serde_json::Value) -> bool {
        reply["result"]["isError"].as_bool().unwrap_or(false)
    }

    #[test]
    fn initialize_echoes_the_clients_protocol_version() {
        let d = tempfile::tempdir().unwrap();
        let req = serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {"protocolVersion": "2024-11-05"}
        });
        let r = handle(&req, d.path(), "task-1", 1, &angles()).unwrap();
        assert_eq!(r["result"]["protocolVersion"], "2024-11-05");
        assert!(r["result"]["capabilities"]["tools"].is_object());
    }

    #[test]
    fn tools_list_offers_exactly_one_tool() {
        let d = tempfile::tempdir().unwrap();
        let req = serde_json::json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list"});
        let r = handle(&req, d.path(), "task-1", 1, &angles()).unwrap();
        let tools = r["result"]["tools"].as_array().unwrap();
        assert_eq!(
            tools.len(),
            1,
            "the reviewer gets one capability, not a toolbox"
        );
        assert_eq!(tools[0]["name"], "submit_findings");
    }

    #[test]
    fn a_notification_gets_no_reply() {
        let d = tempfile::tempdir().unwrap();
        let req = serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"});
        assert!(handle(&req, d.path(), "task-1", 1, &angles()).is_none());
    }

    #[test]
    fn submitting_a_clean_review_writes_the_angles_file() {
        let d = tempfile::tempdir().unwrap();
        let r = handle(
            &call(serde_json::json!({"angle": "security", "verdict": "clean", "findings": []})),
            d.path(),
            "task-1",
            1,
            &angles(),
        )
        .unwrap();
        assert!(!is_error(&r), "{r}");
        let written =
            std::fs::read_to_string(findings_path(d.path(), "task-1", 1, "security")).unwrap();
        let review = parse_review(&written).unwrap();
        assert_eq!(review.verdict, Verdict::Clean);
        assert!(review.findings.is_empty());
    }

    /// The reviewer names no path, so its one write cannot be redirected. A path-like
    /// argument is simply part of the payload — it never reaches the filesystem call.
    #[test]
    fn the_reviewer_cannot_choose_where_its_findings_land() {
        let d = tempfile::tempdir().unwrap();
        let r = handle(
            &call(serde_json::json!({
                "angle": "correctness",
                "verdict": "clean",
                "path": "../../../etc/passwd",
                "file": "/etc/shadow"
            })),
            d.path(),
            "task-1",
            1,
            &angles(),
        )
        .unwrap();
        assert!(!is_error(&r), "{r}");
        assert!(
            findings_path(d.path(), "task-1", 1, "correctness").exists(),
            "the write must land at the drovr-chosen path"
        );
        assert!(
            !d.path().join("etc").exists(),
            "no agent-supplied path may influence the write"
        );
        // …and the stray arguments do not survive into the artifact either.
        let body =
            std::fs::read_to_string(findings_path(d.path(), "task-1", 1, "correctness")).unwrap();
        assert!(
            !body.contains("passwd") && !body.contains("shadow"),
            "only the parsed Review may be persisted, not the raw arguments: {body}"
        );
    }

    /// Serde ignores unknown keys when validating, so writing the raw arguments back
    /// would let anything a reviewer passed into the canonical artifact that the merge
    /// and the web UI read. Only the parsed `Review` is persisted.
    #[test]
    fn only_the_typed_review_is_persisted_not_the_raw_arguments() {
        let d = tempfile::tempdir().unwrap();
        let r = handle(
            &call(serde_json::json!({
                "angle": "security",
                "verdict": "changes",
                "findings": [{
                    "file": "a.rs", "severity": "nit", "summary": "s",
                    "invented_per_finding": "junk"
                }],
                "hallucinated": {"deeply": ["nested", "junk"]},
                "notes": "prose the reviewer felt like adding"
            })),
            d.path(),
            "task-1",
            1,
            &angles(),
        )
        .unwrap();
        assert!(!is_error(&r), "{r}");
        let body =
            std::fs::read_to_string(findings_path(d.path(), "task-1", 1, "security")).unwrap();
        for junk in ["hallucinated", "notes", "invented_per_finding"] {
            assert!(
                !body.contains(junk),
                "'{junk}' must not reach the file: {body}"
            );
        }
        let parsed: serde_json::Value = serde_json::from_str(&body).unwrap();
        let mut keys: Vec<&str> = parsed
            .as_object()
            .unwrap()
            .keys()
            .map(|k| k.as_str())
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            ["findings", "verdict"],
            "exactly the Review fields: {body}"
        );
        assert_eq!(parse_review(&body).unwrap().findings.len(), 1);
    }

    /// Each iteration reviews a different diff, so its verdicts live in their own file.
    /// Two iterations must never collide — that collision is what let a pass inherit
    /// the previous pass's conclusions.
    #[test]
    fn each_iteration_writes_its_own_file() {
        let d = tempfile::tempdir().unwrap();
        for (iter, verdict) in [(1u64, "changes"), (2, "clean")] {
            let r = handle(
                &call(serde_json::json!({
                    "angle": "correctness",
                    "verdict": verdict,
                    "findings": if verdict == "changes" {
                        serde_json::json!([{"file": "a.rs", "severity": "critical", "summary": "boom"}])
                    } else {
                        serde_json::json!([])
                    }
                })),
                d.path(),
                "task-1",
                iter,
                &angles(),
            )
            .unwrap();
            assert!(!is_error(&r), "{r}");
        }
        let one = findings_path(d.path(), "task-1", 1, "correctness");
        let two = findings_path(d.path(), "task-1", 2, "correctness");
        assert_ne!(one, two, "iterations must not share a filename");
        assert_eq!(
            parse_review(&std::fs::read_to_string(&one).unwrap())
                .unwrap()
                .findings
                .len(),
            1,
            "iteration 1's verdict must survive iteration 2 being written"
        );
        assert!(
            parse_review(&std::fs::read_to_string(&two).unwrap())
                .unwrap()
                .findings
                .is_empty()
        );
    }

    /// A verdict outside the advertised enum is a tool error the reviewer can act on,
    /// not a value that silently reaches the merge.
    #[test]
    fn a_verdict_outside_the_advertised_enum_is_refused() {
        let d = tempfile::tempdir().unwrap();
        let r = handle(
            &call(serde_json::json!({"angle": "security", "verdict": "looks-fine"})),
            d.path(),
            "task-1",
            1,
            &angles(),
        )
        .unwrap();
        assert!(is_error(&r), "{r}");
        assert!(!findings_path(d.path(), "task-1", 1, "security").exists());
    }

    /// The tool schema and the seed's schema are rendered from one definition, so a
    /// reviewer is never told to send a shape validation will then reject.
    #[test]
    fn the_tool_schema_and_the_review_schema_are_one_definition() {
        let tool = tool_def(&angles());
        let props = &tool["inputSchema"]["properties"];
        let mut keys: Vec<&str> = props
            .as_object()
            .unwrap()
            .keys()
            .map(|k| k.as_str())
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            ["angle", "findings", "impact", "verdict"],
            "the tool is the review schema plus drovr's routing argument"
        );
        for (k, v) in review_schema().as_object().unwrap() {
            assert_eq!(&props[k], v, "'{k}' must come from review_schema()");
        }
        // The closed enums are the values `parse_review` accepts, not a second copy.
        assert_eq!(props["verdict"]["enum"], serde_json::json!(Verdict::WIRE));
        assert_eq!(props["impact"]["enum"], serde_json::json!(Impact::WIRE));
        assert_eq!(
            props["findings"]["items"]["properties"]["severity"]["enum"],
            serde_json::json!(Severity::WIRE)
        );
    }

    /// The panel treats a parseable file as proof an angle finished, so the write must
    /// be all-or-nothing: a reader must never see a prefix, and must never see the
    /// destination empty because a re-submission truncated it.
    #[test]
    fn the_findings_file_is_replaced_atomically_and_leaves_no_debris() {
        let d = tempfile::tempdir().unwrap();
        let path = findings_path(d.path(), "task-1", 1, "security");

        let submit = |verdict: &str, summary: &str| {
            handle(
                &call(serde_json::json!({
                    "angle": "security",
                    "verdict": verdict,
                    "findings": [{"file": "a.rs", "severity": "critical", "summary": summary}]
                })),
                d.path(),
                "task-1",
                1,
                &angles(),
            )
            .unwrap()
        };

        assert!(!is_error(&submit("changes", "first")), "first submission");
        assert_eq!(
            parse_review(&std::fs::read_to_string(&path).unwrap())
                .unwrap()
                .findings[0]
                .summary,
            "first"
        );

        // A re-submission replaces the file. `fs::write` would truncate it first,
        // leaving a window where the destination is empty or partial.
        assert!(!is_error(&submit("changes", "second")), "re-submission");
        assert_eq!(
            parse_review(&std::fs::read_to_string(&path).unwrap())
                .unwrap()
                .findings[0]
                .summary,
            "second"
        );

        // The rename target is the only thing left in the directory: no `.tmp` sibling
        // survives, which is what a reader scanning the run dir would otherwise trip on.
        let left: Vec<String> = std::fs::read_dir(d.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            left,
            vec![path.file_name().unwrap().to_string_lossy().into_owned()],
            "the atomic write must leave no temp debris: {left:?}"
        );
    }

    /// The temp file is a sibling in the SAME directory. A rename across filesystems is
    /// not atomic (and fails outright), so this is load-bearing, not incidental.
    #[test]
    fn the_temp_file_is_a_sibling_of_its_destination() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("nested").join("out.json");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        write_atomically(&path, "{}").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "{}");
        let siblings: Vec<String> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(siblings, vec!["out.json".to_string()], "{siblings:?}");
    }

    /// A line that does not parse must still get a response. Dropping it leaves the
    /// client waiting out its own timeout on a request it will never see answered —
    /// which looks, from the panel, exactly like a reviewer that went quiet.
    #[test]
    fn a_malformed_line_is_answered_with_a_json_rpc_parse_error() {
        let reply = parse_error("expected value at line 1 column 1");
        assert_eq!(reply["jsonrpc"], "2.0");
        assert!(reply["id"].is_null(), "a line that did not parse has no id");
        assert_eq!(reply["error"]["code"], -32700);
        assert!(
            reply["error"]["message"]
                .as_str()
                .unwrap()
                .contains("Parse error"),
            "{reply}"
        );
    }

    /// A bad payload must come back as a TOOL error the model can read and retry, not a
    /// transport failure and not a silent bad file.
    #[test]
    fn malformed_findings_are_rejected_before_anything_is_written() {
        let d = tempfile::tempdir().unwrap();
        let r = handle(
            &call(serde_json::json!({"angle": "type-design", "summary": "I forgot the verdict"})),
            d.path(),
            "task-1",
            1,
            &angles(),
        )
        .unwrap();
        assert!(
            is_error(&r),
            "a payload that is not a Review must be refused"
        );
        let msg = r["result"]["content"][0]["text"].as_str().unwrap();
        assert!(
            msg.contains("submit_findings again"),
            "the error must tell the reviewer how to recover: {msg}"
        );
        assert!(
            !findings_path(d.path(), "task-1", 1, "type-design").exists(),
            "nothing may be written when validation fails"
        );
    }

    /// The angle selects the file, so it is checked against the panel's configured
    /// angles rather than pasted into a path. Traversal cannot survive that check.
    #[test]
    fn an_angle_outside_the_configured_set_is_refused() {
        let d = tempfile::tempdir().unwrap();
        for bad in [
            "../../../etc/passwd",
            "../escape",
            "correctness/../x",
            "made-up",
        ] {
            let r = handle(
                &call(serde_json::json!({"angle": bad, "verdict": "clean"})),
                d.path(),
                "task-1",
                1,
                &angles(),
            )
            .unwrap();
            assert!(is_error(&r), "angle '{bad}' must be refused");
        }
        // Nothing at all was created, at any depth.
        let mut entries = std::fs::read_dir(d.path()).unwrap();
        assert!(
            entries.next().is_none(),
            "a refused angle must write nothing"
        );
    }

    #[test]
    fn a_missing_angle_is_refused_with_the_valid_ones_listed() {
        let d = tempfile::tempdir().unwrap();
        let r = handle(
            &call(serde_json::json!({"verdict": "clean"})),
            d.path(),
            "task-1",
            1,
            &angles(),
        )
        .unwrap();
        assert!(is_error(&r));
        let msg = r["result"]["content"][0]["text"].as_str().unwrap();
        assert!(msg.contains("type-design"), "list the options: {msg}");
    }

    /// The merge stamps each finding's angle from the filename, so the body must not
    /// carry a second, disagreeing copy.
    #[test]
    fn the_angle_is_not_written_into_the_findings_body() {
        let d = tempfile::tempdir().unwrap();
        let r = handle(
            &call(serde_json::json!({"angle": "error-handling", "verdict": "clean"})),
            d.path(),
            "task-1",
            1,
            &angles(),
        )
        .unwrap();
        assert!(!is_error(&r), "{r}");
        let body = std::fs::read_to_string(findings_path(d.path(), "task-1", 1, "error-handling"))
            .unwrap();
        assert!(
            !body.contains("\"angle\""),
            "angle must be stripped: {body}"
        );
        assert!(parse_review(&body).is_ok());
    }

    /// …and that holds with findings present too: a `Finding`'s own `angle` is stamped
    /// by the merge from the filename, so an unstamped one must not be written beside
    /// it as an empty second copy.
    #[test]
    fn a_findings_angle_is_not_written_into_the_per_angle_file_either() {
        let d = tempfile::tempdir().unwrap();
        let r = handle(
            &call(serde_json::json!({
                "angle": "correctness",
                "verdict": "changes",
                "findings": [{"file": "a.rs", "severity": "nit", "summary": "s"}]
            })),
            d.path(),
            "task-1",
            1,
            &angles(),
        )
        .unwrap();
        assert!(!is_error(&r), "{r}");
        let body =
            std::fs::read_to_string(findings_path(d.path(), "task-1", 1, "correctness")).unwrap();
        assert!(!body.contains("\"angle\""), "{body}");
        assert!(
            !body.contains("\"rationale\""),
            "an empty rationale is noise: {body}"
        );
        // The merge is what fills it in, from the filename.
        let merged = crate::findings::merge_reviews(vec![(
            "correctness".to_string(),
            parse_review(&body).unwrap(),
        )]);
        assert_eq!(merged.findings[0].angle, "correctness");
        assert!(
            serde_json::to_string(&merged)
                .unwrap()
                .contains("\"angle\":\"correctness\""),
            "a stamped angle IS written to the merged review"
        );
    }

    #[test]
    fn an_unknown_tool_is_an_error_not_a_write() {
        let d = tempfile::tempdir().unwrap();
        let req = serde_json::json!({
            "jsonrpc": "2.0", "id": 9, "method": "tools/call",
            "params": {"name": "rm_rf", "arguments": {}}
        });
        let r = handle(&req, d.path(), "task-1", 1, &angles()).unwrap();
        assert!(is_error(&r));
        assert!(!findings_path(d.path(), "task-1", 1, "correctness").exists());
    }

    /// Severity and findings round-trip, so a `changes` verdict survives the tool.
    #[test]
    fn findings_with_severities_round_trip_through_the_tool() {
        let d = tempfile::tempdir().unwrap();
        let r = handle(
            &call(serde_json::json!({
                "angle": "correctness",
                "verdict": "changes",
                "findings": [
                    {"file": "a.rs", "line": 4, "severity": "critical", "summary": "panics on empty input"},
                    {"file": "b.rs", "severity": "nit", "summary": "stringly typed"}
                ],
                "impact": "high"
            })),
            d.path(),
            "task-2",
            2,
            &angles(),
        )
        .unwrap();
        assert!(!is_error(&r), "{r}");
        let review = parse_review(
            &std::fs::read_to_string(findings_path(d.path(), "task-2", 2, "correctness")).unwrap(),
        )
        .unwrap();
        assert_eq!(review.verdict, Verdict::Changes);
        assert_eq!(review.findings.len(), 2);
        assert_eq!(review.findings[0].line, Some(4));
        assert_eq!(review.impact, Some(Impact::High));
    }
}
