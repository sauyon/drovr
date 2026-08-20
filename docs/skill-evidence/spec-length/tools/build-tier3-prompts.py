#!/usr/bin/env python3
"""Build the tier-3 escalation prompts — `PROTOCOL-2.md` item 8b.

One prompt per generation carrying **at least one flagged row**. A generation
with no flagged row gets no prompt and no file: item 8b makes the ABSENCE of
`escalation-2/<id>.json` the record that nothing was flagged.

**The flagged set has two sources and this tool requires both.**

  1. Tier 2's `establishes: false` rows, read out of `adjudication-2/<id>.json`.
  2. `PROTOCOL-3.md` §3's **class-B** rows — a boundary-refused span, or a span
     cited for more than one row. §3 flags such a row "exactly as an
     `establishes: false` flags a row under `PROTOCOL-2.md` item 8a" and
     escalates it to item 8b.

**The class-B half is read from the checker's own output, never recomputed
here.** `SCORING-2-NOTES.md` §3b fixed exactly one implementation of item 8's
span predicate — a second one would give a second answer to what the rule
refuses — and `PROTOCOL-3.md` §1 followed the same discipline when it derived
its diagnosis from the committed refusal reports. So the caller passes the JSON
digest of `spec_length_3_class_b_flags_are_reported_and_never_fatal`'s own
stderr, and this file joins rather than judges.

**The escalator is handed the generated spec and the flagged rows' ledger text.
The tier-1 spans are withheld** — item 8b makes this a fresh reading of the spec
against the row, not a review of the scorer's evidence, and that is the whole
reason tier 3's call is allowed to govern tier 1's.

**Item 9's two blockquotes go in VERBATIM, `up to three spans` clause and all.**
`PROTOCOL-3.md` §5 drops that clause from **item 10's template** — the tier-1
scorer prompt — and §8's departure table is a closed list naming item 10 only.
Item 8b is not amended, so it is not amended here. The clause is inert for an
escalator, which is never shown a span; carrying it is the cost of taking
"verbatim" literally, and it is cheaper than a second unpinned edit to a frozen
prompt.

Usage: build-tier3-prompts.py <out-dir> <class-b.json>
"""
import json
import pathlib
import re
import sys

EV = pathlib.Path(__file__).resolve().parent.parent
PROTO = (EV / "PROTOCOL-2.md").read_text()


def item(n):
    """The body of `## <n>. …` up to the next `## ` heading."""
    m = re.search(rf"^## {re.escape(n)}\. .*?$(.*?)(?=^## )", PROTO, re.M | re.S)
    assert m, f"item {n} not found"
    return m.group(1)


def blockquotes(text):
    """Every contiguous run of `>` lines, marker stripped, in document order."""
    out, cur = [], []
    for line in text.splitlines():
        if line.startswith(">"):
            cur.append(line[2:] if line.startswith("> ") else line[1:])
        elif cur:
            out.append("\n".join(cur).strip("\n"))
            cur = []
    if cur:
        out.append("\n".join(cur).strip("\n"))
    return out


qs = blockquotes(item("9"))
assert len(qs) == 2, f"item 9 has {len(qs)} blockquotes, expected exactly 2"
ITEM9 = "\n\n".join("\n".join(("> " + l).rstrip() for l in q.splitlines()) for q in qs)

# Item 8b carries two fenced blocks: the prompt, in a bare ``` fence, and the
# output schema, in a ```json one. The pattern below matches the bare fence
# only, which is the prompt — asserted rather than assumed, because "the first
# fence" would silently become the schema if the frozen file's order ever moved.
fences = re.findall(r"^```\n(.*?)^```$", item("8b"), re.M | re.S)
assert len(fences) == 1, f"item 8b has {len(fences)} bare-fenced blocks, expected exactly 1"
TEMPLATE = fences[0]
assert "--- BEGIN SPEC ---" in TEMPLATE and "--- BEGIN ROWS ---" in TEMPLATE


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


def main(argv):
    if len(argv) != 3:
        sys.exit(__doc__)
    out_dir, class_b_path = pathlib.Path(argv[1]), pathlib.Path(argv[2])
    fixture_map = json.loads((EV / "fixture-map-2.json").read_text())
    class_b = json.loads(class_b_path.read_text())

    built, unflagged = [], []
    for spec_id in sorted(fixture_map):
        fixture = fixture_map[spec_id]
        rows = ledger_items(fixture)
        order = list(rows)

        adj = json.loads((EV / "adjudication-2" / f"{spec_id}.json").read_text())
        flagged = {p["row"] for p in adj["pairs"] if not p["establishes"]}
        flagged |= set(class_b.get(spec_id, []))
        # Ledger order, which is what item 8b's `rows` must carry.
        flagged = [r for r in order if r in flagged]
        if not flagged:
            unflagged.append(spec_id)
            continue

        spec = (EV / "generated-2" / f"{spec_id}.md").read_text()
        p = TEMPLATE
        p = p.replace("<the contents of generated-2/<id>.md>", spec.rstrip("\n"))
        p = p.replace("<item 9's two blockquotes, verbatim>", ITEM9)
        p = p.replace(
            "<n>. <ledger row item text>",
            "\n".join(f"{n}. {rows[r]}" for n, r in enumerate(flagged, 1)),
        )
        assert "<the contents of" not in p and "<item 9's" not in p
        assert p.count("--- BEGIN ROWS ---") == 1 and p.count("--- END ROWS ---") == 1

        d = out_dir / spec_id
        d.mkdir(parents=True, exist_ok=True)
        (d / "prompt.txt").write_text(p)
        (d / "rows.json").write_text(
            json.dumps({"spec_id": spec_id, "ledger": fixture, "rows": flagged}, indent=2) + "\n"
        )
        built.append(f"{spec_id}  {len(flagged)} flagged row(s)")

    print("\n".join(built))
    print(f"\n{len(built)} escalation prompt(s); "
          f"{len(unflagged)} generation(s) with NO flagged row and therefore no file: "
          f"{unflagged}")


main(sys.argv)
