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
//!   nothing else. All four are required. Its `seq` is the `seq` of the ask it answers, not
//!   a line counter: that is what makes the raw log, and not merely the folded view,
//!   reproducible.
//!
//! A line carrying **both** (or neither) is malformed and is skipped, which keeps the
//! discriminator unambiguous rather than resolving a contradiction silently. That also
//! catches the one mistake the shared [`Ask`] type invites: a serialized `Ask` carries
//! `question` AND a null `answer`, so writing one into the log is rejected by the fold
//! rather than quietly corrupting it.
//!
//! Hand-editing the log is therefore possible but exacting — a record missing a field its
//! kind requires is dropped, silently, by design.
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

use crate::brief::MAX_CONTEXT;
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

/// The log's filename inside a run dir.
pub const LOG_FILE: &str = "interview.jsonl";

/// Cap on any single caller-supplied field — a question, a context, a recommendation, an
/// option's value or label, an answer.
///
/// This is [`MAX_CONTEXT`] deliberately, not a second number: `drovr ask --context <file>`
/// is the same class of input the brief cap already governs, and two limits for one kind of
/// text is how they drift apart.
pub const MAX_FIELD: u64 = MAX_CONTEXT;

/// Cap on an ask id a caller hands in. Ids are minted as `ask-<u32>`, so 64 bytes is
/// generous; the bound exists because in T4 the id arrives in an HTTP body, and an
/// unbounded string should reach neither a lookup nor an error message.
pub const MAX_ID: usize = 64;

/// Cap on the whole log. Only drovr writes it, and each record is bounded by [`MAX_FIELD`],
/// so reaching this needs either a pathological interview or a hand-planted file — and past
/// it, every [`read`] would slurp the lot, on every append.
///
/// Over the cap is an ERROR, not a truncated read: silently dropping the tail of an
/// append-only log would lose exactly the answers the log exists to keep.
pub const MAX_LOG: u64 = 16 << 20;

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
///
/// Two invariants hold for every `Ask` this module hands out, enforced at both places one
/// can be built — `parse_record` and [`append_ask`] — rather than in the type:
///
/// * `answer.is_some() == answered_at.is_some()`.
/// * `recommend`, when `options` is non-empty, is one of their `value`s.
///
/// The fields stay flat and `pub` because the spec pins this record's field set
/// (`{id, seq, question, context, options, recommend, answer, answered_at}`) and the UI
/// reads it; nesting `answer` into its own type would either change the wire shape or need
/// a second, hand-written serialization of it, which is the one thing this module is
/// careful not to have.
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

/// Why this set of offered options is unusable, or `Ok(())`.
///
/// Two rules, both for the same reason the blank-question check in [`append_ask`]
/// exists — an unanswerable record is worse than an error, and the log is append-only,
/// so one written wrong is wrong for good:
///
/// * **No blank value or label.** The `value` is what the answer records and the
///   `label` is the only thing the human sees, so a blank one is respectively an
///   answer that says nothing and a button with no text.
/// * **Distinct values.** A repeat makes the answer ambiguous: nothing downstream can
///   say which entry was picked, and a `recommend` naming it pre-selects one of two
///   indistinguishable choices.
///
/// This module is the AUTHORITY for both, not merely the last line of defence. The CLI
/// happens to reject blanks while splitting `<value>=<label>`, but that is its own
/// string parsing; a caller arriving any other way — the server, a later task — must
/// meet the same rule, and must not have to re-derive it. One implementation, three
/// callers ([`append_ask`], `parse_record`, and the CLI's `validate_ask_request`),
/// returning the reason so each can render it in its own idiom.
pub fn check_options(options: &[AskOption]) -> Result<(), String> {
    let mut seen = std::collections::HashSet::with_capacity(options.len());
    for o in options {
        if o.value.trim().is_empty() {
            return Err("an --option has an empty value; the value is what the answer \
                        records"
                .to_string());
        }
        if o.label.trim().is_empty() {
            return Err("an --option has an empty label; the label is all the human \
                        sees"
                .to_string());
        }
        if !seen.insert(o.value.as_str()) {
            // The offending value is NOT echoed, for the reason `check_id` gives: this
            // runs before any size cap — `validate_ask_request` calls it to fail fast,
            // and `check_field` only runs later inside `append_ask` — so echoing it is
            // unbounded. T4 calls this from an HTTP handler, where that would be a
            // response body mirroring a request's own field back at its sender.
            return Err(
                "two --options share a value, so an answer could not say which was chosen"
                    .to_string(),
            );
        }
    }
    Ok(())
}

/// Whether `recommend` is a legal recommendation for `options`.
///
/// A recommendation that names no offered option pre-selects nothing, and the human
/// sees a question whose "recommended" choice is invisible. With no options at all it
/// is a free-text suggestion, and there is nothing to check it against — so that case
/// is legal, and every caller must agree it is. See [`check_options`] on why
/// this is a shared predicate rather than three copies.
pub fn recommend_is_offered(recommend: Option<&str>, options: &[AskOption]) -> bool {
    match recommend {
        Some(r) if !options.is_empty() => options.iter().any(|o| o.value == r),
        _ => true,
    }
}

/// Fold the log into one [`Ask`] per id, in `seq` order.
///
/// A malformed line is SKIPPED, not fatal. Missing file => `Ok(vec![])`; any other IO
/// error propagates, because an unreadable-but-present log is a real failure and
/// reporting it as "no questions" would silently lose the interview.
///
/// A symlink, a non-regular file, or a log over [`MAX_LOG`] is refused — see
/// [`open_no_follow`].
pub fn read(run_dir: &Path) -> io::Result<Vec<Ask>> {
    let path = log_path(run_dir);
    let file = match open_no_follow(&path)? {
        Some(f) => f,
        None => return Ok(Vec::new()),
    };
    // Bound the READ, not just the metadata check: the file can grow between the two, so a
    // metadata-only cap is a TOCTOU that lets an oversized log through.
    let mut raw = String::new();
    file.take(MAX_LOG + 1).read_to_string(&mut raw)?;
    if raw.len() as u64 > MAX_LOG {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "{} grew past the {MAX_LOG}-byte interview-log limit while being read",
                path.display()
            ),
        ));
    }

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
                    asks[i].answered_at = Some(answered_at);
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
        answered_at: String,
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
        // `answered_at` is REQUIRED, not optional: [`append_answer`] always writes it, so
        // accepting a record without one would only admit a hand-edited line — at the cost
        // of making `answer: Some(_)` with `answered_at: None` representable in a folded
        // [`Ask`], an inconsistency nothing downstream should have to reason about.
        (false, true) => Some(Record::Answer {
            id: id.to_string(),
            answer: obj.get("answer")?.as_str()?.to_string(),
            answered_at: obj.get("answered_at")?.as_str()?.to_string(),
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
            let recommend = opt_str(obj.get("recommend"))?;
            // The same rules [`append_ask`] refuses to write, applied to what is read:
            // such a line can only come from a hand-edit, and admitting one would leave
            // [`Ask`]'s documented invariants true of the records this module mints and
            // false of the ones it hands out. Both are the shared predicates, not a
            // second copy of them — a copy is how a read rule and a write rule drift.
            if check_options(&options).is_err()
                || !recommend_is_offered(recommend.as_deref(), &options)
            {
                return None;
            }
            Some(Record::Ask(Ask {
                id: id.to_string(),
                seq,
                question: obj.get("question")?.as_str()?.to_string(),
                context: opt_str(obj.get("context"))?,
                options,
                recommend,
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

/// Open the log for reading, refusing to follow a symlink and refusing anything that is
/// not a regular file of at most [`MAX_LOG`] bytes. `Ok(None)` if it does not exist.
///
/// The same hardening `brief::read_record_no_follow` carries, and for the same reasons: a
/// link planted at the path would make drovr read (or, at [`append_line`], write) an
/// arbitrary file; a FIFO passes a symlink check and then blocks the reader forever. There
/// is no `O_NOFOLLOW` in std, so this is a `symlink_metadata` check — racy in principle,
/// decisive against a planted link in practice, and the run dir is not a contested
/// directory. Same mechanism, same caveat as `brief::read_record_no_follow`; keep them
/// together if either changes.
fn open_no_follow(path: &Path) -> io::Result<Option<std::fs::File>> {
    match check_no_follow(path)? {
        Some(_len) => Ok(Some(std::fs::File::open(path)?)),
        None => Ok(None),
    }
}

/// The metadata half of [`open_no_follow`]: `Ok(None)` if absent, `Ok(Some(len))` if it is
/// a regular file within [`MAX_LOG`], `Err` otherwise. Split out because the append path
/// needs the check, and the current size, without the read-only handle.
fn check_no_follow(path: &Path) -> io::Result<Option<u64>> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    if meta.file_type().is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "{} is a symlink; refusing to read or append the interview through it",
                path.display()
            ),
        ));
    }
    if !meta.file_type().is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "{} is not a regular file; refusing to use it as the interview log",
                path.display()
            ),
        ));
    }
    if meta.len() > MAX_LOG {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "{} is {} bytes, over the {MAX_LOG}-byte interview-log limit",
                path.display(),
                meta.len()
            ),
        ));
    }
    Ok(Some(meta.len()))
}

/// Append one line. `O_APPEND` + a single `write_all` — never a truncating open, and never
/// through a symlink ([`check_no_follow`] runs first, so a planted link is refused rather
/// than written through).
///
/// [`MAX_LOG`] is enforced on the RESULT, not just on what is already there. Per-field caps
/// bound a field, not a record — enough in-bounds options make one line bigger than the
/// whole cap — and since [`read`] refuses an over-cap log, an append that crossed it would
/// brick the interview permanently. The cap is therefore a precondition on every write: a
/// log drovr can read stays a log drovr can read.
fn append_line(run_dir: &Path, line: &str) -> io::Result<()> {
    let path = log_path(run_dir);
    let have = check_no_follow(&path)?.unwrap_or(0);
    let adding = line.len() as u64 + 1; // the newline
    if have + adding > MAX_LOG {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "appending {adding} bytes would put the interview log over its \
                 {MAX_LOG}-byte limit ({have} bytes already)"
            ),
        ));
    }
    let mut f = OpenOptions::new().create(true).append(true).open(&path)?;
    f.write_all(format!("{line}\n").as_bytes())
}

/// Reject a caller-supplied id over [`MAX_ID`], without echoing it.
fn check_id(id: &str) -> io::Result<()> {
    if id.len() > MAX_ID {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "interview id is {} bytes, over the {MAX_ID}-byte limit",
                id.len()
            ),
        ));
    }
    Ok(())
}

/// Reject a caller-supplied field over [`MAX_FIELD`]. Named, so the error says which one.
fn check_field(name: &str, text: &str) -> io::Result<()> {
    if text.len() as u64 > MAX_FIELD {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "interview {name} is {} bytes, over the {MAX_FIELD}-byte limit",
                text.len()
            ),
        ));
    }
    Ok(())
}

/// Append an ask record; returns the assigned [`Ask`] (id + seq).
///
/// `seq` is the count of existing ask records (0-based) and `id` is `ask-<seq>`, so ids
/// are unique under the single-writer discipline and are `safe_component`-clean.
/// A blank `question` is rejected with `ErrorKind::InvalidInput`: an unanswerable record
/// is worse than an error, because it occupies a `seq` and renders as an empty prompt. So
/// is any field over [`MAX_FIELD`] — the log is append-only, so an oversized record is
/// permanent, and every later `read` pays for it — and so is a `recommend` that names no
/// offered option.
///
/// `ErrorKind::AlreadyExists` if the minted id is already in the log, which is what a
/// non-contiguous set of ask ids produces. The alternative is a question the caller
/// believes it asked and the fold drops.
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
    check_field("question", question)?;
    if let Some(c) = context {
        check_field("context", c)?;
    }
    if let Some(r) = recommend {
        check_field("recommend", r)?;
    }
    for o in options {
        check_field("option value", &o.value)?;
        check_field("option label", &o.label)?;
    }
    check_options(options)
        .map_err(|why| io::Error::new(io::ErrorKind::InvalidInput, format!("drovr ask: {why}")))?;
    if !recommend_is_offered(recommend, options) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "drovr ask: --recommend names a value that is not one of --option's",
        ));
    }

    let existing = read(run_dir)?;
    let seq = u32::try_from(existing.len()).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "interview.jsonl: more asks than a u32 seq can number",
        )
    })?;
    let id = format!("ask-{seq}");
    // `seq` counts folded asks, so a log whose ask ids are not contiguous — a line
    // hand-deleted, or a second writer breaking the single-writer discipline — can produce
    // an id that is already taken. The fold keeps the FIRST ask per id, so writing it would
    // register a question nobody ever sees while this call returned Ok. Refuse instead: a
    // loud failure is recoverable, a silently dropped question is not.
    if existing.iter().any(|a| a.id == id) {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!(
                "interview.jsonl already carries {id}; the log's ids are not contiguous, \
                 so this ask cannot be registered"
            ),
        ));
    }

    let ask = Ask {
        id,
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
/// entitled to make, and the fold treats the id as answered from then on. One over
/// [`MAX_FIELD`] is not, for the reason in [`append_ask`].
pub fn append_answer(run_dir: &Path, id: &str, answer: &str) -> io::Result<()> {
    check_id(id)?;
    check_field("answer", answer)?;
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
    fn append_ask_rejects_an_option_with_an_empty_value_or_label() {
        // The same argument as the blank question above, and it has to live HERE: the
        // CLI's `<value>=<label>` split happens to reject these today, but that is the
        // CLI's string parsing, not this module's rule — and this module is what the
        // server and every future caller reach through. An empty label renders as a
        // blank button; an empty value is what the answer would record.
        let d = tempfile::tempdir().unwrap();
        for bad in [("", "Arm A"), ("a", ""), ("  ", "Arm A"), ("a", "\t")] {
            let opts = [AskOption {
                value: bad.0.into(),
                label: bad.1.into(),
            }];
            let err = append_ask(d.path(), "which arm?", None, &opts, None)
                .expect_err(&format!("must refuse {bad:?}"));
            assert_eq!(err.kind(), io::ErrorKind::InvalidInput, "{bad:?}: {err}");
        }
        assert!(
            !log_path(d.path()).exists(),
            "a rejected ask creates no log"
        );
        // A value or label that merely CONTAINS space is fine — only blank is not.
        let ok = [AskOption {
            value: "arm b".into(),
            label: "Arm B".into(),
        }];
        append_ask(d.path(), "which arm?", None, &ok, None).expect("non-blank");
    }

    #[test]
    fn a_line_offering_an_empty_option_value_or_label_is_skipped() {
        // The write-side rule applied on read, so a hand-edit cannot smuggle one in.
        for line in [
            r#"{"id":"ask-0","seq":0,"question":"q","options":[{"value":"","label":"A"}]}"#,
            r#"{"id":"ask-0","seq":0,"question":"q","options":[{"value":"a","label":" "}]}"#,
        ] {
            let d = tempfile::tempdir().unwrap();
            std::fs::write(log_path(d.path()), format!("{line}\n")).unwrap();
            assert_eq!(read(d.path()).unwrap(), Vec::new(), "line={line}");
        }
    }

    #[test]
    fn no_option_rule_echoes_the_value_it_rejected() {
        // The same discipline as `check_id`, which bounds an id "without echoing it".
        // `check_options` is reached BEFORE `check_field` on the CLI path — it is what
        // `validate_ask_request` calls to fail fast, and the size caps only run later,
        // inside `append_ask` — so an echoing message here is unbounded. T4 will call
        // this from an HTTP handler, where the echo would be a response body mirroring
        // a request's own field back at whoever sent it.
        let huge = "x".repeat(100_000);
        let opts = [
            AskOption {
                value: huge.clone(),
                label: "A".into(),
            },
            AskOption {
                value: huge.clone(),
                label: "B".into(),
            },
        ];
        let why = check_options(&opts).expect_err("duplicate");
        assert!(!why.contains(&huge), "the message echoes the value: {why}");
        assert!(why.len() < 500, "message is {} bytes", why.len());
        // It still says which rule was broken.
        assert!(why.contains("share"), "{why}");
    }

    #[test]
    fn append_ask_rejects_two_options_sharing_a_value() {
        let d = tempfile::tempdir().unwrap();
        let dup = [
            AskOption {
                value: "a".into(),
                label: "Arm A".into(),
            },
            AskOption {
                value: "a".into(),
                label: "Arm B".into(),
            },
        ];
        // `value` is the key an answer is recorded as, so two options sharing one make
        // the answer ambiguous: nothing downstream can say which the human picked, and
        // `recommend` pre-selects an entry nobody can name. The log is append-only, so
        // an ambiguous question written once is ambiguous forever.
        let err = append_ask(d.path(), "which arm?", None, &dup, Some("a")).expect_err("dup");
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput, "{err}");
        assert!(
            !log_path(d.path()).exists(),
            "a rejected ask creates no log"
        );
        // Duplicate LABELS are fine — only the value is a key.
        let same_label = [
            AskOption {
                value: "a".into(),
                label: "Arm".into(),
            },
            AskOption {
                value: "b".into(),
                label: "Arm".into(),
            },
        ];
        append_ask(d.path(), "which arm?", None, &same_label, None).expect("distinct values");
    }

    #[test]
    fn a_line_offering_two_options_with_one_value_is_skipped() {
        // The write-side rule, applied on read: such a line can only be a hand-edit,
        // and admitting it would make the ambiguity real for every later reader.
        let d = tempfile::tempdir().unwrap();
        std::fs::write(
            log_path(d.path()),
            br#"{"id":"ask-0","seq":0,"question":"q","options":[{"value":"a","label":"A"},{"value":"a","label":"B"}]}
"#,
        )
        .unwrap();
        assert_eq!(read(d.path()).unwrap(), Vec::new());
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
    fn an_answer_without_a_timestamp_is_skipped() {
        // Keeps `answer.is_some() <=> answered_at.is_some()` true for every folded Ask.
        let d = tempfile::tempdir().unwrap();
        append_ask(d.path(), "q?", None, &[], None).unwrap();
        let mut f = OpenOptions::new()
            .append(true)
            .open(log_path(d.path()))
            .unwrap();
        f.write_all(b"{\"id\":\"ask-0\",\"seq\":0,\"answer\":\"untimed\"}\n")
            .unwrap();
        drop(f);

        let folded = read(d.path()).unwrap();
        assert_eq!(folded[0].answer, None, "{folded:?}");
        assert_eq!(folded[0].answered_at, None);
        assert_eq!(pending(&folded), vec!["ask-0"], "still pending");
    }

    #[test]
    fn a_wire_shaped_ask_is_not_a_valid_log_line() {
        // The one mistake the shared `Ask` type invites: serializing the wire view back
        // into the log. It carries `question` and a null `answer`, so the fold rejects it.
        let d = tempfile::tempdir().unwrap();
        let ask = append_ask(d.path(), "real?", None, &[], None).unwrap();
        let mut wire = ask.clone();
        wire.id = "ask-1".into();
        wire.seq = 1;
        let mut f = OpenOptions::new()
            .append(true)
            .open(log_path(d.path()))
            .unwrap();
        f.write_all(format!("{}\n", serde_json::to_string(&wire).unwrap()).as_bytes())
            .unwrap();
        drop(f);

        let folded = read(d.path()).unwrap();
        assert_eq!(
            folded.len(),
            1,
            "the wire-shaped line is skipped: {folded:?}"
        );
        assert_eq!(folded[0].id, "ask-0");
    }

    #[test]
    fn a_symlinked_log_is_refused_by_both_read_and_append() {
        // A link planted at the path would make an append write through to its target.
        let d = tempfile::tempdir().unwrap();
        let target = d.path().join("elsewhere.txt");
        fs::write(&target, "secrets\n").unwrap();
        let run = d.path().join("run");
        fs::create_dir(&run).unwrap();
        std::os::unix::fs::symlink(&target, log_path(&run)).unwrap();

        let read_err = read(&run).expect_err("read through a symlink");
        assert_eq!(read_err.kind(), io::ErrorKind::InvalidData, "{read_err}");
        let write_err = append_ask(&run, "q?", None, &[], None).expect_err("append");
        assert_eq!(write_err.kind(), io::ErrorKind::InvalidData, "{write_err}");
        assert_eq!(
            fs::read_to_string(&target).unwrap(),
            "secrets\n",
            "the link's target was not written through"
        );
    }

    #[test]
    fn an_oversized_field_is_refused_rather_than_appended_forever() {
        let d = tempfile::tempdir().unwrap();
        let huge = "x".repeat(MAX_FIELD as usize + 1);
        let err = append_ask(d.path(), &huge, None, &[], None).expect_err("oversized");
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput, "{err}");
        assert!(!log_path(d.path()).exists(), "nothing was appended");

        let ask = append_ask(d.path(), "q?", None, &[], None).unwrap();
        let err = append_answer(d.path(), &ask.id, &huge).expect_err("oversized answer");
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput, "{err}");
        assert_eq!(read_lines(d.path()).len(), 1, "still just the ask");
    }

    #[test]
    fn an_append_that_would_exceed_the_log_cap_is_refused() {
        // Round 2: per-field caps did not bound a RECORD. Enough in-bounds options make one
        // line larger than the whole-log cap, and since `read` refuses an over-cap log, the
        // append would have bricked the interview — permanently, the log being append-only.
        let d = tempfile::tempdir().unwrap();
        let good = append_ask(d.path(), "q?", None, &[], None).unwrap();
        let big = "x".repeat(MAX_FIELD as usize);
        let options: Vec<AskOption> = (0..=(MAX_LOG / MAX_FIELD))
            .map(|i| AskOption {
                value: format!("v{i}"),
                label: big.clone(),
            })
            .collect();

        let err = append_ask(d.path(), "too big?", None, &options, None).expect_err("over cap");
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput, "{err}");
        assert_eq!(read_lines(d.path()).len(), 1, "nothing was appended");
        assert_eq!(read(d.path()).unwrap()[0].id, good.id, "log still readable");
    }

    #[test]
    fn append_answer_rejects_an_over_long_id_before_looking_it_up() {
        // The id reaches this module from an HTTP body (T4). An unbounded one has no
        // business being carried into a lookup, let alone into an error message.
        let d = tempfile::tempdir().unwrap();
        append_ask(d.path(), "q?", None, &[], None).unwrap();
        let err = append_answer(d.path(), &"a".repeat(MAX_ID + 1), "x").expect_err("long id");
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput, "{err}");
        assert!(
            err.to_string().len() < 200,
            "the id must not be echoed whole: {err}"
        );
    }

    #[test]
    fn a_recommendation_must_name_one_of_the_offered_options() {
        let d = tempfile::tempdir().unwrap();
        let opts = vec![AskOption {
            value: "a".into(),
            label: "Alpha".into(),
        }];
        let err = append_ask(d.path(), "which?", None, &opts, Some("z")).expect_err("not offered");
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput, "{err}");
        assert!(!log_path(d.path()).exists(), "nothing was appended");

        append_ask(d.path(), "which?", None, &opts, Some("a")).expect("an offered value");
        // With no options at all, a recommendation is a free-text suggestion, not a choice.
        append_ask(d.path(), "free?", None, &[], Some("anything")).expect("no options");
    }

    #[test]
    fn an_ask_is_refused_rather_than_minted_onto_an_id_already_in_the_log() {
        // Round 4: `seq` counts folded asks, so a log whose ids are not contiguous (a line
        // hand-deleted, a second writer) can mint an id that already exists — and the fold
        // keeps the FIRST ask per id, so the new question would be dropped while
        // `append_ask` returned Ok. A question the agent believes it asked and the human
        // never sees is the worst outcome available here; fail instead.
        let d = tempfile::tempdir().unwrap();
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(log_path(d.path()))
            .unwrap();
        for line in [
            r#"{"id":"ask-1","seq":1,"question":"one?"}"#,
            r#"{"id":"ask-2","seq":2,"question":"two?"}"#,
        ] {
            f.write_all(format!("{line}\n").as_bytes()).unwrap();
        }
        drop(f);

        let err = append_ask(d.path(), "new?", None, &[], None).expect_err("id collision");
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists, "{err}");
        assert_eq!(read_lines(d.path()).len(), 2, "nothing was appended");
    }

    #[test]
    fn an_ask_line_recommending_an_unoffered_value_is_skipped() {
        // The invariant `Ask` documents has to hold for every Ask this module hands out,
        // not only for the ones it minted — otherwise the doc is a wish.
        let d = tempfile::tempdir().unwrap();
        append_ask(d.path(), "real?", None, &[], None).unwrap();
        let mut f = OpenOptions::new()
            .append(true)
            .open(log_path(d.path()))
            .unwrap();
        f.write_all(
            br#"{"id":"ask-1","seq":1,"question":"which?","options":[{"value":"a","label":"A"}],"recommend":"z"}"#,
        )
        .unwrap();
        f.write_all(b"\n").unwrap();
        drop(f);

        let folded = read(d.path()).unwrap();
        assert_eq!(
            folded.len(),
            1,
            "the inconsistent ask is skipped: {folded:?}"
        );
        assert_eq!(folded[0].id, "ask-0");
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
