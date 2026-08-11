# Generation notes — the second attempt's 18 specs

What the generation task observed about `generated-2/` while producing it, recorded beside the
artifact rather than left in a task note so that no later task has to be told it.

**Three things are recorded here, and the first two are breaches of a governed rule:**

1. **Item 5's "no header" is breached by all 18, and "no fixture name" by 11** —
   [§1](#1-item-5s-no-header-and-no-fixture-name-are-both-breached). Neither is repairable and
   neither is an arm channel, with one caveat about generation titles that this attempt does
   **not** inherit from the first, written out in full there.
2. **`R5a` flags two generations** as having copied their input rather than compressed it —
   [§2](#2-r5a--the-copy-check).
3. **One of those two, `fd2c24.md`, is byte-identical to its fixture** —
   [§3](#3-fd2c24md-is-byte-identical-to-its-fixture). The alternative explanation, a harness bug
   affecting the other seventeen, is ruled out as far as committed evidence can rule it out.

**This file is evidence, not a frozen input.** `FREEZE-2.md` says in as many words that measurements
are "evidence rather than a frozen input", so it has no row there and none is owed. It changes no
rule, no gate, no denominator and no arm.

**Every figure below is recomputable from the committed files** — `wc -l`, `wc -c`,
`git hash-object --no-filters` and `sed -n '1p'` over `generated-2/` and `fixtures/`. Nothing here
is a new fact; it is a signpost to facts the corpus already carries. The two places where that is
not true are labelled as such where they appear.

## Committing this now is a deviation from `plan.md` T4, and here is the reason

`plan.md` T4 says the per-generation `wc` figures and the `R5a` check are *"recorded in the task's
own notes, published in `RESULTS-2.md` at T8"* — that is, kept in the task's report until T8. **This
file publishes them earlier, and that is a departure from the plan's stated sequencing.** It is
recorded as one rather than slipped in.

The reason is §1 and §3. A task note lives in the run's local state directory and reaches T8 only by
being carried through four handoffs; the things below are ones a scoring task could otherwise
process as ordinary. Committing them puts them beside the artifact they are about.

**It changes nothing the plan or the protocol decides.** `RESULTS-2.md` still owes the `R5a`
publication at T8 — this file does not discharge that. `PROTOCOL-2.md` is untouched: window 3 shut
at this task's first item-5 dispatch and nothing here revises a governed item.

---

## 1. Item 5's "no header" and "no fixture name" are both breached

**Item 5:** *"The generated file holds **only the spec body** — no header, no arm name, no fixture
name, no id. Anything else is a channel to the scorer."* That is four prohibitions. **Two of them
hold and two do not.**

| clause | how it went |
|---|---|
| no arm name | **holds** — no generation carries `S0`, `S1`, `S2` or `S3` as a token |
| no id | **holds** — no generation carries its own id, or any other id from item 6's pool |
| **no header** | **breached by 18 of 18** |
| **no fixture name** | **breached by 11 of 18** |

The two that hold are checked on every test run by
`spec_length_2_generations_are_unlabelled_and_cover_the_design`. The two that do not are checked by
nothing, which is why they are written down here.

**The header breach: 18 of 18.** Every generation opens with a `# ` title naming its fixture's
subject — `# Spec: TUI deploy-config picker — browse any config in the project`,
`# Tiered (cascade) code review for drovr`, `# Spec: skill stickiness`. One `sed -n '1p'` over the
directory shows it.

**The fixture-name breach: 11 of 18**, inherited from the fixtures rather than invented by the
probes:

- **All 6 `tiered-review` generations** reproduce `fixtures/tiered-review.spec.md`'s line 3
  verbatim, at their own line 3:
  `` **Run:** `tiered-review` · **Worktree:** `.drovr/wt/tiered-review` · **Branch:** `drovr/tiered-review` ``
- **5 of the 6 `tui-dc-picker` generations** carry `.drovr/wt/tui-dc-picker`, from that fixture's
  own text — `26d7a2` does not.
- The `tiered-review` fixture also names `skill-stickiness` twice and `blind-map` once, and its
  generations inherit those too.
- No `skill-stickiness` generation carries its fixture's hyphenated name; that fixture never writes
  it.

### This is the first attempt's finding too, and one part of its answer does not transfer

`RESULTS.md` §5 item 6 records **exactly this**, in the same proportions — *"breached on the header
count in every generation and on the fixture-name count in 11 of the 18, and neither could have been
otherwise"*, all 18 opening with a `# ` title, the hyphenated name in 11. Items 4 and 5 are
inherited verbatim by `PROTOCOL-2.md`, so the structural cause is unchanged and so is the outcome.

**Its argument that neither is an arm channel had two halves. One transfers intact; the other does
not, and that is a finding rather than a footnote.**

**What still holds — the fixture name is not an arm channel.** For the `tiered-review` line the
demonstration is exact: it is byte-identical, at the same line number, in all six generations of
that fixture — two from each of the three arms. A string present identically in every arm
distinguishes none of them. And fixture identity is published deliberately anyway: item 7a exists so
that every scoring task is *told* which fixture an id belongs to, so a scorer learning it from the
text learns nothing it was not handed. (For the `tui-dc-picker` mentions the shape is looser — 5 of
6, at varying line numbers — but the same item 7a argument covers it regardless of shape.)

**What does not hold — the titles are no longer arm-invariant.** `RESULTS.md` could say the title
was *"byte-identical across all six generations of each fixture … arm-invariant by construction, not
by luck"*, because item 4 defines each task line as the fixture's `# ` title plus a restatement, and
item 5 hands that same line to every probe. **In this attempt the probes did not all copy it
verbatim:**

| fixture | distinct titles across its 6 generations |
|---|---|
| `tui-dc-picker` | **1** — `# Spec: TUI deploy-config picker — browse any config in the project` ×6 |
| `skill-stickiness` | **2** — `# Spec: skill stickiness` ×5, `# Spec: Skill Stickiness` ×1 |
| `tiered-review` | **3** — `# Tiered (cascade) code review for drovr` ×4, `… — spec` ×1, `# Spec: Tiered (cascade) code review for drovr` ×1 |

So for two of three fixtures the title varies, and **variation is the thing arm-invariance ruled
out.** The first attempt's guarantee was structural; here it survives only for `tui-dc-picker`.

**Whether the variants correlate with the arm has deliberately not been checked here — and that is
a deferral, not a refusal.** Answering it now means reading `blind-map-2.json` against the titles
and writing down what came back, and a sentence here saying "they do not correlate" would be a
second, weaker record of the same secret — which is the thing item 7 exists to prevent, and its
stated reason for having no `salt` field.

**That the bar lifts after the join is a reasoned extension of item 7, not a quotation of it**, and
the distinction is worth keeping straight in a file this careful. Item 7's explicit temporal
qualifier — *"at any point **before** the unblinding task has joined the verdicts"* — is attached to
an enumerated list of draw mechanics: the salt, the ordering rule, the sort direction, the
within-arm fixture order, the within-fixture sample order, a per-file digit sequence. A correlation
between title phrasing and arm is none of those. What carries the argument is item 7's *rationale*
rather than its list, and that rationale is temporal on its face.

**After that join the answer costs nothing, so it is owed rather than dropped.** Item 11's stated
ethos is that *"this run documents every channel it knows about"*, and item 14a's `gaps-2/<id>.md`
already resolves its `A`/`B` labels to arm names **after** the join — so once the join exists the
correlation is reconstructable by any reader from committed artifacts, and withholding it protects
nothing. **The unblinding task (`plan.md` T8) and the write-up owe this check and its answer:**
recompute the titles per fixture, join them to the arms, and report whether the phrasing variants
track the arm. If they do, that is a limitation on the blinding and belongs beside item 11's other
channels; if they do not, saying so retires the question.

**Until then it is recorded as an open channel, not as a closed one**, and the paragraph below is
what a later task needs to judge how much it matters.

**Where it could bite, so a later task can judge it.** Tier-1 scoring is sharded one generation at a
time, so a scorer never holds two titles of one fixture. **Item 14a's pairing adjudicator is the one
stage that does** — it is handed two generations of the same fixture relabelled `A` and `B`. A
title-phrasing difference is visible to it. It is told nothing about arms and is not asked which
spec is better, and `A`/`B` is fixed by lexicographic id rather than by arm, so the difference is
uninterpretable to it; but it is a difference the first attempt did not have, and **the task that
runs item 14a should read this paragraph before it dispatches.**

**Name that task carefully, because the two documents number it differently.** It is **`plan.md`
T10** — *"(conditional, `R7`) The item-14a gap test"*. `PROTOCOL-2.md` item 14a calls the same task
**T9** (*"T9 runs after T7 has unblinded"*), because it kept the numbering of a decomposition that
was later replanned — the same staleness `PROTOCOL-2.md` item 2 deviation 1 records for
`FREEZE.md`'s "Who appends what" table. Under `plan.md`'s numbering T9 is the item-**14**
transmission test, which is a different task and not the one this paragraph is addressed to.
Everywhere else in this file, task numbers are `plan.md`'s.

### Not repaired, and not repairable

The specs are the raw measurement; editing generated text to satisfy a rule about generated text
would destroy the thing being measured, which is why the suite's own leak message says that repair
is not available. `R6` does not license a re-run either — its trigger list is closed and does not
include this — and nothing may be added to `PROTOCOL-2.md` now that window 3 is shut. So it is
recorded.

---

## 2. `R5a` — the copy check

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

**What `R5a` obliges**, quoted rather than extended:

- `RESULTS-2.md` **flags every generation over that threshold** — which is both ids above.
- The write-up **may not read a flagged generation as a win**.

The rest is commentary, and is marked as such because window 3 is shut and this file may not grow a
governed rule. That `fd2c24` is a verbatim copy rather than merely a long spec is a measurement
recorded in §3, not a third clause of `R5a`; `R5a` requires the flag and says nothing about what a
flag must be annotated with. And `R3` asks for per-arm and per-fixture means against the fixtures'
own lengths — `fd2c24`'s 791 lines *are* its fixture's length, so its arm's mean is pulled toward
the reference point by a generation that compressed nothing. Whether to show the per-generation
numbers beside the means is the write-up's call, not a requirement this file can create.

---

## 3. `fd2c24.md` is byte-identical to its fixture

`generated-2/fd2c24.md` and `fixtures/skill-stickiness.spec.md` are the **same blob**,
`79525341f6c4699417fc1f8b6b20d84b8ddaacad` — 791 lines, 52355 bytes, zero diff. `FREEZE-2.md`
records that hash against the generation and `FREEZE.md` records it against the fixture, so the two
records already say this; this section is so that nobody has to notice it by comparing hash columns.

That matters because there is a second explanation, and it is not the innocent one: **a harness
bug.** The generation task rewrites all 18 files in id-lexicographic order before committing, and a
rewrite that copied the wrong source into a slot would look exactly like this from outside. **If
that is what happened, the worry is not `fd2c24` — it is the other seventeen.**

Three things bear on it. Only the first is checkable without trusting this file's author, and they
are ordered accordingly.

**1. The blast radius is bounded, and a reader can confirm that from committed files alone.** Over
`generated-2/` and `fixtures/`:

- **Exactly one** of the 18 generations hashes to any fixture's blob — `fd2c24`. The other
  seventeen match none of `79525341f6c4699417fc1f8b6b20d84b8ddaacad` (skill-stickiness),
  `7f11a08cff6cd95999e55eb353ad38c250b7a78d` (tiered-review),
  `12ceaa75f26125c93835587815a21e43e96de6b1` (tui-dc-picker).
- **All 18 generation blobs are distinct**, so no slot holds a duplicate of another slot.
- **Every generation's opening line names the fixture the map assigns it** — 18 of 18, checked
  against `fixture-map-2.json`. A slot generated under another fixture's prompt would show here.

The first two are one `git hash-object --no-filters` loop; the third is one read of each file's
first line. **State the conclusion no wider than they carry:** they rule out a fixture substituted
into another slot, two slots holding the same bytes, and a slot written against the wrong fixture.
They do **not** rule out every shape a bug could take — a truncated file, or one subtly corrupted
into bytes that are still unique and still on-topic, would pass all three. What they establish is
that the failure mode `fd2c24` exhibits is confined to `fd2c24`, and that much needs no testimony.

**2. The rewrite step is committed, and it is the wrong shape to have done this.**
`tools/normalise-generated-2.py` opens `generated-2/`, `blind-map-2.json` and `fixture-map-2.json`
and nothing else — its only mentions of `fixtures` are the fixture *map* and `R5a`'s three reference
line counts, neither of which is a file under `fixtures/` — and it asserts every body byte-identical
across the teardown (`assert fh.read() == bodies[i]`).

**What this does and does not establish** — the distinction was overclaimed here once already:
a reader can check that a script *of this shape* cannot substitute a fixture. A reader **cannot**
check that these are the bytes that ran. The script was committed at `1dd9a7b`, about half an hour
after `2e183a5` produced `generated-2/`, and specifically in answer to a review finding; `git
ls-tree 2e183a5` does not list it, and no hash of it was recorded beforehand. **So this is the same
category of evidence as point 3 — the author's word — dressed in more legible clothing.** It is
worth committing anyway, because a claim written as runnable code is one a reader can argue with.

It is also **not hash-pinned**, and that is deliberate rather than an oversight: `FREEZE-2.md`'s
"Who appends what, and when" closes this run's row set at the generation task's 20 rows and says
*"Nothing else is appended by this run"*, so giving the script a row would break that file's own
rule to protect a file that decides nothing. A later edit to it would therefore go undetected. Point
1 is what does not depend on it.

**3. The probe's own transcript shows it ran the `cp`** — and that transcript is **not committed**.
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

Taken together: (1) bounds the damage to this one file on committed evidence, and (2) and (3) name
the actor on the author's word. **The artifact is the raw measurement, and it stands** — and a
reader who discounts (2) and (3) entirely still has (1), which is the part that would matter.

**It is not an `R6` re-run.** `R6`'s list of protocol failures is closed at *the probe wrote no
file*, *a shard or a verdict is malformed*, and *a verdict fails the item-8 mechanical check*. A
generation that copies its input is none of those — it is the outcome `R5a` and limitation 1 were
written to anticipate. Re-running it because the output looks wrong is the exact move `R6` exists to
forbid. Tier-1 will score it at or near full retention for free, and `R1`'s union makes that one of
six generations for whichever arm holds it.

---

## What no test enforces

**Nothing in this file is checked by the suite, and that is stated rather than closed.** The two
item-5 clauses of §1 are unchecked because
`spec_length_2_generations_are_unlabelled_and_cover_the_design` screens for `S0`–`S3`, a
generation's own id and the probe template's markers, and for nothing else; a future attempt wanting
"no header" enforced mechanically would have to decide first what it means for a document whose
input is full of headers. `R5a` and `R3` are unchecked because they are rules about how a result is
*read*, and the suite checks shape and ordering, not interpretation.

Closing either gap now would mean inventing a checked artifact after the measurement exists, which
is the move this second attempt was set up to rule out — and `PROTOCOL-2.md` is closed, window 3
having shut at the first item-5 dispatch.

**So they are owed by people, not by tests, and here is the whole list of what this file leaves
open and to whom:**

| open item | owed by | where |
|---|---|---|
| flag both `R5a` generations, and read neither as a win | the unblinding task (`plan.md` T8) and the write-up | §2 |
| log the early publication of the `wc`/`R5a` figures in `RESULTS-2.md`'s deviation list | the same | the section above §1 |
| do not read `fd2c24`'s free full retention as evidence | the same | §2, §3 |
| join the title phrasings to the arms and report whether they track | the same, **after** the join | §1 |
| read §1's title-variance caveat before dispatching item 14a | **`plan.md` T10** (`PROTOCOL-2.md` calls it T9) | §1 |
| enforce item 5's "no header" / "no fixture name" mechanically, or decide they cannot be | whoever pre-registers a third attempt | §1 |

**Nothing tests any row of that table, and — stated plainly, because it is the weakest link in this
file — no structural document points at this file at all.** Not `plan.md`, not `PROTOCOL-2.md`, not
`FREEZE-2.md`. `plan.md` T8's list of what `RESULTS-2.md` must contain does not mention the
title-correlation check, because the question did not exist when the plan was written; window 3
forbids adding it to `PROTOCOL-2.md`, and `FREEZE-2.md` is append-only and closed at this run's own 20
rows (the table totals 21; T1's `PROTOCOL-2.md` row is the first).

**Two carriers exist, and both are people-shaped.** The first is proximity: this file sits beside
`generated-2/`, and `plan.md` T8 already owes per-generation `wc` figures and `R5a` flags, so the
task most of these rows are addressed to has a reason to be in this directory. The second is the
run's own phase handoff — **the generation task's handoff carries this table, and every handoff
after it must carry any row still undischarged.**

**A driver who wants it structural instead should add the row to `plan.md` T8's deliverables.**
`plan.md` is not frozen and is the driver's to amend; it is not this note's to change, and the
generation task deliberately did not change it.
