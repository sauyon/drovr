#!/usr/bin/env python3
"""Build the tier-1 scorer prompts for one fixture, from `PROTOCOL-2.md` itself.

Every part of a prompt is **extracted, never transcribed**: item 10's template is
the fenced block of item 10, item 9's two blockquotes are the two blockquotes of
item 9, and the ledger rows are the ledger's own lines. A hand-typed prompt is a
prompt that can drift from the frozen instrument between one arm and the next.

Each prompt is written into its **own directory** so a scorer cannot list its
siblings and recover which generations share anything. Prompts are written OUTSIDE
the repository: a scorer runs with a working directory inside the worktree, and
`v1`'s `RESULTS.md` §7.6 deviation 7 is the record of what a dispatch that can
read the run's own working material costs, and §5 deviation 4 is the leak whose control
§7.6 deviation 7 describes applying. (§5 deviation 1 is the feasibility-generation choice and has nothing
to do with this; the miscitation is corrected here.)

What a scorer is handed is fixed by item 10 and is **only** these: the generated
spec (by absolute path), its own shard's ledger rows, and the item-8 schema plus
item-9 definition that the template carries. Never a map, never an arm, never
`PROTOCOL-2.md`, never another generation.

Usage: build-tier1-prompts.py <out-dir> <fixture> [<id> ...]      (default: all ids of that fixture)

Two placeholder notes, because item 10's template writes `<k>` for two different
things and a reader should not have to guess which reading ran:
  - "Below are <k> ledger rows" is substituted with the shard's ROW COUNT.
  - `<id>-<k>.json` and `"shard":<k>` are substituted with the 1-based SHARD NUMBER.
That is the only reading under which both sentences are true.

A record of what ran, committed beside the artifact.
"""
import json
import pathlib
import re
import sys

EV = pathlib.Path(__file__).resolve().parent.parent
PROTO = (EV / "PROTOCOL-2.md").read_text()

# Item 10's shard table. Fixed there, and NEVER adjusted per arm.
SHARDS = {
    "skill-stickiness": [(1, 1, 46), (2, 47, 91)],
    "tiered-review": [(1, 1, 42), (2, 43, 84)],
    "tui-dc-picker": [(1, 1, 55)],
}
EXPECTED_ROWS = {"skill-stickiness": 91, "tiered-review": 84, "tui-dc-picker": 55}


def item(n):
    """The body of `## <n>. …` up to the next `## ` heading."""
    m = re.search(rf"^## {re.escape(n)}\. .*?$(.*?)(?=^## )", PROTO, re.M | re.S)
    assert m, f"`PROTOCOL-2.md` item {n} not found"
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
ITEM9 = "\n\n".join(
    "\n".join(("> " + l).rstrip() for l in q.splitlines()) for q in qs
)

fences = re.findall(r"^```\n(.*?)^```$", item("10"), re.M | re.S)
assert len(fences) == 1, f"item 10 has {len(fences)} fenced blocks, expected exactly 1"
TEMPLATE = fences[0]

# **Item 10's template is not the whole of what item 10 hands over**, and reading
# it as though it were is the delivery defect this script was written with. Item
# 10's own prose: *"The scorer is handed only the generated spec file, its
# fixture's ledger rows for its shard, and the item-8 schema plus the item-9
# definition."* `plan.md` T5a says the same in fewer words — "items 8 + 9". The
# template carries item 9 through an explicit placeholder and carries item 8 only
# as a one-sentence paraphrase ("begin at the start of a sentence, table cell,
# list item, heading or line-block"), which is NOT the character-level predicate
# the gate actually runs.
#
# So item 8 is handed over too, verbatim and entire, appended after the template.
# **The template itself is not edited** — it is delivered byte-identical, and this
# is an addition to the handover, not a rewrite of the frozen text. Nor is it
# steering: no guidance is invented, the same frozen bytes go to every shard of
# every generation, and a rule the protocol says the scorer receives is the
# opposite of a hint aimed at one arm.
ITEM8 = item("8").strip("\n")


def main(argv):
    if len(argv) < 3:
        sys.exit(__doc__)
    out_dir, fixture = pathlib.Path(argv[1]), argv[2]
    if fixture not in SHARDS:
        sys.exit(f"unknown fixture {fixture!r}; expected one of {sorted(SHARDS)}")

    rows = [
        l for l in (EV / "ledger" / f"{fixture}.md").read_text().splitlines()
        if l.startswith(f"| {fixture}-")
    ]
    assert len(rows) == EXPECTED_ROWS[fixture], \
        f"{len(rows)} ledger rows for {fixture}, expected {EXPECTED_ROWS[fixture]}"
    by_n = {int(re.match(rf"\| {fixture}-(\d+) \|", l).group(1)): l for l in rows}
    assert sorted(by_n) == list(range(1, len(rows) + 1)), "ledger ids are not 1..n"

    fixture_map = json.loads((EV / "fixture-map-2.json").read_text())
    ids = argv[3:] or sorted(i for i, f in fixture_map.items() if f == fixture)
    for spec_id in ids:
        if fixture_map.get(spec_id) != fixture:
            sys.exit(f"{spec_id} is not a {fixture} generation")

    built = []
    for spec_id in ids:
        for k, lo, hi in SHARDS[fixture]:
            doc = EV / "generated-2" / f"{spec_id}.md"
            dest = EV / "retention-2" / "parts" / f"{spec_id}-{k}.json"
            p = TEMPLATE
            p = p.replace("<abs path to generated-2/<id>.md>", str(doc))
            p = p.replace("Below are <k> ledger rows", f"Below are {hi - lo + 1} ledger rows")
            p = p.replace("<item 9's two blockquotes, verbatim>", ITEM9)
            p = p.replace("<the shard's rows, verbatim from the ledger>",
                          "\n".join(by_n[n] for n in range(lo, hi + 1)))
            p = p.replace("<abs path to retention-2/parts/<id>-<k>.json>", str(dest))
            p = p.replace('"spec_id": "<id>", "ledger": "<fixture>"',
                          f'"spec_id": "{spec_id}", "ledger": "{fixture}"')
            p = p.replace('{"spec_id":"<id>","shard":<k>,"wrote":"<path>","ok":true}',
                          f'{{"spec_id":"{spec_id}","shard":{k},'
                          f'"wrote":"{dest}","ok":true}}')
            # `<row id>` and `<span>` are the template's own illustrative
            # placeholders inside the example object and stay as they are.
            for leftover in ("<abs path", "<fixture>", "<path>", '"<id>"', "<k>"):
                assert leftover not in p, f"unsubstituted {leftover!r} in {spec_id}-{k}"
            p = (
                f"{p}\n"
                "--- BEGIN THE ITEM-8 SCHEMA, WHICH GOVERNS AND IS NOT A SUMMARY ---\n"
                f"{ITEM8}\n"
                "--- END THE ITEM-8 SCHEMA ---\n"
            )
            d = out_dir / f"{spec_id}-{k}"
            d.mkdir(parents=True, exist_ok=True)
            (d / "prompt.txt").write_text(p)
            built.append(str(d / "prompt.txt"))

    print("\n".join(built))
    print(f"\n{len(built)} prompt(s) for {fixture}; "
          f"item-9 blockquotes {len(ITEM9)} bytes, item-10 template {len(TEMPLATE)} bytes")


main(sys.argv)
