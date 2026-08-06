# Run ledger

Every probe subagent this run spawns is counted here. **Append-only**: each probe-spawning
task adds its rows as its *last* evidence write, and rewrites nothing above.

`spec.md` §7.3 sets a **122-run hard ceiling** across the whole run, split into per-stage
ceilings. This table is the only mechanism tracking it. Tasks 16–21 read the cumulative
total *before* starting and **halt with a null** rather than cross a ceiling — a stage that
hits its ceiling records the null in `docs/skill-evidence/<skill>.md` and stops; it does not
silently extend.

**A retried run counts.** If a probe is re-dispatched because its first attempt failed,
returned nothing, or wrote a bad transcript, that is two runs against the ceiling, not one.
The `runs this stage` cell records what was actually spent, not what was planned.

**The table's arithmetic is checked**, by
`cli/tests/skills_valid.rs::run_ledger_cumulative_is_a_running_total`: `cumulative` must be the
running total of `runs this stage`, and the last one must be at or under 122. Two rules follow
from how it reads this file, and they are stated here because breaking either is silent
otherwise. **The four load-bearing columns are resolved by their header text, never by position**
— `task`, `stage (§7.3 row)`, `runs this stage`, `cumulative` — so they may be reordered but not
renamed, dropped or duplicated. And **no line after the table may begin with `|`**: every such
line is read as a data row, exactly as `arms/MANIFEST.md` requires of its own table, so that a
blank line between two rows cannot quietly end the table and leave the check validating a prefix.

| task | stage (§7.3 row) | runs this stage | cumulative | stage ceiling | ceiling hit? |
|---|---|---|---|---|---|
| 6 | RED / baseline on dev set | 10 | 10 | 10 | no |
| 16 | Arm A on held-out (`tdd`) | 4 | 14 | 20 | no |
| 16 | Arm A′ on held-out (`tdd`) | 4 | 18 | 16 | no |
| 16 | Arm B on held-out (`tdd`) | 5 | 23 | 20 | no |
| 16 | REFACTOR re-tests (`tdd`) | 0 | 23 | 20 | no |
| 16 | **Unaided control (`tdd`) — not a §7.3 row** | 4 | 27 | 4 (this stage only) | n/a |
| 17 | Arm A on held-out (`systematic-debugging`) | 4 | 31 | 20 | no |
| 17 | Arm A′ on held-out (`systematic-debugging`) | 4 | 35 | 16 | no |
| 17 | Arm B on held-out (`systematic-debugging`) | 4 | 39 | 21 | no |
| 17 | REFACTOR re-tests (`systematic-debugging`) | 0 | 39 | 20 | no |
| 18 | Arm A on held-out (`verification-before-completion`) | 4 | 43 | 20 | no |
| 18 | Arm A′ on held-out (`verification-before-completion`) | 4 | 47 | 16 | no |
| 18 | Arm B on held-out (`verification-before-completion`) | 4 | 51 | 21 | no |
| 18 | REFACTOR re-tests (`verification-before-completion`) | 0 | 51 | 20 | no |
| 18 | **Unaided control (`verification-before-completion`) — not a §7.3 row** | 4 | 55 | 4 (this stage only) | n/a |
| discrimination-test | **Discrimination probe (`tdd`) — not a §7.3 row** | 4 | 59 | 4 (this stage only) | n/a |
| discrimination-test | **Discrimination probe (`systematic-debugging`) — not a §7.3 row** | 4 | 63 | 4 (this stage only) | n/a |
| discrimination-test | **Discrimination probe (`verification-before-completion`) — not a §7.3 row** | 4 | 67 | 4 (this stage only) | n/a |
| discrimination-test | **Discrimination probe (`code-review`) — not a §7.3 row** | 4 | 71 | 4 (this stage only) | n/a |
| discrimination-test | **Discrimination probe (`using-drovr`) — not a §7.3 row** | 4 | 75 | 4 (this stage only) | n/a |
| remeasure-tdd | Arm A on held-out RE-MEASURED (`tdd`) | 4 | 79 | 20 | no |
| remeasure-tdd | Arm A′ on held-out RE-MEASURED (`tdd`) | 4 | 83 | 16 | **YES — exactly at 16 of 16** |
| remeasure-tdd | Arm B on held-out RE-MEASURED (`tdd`) | 4 | 87 | 21 | no |
| remeasure-tdd | REFACTOR re-tests (`tdd`) | 0 | 87 | 20 | no |

**Task 6's 10 runs, in detail:** 5 skills × 1 `dev` scenario × 2 samples, arm A text, model
`sonnet`, foreground `general-purpose` subagents. **Zero retries** — all 10 probes returned a
confirmation and wrote a transcript on their first dispatch. The stage completed its planned
10 of 10, so `ceiling hit?` is `no` in the sense that matters: no work was cut short by the
ceiling and nothing was left unmeasured. There is no headroom left in this row.

**Task 16's 13 runs, in detail:** 3 arms × 2 held-out scenarios × 2 samples = 12 planned, plus
**1 retry**. Probe `79bd97` (arm B, `tdd-3`, sample 1) returned its answer as its final message
instead of writing its transcript file, producing no transcript; it was re-dispatched and the
retry succeeded. Per this table's own rule that a retried run counts, the arm B row reads **5**,
not 4, and the phase total is **13**, not 12. Model `sonnet`, `general-purpose` subagents,
per `plan.md` C5.

The **REFACTOR row is 0 by construction, not by omission.** Arm A scored 4 of 4, so `plan.md`'s
pre-registered branch **(a)** fired and evaluation stopped; REFACTOR is reachable only via branch
(d). `tdd` reverts to A′ with its ≤4 REFACTOR allotment unspent. See
`docs/skill-evidence/tdd.md`.

**Headroom after this phase: 99 runs of 122.** Per-stage: *Arm A on held-out* has 16 of 20 left
and needs 16 (4 skills × 4) — **exactly zero slack**. *Arm A′ on held-out* has 12 of 16 and needs
12 (3 discipline skills × 4) — **exactly zero slack**.

**The *Arm B on held-out* row is now over-subscribed, and this needs a run-level decision before
Task 17 starts.** Its ceiling is 20. Task 16 spent **5** of them (4 runs + the retry), leaving
**15**. Tasks 17–20 have 4 skills left and need **16**. That is a shortfall of **1 run, already
incurred** — not a margin that a future retry would erase. Even with zero further retries, the
last skill in that stage reaches its final run with the ceiling crossed, and §7.3's rule is
explicit: work **halts and records a null**, it does not silently extend.

The three ways out are all run-level calls and **none of them is taken here**: raise the arm B
row's ceiling (a change to frozen `spec.md` §7.3); accept a null on one skill's arm B and record
which; or rule that a protocol-failure retry producing no transcript is not a charged run (a
change to this table's stated rule, which would also retroactively rewrite the row above).
**Escalated in `ab-tdd-HANDOFF.md` rather than decided by this phase.** The ledger records what
was actually spent; it is not the place to resolve the shortfall by relabelling it.

> **RESOLVED 2026-08-05 by the user, who authorized further measurement at this phase's
> discretion.** The call taken is **option 1: the *Arm B on held-out* row's ceiling rises from 20
> to 21**, so Tasks 17–20 have the 16 runs they need. Option 2 was rejected because accepting a
> null would discard a whole skill's arm B measurement to pay for a protocol bug. Option 3 was
> rejected because rewriting the retry rule would retroactively edit a spent row and weaken the
> guard that stops a bad probe being re-rolled until it says something better.
>
> **This is a deviation from frozen `spec.md` §7.3, recorded as one rather than folded in
> silently.** Tasks 17–20 must still read this table before starting and halt with a null rather
> than extend any *other* row.

**Task 16's unaided control (4 runs, 2026-08-05) is not a `spec.md` §7.3 stage.** §7.3's budget
table has no unaided row — the gap Task 6 recorded as its first limitation — and the user
authorized these runs after `ab-tdd` had shipped. They enter **no pre-registered bar** and do not
change the ab-tdd verdict. 2 held-out scenarios × 2 samples, no skill text of any arm in the
prompt, blind-scored like every other stage. Result: **3 of 4 compliant unaided**, against 4/4
for each of A, A′ and B. Full record in `docs/skill-evidence/tdd.md` under *Unaided control*.

**It is scoped to `tdd`.** The other four skills still have no unaided condition, and this result
does not transfer — discriminating power belongs to each scenario pair, and `tdd`'s two differed
sharply (`tdd-2` produced the only failure; `tdd-3` was saturated at 2 of 2 even unaided).
**Cumulative is now 27**, and the run's arithmetic totals ≈123 against a table §7.3 itself writes
as "≈122".

**Task 17's 12 runs, in detail:** 3 arms × 2 held-out scenarios × 2 samples = 12 planned,
**zero retries** — every probe wrote its transcript and returned a well-formed confirmation on
first dispatch, and every one appended its `## Meta-test` block on the follow-up turn. The phase
total is **12**, not 13. Model `sonnet`, `general-purpose` subagents, per `plan.md` C5.

**One scorer verdict was rejected and re-scored, and that is not a charged run.** `6c8221`'s
`evidence` field was not verbatim in its `## Response` block (a hard-wrapped sentence un-wrapped
into one line), so per `scoring-rubric.md` the phase agent rejected it and re-ran the scorer for
that transcript rather than repairing it. This table counts **probe** subagents — the runs that
produce measurements. A scorer re-reading an existing transcript produces no new measurement and
selects nothing, which is exactly what the retry rule is there to prevent. Recorded here so the
distinction is on the record and not inferred. Full account in
`docs/skill-evidence/systematic-debugging.md` under *Protocol events, honestly*.

The **REFACTOR row is 0 by construction, not by omission.** Arm A scored 4 of 4, so `plan.md`'s
pre-registered branch **(a)** fired and evaluation stopped; REFACTOR is reachable only via branch
(d). `systematic-debugging` reverts to A′ with its ≤4 REFACTOR allotment unspent.

**Headroom after Task 17: 83 runs against the 122 global ceiling** — 39 spent. The arm-B raise
above lifted one *stage* ceiling, not this one; it is why the stage ceilings now sum to ≈123
while the global figure stays 122, as the resolution note records. Per-stage after this phase: *Arm A on held-out* **8 of 20 spent**, 12 left and 12
needed (3 skills × 4) — **exactly zero slack**, unchanged. *Arm A′ on held-out* **8 of 16
spent**, 8 left and 8 needed (2 discipline skills × 4) — **exactly zero slack**, unchanged.
*Arm B on held-out* **9 of 21 spent**, 12 left and 12 needed — **exactly zero slack**, which is
what the ceiling raise bought and no more. **Any retry in any of those three rows from here on
crosses a ceiling**, and §7.3's rule then applies: halt and record a null.

Two transcripts (`code-review/42a94a.md`, `code-review/d7006e.md`) needed their `## Scenario`
block repaired after the fact — the probes abridged their copy of it. **This cost no runs**:
the block was restored from the checked-in `code-review-1.md`, and the `## Response` blocks
were left byte-identical. See `docs/skill-evidence/code-review.md` for the full record.

**Task 18's 16 runs, in detail:** 3 arms × 2 held-out scenarios × 2 samples = **12** planned,
plus a **4-run unaided control**, **zero retries** — every probe wrote its transcript and returned
a well-formed confirmation on first dispatch, and every arm probe appended its `## Meta-test`
block on the follow-up turn. Model `sonnet`, `general-purpose` subagents, per `plan.md` C5.

**One clarifying question was re-asked of the four control probes, and that is not a charged
run.** Their first confirmation line asked whether *"that file"* contained a skill; all four
answered about the transcript file they had written rather than the prompt file they had read, so
the answer said nothing. A follow-up turn in the same session asked it with an explicit referent
and required the prompt file's first line as corroboration. Same rule as Task 17's re-scored
verdict: this table counts **probe** dispatches, and a resumed session answering a clarifying
question produces no new measurement and selects nothing.

The **REFACTOR row is 0 by construction, not by omission.** Arm A scored 4 of 4, so `plan.md`'s
pre-registered branch **(a)** fired and evaluation stopped; REFACTOR is reachable only via branch
(d). `verification-before-completion` reverts to A′ with its ≤4 REFACTOR allotment unspent.

**Task 18's unaided control came back 4 of 4** — an agent given no skill text complied on both
held-out scenarios, both samples, against 4/4 for each of A, A′ and B. Unlike `tdd`'s control
(3 of 4), this pair has **zero** discriminating power, and the phase's branch-(a) revert therefore
rests on an absence of evidence rather than on arm A being shown adequate. Full record in
`docs/skill-evidence/verification-before-completion.md` under *Unaided control*. **It is scoped to
`verification-before-completion`**: `code-review` and `using-drovr` still have no unaided
condition, and neither `tdd`'s result nor this one transfers to them.

**Headroom after Task 18: 67 runs against the 122 global ceiling** — **55 spent**. Per-stage, all
three held-out rows still have **exactly zero slack**: *Arm A on held-out* **12 of 20**, 8 left and
8 needed (2 skills × 4); *Arm A′ on held-out* **12 of 16**, 4 left and 4 needed — `code-review` is
the last skill in that row, because `using-drovr`'s A′ runs belong to the no-skill-applies row, not
this one; *Arm B on held-out* **13 of 21**, 8 left and 8 needed. **Any retry in any of those rows
crosses a ceiling**, and §7.3's rule applies: halt and record a null.

**What the global ceiling now permits, stated as arithmetic so Tasks 19–21 do not have to
re-derive it.** Remaining planned work is Task 19 (12) + Task 20 (4 + 4 + 12 = 20) + Task 21
(`ab-voice`, 24) = **56**, which lands at **111 of 122**. That leaves **11 runs of global slack**,
and there are **four** unbudgeted 4-run claims outstanding against it: a REFACTOR loop for Task 19,
a REFACTOR loop for Task 20, an unaided control for `code-review`, and an unaided control for
`using-drovr`. That is up to 16 runs against 11. **At most two of the four fit; a third crosses
122.** Concretely: two controls and no REFACTOR lands at 119; one control and both REFACTOR loops
lands at 123 and **crosses**. A phase that wants more than two must escalate before spending,
exactly as `ab-tdd` did with the arm-B row — not discover it at the last run.

## 2026-08-05 — `harden-scenarios`: all ten held-out bodies replaced, **0 runs spent**

**Authoring is free and this phase spent none of the budget. The count stays at 55 of 122.**

The phase rewrote every held-out scenario — `<skill>-2.md` and `<skill>-3.md` for all five
skills — because three stages plus two unaided controls established that the pair could not
separate the arms. The `dev` scenarios and the two `using-drovr-noskill-<n>.md` veto-class
scenarios are **untouched**: arms were authored against the dev set, and the veto class has never
been measured, so neither had a ceiling to fix.

**The consequence for every count already in this ledger.** The 36 held-out bar runs and the 8
unaided control runs were measured on bodies that no longer exist at those paths. Each affected
evidence file now opens its scored-results section with a *Held-out scenario provenance* block
naming the blob the probes actually read, and
`skills_valid.rs::held_out_measurements_name_the_scenario_body_they_ran_on` recomputes the
`CURRENT`/`SUPERSEDED` verdict from `git hash-object` rather than reading it — so the claim fails
if it stops being true in either direction. **§9 must not pool pre-rewrite and post-rewrite counts
into one rate.** They are two instruments.

**What is still owed against the ceiling is unchanged in size but not in meaning.** The
per-stage rows in the section above still have zero slack, and the four unbudgeted 4-run claims
still contend for 11 runs of global slack. What has changed is that the ~20 runs the driver
plans to spend proving these scenarios discriminate are **a new claim on that same slack**, not a
free addition — so the arithmetic above has to be re-derived before those runs are spent, not
after.

## 2026-08-06 — `discrimination-test`: 20 runs, does the rewritten corpus discriminate?

**20 runs spent, zero retries, cumulative 75 of 122.** 5 skills × 2 held-out scenarios × 2
samples, unaided. **No arm was measured**, and no `spec.md` §7.3 row was touched: this stage
measures the *instrument* — whether the bodies `harden-scenarios` wrote can be failed by an agent
given no skill at all — and it enters no pre-registered bar.

**The bar was pre-registered before any run**, per skill over its 4 runs: **≤1 of 4** compliant is
good dynamic range · **2 of 4** marginal · **≥3 of 4** still saturated. The result:

- `tdd` **0 of 4** — good range · `systematic-debugging` **0 of 4** — good range
- `verification-before-completion` **2 of 4** — marginal · `using-drovr` **2 of 4** — marginal
- `code-review` **3 of 4** — **SATURATED**
- **7 of 20 compliant unaided overall**, against 16 of 16 on the superseded bodies.

**Per-scenario, which is where the decisions are** — the pair-level number hides a 0/2 beside a
2/2 in three of the five skills. Scenarios at **0 of 2**: `tdd-2`, `tdd-3`, `sd-2`, `sd-3`,
`vbc-3`, `ud-2`. At **1 of 2**: `cr-2`. At **2 of 2**, i.e. saturated: `vbc-2`, `cr-3`, `ud-3`.

### Method

Each run: a fresh `general-purpose` subagent on `sonnet` (`plan.md` C5). Per C5a each probe wrote
its own response file and returned a one-line confirmation; **no probe's words entered the
orchestrator's context** at any point, including during assembly and scoring.

1. **Ten prompt files, verified byte-exact.** Each = the harness preamble with Task 16's single
   sentence changed to the unaided form, then the scenario body between
   `----- BEGIN SITUATION -----` / `----- END SITUATION -----`. A script extracted each region and
   compared it to the scenario file; **all 10 matched**, all 10 opened with the unaided sentence,
   and all 10 were positively asserted to contain **no** skill region. The script fails loudly on
   a missing delimiter, an empty region, or a checked-file count other than 10.
2. **The preamble's provenance, stated exactly.** The prompt-file hash Tasks 16–18 recorded
   (`5a6a5d3d68eaf2fe17d02f160bc37d064f38d414`) **could not be reproduced** — that file evidently
   carried more than the preamble text, and it no longer exists. Byte-identity was therefore
   established against the **verbatim quote** in `tdd.md` under *Method*, extracted
   programmatically and diffed: identical. Recorded as a weaker link than a hash match, because
   it is one.
3. **The probes wrote to neutral scratch paths**, not to `transcripts/<skill>/<id>.md`. Every
   prior stage handed each probe an output path naming the skill under test — a hint in exactly
   the direction that inflates unaided compliance. The orchestrator copied the files in
   afterwards, which keeps C5a intact for the reason `code-review.md` already gives: the probe's
   words still never pass through the orchestrator's context as tool output.
4. **The probe wrote only `## Response`.** `## Forced choice` and `## Scenario` were prepended by
   the phase agent from the checked-in scenario file, so `correct_option` never reached a probe.
   Assembly refuses a missing file, a file not beginning with `## Response`, an empty response, a
   second prepend, and a `## Meta-test` block — an unaided run has no skill to ask about, so
   `meta_test_clear` is `false` on all 20 **by rule, not by measurement**. Each response block was
   SHA-256-verified byte-identical before and after assembly.
5. **Scoring was sealed and split.** Transcripts and a `git hash-object`-verified copy of
   `scoring-rubric.md` (`1a2b1c552071192bcbeb5660ead5ef492b43275f`, the value Tasks 17 and 18
   record) were copied into five scratch directories; the scorers wrote outside the evidence tree
   and never had a blind map within reach. **The 20 transcripts were shuffled across skills**, so
   no scorer held one skill's 2×2 block.
6. **Joined to the blind maps only after every verdict was recorded.** The maps were written
   before scoring, one per skill, all `arm: "none"`.

### Positive control

The hazard is a probe that silently failed and produced a plausible transcript. Inverted, as
Task 18's control was: these probes had to prove they received **no** skill.

- **20 of 20 quoted the first line of the file they read, verbatim, as `Below is a situation you
  are in right now.`** — the unaided preamble's opening, which no arm prompt carries.
- **The announcement substitution fired 0 times**, against a deliberately *over-broad* candidate
  set: every fenced line of 30+ characters from all five `SKILL.md` files, 92 sentences, a
  superset of the four announcements. Nothing from those files was read into context.
- **20 of 20 response bodies are distinct texts** — the check against a harness that dispatched
  one probe and copied it.
- `git status` after all 20 runs showed **no file changed outside the scratch directory**. The
  preamble's sandbox constraint held for the fourth stage running.

### A second blind pass, and it is not a charged run

Every one of the 20 verdicts was re-read by a **second** set of five blind agents, on a
**different** shuffle, asked one question: which option did the response commit to, and does it
match the key? `matches_key` was then recomputed against the key rather than trusted.
**20 of 20 agreed with the scorers on `compliant`, and 20 of 20 recomputed correctly.** Per this
table's own distinction — it counts **probe** dispatches, and an agent re-reading an existing
transcript produces no new measurement and selects nothing — the second pass is not charged.

### `plan.md` C5's FOREGROUND rule was again not honoured

The harness dispatched every subagent asynchronously without being asked to, for the fourth stage
running (`plan-HANDOFF.md` dead-end 4). Recorded rather than papered over. The measurement is
unaffected — the 20 cells are mutually independent, each probe wrote only its own file, and every
one was confirmed complete before any transcript was assembled or scored — but the single-writer
property C5 protects was again held by the sandbox, not by the scheduling.

### What the global ceiling now permits — re-derived, and it does not close

**75 of 122 spent. 47 remain.** Remaining planned work is unchanged in size: Task 19 (12) +
Task 20 (4 + 4 + 12 = 20) + Task 21 `ab-voice` (24) = **56**. That lands at **131 of 122** and
**crosses the global ceiling by 9 runs**, before any of the four unbudgeted 4-run claims the
section above already records as contending for slack that no longer exists.

**This phase does not resolve that, and must not.** It is the run-level call the ledger's standing
rule reserves — *decide what gets dropped before spending, not at the last run*. What this stage
contributes is evidence that bears directly on the cheapest way out: **`code-review`'s held-out
pair came back saturated at 3 of 4**, so Task 19's 12 runs would be spent on an instrument already
shown unable to separate the arms. Deferring Task 19 until `code-review-3` is rewritten lands the
run at **119 of 122** and needs nothing else cut. **Escalated in `discrimination-test-HANDOFF.md`
rather than decided here.**

## 2026-08-06 — `remeasure-tdd`: 12 runs, the bars re-applied on an instrument with range

**12 runs spent, zero retries, cumulative 87 of 122.** 3 arms × 2 held-out scenarios × 2
samples, for `tdd` only, against the bodies `harden-scenarios` wrote. **These are §7.3 rows** —
the same three *Arm … on held-out* rows Task 16 charged — and they are charged there rather than
under a new non-§7.3 label, because they are the same kind of spend and hiding them under a fresh
name would understate the pressure on those ceilings.

**Why the stage exists.** Task 16 reverted `tdd` on branch (a) with arm A at 4 of 4 — measured on
a pair an unaided agent also passed 3 times in 4. `discrimination-test` then measured the
rewritten pair at **0 of 4 unaided**, the strongest of the five. The verdict was sound on its own
terms and made on an instrument with almost no dynamic range; this stage re-applies the same
pre-registered bars where a passing arm can mean something. **Task 16's verdict is superseded,
not deleted** — its counts stay in `tdd.md` with their `SUPERSEDED` provenance rows.

### What it cost against the per-stage ceilings, stated before anything else

Written as prose, not as a table: this file's parser reads **every** line after the header that
begins with `|` as a data row, so a second table here would be counted as spend.

- *Arm A on held-out* — 12 spent before, +4, now **16 of 20**; 4 left.
- *Arm A′ on held-out* — 12 spent before, +4, now **16 of 16**; **0 left**.
- *Arm B on held-out* — 13 spent before, +4, now **17 of 21**; 4 left.

**The Arm A′ row is now exactly at its ceiling, and Tasks 19 and 20 need 4 each.** Nothing was
crossed — 16 of 16 is at the ceiling, not past it — and this stage was authorised at 12 runs with
the ledger read first, so it spent what it was given. But the row is now closed, and
`ab-code-review` and `ab-using-drovr` cannot run their A′ arm without a run-level decision.
**This stage does not make that decision**, for the same reason `discrimination-test` did not make
the global one: the ledger's standing rule reserves it, and it compounds a global overrun that was
already escalated. Both are escalated together in `remeasure-tdd-HANDOFF.md`.

**Global: 87 of 122, 35 remain.** Remaining planned work is Task 19 (12) + Task 20 (20) +
Task 21 `ab-voice` (24) = **56**, landing at **143 of 122** — the 9-run overrun
`discrimination-test` recorded, plus this stage's 12. That arithmetic is the escalation, not a
side effect of it: the human authorised these 12 knowing the total, and what changes is the size
of the cut the run-level call has to make.

### Method

Tasks 16–18's method, with the three strengthenings they asked their successors to copy, plus two
this stage added. Each run: a fresh `general-purpose` subagent on `sonnet` (`plan.md` C5). Per C5a
each probe wrote its own response file and returned a one-line confirmation; **no probe's words
entered the orchestrator's context** at any point, including during assembly and scoring.

1. **Twelve prompt files, assembled mechanically and verified byte-exact** — one per run rather
   than Task 16's one per cell, so the output path is inside the verified file. Each = the harness
   preamble, the arm text between `----- BEGIN SKILL -----` / `----- END SKILL -----`, and the
   scenario body plus its three rendered options between `----- BEGIN SITUATION -----` /
   `----- END SITUATION -----`. A script extracted each region and compared it by
   `git hash-object` to the arm snapshot and to the scenario body: **all 12 matched**, all 12
   carried all three options verbatim, and all 12 were positively asserted to contain no
   `correct_option`, `forced_choice:`, `tag:` or `pressures:` line. Prompt files are named by the
   run's opaque id, so a probe cannot read its arm off the path.
2. **The verifier was mutation-checked before it was trusted.** An unmutated copy was confirmed
   GREEN first — without that control, a "red" says only that the copy is broken — and then seven
   mutations each turned it red: a swapped arm, a reworded option, a leaked `correct_option`, a
   one-word body edit, a deleted file, a corrupted preamble, a wrong output path. This is the
   direct answer to Task 16's *"if you write a verification script this phase, make its not-found
   path loud"*, and the control-first ordering is what that lesson was missing.
3. **The probe wrote only `## Response`; the meta-test was a separate file in a later turn.**
   `## Forced choice` and `## Scenario` were prepended by the phase agent from the checked-in
   scenario file, so `correct_option` never reached a probe. Assembly refuses a missing file, a
   file not beginning with its block header, an empty block, a second assembly, and a probe that
   wrote a block only the phase agent may write. The 12 response files were SHA-256-verified
   **byte-identical before and after the meta-test turn**.
4. **Scoring was sealed and split to one scorer per transcript.** Twelve sealed directories, each
   holding exactly two files: one transcript and a `git hash-object`-verified copy of
   `scoring-rubric.md` (`1a2b1c552071192bcbeb5660ead5ef492b43275f`, the value Tasks 17, 18 and
   `discrimination-test` record). No scorer had a blind map, an arm snapshot or a second
   transcript within reach; every scorer wrote outside the evidence tree. **This closes `tdd.md`
   open item 5** — the rubric prescribes one scorer per set while also requiring that transcripts
   be scored independently and never compared, and one agent holding twelve cannot honour both.
   One scorer per transcript removes the tension rather than restating it in a brief.
5. **Joined to the blind map only after every verdict was recorded**, and the map was written
   before any scorer ran.

### Positive control — two independent mechanisms, agreeing on all 12 cells

Each probe returned three facts derivable only from the text it was given: the verbatim
`description:` line, the verbatim last non-empty line of the skill region, and that region's line
count. **The `description:` line separates A from {A′, B} — that difference *is* fix 1 — and the
last line separates B from {A, A′}, so the pair is jointly sufficient; the line count is not, and
this stage is why.**

- **11 of 12 confirmed on the first return.** One (`62cfb0`, an A′ cell) reported a line count of
  29 and a "last line" that occurs at line 28 of all three arms — a fingerprint that identifies no
  arm. It was re-asked, with the region boundaries restated and no expected value named, and
  returned the A′ fingerprint exactly: 44 lines, ending `and confirm with evidence, not
  assertion.` The re-ask is a question about the prompt, not a new measurement: **the response had
  already been written and was byte-unchanged**, so nothing was selected on. **Not charged as a
  retry**, and the reason is the distinction the ledger already draws — a retry re-runs a probe
  and produces a new response to choose between; this produced none.
- **The line-count leg proved unreliable and is recorded as such**: three probes counted the
  delimiter lines or unwrapped a hard-wrapped paragraph (44→46, 175→177, and `62cfb0`'s 29). No
  arm assignment ever rested on it.
- **The announcement substitution fired exactly 4 times — once in each arm-B cell, and 0 times in
  every A and A′ cell.** Arm B is the only arm carrying an announcement sentence, so a second
  mechanism, independent of what any probe reported, agrees on the arm of all 12 runs.
- **12 of 12 response bodies are distinct texts** — the check against a harness that dispatched
  one probe and copied it.
- `git status` after all 12 runs showed **no file changed outside the scratch directory**. The
  preamble's sandbox constraint has now held for five stages.

### A second blind pass, and it is not a charged run

All 12 transcripts were re-read by a **second** set of twelve blind agents — one per transcript,
no rubric, no arm labels, no blind map — asked only which option the `## Response` block commits
to and which quotes it advances for an option it does not take. `matches_key` was then recomputed
against the transcript's own key rather than trusted. **12 of 12 agreed with the scorers on
`compliant`, 12 of 12 recomputed correctly, and 0 quotes were advanced for an option not taken.**
Recorded at `transcripts/tdd/remeasure-adjudication.json`, and enforced in two places:
`skills_valid.rs::scores_json_verdicts_obey_the_rubric` resolves a re-adjudication **per verdict
bundle**, so this file is cross-checked exactly as Task 16's is; and
`remeasure_stage_records_the_bodies_it_ran_on` **requires it to exist and to hold one record per
run**. The second half was missing from the first version of this guard — the file was validated
when present and silently optional when absent, so deleting it would have deleted this paragraph's
evidence and left the suite green while this paragraph still claimed the check ran. **The review
panel found that, not the tree.** Per this table's own distinction — it counts **probe**
dispatches — a re-read of an existing transcript is not charged.

### `plan.md` C5's FOREGROUND rule was again not honoured

The harness dispatched every subagent asynchronously without being asked to, for the fifth stage
running (`plan-HANDOFF.md` dead-end 4). Recorded rather than papered over. The measurement is
unaffected — the 12 cells are mutually independent, each probe wrote only its own file, and every
one was confirmed complete before any transcript was assembled or scored — but the single-writer
property C5 protects was again held by the sandbox, not by the scheduling.
