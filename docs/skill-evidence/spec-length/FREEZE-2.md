# Freeze record — the spec-length A/B, second attempt

This is the hash record for everything the **second attempt** freezes. The first attempt's record is
`docs/skill-evidence/spec-length/FREEZE.md`, it is **not edited by this run**, and neither file
supersedes the other: `FREEZE.md` holds the fixtures, the three ledgers and the four arms, all of
which this attempt **reuses unchanged**, and this file holds what is new — `PROTOCOL-2.md`, and later
the 18 generations of `generated-2/` and the two map files.

**Nothing in this file re-freezes anything `FREEZE.md` already froze.** A second row for the same
path would leave two records claiming the same identity with no check able to say which was right.
The ledgers and fixtures this attempt is graded against are frozen exactly once, by `FREEZE.md`,
before any arm of either attempt existed — and that, not this file, is what makes the experiment's
rubric un-shapeable by its arms.

**This file is append-only.** Later tasks add rows for the artifacts they freeze; **none of them
rewrites a row above.** Correcting a hash means the frozen artifact changed, which is the thing that
must not happen — so a wrong hash is a finding, not an edit.

**`FREEZE.md` records one breach of exactly that rule, and it is here as the thing not to repeat.**
Its closing section, *"Recorded breach of the append-only rule — `S3`'s row, T3, 2026-08-08"*,
records a row rewritten in place after the arm behind it was corrected. The breach was disclosed
rather than hidden, which is the only reason it is recoverable at all — but its own root cause is the
instruction that binds here: **the freeze happened before the checking finished.** Finish every check
the protocol names, then freeze. `PROTOCOL-2.md` item 2 repeats it as deviation 4 of the first
attempt, where it is one of four.

## The hash column

Every hash is a `git hash-object` blob SHA, not a raw SHA-256. **Compute it with
`git hash-object --no-filters <path>`.** Without `--no-filters` the value also depends on the
invoking user's `core.autocrlf` and on any `.gitattributes` in scope, so the same bytes can hash
differently on another machine — which would read as a corrupted freeze when nothing had changed.
This is the same rule, for the same reason, as `FREEZE.md` and `docs/skill-evidence/arms/MANIFEST.md`.

## The `frozen at commit` column

**What was `HEAD` when the freeze was taken.** It is provenance for the derivation, not a containment
claim, and it is deliberately **not** a hand-copied record of the commit that introduced each path —
git already answers that, and `cli/tests/skills_valid.rs` reads git for it rather than this column.
Recording it here as a second copy of a fact that can drift from the first would leave no check able
to say which copy was right.

For `PROTOCOL-2.md`'s row below the two happen to coincide: the freeze was taken immediately after
the commit that introduced it, so the cell names the introducing commit as well. That is a fact about
this one row, not a rule about the column.

## The freeze

| path | `git hash-object --no-filters` | frozen at commit | date |
|---|---|---|---|
| `docs/skill-evidence/spec-length/PROTOCOL-2.md` | `b928dfcd5a0d80bda5ca2542dc074265e5c81c59` | `ea011be37c8279c5eae6adb743dd78f07e6dea52` | 2026-08-10 |

## Who appends what, and when

The task that generates the 18 new specs appends **21** rows in one commit: the 18
`generated-2/<id>.md` files, `blind-map-2.json` and `fixture-map-2.json`. **Nothing else is appended
by this run** — verdicts, adjudications, escalations, the calibration record and `RESULTS-2.md` are
measurements, and a measurement is evidence rather than a frozen input.

**Appending the two map files is deliberate and it does not publish anything.** A blob hash of
`blind-map-2.json` reveals no cell of it; what the row buys is that the map cannot be quietly rewritten
to fit the verdicts once they are in. `spec_length_2_blind_map_precedes_every_retention_verdict`
checks the same property from git's side, and the two are independent.

## What checks this

- `cli/tests/skills_valid.rs::spec_length_2_freeze_rows_still_hash_to_their_files` — **every row above
  is re-hashed on every test run.** This is what makes the freeze a freeze rather than a claim: a
  frozen artifact edited on disk after the fact turns the suite red instead of passing unnoticed. **An
  empty table fails too**; a freeze record with no rows has never been a correct state, and this file
  has carried one row since the commit that created it.
- `cli/tests/skills_valid.rs::spec_length_2_protocol_precedes_every_generation` and
  `spec_length_2_protocol_stops_moving_before_the_first_probe` — the ordering claims `PROTOCOL-2.md`
  makes about itself, executed against `git log` rather than left to a reader.

**What this record does not close.** `FREEZE.md`'s *"Not detectable, by construction"* section lists
three routes no hash record can close — cherry-picking a pre-freeze commit onto a post-freeze branch,
renaming a file into place while rewriting it, and simply retyping text composed earlier. They are
inherited here unchanged and no check below closes them. What carries the weight is procedural: this
file and `PROTOCOL-2.md` are committed **before any new generation, verdict or measurement exists**,
and `PROTOCOL-2.md` item 1's limitations state what the measurement cannot do.

Re-verifying by hand is still what a task owes at a gate rather than at CI time —
`git hash-object --no-filters <path>` for each row. The generation task does exactly this before its
first probe, so the freeze is confirmed at the moment it is relied on and not merely at some point
since.
