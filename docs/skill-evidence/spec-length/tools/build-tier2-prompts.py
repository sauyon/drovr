#!/usr/bin/env python3
"""Build the tier-2 relevance-adjudication prompts — `PROTOCOL-2.md` item 8a.

One prompt per generation. The adjudicator is handed `(ledger row text, the
cited spans)` pairs **and nothing else: not the generated spec, not the arm, not
the other rows** — which is item 8a's whole design, because a judge that never
sees the document cannot be swayed by how good the document looks.

**The prompt template is extracted from the frozen file, never transcribed**, and
**nothing of this builder's own is appended to it.** `SCORING-3-NOTES.md` §5.1 is
the reason that sentence is here: the first tier-1 build of this run wrapped the
frozen sections in framing of its own, the framing measurably moved scorer
behaviour, and the whole pass had to be re-run. Item 8a's prompt is complete on
its own — it is handed one kind of frozen text rather than three contradicting
ones — so there is nothing here to disambiguate and nothing is added.

**The sample is item 8a's stride rule, recomputed from the verdict**: every 5th
`present: true` row in ledger order, 1-based among `present: true` rows.
`floor(n/5)` of `n`. Fewer than five `present: true` rows gives an EMPTY sample,
and item 8a forbids re-striding to manufacture one — such a generation gets no
prompt, and `"pairs": []` is recorded against it.

**The tier-1 record is `retention-3/`**, not `retention-2/`: `PROTOCOL-3.md` §5
re-scores all eighteen generations into it and `retention-2/` is "not edited, not
deleted, and not re-interpreted".

**Spans are joined with ` | ` and their own newlines are preserved.** A span is
frequently multi-line — item 8's self-containment rule makes multi-line spans the
normal case on hard-wrapped prose — so the `SPANS:` line is not always one line.
Item 8a pins the join format and calls the three placeholders "a *join format* —
spans separated by ` | ` — and not a cap"; re-flowing a span to fit one line
would change the text being judged, which is the one thing this pass must not do.

Usage: build-tier2-prompts.py <out-dir> [<id> ...]      (default: all 18 ids)
"""
import json
import pathlib
import re
import sys

EV = pathlib.Path(__file__).resolve().parent.parent
PROTO = (EV / "PROTOCOL-2.md").read_text()

SAMPLE_RULE = "every 5th present:true row in ledger order, 1-based among present:true rows"


def item(n):
    """The body of `## <n>. …` up to the next `## ` heading."""
    m = re.search(rf"^## {re.escape(n)}\. .*?$(.*?)(?=^## )", PROTO, re.M | re.S)
    assert m, f"item {n} not found"
    return m.group(1)


fences = re.findall(r"^```\n(.*?)^```$", item("8a"), re.M | re.S)
assert len(fences) == 1, f"item 8a has {len(fences)} fenced blocks, expected exactly 1"
TEMPLATE = fences[0]
assert "one to\nfive spans" in TEMPLATE, (
    "item 8a's prompt no longer states the five-span cap; the substitution "
    "`one to three spans` -> `one to five spans` is the one change this file is "
    "authorised to carry and it must be present in the frozen text already"
)

PAIR_PLACEHOLDER = "<n>. ROW: <ledger row item text>\n    SPANS: <span 1> | <span 2> | <span 3>\n"
assert PAIR_PLACEHOLDER in TEMPLATE, "item 8a's pair placeholder is not where it was"


def ledger_items(fixture):
    """`id -> item text` for one fixture's frozen ledger."""
    out = {}
    for line in (EV / "ledger" / f"{fixture}.md").read_text().splitlines():
        if not line.startswith(f"| {fixture}-"):
            continue
        cells = [c.strip() for c in line.strip().strip("|").split(" | ")]
        assert len(cells) == 3, f"{fixture}: {len(cells)} cells in {line!r}"
        out[cells[0]] = cells[2]
    return out


def sample_of(verdict):
    """Item 8a's stride rule, recomputed. `floor(n/5)` of `n`."""
    present = [r["id"] for r in verdict["rows"] if r["present"]]
    return [row for i, row in enumerate(present, 1) if i % 5 == 0]


def main(argv):
    if len(argv) < 2:
        sys.exit(__doc__)
    out_dir = pathlib.Path(argv[1])
    fixture_map = json.loads((EV / "fixture-map-2.json").read_text())
    ids = argv[2:] or sorted(fixture_map)

    built, empty = [], []
    for spec_id in ids:
        fixture = fixture_map[spec_id]
        verdict = json.loads((EV / "retention-3" / f"{spec_id}.json").read_text())
        assert verdict["spec_id"] == spec_id, f"{spec_id}: verdict names {verdict['spec_id']}"
        assert verdict["ledger"] == fixture, f"{spec_id}: verdict ledger {verdict['ledger']}"
        quotes = {r["id"]: r["quotes"] for r in verdict["rows"]}
        rows = ledger_items(fixture)

        sample = sample_of(verdict)
        if not sample:
            empty.append(spec_id)
            continue

        pairs = []
        for n, row_id in enumerate(sample, 1):
            spans = quotes[row_id]
            assert spans, f"{spec_id}/{row_id}: a `present: true` row with no spans"
            pairs.append(f"{n}. ROW: {rows[row_id]}\n    SPANS: {' | '.join(spans)}\n")
        p = TEMPLATE.replace(PAIR_PLACEHOLDER, "".join(pairs))

        # Nothing may leak the identity of the document or the arm: item 8a's
        # adjudicator judges row text against spans and is shown nothing else.
        #
        # **`blind-map` alone is deliberately NOT on this list**, and the reason
        # is worth recording. `tiered-review-52` is the ledger row "There is no
        # `blind-map.json`; the dev/held-out split is the sole load-bearing
        # control…" — the FIXTURE spec has a blind map of its own, and the
        # collision with this experiment's `blind-map-2.json` is coincidental.
        # A guard on the bare substring refuses a legitimate ledger row, so the
        # guard names the artifact that must not leak.
        for leaked in (spec_id, "generated-2", "blind-map-2", "fixture-map-2"):
            assert leaked not in p, f"{spec_id}: prompt leaks {leaked!r}"
        # **The substitution is checked against the placeholder itself, not
        # against fragments of it.** `skill-stickiness-61` is the ledger row
        # "Scenario prompts are checked in at
        # `skills/writing-skills/scenarios/<skill>-<n>.md`" — a bare `<n>` guard
        # refuses a real row. The claim worth checking is that item 8a's one
        # pair placeholder was replaced, and that is exact.
        assert PAIR_PLACEHOLDER not in p, f"{spec_id}: the pair placeholder survived"
        assert p.count("--- BEGIN PAIRS ---") == 1 and p.count("--- END PAIRS ---") == 1, \
            f"{spec_id}: the PAIRS delimiters are not intact"

        d = out_dir / spec_id
        d.mkdir(parents=True, exist_ok=True)
        (d / "prompt.txt").write_text(p)
        (d / "sample.json").write_text(
            json.dumps({"spec_id": spec_id, "ledger": fixture,
                        "sample_rule": SAMPLE_RULE,
                        "sample": sample}, indent=2) + "\n"
        )
        built.append(str(d / "prompt.txt"))

    print("\n".join(built))
    print(f"\n{len(built)} prompt(s); {len(empty)} generation(s) sampled empty: {empty}")


main(sys.argv)
