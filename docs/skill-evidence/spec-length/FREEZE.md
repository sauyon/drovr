# Freeze record — the spec-length A/B

This experiment asks whether `skills/pipeline/phase-prompts/brainstorm.md`'s spec-authoring
instruction can be made **shorter without losing key points**. Every arm is graded against a
**key-point ledger**, and the ledger is what this file freezes.

**The ledger's only inputs are the three control `spec.md` files under `fixtures/`.** No prompt
text — not the control arm `S0`, not the shipped rewrite `S1`, not any shorter candidate — is ever
an input to a ledger row. An arm written before the ledger was fixed, or a ledger revised once an
arm's weaknesses were visible, is the experiment grading itself, and every measurement taken
against it is void. That is the failure this file exists to make impossible, and the reason it is
written and committed one task before the first candidate arm exists.

**This file is append-only.** Later tasks add rows for the arms they author (`S1`, then `S2`/`S3`);
none of them rewrites a row above. Correcting a hash means the frozen artifact changed, which is
the thing that must not happen — so a wrong hash is a finding, not an edit.

## The hash column

Every hash is a `git hash-object` blob SHA, not a raw SHA-256. **Compute it with
`git hash-object --no-filters <path>`.** Without `--no-filters` the value also depends on the
invoking user's `core.autocrlf` and on any `.gitattributes` in scope, so the same bytes can hash
differently on another machine — which would read as a corrupted freeze when nothing had changed.
This is the same rule, for the same reason, as `docs/skill-evidence/arms/MANIFEST.md`.

## The `frozen at commit` column is NOT `MANIFEST.md`'s commit column

`MANIFEST.md`'s `commit HEAD at copy time` cell must name a commit that **contains** the recorded
blob at the recorded source path, and `manifest_commits_contain_their_snapshots` enforces it. That
is right there, because a MANIFEST row's `source path` is usually a *live* file whose content moves
on; the commit is what lets a reader recover the exact text an arm measured.

The column below answers a different question: **what was `HEAD` when the freeze was taken.** It is
provenance for the derivation, not a containment claim — most plainly for `S0.md`, whose content is
defined as `brainstorm.md`'s spec-authoring instruction *at that commit* and is recoverable with
`git show <that commit>:skills/pipeline/phase-prompts/brainstorm.md`.

It is deliberately **not** the commit that introduced each frozen file. Git already answers that
(`git log --diff-filter=A --format=%H -1 -- <path>`), and it is exactly what
`cli/tests/skills_valid.rs::freeze_precedes_every_candidate_arm` reads to order the arms. Recording
it here by hand would be a second copy of a fact that can drift from the first, with no check able
to say which copy was right. One authoritative answer, held by git.

## The frozen sentinels

`brainstorm.md`'s spec-authoring section is delimited by these two comment lines, **verbatim**:

```
<!-- SPEC-AUTHORING-SECTION:BEGIN — frozen arm; see docs/skill-evidence/spec-length/FREEZE.md -->
<!-- SPEC-AUTHORING-SECTION:END -->
```

**Extraction is always this command, never a line range:**

```
awk '/SPEC-AUTHORING-SECTION:BEGIN/{f=1;next}/SPEC-AUTHORING-SECTION:END/{f=0}f' <file>
```

A line range cannot survive the surrounding steps being renumbered, and they will be. The sentinel
strings are part of the frozen contract: changing either string breaks every later hash check, so
they are recorded here rather than left implicit in whichever task last touched the file.

The task that places the sentinels around an arm freezes **whatever that extraction emits**, so a
stored `S<n>.md` (`n >= 1`) is by construction what the canonical command produces — no list
numeral, exactly one trailing newline. A numeral, if the surrounding list needs one, lives *outside*
the `BEGIN` sentinel.

`S0.md` predates the sentinels and so is defined by an exact recipe instead: `brainstorm.md` lines
32-33 at `370e211174fcb23cfc48a9732fc528754e9b02c6`, with the three-byte list numeral `3. ` stripped
from the head of the first line, the second line's existing three-space continuation indent **left
as-is**, and exactly one trailing newline. The dangling indent is deliberate — it is what makes the
stored bytes and the in-file bytes identical.

**`S0` is a measurement baseline and is never shipped.** It ends "…scope boundaries, **open
questions**", which is the instruction the spec's locked decision 6 forbids. The length bar for a
candidate arm is `S1`, the mandated rewrite — beating `S0` is not an achievement.

## The freeze

| path | `git hash-object --no-filters` | frozen at commit | date |
|---|---|---|---|
| `docs/skill-evidence/spec-length/fixtures/skill-stickiness.spec.md` | `79525341f6c4699417fc1f8b6b20d84b8ddaacad` | `370e211174fcb23cfc48a9732fc528754e9b02c6` | 2026-08-06 |
| `docs/skill-evidence/spec-length/fixtures/tiered-review.spec.md` | `7f11a08cff6cd95999e55eb353ad38c250b7a78d` | `370e211174fcb23cfc48a9732fc528754e9b02c6` | 2026-08-06 |
| `docs/skill-evidence/spec-length/fixtures/tui-dc-picker.spec.md` | `12ceaa75f26125c93835587815a21e43e96de6b1` | `370e211174fcb23cfc48a9732fc528754e9b02c6` | 2026-08-06 |
| `docs/skill-evidence/spec-length/ledger/skill-stickiness.md` | `53b10033680f27ca10584c3e5b32cfec300e6527` | `370e211174fcb23cfc48a9732fc528754e9b02c6` | 2026-08-06 |
| `docs/skill-evidence/spec-length/ledger/tiered-review.md` | `c0e58fa4467b7a3714b250812cfae289c4fe2c02` | `370e211174fcb23cfc48a9732fc528754e9b02c6` | 2026-08-06 |
| `docs/skill-evidence/spec-length/ledger/tui-dc-picker.md` | `477b32b340e7bb3373fb80d623a9c1b897bc4d03` | `370e211174fcb23cfc48a9732fc528754e9b02c6` | 2026-08-06 |
| `docs/skill-evidence/arms/spec-length/S0.md` | `db89be9ee06913386afcb6f1053597fdb9728a3a` | `370e211174fcb23cfc48a9732fc528754e9b02c6` | 2026-08-06 |

## Who appends what, and when

| task | rows it appends |
|---|---|
| T1 (this one) | the fixtures, the three ledgers, `S0.md` |
| T6 | `S1.md` — the shipped rewrite, frozen the moment its sentinels are placed |
| T8 | `S2.md`, `S3.md` — the shorter candidates, each frozen when it is authored |

`FREEZE.md` is the single **hash** record every sentinel check reads; `MANIFEST.md` is the
**provenance** record. Both get a row for every arm; neither substitutes for the other. Without a
`FREEZE.md` row for a winning arm, T9's byte-for-byte verification has nothing to compare against.

## What checks this

- `cli/tests/skills_valid.rs::freeze_precedes_every_candidate_arm` — every
  `docs/skill-evidence/arms/spec-length/S<n>.md` with `n >= 1` was introduced by a commit that
  descends from the commit introducing **this file**. Zero such arms is a correct state (it is the
  state at T1) and does not fail; an arm on disk with no introducing commit does fail, because a
  draft that history cannot place is a draft this check cannot speak to.
- `cli/tests/skills_valid.rs::freeze_rows_still_hash_to_their_files` — **every row above is
  re-hashed on every test run.** This is what makes the freeze a freeze rather than a claim: a
  fixture spec or a key-point ledger edited on disk after the fact — a ledger quietly revised once a
  candidate arm's weaknesses were visible is exactly the contamination this experiment is built to
  prevent — turns the suite red instead of passing unnoticed. An empty table fails too; a freeze
  record with no rows has never been a correct state.

  It deliberately does **not** check the `frozen at commit` cell against history, for the reason in
  "The `frozen at commit` column is NOT `MANIFEST.md`'s commit column" above: for the fixtures and
  ledgers that commit is one at which those paths did not yet exist, so a containment assertion
  would fail by design.
- `cli/tests/skills_valid.rs::spec_length_ledgers_are_the_closed_lists_they_claim` — each ledger's
  `| id | kind | item |` shape, its `kind` vocabulary, its gap-free id sequence, and its declared
  row count against the table beneath it. The ledgers to check are discovered from the rows above,
  not from a second list that could forget one.

Re-verifying by hand is still what a task owes at a gate rather than at CI time —
`git hash-object --no-filters <path>` for each row. T8's start gate does exactly this before its
first probe, so the freeze is confirmed at the moment it is relied on and not merely at some point
since.
