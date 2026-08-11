#!/usr/bin/env python3
"""What fraction of a generated spec's own complete lines are admissible spans?

The question item 8's self-containment rule raises is not "did this scorer quote
badly" but "what can be quoted at all". This measures that, and it measures it
with the AUTHORITATIVE check — `spec_length_2_retention_verdicts_are_complete_and_quoted`
— rather than a second boundary implementation, so there is only ever one answer
to what the rule refuses.

Method: cite each distinct non-blank line of the spec as a one-span `present: true`
row, 91 rows at a time (the ledger's size), run the check, and count how many the
check names. Runs in a throwaway clone; writes nothing to the real corpus.
"""
import json
import pathlib
import re
import shutil
import subprocess
import sys

CLONE = pathlib.Path(sys.argv[1])
EV = CLONE / "docs/skill-evidence/spec-length"
RET = EV / "retention-2"
TEST = "spec_length_2_retention_verdicts_are_complete_and_quoted"
IDS = ["66530f", "6e7393", "e085f2", "e790f5", "fd2c24", "fe4059"]

LEDGER = [
    l.split("|")[1].strip()
    for l in (EV / "ledger/skill-stickiness.md").read_text().splitlines()
    if l.startswith("| skill-stickiness-")
]
assert len(LEDGER) == 91


def refusals_for(spec_id, batch):
    """How many of `batch` (<=91 distinct spans) does the check refuse?"""
    if RET.exists():
        shutil.rmtree(RET)
    RET.mkdir(parents=True)
    rows = []
    for i, rid in enumerate(LEDGER):
        if i < len(batch):
            rows.append({"id": rid, "present": True, "quotes": [batch[i]]})
        else:
            rows.append({"id": rid, "present": False, "quotes": []})
    (RET / f"{spec_id}.json").write_text(
        json.dumps({"spec_id": spec_id, "ledger": "skill-stickiness", "rows": rows})
    )
    p = subprocess.run(
        ["cargo", "test", "--manifest-path", "cli/Cargo.toml", "--test", "skills_valid",
         TEST, "--", "--exact"],
        cwd=CLONE, capture_output=True, text=True,
    )
    out = p.stdout + p.stderr
    assert "running 1 test" in out, "filter matched no test"
    if p.returncode == 0:
        return 0
    boundary = len(re.findall(r"no occurrence of it begins and\s+ends on a boundary", out))
    other = len(re.findall(r"^\s+\S+\.json: ", out, re.M)) - boundary
    assert other == 0, f"unexpected non-boundary complaints ({other}) for {spec_id}:\n{out[-2000:]}"
    return boundary


print(f"{'spec':8} {'lines':>6} {'admissible':>11} {'refused':>8} {'admitted':>9}")
total_l = total_a = 0
for spec_id in IDS:
    lines = (EV / "generated-2" / f"{spec_id}.md").read_text().split("\n")
    cands, seen = [], set()
    for l in lines:
        if l.strip() and l not in seen:
            seen.add(l)
            cands.append(l)
    refused = 0
    for i in range(0, len(cands), 91):
        refused += refusals_for(spec_id, cands[i:i + 91])
    adm = len(cands) - refused
    total_l += len(cands)
    total_a += adm
    print(f"{spec_id:8} {len(cands):6d} {adm:11d} {refused:8d} {adm/len(cands):8.1%}")

shutil.rmtree(RET)
print(f"{'TOTAL':8} {total_l:6d} {total_a:11d} {total_l-total_a:8d} {total_a/total_l:8.1%}")
