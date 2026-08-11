#!/usr/bin/env python3
"""What fraction of a generated spec's own complete lines are admissible spans?

The question item 8's self-containment rule raises is not "did this scorer quote
badly" but "what can be quoted at all". This measures that, and it measures it
with the AUTHORITATIVE check — `spec_length_2_retention_verdicts_are_complete_and_quoted`
— rather than a second boundary implementation, so there is only ever one answer
to what the rule refuses. **This script contains no boundary logic**; grep it for
one. It classifies lines by markdown SHAPE, which is a different question, and
hands every admissibility verdict to the Rust check.

Method: cite each distinct non-blank line of the spec as a one-span `present: true`
row, one ledger's worth of rows at a time, run the check, count how many the check
names. Runs in a throwaway clone; writes nothing to the real corpus.

**Aggregates only, by design.** An earlier version printed a per-id row. Six
generations of one fixture are three arms of two samples each, so a size-ordered
per-id table is a plausible guess at the arm partition — `SCORING-2-NOTES.md` §6
records T5a making exactly that mistake and T4's instruction against it. The
dispatching context does not need per-id sizes and must not hold them, so this
prints one line per fixture and nothing finer.

**Reported in two populations, because the first alone misleads.** A whole table
row, a heading, a `|---|---|` separator and a `---` break are all admissible and
none is something a scorer would ever cite as evidence of a ledger row. The
`prose` column excludes them and is the number that bears on whether a verdict can
be written; the `all` column is what a naive census returns.

**Three limitations, stated because the figure gets quoted.**
  - It measures WHOLE LINES, the strategy item 10's prompt tells the scorer to use
    ("quote the whole sentence"). Admissible sub-line spans exist — a sentence
    preceded by `. ` on the same line qualifies — so this is not a census of every
    admissible substring.
  - Lines are DEDUPLICATED, which is load-bearing and not cosmetic: citing one
    string under two rows trips item 8's no-shared-span rule, which would be
    counted as a refusal it is not. Deduplication also means a line counts
    admissible if ANY of its occurrences is, so this is an upper bound on
    per-position admissibility.
  - It is a property of THIS corpus's markup, not a claim about prose generally.

Usage: measure-span-admissibility.py <path-to-a-throwaway-clone> <fixture>

The fixture argument is REQUIRED and is not optional politeness. The first
version of this script took only a clone path and hardcoded `skill-stickiness`
— its id list, its ledger path and a bare `assert len(LEDGER) == 91`. Handed
`tiered-review` it ignored the word, measured `skill-stickiness` anyway and
exited 0 with a table that looked entirely plausible. A tool that answers a
question you did not ask, successfully, is worse than one that crashes.

A record of what ran, committed beside the artifact.
"""
import json
import pathlib
import re
import shutil
import subprocess
import sys

def repo_root(start):
    """The checkout `start` lives in — the first ancestor holding `.git`.

    Found by walking up rather than by counting `.parents[n]`, because counting
    is exactly how the first version of this guard failed: it resolved to
    `docs/` and let this script run against the live worktree, deleting
    `retention-2/`. It was recoverable from git; the guard is written this way
    so the next reader does not have to be.
    """
    for p in [start, *start.parents]:
        if (p / ".git").exists():
            return p
    raise SystemExit(f"{start} is not inside a git checkout")


# Item 10's ledger sizes. The same table the two other tools carry; a fixture
# this script does not know the size of is a fixture it will not measure.
EXPECTED_ROWS = {"skill-stickiness": 91, "tiered-review": 84, "tui-dc-picker": 55}

if len(sys.argv) != 3:
    sys.exit(__doc__)

CLONE = pathlib.Path(sys.argv[1]).resolve()
FIXTURE = sys.argv[2]
if FIXTURE not in EXPECTED_ROWS:
    sys.exit(f"unknown fixture {FIXTURE!r}; expected one of {sorted(EXPECTED_ROWS)}")

REAL = repo_root(pathlib.Path(__file__).resolve())
# This script deletes `retention-2/` outright.
if CLONE == REAL or repo_root(CLONE) != CLONE:
    sys.exit(
        f"refusing to run against {CLONE}\n"
        f"  pass the root of a throwaway `git clone --shared`, never {REAL}"
    )

EV = CLONE / "docs/skill-evidence/spec-length"
RET = EV / "retention-2"
TEST = "spec_length_2_retention_verdicts_are_complete_and_quoted"

# The id list comes from `fixture-map-2.json`, which is arm-free (plan P2). The
# blind map is never opened by anything in the scoring chain.
FIXTURE_MAP = json.loads((EV / "fixture-map-2.json").read_text())
IDS = sorted(i for i, f in FIXTURE_MAP.items() if f == FIXTURE)
assert IDS, f"no generations mapped to {FIXTURE!r} in fixture-map-2.json"

LEDGER = [
    l.split("|")[1].strip()
    for l in (EV / "ledger" / f"{FIXTURE}.md").read_text().splitlines()
    if l.startswith(f"| {FIXTURE}-")
]
assert len(LEDGER) == EXPECTED_ROWS[FIXTURE], \
    f"{len(LEDGER)} ledger rows for {FIXTURE}, expected {EXPECTED_ROWS[FIXTURE]}"


def is_structural(line):
    """Furniture: a whole table row, a heading, a separator, a break, a fence.

    Shape only. Nothing here decides admissibility.
    """
    s = line.strip()
    return (
        s.startswith("|")
        or s.startswith("#")
        or s.startswith("```")
        or set(s) <= set("-: ")
        or set(s) <= set("=* ")
    )


def refusals_for(spec_id, batch):
    """How many of `batch` (<= one ledger's worth of distinct spans) is refused?"""
    if RET.exists():
        shutil.rmtree(RET)
    RET.mkdir(parents=True)
    rows = [
        {"id": rid, "present": True, "quotes": [batch[i]]}
        if i < len(batch)
        else {"id": rid, "present": False, "quotes": []}
        for i, rid in enumerate(LEDGER)
    ]
    (RET / f"{spec_id}.json").write_text(
        json.dumps({"spec_id": spec_id, "ledger": FIXTURE, "rows": rows})
    )
    p = subprocess.run(
        ["cargo", "test", "--manifest-path", "cli/Cargo.toml", "--test", "skills_valid",
         TEST, "--", "--exact"],
        cwd=CLONE, capture_output=True, text=True,
    )
    out = p.stdout + p.stderr
    assert "running 1 test" in out, "filter matched no test; refusing to score the batch"
    if p.returncode == 0:
        return 0
    # The check reports EVERY problem in one pass, so a batch that failed for a
    # reason other than the boundary rule must abort rather than be counted as
    # boundary refusals. `problem(s) across` is the check's own total; comparing
    # against it catches a panic that produced no per-span complaint at all,
    # which a bare regex count would silently score as zero refusals.
    total = re.search(r"^(\d+) problem\(s\) across", out, re.M)
    assert total, f"{spec_id}: check failed with no problem count — did it panic?\n{out[-2000:]}"
    boundary = len(re.findall(r"no occurrence of it begins and\s+ends on a boundary", out))
    assert int(total.group(1)) == boundary, (
        f"{spec_id}: {total.group(1)} problems but {boundary} are boundary refusals; "
        f"this batch tripped some other rule and its count would be wrong\n{out[-2000:]}"
    )
    return boundary


def admissible(spec_id, cands):
    refused = 0
    n = len(LEDGER)
    for i in range(0, len(cands), n):
        refused += refusals_for(spec_id, cands[i:i + n])
    return len(cands) - refused


t_all = t_alladm = t_pr = t_pradm = 0
for spec_id in IDS:
    seen, cands = set(), []
    for l in (EV / "generated-2" / f"{spec_id}.md").read_text().split("\n"):
        if l.strip() and l not in seen:
            seen.add(l)
            cands.append(l)
    prose = [l for l in cands if not is_structural(l)]
    # Accumulated only. Nothing per-id is printed — see the aggregates note above.
    t_all += len(cands); t_alladm += admissible(spec_id, cands)
    t_pr += len(prose); t_pradm += admissible(spec_id, prose)

shutil.rmtree(RET)
print(f"{'fixture':16} {'gens':>4} {'rows':>5} {'lines':>6} {'adm':>5} {'all%':>7}   "
      f"{'prose':>6} {'adm':>5} {'prose%':>7}")
print(f"{FIXTURE:16} {len(IDS):4d} {len(LEDGER):5d} {t_all:6d} {t_alladm:5d} "
      f"{t_alladm/t_all:7.1%}   {t_pr:6d} {t_pradm:5d} {t_pradm/t_pr:7.1%}")
print(f"\n`prose` excludes table rows, headings, separators, breaks and fences — "
      f"admissible furniture no scorer would cite. {len(LEDGER)} rows must be "
      f"evidenced per generation.\nSummed over {len(IDS)} generations; per-id "
      f"figures are deliberately not printed (`SCORING-2-NOTES.md` §6).")
