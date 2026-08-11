#!/usr/bin/env python3
"""RED/GREEN harness for `spec_length_2_retention_verdicts_are_complete_and_quoted`.

Runs in a throwaway `git clone --shared`, against a SYNTHETIC generated spec that
overwrites the real one inside the clone. No RED check reads the real corpus, so
none of them can touch the draw.

Each case seeds `retention-2/` and runs the one test. A case is RED when the test
fails; the GREEN baseline must pass. `running 1 test` is asserted first — a filter
that matches zero tests prints `ok. 0 passed` and would score a false GREEN.
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
    `docs/` and let the harness run against the live worktree, overwriting a
    frozen generation and deleting `retention-2/`. Both were recoverable from
    git; the guard is written this way so the next reader does not have to be.
    """
    for p in [start, *start.parents]:
        if (p / ".git").exists():
            return p
    raise SystemExit(f"{start} is not inside a git checkout")


CLONE = pathlib.Path(sys.argv[1]).resolve()
REAL = repo_root(pathlib.Path(__file__).resolve())
# This harness deletes `retention-2/` and OVERWRITES a frozen `generated-2/*.md`.
if CLONE == REAL or repo_root(CLONE) != CLONE:
    sys.exit(
        f"refusing to run against {CLONE}\n"
        f"  pass the root of a throwaway `git clone --shared`, never {REAL}"
    )

EV = CLONE / "docs/skill-evidence/spec-length"
RET = EV / "retention-2"
TEST = "spec_length_2_retention_verdicts_are_complete_and_quoted"

SPEC = """# A synthetic document

The gate is a hard error and never a warning.

- The budget rises from 2200 to 12000 bytes for the four methodology skills.
- Every prohibition is paired with the required action in the same breath and never
  left standing alone as a bare refusal.

The final paragraph closes the file.
"""
GOOD = "The gate is a hard error and never a warning."
GOOD2 = "The budget rises from 2200 to 12000 bytes for the four methodology skills."
# Verbatim, but clipped at the hard wrap in the middle of a phrase: item 8 refuses it.
CLIPPED = "Every prohibition is paired with the required action in the same breath and never"

IDS = [
    l.split("|")[1].strip()
    for l in (EV / "ledger/skill-stickiness.md").read_text().splitlines()
    if l.startswith("| skill-stickiness-")
]
assert len(IDS) == 91, len(IDS)


def base():
    return {
        "spec_id": "66530f",
        "ledger": "skill-stickiness",
        "rows": [{"id": i, "present": False, "quotes": []} for i in IDS],
    }


def present(v, n, quotes):
    v["rows"][n]["present"] = True
    v["rows"][n]["quotes"] = quotes
    return v


def run(name, verdict, expect_red, filename="66530f.json"):
    if RET.exists():
        shutil.rmtree(RET)
    RET.mkdir(parents=True)
    (RET / filename).write_text(
        verdict if isinstance(verdict, str) else json.dumps(verdict, indent=2)
    )
    p = subprocess.run(
        ["cargo", "test", "--manifest-path", "cli/Cargo.toml", "--test", "skills_valid",
         TEST, "--", "--exact"],
        cwd=CLONE, capture_output=True, text=True,
    )
    out = p.stdout + p.stderr
    assert "running 1 test" in out, f"{name}: filter matched no test, refusing to score it"
    red = p.returncode != 0
    ok = red == expect_red
    print(f"{'PASS' if ok else 'FAIL'}  {name}: "
          f"{'RED' if red else 'GREEN'} (wanted {'RED' if expect_red else 'GREEN'})")
    if not ok:
        print(out[-3000:])
    return ok


def main():
    EV.joinpath("generated-2/66530f.md").write_text(SPEC)
    results = []
    # GREEN baseline first: a harness that cannot go green is not measuring anything.
    results.append(run("baseline all-false", base(), False))
    results.append(run("baseline one present, self-contained span",
                       present(base(), 0, [GOOD]), False))

    v = base(); v["rows"].pop(30)
    results.append(run("missing ledger row", v, True))
    v = base(); v["rows"].insert(5, dict(v["rows"][5]))
    results.append(run("duplicated row", v, True))
    v = base(); v["rows"][3], v["rows"][4] = v["rows"][4], v["rows"][3]
    results.append(run("rows out of ledger order", v, True))
    v = base(); v["rows"][7]["id"] = "skill-stickiness-99"
    results.append(run("row id not in the ledger", v, True))

    results.append(run("present true with zero spans", present(base(), 2, []), True))
    results.append(run("present true with six spans",
                       present(base(), 2, [GOOD, GOOD2, "The final paragraph closes the file.",
                                           "# A synthetic document", "- The budget rises",
                                           "The gate is a hard error"]), True))
    results.append(run("present true with an empty span", present(base(), 2, [""]), True))
    results.append(run("span is not a verbatim substring",
                       present(base(), 2, ["The gate is a soft warning."]), True))
    results.append(run("span verbatim but clipped mid-phrase at a wrap",
                       present(base(), 2, [CLIPPED]), True))
    v = present(base(), 2, [GOOD]); v["rows"][3]["present"] = True
    v["rows"][3]["quotes"] = [GOOD]
    results.append(run("one span cited for two rows", v, True))
    v = base(); v["rows"][9]["quotes"] = [GOOD]
    results.append(run("present false carrying a span", v, True))

    v = base(); v["spec_id"] = "6e7393"
    results.append(run("spec_id disagrees with the filename", v, True))
    v = base(); v["spec_id"] = "deadbe"
    results.append(run("spec_id outside the eighteen-id pool", v, True))
    v = base(); v["rows"][0]["note"] = "extra"
    results.append(run("an extra key in a row", v, True))
    v = base(); v["extra"] = 1
    results.append(run("an extra key in the verdict", v, True))
    v = base(); v["rows"][0]["present"] = None
    results.append(run("a null where a bool belongs", v, True))
    v = base(); del v["ledger"]
    results.append(run("a missing top-level key", v, True))
    results.append(run("not JSON at all", "{ this is not json", True))
    results.append(run("a verdict filed under an invented id", base(), True,
                       filename="ffffff.json"))
    results.append(run("a stray entry in retention-2/", base(), True,
                       filename="notes.txt"))

    shutil.rmtree(RET)
    print(f"\n{sum(results)}/{len(results)} cases behaved as specified")
    return 0 if all(results) else 1


sys.exit(main())
