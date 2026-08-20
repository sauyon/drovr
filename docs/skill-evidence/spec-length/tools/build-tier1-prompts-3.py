#!/usr/bin/env python3
"""Build the tier-1 scorer prompts for one fixture under `PROTOCOL-3.md`.

The v2 builder (`build-tier1-prompts.py`) with four differences, each of them
pre-registered in `PROTOCOL-3.md` §5 before any dispatch:

  1. The destination is `retention-3/parts/<id>-<k>.json`, not `retention-2/`.
  2. `PROTOCOL-3.md` §2 — the revised span rule, clauses F, M and E — is appended
     after item 8, so **the frozen text the scorer gets is the text the gate
     runs**. That is `SCORING-2-NOTES.md` §2a's finding: the boundary refusal
     rate fell by at least 1.9x on the matched probe when scorers were handed the
     actual rule instead of item 10's one-sentence paraphrase of it.
  3. `PROTOCOL-3.md` §3 — the class-A/class-B split — is appended too. A scorer
     that knows a shared span costs one flagged row rather than the whole verdict
     has no incentive to hide one.
  4. Item 9's *"and may be evidenced by up to three spans"* clause is **dropped**.
     Item 8 says one to five and always did; `SCORING-2-NOTES.md` §9.5a item 13
     records the contradiction, §5 pre-registers the correction, and the
     substitution below is asserted rather than assumed so a drift in the frozen
     text fails the build instead of silently shipping both numbers.

Everything else is the v2 builder unchanged, including the properties that
matter: every part is **extracted, never transcribed**; each prompt goes in its
own directory so a scorer cannot list its siblings; prompts are written OUTSIDE
the repository; and a scorer is handed only the generated spec, its own shard's
ledger rows, and the schema and definitions the template carries — never a map,
never an arm, never a protocol file in full, never another generation.

Usage: build-tier1-prompts-3.py <out-dir> <fixture> [<id> ...]    (default: all ids of that fixture)
"""
import json
import pathlib
import re
import sys

EV = pathlib.Path(__file__).resolve().parent.parent
PROTO = (EV / "PROTOCOL-2.md").read_text()
PROTO3 = (EV / "PROTOCOL-3.md").read_text()

# Item 10's shard table. Fixed there, and NEVER adjusted per arm.
SHARDS = {
    "skill-stickiness": [(1, 1, 46), (2, 47, 91)],
    "tiered-review": [(1, 1, 42), (2, 43, 84)],
    "tui-dc-picker": [(1, 1, 55)],
}
EXPECTED_ROWS = {"skill-stickiness": 91, "tiered-review": 84, "tui-dc-picker": 55}


def item(n, text=None):
    """The body of `## <n>. …` up to the next `## ` heading."""
    src = PROTO if text is None else text
    m = re.search(rf"^## {re.escape(n)}\. .*?$(.*?)(?=^## )", src, re.M | re.S)
    assert m, f"item {n} not found"
    return m.group(1)


def section3(n):
    """The body of `## <n>. …` in `PROTOCOL-3.md`, heading included."""
    m = re.search(rf"^(## {re.escape(n)}\. .*?$.*?)(?=^## )", PROTO3, re.M | re.S)
    assert m, f"`PROTOCOL-3.md` section {n} not found"
    return m.group(1).strip("\n")


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

# `PROTOCOL-3.md` §5's item-9 correction. The clause spans a hard wrap in the
# frozen file, so it is matched after the quote marker is stripped and the text
# rejoined — and it is asserted, because a silent no-op here would ship a prompt
# that states both 3 and 5 and blame the scorer for choosing.
STALE = " A paraphrase counts, and may be evidenced by up to three spans."
FIXED = " A paraphrase counts."
flat = re.sub(r"\s+", " ", qs[0])
assert STALE in flat, (
    "item 9's `up to three spans` clause was not found; `PROTOCOL-3.md` §5 "
    "pre-registers dropping it, and a builder that cannot find it must fail "
    "rather than ship the contradiction"
)
qs[0] = flat.replace(STALE, FIXED)
assert "three spans" not in qs[0]

ITEM9 = "\n\n".join(
    "\n".join(("> " + l).rstrip() for l in q.splitlines()) for q in qs
)

fences = re.findall(r"^```\n(.*?)^```$", item("10"), re.M | re.S)
assert len(fences) == 1, f"item 10 has {len(fences)} fenced blocks, expected exactly 1"
TEMPLATE = fences[0]

ITEM8 = item("8").strip("\n")
SPAN_RULE_V3 = section3("2")
DISPOSITION_V3 = section3("3")


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
            dest = EV / "retention-3" / "parts" / f"{spec_id}-{k}.json"
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
            for leftover in ("<abs path", "<fixture>", "<path>", '"<id>"', "<k>"):
                assert leftover not in p, f"unsubstituted {leftover!r} in {spec_id}-{k}"
            # **Delimiters and an ordering statement, and nothing else.** The
            # first build of this pass wrapped §2 and §3 in editorial framing of
            # its own — most damagingly a sentence telling scorers there was "NO
            # reason to ... drop a row you cannot cite cleanly, or to reuse a
            # span you have already used." §5 pre-registers §2 and §3
            # **verbatim**, and this run's own `SCORING-2-NOTES.md` §2a finding is
            # that prompt wording moves scorer behaviour measurably — so that
            # framing was an undisclosed intervention on both metrics the pass
            # then reported. `SCORING-3-NOTES.md` §5.1 records both the deviation
            # and the re-run it forced; the hashes of the prompts actually
            # dispatched are at `retention-3/parts/prompt-hashes.txt`.
            #
            # **What remains is not framing and is required.** Three frozen texts
            # are handed over and two of them disagree; a scorer that is not told
            # which governs has to guess, and item 8's own closing sentence
            # contradicts §3 on the page. Naming the precedence is the minimum
            # that makes the handover coherent, it says nothing about what to
            # score or how to cite, and it is identical for every shard.
            p = (
                f"{p}\n"
                "--- BEGIN THE ITEM-8 SCHEMA ---\n"
                f"{ITEM8}\n"
                "--- END THE ITEM-8 SCHEMA ---\n"
                "\n"
                "--- BEGIN `PROTOCOL-3.md` SECTION 2 ---\n"
                "Where this section and the item-8 schema above differ on the span boundary\n"
                "rule, this section governs.\n"
                f"{SPAN_RULE_V3}\n"
                "--- END `PROTOCOL-3.md` SECTION 2 ---\n"
                "\n"
                "--- BEGIN `PROTOCOL-3.md` SECTION 3 ---\n"
                "Where this section and the item-8 schema above differ on what a violation\n"
                "costs, this section governs.\n"
                f"{DISPOSITION_V3}\n"
                "--- END `PROTOCOL-3.md` SECTION 3 ---\n"
            )
            d = out_dir / f"{spec_id}-{k}"
            d.mkdir(parents=True, exist_ok=True)
            (d / "prompt.txt").write_text(p)
            built.append(str(d / "prompt.txt"))

    print("\n".join(built))
    print(f"\n{len(built)} prompt(s) for {fixture}; item-9 blockquotes {len(ITEM9)} bytes, "
          f"item-10 template {len(TEMPLATE)} bytes, item 8 {len(ITEM8)} bytes, "
          f"PROTOCOL-3 §2 {len(SPAN_RULE_V3)} bytes, §3 {len(DISPOSITION_V3)} bytes")


main(sys.argv)
