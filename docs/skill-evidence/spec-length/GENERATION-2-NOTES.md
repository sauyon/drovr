# Generation notes — the second attempt's 18 specs

Measurements taken by the generation task over `generated-2/`, recorded here rather than left in a
task note so that no later task has to be told them. **This file is evidence, not a frozen input:**
`FREEZE-2.md` says in as many words that measurements are "evidence rather than a frozen input", so
it has no row here and none is owed. It changes no rule, no gate and no denominator.

**Every figure below is recomputable from the committed files** — `wc -l`, `wc -c` and
`git hash-object --no-filters` over `generated-2/` and `fixtures/`. Nothing here is a new fact; it
is a signpost to facts the corpus already carries.

## Committing this now is a deviation from `plan.md` T4, and here is the reason

`plan.md` T4 says the per-generation `wc` figures and the `R5a` check are *"recorded in the task's
own notes, published in `RESULTS-2.md` at T8"* — that is, kept in the task's report until T8. **This
file publishes them earlier, and that is a departure from the plan's stated sequencing.** It is
recorded as one rather than slipped in.

The reason is the `fd2c24` finding below. A task note lives in the run's local state directory and
reaches T8 only by being carried through four handoffs; the anomaly it describes is one a scoring
task could otherwise process as ordinary. Committing it puts it beside the artifact it is about.

**It changes nothing the plan or the protocol decides.** `RESULTS-2.md` still owes the `R5a`
publication at T8 — this file does not discharge that — and no gate, denominator, rule or arm moves.
`PROTOCOL-2.md` is untouched: window 3 shut at this task's first item-5 dispatch and nothing here
revises a governed item.

## `R5a` — the copy check

`PROTOCOL-2.md` item 12's `R5a`: a generation whose length is **≥ 95% of its fixture's** (791 / 463
/ 414 lines) has substantially copied its input rather than compressed it. That is **not a failure**
— it is the correct null-ward behaviour under D2, and limitation 1 says a probe that copies its
input scores full retention at full length. But **a high-retention verdict on such a generation says
nothing about the instruction**, so it is flagged, and the write-up may not read a flagged
generation as a win.

**Two of the 18 are over the threshold.**

| id | fixture | lines | bytes | fixture lines | % of fixture | `R5a` |
|---|---|---|---|---|---|---|
| `031cc4` | tui-dc-picker | 286 | 15960 | 414 | 69.1% | |
| `054872` | tiered-review | 272 | 17521 | 463 | 58.7% | |
| `08ae18` | tui-dc-picker | 288 | 16043 | 414 | 69.6% | |
| `26d7a2` | tui-dc-picker | 226 | 13131 | 414 | 54.6% | |
| `2c4295` | tiered-review | 328 | 19195 | 463 | 70.8% | |
| `2d2629` | tiered-review | 248 | 15991 | 463 | 53.6% | |
| `47173f` | tui-dc-picker | 377 | 21808 | 414 | 91.1% | |
| `48527b` | tiered-review | 317 | 18071 | 463 | 68.5% | |
| `66530f` | skill-stickiness | 642 | 41306 | 791 | 81.2% | |
| `6e7393` | skill-stickiness | 541 | 34094 | 791 | 68.4% | |
| `a9fcf9` | tui-dc-picker | 410 | 24225 | 414 | 99.0% | **FLAGGED** |
| `b2b8cf` | tui-dc-picker | 317 | 17908 | 414 | 76.6% | |
| `b49ff1` | tiered-review | 336 | 18827 | 463 | 72.6% | |
| `e085f2` | skill-stickiness | 418 | 25828 | 791 | 52.8% | |
| `e790f5` | skill-stickiness | 580 | 36646 | 791 | 73.3% | |
| `fd230c` | tiered-review | 402 | 22731 | 463 | 86.8% | |
| `fd2c24` | skill-stickiness | 791 | 52355 | 791 | 100.0% | **FLAGGED** |
| `fe4059` | skill-stickiness | 424 | 25818 | 791 | 53.6% | |

The table carries **no arm**, and it discloses nothing that committing the specs did not already
disclose: `wc -l generated-2/*.md` reproduces it in one command.

## `fd2c24.md` is byte-identical to its fixture

`generated-2/fd2c24.md` and `fixtures/skill-stickiness.spec.md` are the **same blob**,
`79525341f6c4699417fc1f8b6b20d84b8ddaacad` — 791 lines, 52355 bytes, zero diff. `FREEZE-2.md`
records that hash against the generation and `FREEZE.md` records it against the fixture, so the two
records already say this; this section is so that nobody has to notice it by comparing hash columns.

That matters because there is a second explanation, and it is not the innocent one: **a harness
bug.** The generation task rewrites all 18 files in id-lexicographic order before committing, and a
rewrite that copied the wrong source into a slot would look exactly like this from outside. The two
explanations are told apart by two pieces of evidence, and their evidentiary status differs — which
is said here rather than smoothed over, because the rest of this file is recomputable and this part
is not.

**1. The rewrite step is committed, so the claim about it is checkable.**
`tools/normalise-generated-2.py` is the exact script that tore down and rebuilt the directory. It
opens `generated-2/`, `blind-map-2.json` and `fixture-map-2.json` and **nothing else** — its only
mentions of `fixtures` are the fixture *map* and `R5a`'s three reference line counts, neither of
which is a file under `fixtures/`. It also asserts every body byte-identical across the teardown
(`assert fh.read() == bodies[i]`). A rewrite that substituted a fixture is not a thing that script
can do, and a reader can now confirm that rather than take it on trust.

**2. The probe's own transcript shows it ran the `cp`** — and that transcript is **not committed**.
It lives in this harness's ephemeral per-session subagent log, which no later reader will have. What
is committed is this excerpt of it, which is a self-report and should be read as one. The probe made
five tool calls in total, in this order:

```
Read  <the probe prompt's own directory>
Read  .../docs/skill-evidence/spec-length/fixtures/skill-stickiness.spec.md
Bash  ls -la .../docs/skill-evidence/spec-length/generated-2/ 2>/dev/null | head -50
Bash  ls -la .../docs/skill-evidence/spec-length/ 2>/dev/null
Bash  mkdir -p .../docs/skill-evidence/spec-length/generated-2
      cp .../docs/skill-evidence/spec-length/fixtures/skill-stickiness.spec.md \
         .../docs/skill-evidence/spec-length/generated-2/fd2c24.md
      wc -c .../docs/skill-evidence/spec-length/generated-2/fd2c24.md
```

Taken together: (1) rules out the rewrite as the source on committed evidence, and (2) names the
actor on uncommitted evidence. **The artifact is the raw measurement, and it stands.**

**It is not an `R6` re-run.** `R6`'s list of protocol failures is closed at *the probe wrote no
file*, *a shard or a verdict is malformed*, and *a verdict fails the item-8 mechanical check*. A
generation that copies its input is none of those — it is the outcome `R5a` and limitation 1 were
written to anticipate. Re-running it because the output looks wrong is the exact move `R6` exists to
forbid.

**What it obliges.** Tier-1 will score it at or near full retention for free, and `R1`'s union makes
that one of six generations for whichever arm holds it.

Two of these are `R5a`'s own pre-registered requirements, quoted rather than extended:

- `RESULTS-2.md` **flags every generation over the threshold** — both ids above, `fd2c24` with the
  fact that it is a verbatim copy and not merely a long spec.
- The write-up **may not read a flagged generation as a win**.

One is commentary, and is marked as such because window 3 is shut and this file may not add to a
governed rule: `R3` asks for per-arm and per-fixture means against the fixtures' own lengths, and
`fd2c24`'s 791 lines *are* its fixture's length, so its arm's mean is pulled toward the reference
point by a generation that compressed nothing. Whether to show the per-generation numbers beside the
means is the write-up's call, not a requirement this file can create.

**No test enforces any of this.** `R5a` is a rule about how a result is *read*, and the suite checks
shape and ordering, not interpretation. That gap is stated here rather than closed: inventing a new
checked artifact after the measurement exists is the move this second attempt was set up to rule
out, and `PROTOCOL-2.md` is closed — window 3 shut at the first item-5 dispatch.
