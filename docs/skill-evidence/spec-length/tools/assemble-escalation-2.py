#!/usr/bin/env python3
"""Assemble `escalation-2/<id>.json` from one tier-3 escalator's reply.

Item 8b's escalator returns a bare JSON array of
`{"n":…,"present":…,"reason":…}` and is never told which ledger rows it judged —
it is shown their text, not their ids. **The join back onto row ids is this
tool's job**, and it reads the flagged set from the `rows.json` the prompt
builder wrote beside each prompt, so the ids joined here are exactly the ids
whose text was sent.

**That join is then checked twice more against sources this file does not
control**: `spec_length_2_final_disposition_is_the_recorded_join` recomputes the
flagged set from `adjudication-2/` and from the item-8 checker's class-B output
and requires `rows` to be exactly it, and `final_disposition` requires every
escalated row to be one its tier-1 verdict actually scored.

**A generation with no flagged row gets no file, not an empty one.** Item 8b
makes the ABSENCE of `escalation-2/<id>.json` the record that nothing was
flagged, and an empty-`rows` file would say something different.

**The reply is hashed as it is read**, for the same reason the tier-1 and tier-2
assemblers hash theirs: `SCORING-2-NOTES.md` §9.6 could not distinguish a
re-emission from a cached completion, and an unhashed reply cannot even raise
the question.

**Refusals are hard.** A reply whose length, `n` sequence or shape does not match
the flagged set is reported, not patched — the remedy is `R6`'s, a whole re-run
of that dispatch.

Usage: assemble-escalation-2.py <prompts-dir> <replies-dir> <id> [<id> ...]
"""
import hashlib
import json
import pathlib
import re
import sys

EV = pathlib.Path(__file__).resolve().parent.parent


def parse_reply(text, whose):
    """The escalator's one line, as a list. A ```json fence is unwrapped.

    Unwrapping a fence changes no judgement — the array inside is
    byte-identical. Anything else is refused rather than coaxed.
    """
    stripped = text.strip()
    m = re.fullmatch(r"```(?:json)?\s*(.*?)\s*```", stripped, re.S)
    if m:
        stripped = m.group(1).strip()
    try:
        return json.loads(stripped)
    except json.JSONDecodeError as e:
        sys.exit(f"{whose}: the reply is not JSON ({e}). Re-run the dispatch under `R6`.")


def main(argv):
    if len(argv) < 4:
        sys.exit(__doc__)
    prompts, replies = pathlib.Path(argv[1]), pathlib.Path(argv[2])
    out_dir = EV / "escalation-2"
    out_dir.mkdir(exist_ok=True)

    log = []
    for spec_id in argv[3:]:
        meta = json.loads((prompts / spec_id / "rows.json").read_text())
        flagged = meta["rows"]
        assert flagged, f"{spec_id}: an empty flagged set must produce NO file"

        reply_path = replies / f"{spec_id}.json"
        raw = reply_path.read_bytes()
        digest = hashlib.sha256(raw).hexdigest()
        answers = parse_reply(raw.decode(), reply_path)

        if not isinstance(answers, list):
            sys.exit(f"{reply_path}: the reply is {type(answers).__name__}, not an array")
        if len(answers) != len(flagged):
            sys.exit(
                f"{reply_path}: {len(answers)} answer(s) for {len(flagged)} flagged row(s). "
                "The rows and the answers are joined on position, so a short or long reply "
                "cannot be joined at all. Re-run the dispatch whole under `R6`."
            )

        rows = []
        for i, (answer, row) in enumerate(zip(answers, flagged), 1):
            if sorted(answer) != ["n", "present", "reason"]:
                sys.exit(f"{reply_path}: answer {i} has keys {sorted(answer)}")
            if answer["n"] != i:
                sys.exit(
                    f"{reply_path}: answer at position {i} carries `n` = {answer['n']}; "
                    "a renumbered reply is not joinable."
                )
            if not isinstance(answer["present"], bool):
                sys.exit(f"{reply_path}: answer {i}'s `present` is not a boolean")
            reason = answer["reason"]
            if not isinstance(reason, str) or not reason.strip():
                sys.exit(
                    f"{reply_path}: answer {i} has no reason. Item 8b records tier 3's call "
                    "WITH its reason; an overturn with no reason given does not parse."
                )
            rows.append({"id": row, "present": answer["present"], "reason": reason})

        record = {"spec_id": spec_id, "ledger": meta["ledger"], "rows": rows}
        (out_dir / f"{spec_id}.json").write_text(json.dumps(record, indent=2) + "\n")
        absent = [r["id"] for r in rows if not r["present"]]
        log.append(f"{spec_id}  {digest}  {len(rows)} escalated, "
                   f"{len(absent)} answered present:false: {absent or '-'}")

    print("\n".join(log))


main(sys.argv)
