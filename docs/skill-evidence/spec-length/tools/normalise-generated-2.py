#!/usr/bin/env python3
"""Normalise `generated-2/`, and move the two maps into the repository.

v1's `RESULTS.md` §5 deviation 9: dispatching probes lets `ls -l` mtimes, inode
order and `readdir` order reconstruct the write order from the filesystem alone.
The directory is therefore torn down and rebuilt with all 18 files written in
id-lexicographic order, so nothing about the draw survives in the filesystem.

The maps were drawn and written before the first dispatch, outside the repo, so
that no probe running with a working directory inside this worktree could read
`blind-map-2.json`. Their blob hashes are re-checked here against the values
recorded before dispatch: the bytes that are committed are the bytes that were
fixed before any generation existed.
"""
import hashlib
import json
import os
import shutil
import sys

SCRATCH = sys.argv[1]
EXPECTED = {  # `git hash-object --no-filters`, recorded before the first dispatch
    "blind-map-2.json": "89a73f25ef1fb779a854b19ace7e91e36fc2142d",
    "fixture-map-2.json": "4d97354261402b03f01ad6550ec2092c0be00d6d",
}
REPO = "/home/sauyon/devel/drovr/.drovr/wt/spec-length-ab2"
SPEC = os.path.join(REPO, "docs/skill-evidence/spec-length")
GEN = os.path.join(SPEC, "generated-2")

POOL = [
    "031cc4", "054872", "08ae18", "26d7a2", "2c4295", "2d2629",
    "47173f", "48527b", "66530f", "6e7393", "a9fcf9", "b2b8cf",
    "b49ff1", "e085f2", "e790f5", "fd230c", "fd2c24", "fe4059",
]


def blob(data: bytes) -> str:
    return hashlib.sha1(b"blob %d\0" % len(data) + data).hexdigest()


# --- read all 18 out, verify, tear the directory down, write back in order ---
names = sorted(os.listdir(GEN))
assert names == sorted(f"{i}.md" for i in POOL), f"unexpected contents: {names}"
bodies = {}
for i in POOL:
    with open(os.path.join(GEN, f"{i}.md"), "rb") as fh:
        bodies[i] = fh.read()
    assert bodies[i].strip(), f"{i}.md is empty -- an R6 re-run, not a spec that says nothing"

shutil.rmtree(GEN)
os.makedirs(GEN)
for i in sorted(POOL):  # id-lexicographic, and nothing else
    with open(os.path.join(GEN, f"{i}.md"), "wb") as fh:
        fh.write(bodies[i])

after = sorted(os.listdir(GEN))
assert after == sorted(f"{i}.md" for i in POOL)
for i in POOL:
    with open(os.path.join(GEN, f"{i}.md"), "rb") as fh:
        assert fh.read() == bodies[i], f"{i}.md changed across the rewrite"

# --- move the maps in, byte-identical to what was fixed before dispatch ------
for name, want in EXPECTED.items():
    src = os.path.join(SCRATCH, name)
    with open(src, "rb") as fh:
        data = fh.read()
    got = blob(data)
    assert got == want, f"{name}: hashes to {got}, was {want} before the first dispatch"
    with open(os.path.join(SPEC, name), "wb") as fh:
        fh.write(data)

blind = json.load(open(os.path.join(SPEC, "blind-map-2.json")))
fixtures = json.load(open(os.path.join(SPEC, "fixture-map-2.json")))
assert sorted(blind) == sorted(POOL) == sorted(fixtures)

# --- the label-leak check, run here so a bad probe is caught before commit ---
leaks = []
for i in POOL:
    text = bodies[i].decode()
    for label in ("S0", "S1", "S2", "S3"):
        # Token-ish: the test's own predicate is stricter, this is the early warning.
        if label in text:
            leaks.append(f"{i}: contains {label!r}")
    if i in text:
        leaks.append(f"{i}: contains its own id")
    for marker in ("BEGIN INSTRUCTION", "END INSTRUCTION"):
        if marker in text:
            leaks.append(f"{i}: echoes {marker!r}")
assert not leaks, "label leaks:\n  " + "\n  ".join(leaks)

# --- R3 / R5a: per-generation length, keyed by id and fixture only -----------
REF = {"skill-stickiness": 791, "tiered-review": 463, "tui-dc-picker": 414}
print(f"{'id':8} {'fixture':18} {'lines':>6} {'bytes':>7} {'ref':>5} {'% of ref':>9}  R5a")
rows = []
for i in POOL:
    text = bodies[i].decode()
    lines = text.count("\n")
    ref = REF[fixtures[i]]
    pct = 100.0 * lines / ref
    flag = "FLAGGED" if pct >= 95.0 else ""
    rows.append((i, fixtures[i], lines, len(bodies[i]), ref, pct, flag))
    print(f"{i:8} {fixtures[i]:18} {lines:6} {len(bodies[i]):7} {ref:5} {pct:8.1f}%  {flag}")

json.dump(
    [
        {"id": i, "fixture": f, "lines": l, "bytes": b, "fixture_lines": r,
         "pct_of_fixture": round(p, 2), "r5a_flagged": bool(g)}
        for i, f, l, b, r, p, g in rows
    ],
    open(os.path.join(SCRATCH, "lengths.json"), "w"),
    indent=2,
)
print(f"\nR5a flagged: {sum(1 for r in rows if r[6])} of 18")
print("directory rebuilt, 18 files written in id-lexicographic order")
print("maps moved in, both hashing to their pre-dispatch values")
