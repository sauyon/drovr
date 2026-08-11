# Generation notes — the second attempt's 18 specs

Measurements taken by the generation task over `generated-2/`, recorded here rather than left in a
task note so that no later task has to be told them. **This file is evidence, not a frozen input:**
`FREEZE-2.md` says in as many words that measurements are "evidence rather than a frozen input", so
it has no row here and none is owed. It changes no rule, no gate and no denominator.

**Every figure below is recomputable from the committed files** — `wc -l`, `wc -c` and
`git hash-object --no-filters` over `generated-2/` and `fixtures/`. Nothing here is a new fact; it
is a signpost to facts the corpus already carries.

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

**The probe did this itself, and that was established from its own transcript rather than assumed.**
Its final action was, verbatim:

```
cp .../docs/skill-evidence/spec-length/fixtures/skill-stickiness.spec.md \
   .../docs/skill-evidence/spec-length/generated-2/fd2c24.md
```

That matters because the alternative explanation is a harness bug — the generation task rewrites
every file in id-lexicographic order before committing, and a rewrite that copied the wrong source
into a slot would look exactly like this from the outside. It was not that: the rewrite step reads
only `generated-2/` and the two maps and never opens a path under `fixtures/` at all, and the `cp`
above is in the probe's transcript. **The artifact is the raw measurement, and it stands.**

**It is not an `R6` re-run.** `R6`'s list of protocol failures is closed at *the probe wrote no
file*, *a shard or a verdict is malformed*, and *a verdict fails the item-8 mechanical check*. A
generation that copies its input is none of those — it is the outcome `R5a` and limitation 1 were
written to anticipate. Re-running it because the output looks wrong is the exact move `R6` exists to
forbid.

**What it obliges.** Tier-1 will score it at or near full retention for free, and `R1`'s union makes
that one of six generations for whichever arm holds it. So:

- `RESULTS-2.md` must carry both flagged ids where `R5a` requires, `fd2c24` with the fact that it is
  a verbatim copy and not merely a long spec.
- The write-up may not read either flagged generation as evidence that its instruction works.
- `R3`'s length comparison runs only among arms that clear `R2`; `fd2c24`'s 791 lines are the
  fixture's own length and inflate its arm's mean, which is a reason to report the per-generation
  numbers above beside any per-arm mean rather than the mean alone.

**No test enforces any of this.** `R5a` is a rule about how a result is *read*, and the suite checks
shape and ordering, not interpretation. That gap is stated here rather than closed: inventing a new
checked artifact after the measurement exists is the move this second attempt was set up to rule
out, and `PROTOCOL-2.md` is closed — window 3 shut at the first item-5 dispatch.
