//! `<run_dir>/interview.jsonl` — the append-only interview log and its fold.
//!
//! One module owns the log's shape, its append and its fold, so the CLI (`drovr ask`)
//! and the server (`POST answer`) share exactly one implementation of "what is pending".
//!
//! # Append-only is the point, not an implementation detail
//!
//! `feedback.json` is rewritten in place on every submit, so a reviewer's earlier-round
//! answers are unrecoverable (`docs/known-issues.md`, *"`feedback.json` is overwritten
//! every turn, so earlier turns' annotations are unrecoverable"*). This log exists to not
//! do that: **every write is an append**, and no code path here opens the file for
//! truncation or rewrites a byte already on disk. A fold that edits a record, or a
//! writer that rewrites the ask when its answer arrives, reintroduces the defect this
//! replaces.
//!
//! # Two record kinds, told apart by which fields are present
//!
//! There is no discriminator field. A line is:
//!
//! - an **ask** record if it carries `question` — plus `id`, `seq`, and optionally
//!   `context`, `options`, `recommend`. It never carries `answer` or `answered_at`.
//! - an **answer** record if it carries `answer` — plus `id`, `seq`, `answered_at`, and
//!   nothing else. Its `seq` is the `seq` of the ask it answers, not a line counter: that
//!   is what makes the raw log, and not merely the folded view, reproducible.
//!
//! A line carrying **both** (or neither) is malformed and is skipped, which keeps the
//! discriminator unambiguous rather than resolving a contradiction silently.
//!
//! # Reader semantics
//!
//! [`read`] folds by `id`; the **last** answer record for an id wins. A line that does not
//! parse into a well-formed record is skipped **in its entirety** — a half-written line
//! must not make the whole interview unreadable — and is never fatal. The fold is by `id`
//! alone, so an answer record's `seq` is redundant provenance: it is written and tested,
//! but a disagreeing one does not invalidate the answer.
//!
//! # Concurrency
//!
//! Appends use `O_APPEND` and a single `write_all` of `line + "\n"`, so the two writers
//! (the `ask` CLI and the server's `POST answer`) cannot interleave at an offset.
//! `seq` allocation is *not* locked: it is `read`-then-append, and its uniqueness rests on
//! the single-writer discipline — only the agent side calls [`append_ask`], and it calls
//! it one at a time. Answers allocate nothing, so the server never races for a `seq`.

// The module lands ahead of its callers: `drovr ask` (T3) and the server's `GET
// interview` / `POST answer` (T4) are the consumers, and until they exist every item here
// is dead to the non-test build. Delete this allow once the server route lands — by then
// nothing in here should be unreachable, and a fresh warning would be a real finding.
#![allow(dead_code)]

use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// The log's filename inside a run dir.
pub const LOG_FILE: &str = "interview.jsonl";

/// One selectable answer: the `value` recorded if it is chosen, and the `label` shown.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct AskOption {
    pub value: String,
    pub label: String,
}

/// One interview question, folded together with its latest answer (if any).
///
/// This is the *view*, not the on-disk record: on disk an ask and its answer are two
/// separate lines. Serialization here is the **wire** shape — every field present, `null`
/// where absent — because a client rendering the interview should not have to distinguish
/// "absent" from "empty". The on-disk shape is built separately, in [`append_ask`] and
/// [`append_answer`], where field *presence* carries meaning.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Ask {
    pub id: String,
    pub seq: u32,
    pub question: String,
    pub context: Option<String>,
    pub options: Vec<AskOption>,
    pub recommend: Option<String>,
    pub answer: Option<String>,
    pub answered_at: Option<String>,
}

/// Path of `<run_dir>/interview.jsonl`.
pub fn log_path(run_dir: &Path) -> PathBuf {
    run_dir.join(LOG_FILE)
}

/// Fold the log into one [`Ask`] per id, in `seq` order.
///
/// A malformed line is SKIPPED, not fatal. Missing file => `Ok(vec![])`; any other IO
/// error propagates, because an unreadable-but-present log is a real failure and
/// reporting it as "no questions" would silently lose the interview.
pub fn read(run_dir: &Path) -> io::Result<Vec<Ask>> {
    let raw = match std::fs::read_to_string(log_path(run_dir)) {
        Ok(s) => s,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };

    let mut asks: Vec<Ask> = Vec::new();
    let mut at: HashMap<String, usize> = HashMap::new();
    for line in raw.lines() {
        match parse_record(line) {
            // The FIRST ask for an id wins: an ask is never rewritten, so a second one
            // can only come from a hand-edited log, and honouring it would let an edit
            // change a question a human has already been shown.
            Some(Record::Ask(a)) => {
                if !at.contains_key(&a.id) {
                    at.insert(a.id.clone(), asks.len());
                    asks.push(a);
                }
            }
            // Last answer wins. An answer whose ask is not (yet) in the log is dropped:
            // appends are ordered, so this too is only reachable by hand-editing.
            Some(Record::Answer {
                id,
                answer,
                answered_at,
            }) => {
                if let Some(&i) = at.get(&id) {
                    asks[i].answer = Some(answer);
                    asks[i].answered_at = answered_at;
                }
            }
            None => continue,
        }
    }

    // Stable, so asks sharing a seq (again: only by hand-edit) keep file order.
    asks.sort_by_key(|a| a.seq);
    Ok(asks)
}

/// One parsed line of the log.
enum Record {
    Ask(Ask),
    Answer {
        id: String,
        answer: String,
        answered_at: Option<String>,
    },
}

/// Parse one line, or `None` if it is not a well-formed record.
///
/// Strict at the line level and lenient about the file: any structural violation skips
/// the whole line rather than half-admitting a record whose meaning is unclear.
fn parse_record(line: &str) -> Option<Record> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    let obj = value.as_object()?;

    let id = obj.get("id")?.as_str()?;
    if id.is_empty() {
        return None;
    }
    let seq = u32::try_from(obj.get("seq")?.as_u64()?).ok()?;

    match (obj.contains_key("question"), obj.contains_key("answer")) {
        (false, true) => Some(Record::Answer {
            id: id.to_string(),
            answer: obj.get("answer")?.as_str()?.to_string(),
            answered_at: opt_str(obj.get("answered_at"))?,
        }),
        (true, false) => {
            let options = match obj.get("options") {
                None | Some(serde_json::Value::Null) => Vec::new(),
                Some(v) => v
                    .as_array()?
                    .iter()
                    .map(|o| {
                        let o = o.as_object()?;
                        Some(AskOption {
                            value: o.get("value")?.as_str()?.to_string(),
                            label: o.get("label")?.as_str()?.to_string(),
                        })
                    })
                    .collect::<Option<Vec<_>>>()?,
            };
            Some(Record::Ask(Ask {
                id: id.to_string(),
                seq,
                question: obj.get("question")?.as_str()?.to_string(),
                context: opt_str(obj.get("context"))?,
                options,
                recommend: opt_str(obj.get("recommend"))?,
                answer: None,
                answered_at: None,
            }))
        }
        // Both => a contradiction; neither => not a record at all.
        _ => None,
    }
}

/// An absent or `null` field is `Ok(None)`; a present string is `Ok(Some(_))`; a present
/// non-string is a structural violation (`None`, i.e. skip the line).
fn opt_str(v: Option<&serde_json::Value>) -> Option<Option<String>> {
    match v {
        None | Some(serde_json::Value::Null) => Some(None),
        Some(v) => Some(Some(v.as_str()?.to_string())),
    }
}

/// Append one line. `O_APPEND` + a single `write_all` — never a truncating open.
fn append_line(run_dir: &Path, line: &str) -> io::Result<()> {
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path(run_dir))?;
    f.write_all(format!("{line}\n").as_bytes())
}

/// Append an ask record; returns the assigned [`Ask`] (id + seq).
///
/// `seq` is the count of existing ask records (0-based) and `id` is `ask-<seq>`, so ids
/// are unique under the single-writer discipline and are `safe_component`-clean.
/// A blank `question` is rejected with `ErrorKind::InvalidInput`: an unanswerable record
/// is worse than an error, because it occupies a `seq` and renders as an empty prompt.
pub fn append_ask(
    run_dir: &Path,
    question: &str,
    context: Option<&str>,
    options: &[AskOption],
    recommend: Option<&str>,
) -> io::Result<Ask> {
    if question.trim().is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "drovr ask: --question is empty",
        ));
    }

    let existing = read(run_dir)?;
    let seq = u32::try_from(existing.len()).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "interview.jsonl: more asks than a u32 seq can number",
        )
    })?;

    let ask = Ask {
        id: format!("ask-{seq}"),
        seq,
        question: question.to_string(),
        context: context.map(str::to_string),
        options: options.to_vec(),
        recommend: recommend.map(str::to_string),
        answer: None,
        answered_at: None,
    };

    // The on-disk shape, built explicitly rather than via `Ask`'s Serialize: here field
    // *presence* is the discriminator, so the optional fields must be omitted rather than
    // written as null, and `answer`/`answered_at` must not appear at all.
    let mut rec = serde_json::Map::new();
    rec.insert("id".into(), ask.id.clone().into());
    rec.insert("seq".into(), ask.seq.into());
    rec.insert("question".into(), ask.question.clone().into());
    if let Some(c) = &ask.context {
        rec.insert("context".into(), c.clone().into());
    }
    if !ask.options.is_empty() {
        rec.insert(
            "options".into(),
            serde_json::to_value(&ask.options).map_err(io::Error::other)?,
        );
    }
    if let Some(r) = &ask.recommend {
        rec.insert("recommend".into(), r.clone().into());
    }

    append_line(run_dir, &serde_json::Value::Object(rec).to_string())?;
    Ok(ask)
}

/// Append an answer record for `id`, carrying that ask's `seq`.
///
/// `Err(ErrorKind::NotFound)` if no ask carries that id. Does NOT touch
/// `review.state.json`, and does NOT rewrite the ask — the ask's bytes are already on
/// disk and stay there.
///
/// An empty `answer` is accepted: "I have nothing to add" is a decision the human is
/// entitled to make, and the fold treats the id as answered from then on.
pub fn append_answer(run_dir: &Path, id: &str, answer: &str) -> io::Result<()> {
    let asks = read(run_dir)?;
    let ask = asks.iter().find(|a| a.id == id).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("interview.jsonl: no ask with id {id}"),
        )
    })?;

    // "and nothing else": id, the ask's own seq, the answer, and when. The question is
    // already on disk one line up and is not restated.
    let rec = serde_json::json!({
        "id": ask.id,
        "seq": ask.seq,
        "answer": answer,
        "answered_at": now_rfc3339_utc(),
    });
    append_line(run_dir, &rec.to_string())
}

/// Ids with no answer, in `seq` order.
///
/// Sorts defensively — [`read`] already returns `seq` order, but the ordering is part of
/// this function's contract and should not depend on how the caller got its slice.
pub fn pending(asks: &[Ask]) -> Vec<String> {
    let mut unanswered: Vec<&Ask> = asks.iter().filter(|a| a.answer.is_none()).collect();
    unanswered.sort_by_key(|a| a.seq);
    unanswered.into_iter().map(|a| a.id.clone()).collect()
}

/// The `Vec<Ask>` as the JSON array the server serves and the CLI prints.
///
/// Serializing these types cannot fail (no non-string map keys, no non-finite floats), so
/// the fallback is unreachable; it exists so a serving path can never panic on a request.
pub fn to_json(asks: &[Ask]) -> String {
    serde_json::to_string(asks).unwrap_or_else(|_| "[]".to_string())
}

/// Now, as `YYYY-MM-DDTHH:MM:SSZ`. A clock before the epoch reads as the epoch rather
/// than failing an append — the answer matters more than its timestamp.
fn now_rfc3339_utc() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    rfc3339_utc(secs)
}

/// `secs` since the Unix epoch as `YYYY-MM-DDTHH:MM:SSZ`.
///
/// Hand-rolled because drovr has no date dependency and this is the only place that needs
/// one. The civil-from-days conversion is Howard Hinnant's `civil_from_days`, shifted to a
/// March-based year so the leap day falls at the end of it.
fn rfc3339_utc(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let tod = secs % 86_400;

    let z = days + 719_468;
    let era = z / 146_097; // `secs` is u64, so z >= 0 and the negative-era case cannot arise.
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11], March-based
    let day = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);

    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        tod / 3600,
        (tod % 3600) / 60,
        tod % 60,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn read_lines(dir: &Path) -> Vec<String> {
        fs::read_to_string(log_path(dir))
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }

    fn parse_line(line: &str) -> serde_json::Value {
        serde_json::from_str(line).expect("line is JSON")
    }

    #[test]
    fn append_ask_assigns_sequential_ids() {
        let d = tempfile::tempdir().unwrap();
        let a0 = append_ask(d.path(), "first?", None, &[], None).unwrap();
        let a1 = append_ask(d.path(), "second?", None, &[], None).unwrap();
        let a2 = append_ask(d.path(), "third?", None, &[], None).unwrap();

        assert_eq!((a0.id.as_str(), a0.seq), ("ask-0", 0));
        assert_eq!((a1.id.as_str(), a1.seq), ("ask-1", 1));
        assert_eq!((a2.id.as_str(), a2.seq), ("ask-2", 2));

        let ids: Vec<String> = read(d.path()).unwrap().into_iter().map(|a| a.id).collect();
        assert_eq!(ids, vec!["ask-0", "ask-1", "ask-2"]);
    }

    #[test]
    fn read_folds_answer_onto_its_ask() {
        let d = tempfile::tempdir().unwrap();
        let ask = append_ask(d.path(), "ship it?", None, &[], None).unwrap();
        append_answer(d.path(), &ask.id, "yes").unwrap();

        let folded = read(d.path()).unwrap();
        assert_eq!(folded.len(), 1, "folds to one Ask: {folded:?}");
        assert_eq!(folded[0].answer.as_deref(), Some("yes"));
        assert!(folded[0].answered_at.is_some(), "answered_at is stamped");

        // Append-only: the answer did not rewrite the ask, it followed it.
        assert_eq!(read_lines(d.path()).len(), 2, "two lines on disk");
    }

    #[test]
    fn an_answer_record_carries_its_asks_seq() {
        let d = tempfile::tempdir().unwrap();
        append_ask(d.path(), "q0?", None, &[], None).unwrap();
        let a1 = append_ask(d.path(), "q1?", None, &[], None).unwrap();
        append_answer(d.path(), &a1.id, "b").unwrap();

        let lines = read_lines(d.path());
        let rec = parse_line(lines.last().expect("an answer line"));
        assert_eq!(rec["id"], "ask-1");
        assert_eq!(rec["seq"], 1, "the ask's seq, not a line counter: {rec}");
        assert_eq!(rec["answer"], "b");
        assert!(rec["answered_at"].is_string());
        // "and nothing else" — the answer record does not restate the question.
        let keys: Vec<&str> = rec
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(keys.len(), 4, "exactly id/seq/answer/answered_at: {keys:?}");
    }

    #[test]
    fn an_ask_record_never_carries_an_answer_field() {
        let d = tempfile::tempdir().unwrap();
        let ask = append_ask(d.path(), "q?", Some("ctx"), &[], Some("a")).unwrap();
        append_answer(d.path(), &ask.id, "a").unwrap();

        let lines = read_lines(d.path());
        let first = parse_line(&lines[0]);
        assert!(
            first.get("question").is_some(),
            "line 0 is the ask: {first}"
        );
        assert!(
            first.get("answer").is_none(),
            "ask carries no answer: {first}"
        );
        assert!(
            first.get("answered_at").is_none(),
            "nor answered_at: {first}"
        );
    }

    #[test]
    fn last_answer_for_an_id_wins() {
        let d = tempfile::tempdir().unwrap();
        let ask = append_ask(d.path(), "colour?", None, &[], None).unwrap();
        append_answer(d.path(), &ask.id, "red").unwrap();
        append_answer(d.path(), &ask.id, "blue").unwrap();

        let folded = read(d.path()).unwrap();
        assert_eq!(folded.len(), 1);
        assert_eq!(folded[0].answer.as_deref(), Some("blue"));
        assert_eq!(read_lines(d.path()).len(), 3, "both answers kept on disk");
    }

    #[test]
    fn earlier_turns_answers_survive_a_later_round() {
        // `docs/known-issues.md`, "`feedback.json` is overwritten every turn, so earlier
        // turns' annotations are unrecoverable", as a test. This is the whole reason the
        // log is append-only: a later round must not cost an earlier round's answer.
        let d = tempfile::tempdir().unwrap();
        let a0 = append_ask(d.path(), "round one?", None, &[], None).unwrap();
        append_answer(d.path(), &a0.id, "one").unwrap();
        let a1 = append_ask(d.path(), "round two?", None, &[], None).unwrap();
        append_answer(d.path(), &a1.id, "two").unwrap();

        let folded = read(d.path()).unwrap();
        assert_eq!(folded.len(), 2);
        assert_eq!(folded[0].answer.as_deref(), Some("one"), "turn 1 survives");
        assert_eq!(folded[1].answer.as_deref(), Some("two"));
    }

    #[test]
    fn every_append_only_extends_the_file() {
        // The property itself: after any write, the file's earlier bytes are unchanged.
        // An `fs::write`-based implementation fails this even when the fold looks right.
        let d = tempfile::tempdir().unwrap();
        let mut prev = Vec::new();
        let snapshot = |dir: &Path, prev: &mut Vec<u8>| {
            let now = fs::read(log_path(dir)).unwrap();
            assert!(
                now.starts_with(prev),
                "a write rewrote earlier bytes:\nbefore: {}\nafter:  {}",
                String::from_utf8_lossy(prev),
                String::from_utf8_lossy(&now),
            );
            assert!(now.len() > prev.len(), "the write appended nothing");
            *prev = now;
        };

        let a0 = append_ask(d.path(), "q0?", None, &[], None).unwrap();
        snapshot(d.path(), &mut prev);
        append_answer(d.path(), &a0.id, "a0").unwrap();
        snapshot(d.path(), &mut prev);
        let a1 = append_ask(d.path(), "q1?", None, &[], None).unwrap();
        snapshot(d.path(), &mut prev);
        append_answer(d.path(), &a1.id, "a1").unwrap();
        snapshot(d.path(), &mut prev);
        append_answer(d.path(), &a0.id, "a0 revised").unwrap();
        snapshot(d.path(), &mut prev);
    }

    #[test]
    fn a_malformed_line_is_skipped_not_fatal() {
        let d = tempfile::tempdir().unwrap();
        let a0 = append_ask(d.path(), "q0?", None, &[], None).unwrap();
        // A half-written line, as a crash mid-append would leave.
        let mut f = OpenOptions::new()
            .append(true)
            .open(log_path(d.path()))
            .unwrap();
        f.write_all(b"{\"id\":\"ask-1\",\"seq\":1,\"quest\n")
            .unwrap();
        drop(f);
        append_answer(d.path(), &a0.id, "still works").unwrap();

        let folded = read(d.path()).unwrap();
        assert_eq!(folded.len(), 1, "the good record survives: {folded:?}");
        assert_eq!(folded[0].answer.as_deref(), Some("still works"));
    }

    #[test]
    fn a_line_carrying_both_question_and_answer_is_skipped() {
        let d = tempfile::tempdir().unwrap();
        append_ask(d.path(), "real?", None, &[], None).unwrap();
        let mut f = OpenOptions::new()
            .append(true)
            .open(log_path(d.path()))
            .unwrap();
        f.write_all(b"{\"id\":\"ask-9\",\"seq\":9,\"question\":\"both?\",\"answer\":\"x\"}\n")
            .unwrap();
        drop(f);

        let folded = read(d.path()).unwrap();
        assert_eq!(
            folded.len(),
            1,
            "the contradictory line is skipped: {folded:?}"
        );
        assert_eq!(folded[0].id, "ask-0");
    }

    #[test]
    fn pending_lists_only_unanswered_in_seq_order() {
        let d = tempfile::tempdir().unwrap();
        append_ask(d.path(), "q0?", None, &[], None).unwrap();
        let a1 = append_ask(d.path(), "q1?", None, &[], None).unwrap();
        append_ask(d.path(), "q2?", None, &[], None).unwrap();
        append_answer(d.path(), &a1.id, "done").unwrap();

        let folded = read(d.path()).unwrap();
        assert_eq!(pending(&folded), vec!["ask-0", "ask-2"]);
    }

    #[test]
    fn append_answer_rejects_an_unknown_id() {
        let d = tempfile::tempdir().unwrap();
        append_ask(d.path(), "q0?", None, &[], None).unwrap();
        let err = append_answer(d.path(), "ask-7", "x").expect_err("unknown id");
        assert_eq!(err.kind(), io::ErrorKind::NotFound, "{err}");
        assert_eq!(
            read_lines(d.path()).len(),
            1,
            "a rejected answer writes nothing"
        );
    }

    #[test]
    fn append_ask_rejects_a_blank_question() {
        let d = tempfile::tempdir().unwrap();
        let err = append_ask(d.path(), "   \n", None, &[], None).expect_err("blank");
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput, "{err}");
        assert!(
            !log_path(d.path()).exists(),
            "a rejected ask creates no log"
        );
    }

    #[test]
    fn read_of_a_missing_log_is_empty() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(read(d.path()).unwrap(), Vec::new());
        assert_eq!(pending(&[]), Vec::<String>::new());
        assert_eq!(to_json(&[]), "[]");
    }

    #[test]
    fn a_multiline_question_stays_one_line() {
        // JSONL's one hazard: a newline inside a field would split the record in two.
        let d = tempfile::tempdir().unwrap();
        let ask = append_ask(d.path(), "line one\nline two", Some("ctx\nmore"), &[], None).unwrap();
        assert_eq!(read_lines(d.path()).len(), 1, "still one physical line");

        let folded = read(d.path()).unwrap();
        assert_eq!(folded[0].question, "line one\nline two");
        assert_eq!(folded[0].context.as_deref(), Some("ctx\nmore"));
        assert_eq!(folded[0].id, ask.id);
    }

    #[test]
    fn an_ask_round_trips_its_options_and_recommendation() {
        let d = tempfile::tempdir().unwrap();
        let opts = vec![
            AskOption {
                value: "a".into(),
                label: "Alpha".into(),
            },
            AskOption {
                value: "b".into(),
                label: "Beta".into(),
            },
        ];
        append_ask(d.path(), "which?", Some("why it matters"), &opts, Some("b")).unwrap();

        let folded = read(d.path()).unwrap();
        assert_eq!(folded[0].options, opts);
        assert_eq!(folded[0].recommend.as_deref(), Some("b"));
        assert_eq!(folded[0].context.as_deref(), Some("why it matters"));
    }

    #[test]
    fn to_json_carries_every_field_including_the_absent_ones() {
        let d = tempfile::tempdir().unwrap();
        append_ask(d.path(), "q?", None, &[], None).unwrap();
        let folded = read(d.path()).unwrap();

        let v: serde_json::Value = serde_json::from_str(&to_json(&folded)).expect("JSON array");
        let rec = &v.as_array().expect("array")[0];
        assert_eq!(rec["id"], "ask-0");
        assert_eq!(rec["seq"], 0);
        assert_eq!(rec["question"], "q?");
        assert!(rec["context"].is_null(), "absent renders as null: {rec}");
        assert!(rec["recommend"].is_null());
        assert!(rec["answer"].is_null());
        assert!(rec["answered_at"].is_null());
        assert_eq!(rec["options"], serde_json::json!([]));
    }

    #[test]
    fn rfc3339_utc_formats_known_instants() {
        assert_eq!(rfc3339_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339_utc(1_000_000_000), "2001-09-09T01:46:40Z");
        // The day after a leap day, the case an off-by-one in the civil calendar hits.
        assert_eq!(rfc3339_utc(1_583_020_800), "2020-03-01T00:00:00Z");
        assert_eq!(rfc3339_utc(4_102_444_799), "2099-12-31T23:59:59Z");
    }
}
