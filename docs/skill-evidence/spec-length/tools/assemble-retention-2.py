#!/usr/bin/env python3
"""Assemble `retention-2/<id>.json` from its `retention-2/parts/<id>-<k>.json` shards.

`PROTOCOL-2.md` item 10: **concatenation is assembly, not repair.** This script
concatenates the shards' `rows` in ledger order and does nothing else. It never
edits a row, never fills a gap, never drops a duplicate and never rewrites a span.
If a shard does not have item 8's closed shape over exactly its own shard's ids in
ledger order, it REFUSES and names the shard — a malformed shard is re-run whole
under `R6`, never patched.

The authority on whether an assembled verdict is well-formed is
`cli/tests/skills_valid.rs::spec_length_2_retention_verdicts_are_complete_and_quoted`,
not this script. Nothing here checks a span: this script cannot see the generated
spec and does not open it. It checks only what it must in order to know that the
concatenation it is about to perform is a concatenation.

Usage: assemble-retention-2.py <fixture> <id> [<id> ...]

A record of what ran, committed beside the artifact. Paths are resolved relative to
this file's own location, so it runs from anywhere inside a checkout.
"""
import json
import pathlib
import re
import sys

EV = pathlib.Path(__file__).resolve().parent.parent
RET = EV / "retention-2"

# Item 10's shard table. Never adjusted per arm.
SHARDS = {
    "skill-stickiness": [(1, 1, 46), (2, 47, 91)],
    "tiered-review": [(1, 1, 42), (2, 43, 84)],
    "tui-dc-picker": [(1, 1, 55)],
}


def ledger_ids(fixture):
    text = (EV / "ledger" / f"{fixture}.md").read_text()
    ids = [
        l.split("|")[1].strip()
        for l in text.splitlines()
        if l.startswith(f"| {fixture}-")
    ]
    assert ids, f"no rows parsed out of the {fixture} ledger"
    return ids


def refuse(msg):
    sys.exit(
        f"REFUSING to assemble: {msg}\n"
        "A malformed shard is re-run WHOLE under `R6` and logged; it is never patched, "
        "and this script will not paper over it."
    )


def assemble(fixture, spec_id):
    ids = ledger_ids(fixture)
    by_n = {int(re.match(rf"{fixture}-(\d+)$", i).group(1)): i for i in ids}
    rows = []
    for k, lo, hi in SHARDS[fixture]:
        path = RET / "parts" / f"{spec_id}-{k}.json"
        if not path.exists():
            refuse(f"{path} does not exist")
        try:
            shard = json.loads(path.read_text())
        except json.JSONDecodeError as e:
            refuse(f"{path} is not JSON: {e}")
        if set(shard) != {"spec_id", "ledger", "rows"}:
            refuse(f"{path} has keys {sorted(shard)}, expected spec_id/ledger/rows")
        if shard["spec_id"] != spec_id:
            refuse(f"{path} carries spec_id {shard['spec_id']!r}, expected {spec_id!r}")
        if shard["ledger"] != fixture:
            refuse(f"{path} carries ledger {shard['ledger']!r}, expected {fixture!r}")
        want = [by_n[n] for n in range(lo, hi + 1)]
        got = [r.get("id") for r in shard["rows"]]
        if got != want:
            refuse(
                f"{path} does not cover exactly its own shard's ids in ledger order "
                f"({len(got)} row(s); first divergence at index "
                f"{next((i for i, (a, b) in enumerate(zip(got, want)) if a != b), min(len(got), len(want)))})"
            )
        rows.extend(shard["rows"])

    if [r["id"] for r in rows] != ids:
        refuse(f"the concatenated rows are not the {fixture} ledger in order")

    out = RET / f"{spec_id}.json"
    out.write_text(json.dumps({"spec_id": spec_id, "ledger": fixture, "rows": rows}, indent=2) + "\n")
    present = sum(1 for r in rows if r.get("present") is True)
    spans = sum(len(r.get("quotes") or []) for r in rows)
    print(f"{out}  rows={len(rows)}  present={present}  spans={spans}")


def main(argv):
    if len(argv) < 3:
        sys.exit(__doc__)
    fixture = argv[1]
    if fixture not in SHARDS:
        sys.exit(f"unknown fixture {fixture!r}; expected one of {sorted(SHARDS)}")
    for spec_id in argv[2:]:
        assemble(fixture, spec_id)


main(sys.argv)
