#!/usr/bin/env python3
"""Assemble `adjudication-2/<id>.json` from one tier-2 adjudicator's reply.

Item 8a's adjudicator returns a bare JSON array of `{"n":…,"establishes":…}` —
it is never told which ledger rows it judged, because it is not shown them by
id. **The join back onto row ids is this tool's job, and it recomputes the
sample rather than trusting anything the adjudicator or the prompt builder
wrote**: same stride rule, same source verdict, evaluated here a second time.
`spec_length_2_adjudication_sample_matches_the_stride_rule` then recomputes it a
third time from the committed record, so a mistake here is caught rather than
carried.

**The reply is hashed as it is read**, exactly as the tier-1 assembler hashes its
shards, and the hash is printed for the run log. `SCORING-2-NOTES.md` §9.6 found
one shard byte-identical to its own earlier attempt and could not tell a
re-emission from a cached completion; an unhashed reply cannot even raise the
question.

**Refusals are hard.** A reply whose length, `n` sequence or JSON shape does not
match the recomputed sample is not patched, not trimmed and not padded — it is
reported, and the remedy is `R6`'s: re-run that dispatch whole.

Usage: assemble-adjudication-2.py <replies-dir> <id> [<id> ...]
"""
import hashlib
import json
import pathlib
import re
import sys

EV = pathlib.Path(__file__).resolve().parent.parent
SAMPLE_RULE = "every 5th present:true row in ledger order, 1-based among present:true rows"


def sample_of(verdict):
    """Item 8a's stride rule, recomputed. `floor(n/5)` of `n`."""
    present = [r["id"] for r in verdict["rows"] if r["present"]]
    return [row for i, row in enumerate(present, 1) if i % 5 == 0]


def parse_reply(text, whose):
    """The adjudicator's one line, as a list. Fenced output is unwrapped.

    A model asked for "exactly one line and nothing else" sometimes wraps it in
    a ```json fence. Unwrapping a fence changes no judgement — the array inside
    is byte-identical — so it is accepted; anything else is refused rather than
    coaxed.
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
    if len(argv) < 3:
        sys.exit(__doc__)
    replies = pathlib.Path(argv[1])
    fixture_map = json.loads((EV / "fixture-map-2.json").read_text())
    out_dir = EV / "adjudication-2"
    out_dir.mkdir(exist_ok=True)

    log = []
    for spec_id in argv[2:]:
        fixture = fixture_map[spec_id]
        verdict = json.loads((EV / "retention-3" / f"{spec_id}.json").read_text())
        sample = sample_of(verdict)

        reply_path = replies / f"{spec_id}.json"
        raw = reply_path.read_bytes()
        digest = hashlib.sha256(raw).hexdigest()
        answers = parse_reply(raw.decode(), reply_path)

        if not isinstance(answers, list):
            sys.exit(f"{reply_path}: the reply is {type(answers).__name__}, not an array")
        if len(answers) != len(sample):
            sys.exit(
                f"{reply_path}: {len(answers)} answer(s) for a sample of {len(sample)}. "
                "The pairs and the answers are joined on position, so a short or long "
                "reply cannot be joined at all. Re-run the dispatch whole under `R6`."
            )

        pairs = []
        for i, (answer, row) in enumerate(zip(answers, sample), 1):
            if sorted(answer) != ["establishes", "n"]:
                sys.exit(f"{reply_path}: answer {i} has keys {sorted(answer)}")
            if answer["n"] != i:
                sys.exit(
                    f"{reply_path}: answer at position {i} carries `n` = {answer['n']}. "
                    "`n` is the 1-based position in the prompt's own list and the join "
                    "reads it; a renumbered reply is not joinable."
                )
            if not isinstance(answer["establishes"], bool):
                sys.exit(f"{reply_path}: answer {i}'s `establishes` is not a boolean")
            pairs.append({"n": i, "row": row, "establishes": answer["establishes"]})

        record = {
            "spec_id": spec_id,
            "ledger": fixture,
            "sample_rule": SAMPLE_RULE,
            "pairs": pairs,
        }
        (out_dir / f"{spec_id}.json").write_text(json.dumps(record, indent=2) + "\n")
        false_rows = [p["row"] for p in pairs if not p["establishes"]]
        log.append(f"{spec_id}  {digest}  {len(pairs)} pair(s), "
                   f"{len(false_rows)} false: {false_rows or '-'}")

    print("\n".join(log))


main(sys.argv)
