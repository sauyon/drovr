# Freeze record — the spec-length A/B's third tier-1 instrument

This is the hash record for what **`PROTOCOL-3.md` freezes**, and for nothing else. There are now
three freeze records in this directory and none supersedes another:

- **`FREEZE.md`** holds the fixtures, the three ledgers and the four arms. Both attempts reuse them
  unchanged, and it is **not edited by this run**.
- **`FREEZE-2.md`** holds `PROTOCOL-2.md`, the 18 generations of `generated-2/`, and the two map
  files. It is **not edited by this file**.
- **This file** holds `PROTOCOL-3.md`, the third tier-1 instrument, and nothing that either of the
  others already froze.

**Nothing here re-freezes anything `FREEZE.md` or `FREEZE-2.md` already froze.** A second row for the
same path would leave two records claiming one identity with no check able to say which was right.
`spec_length_3_freeze_rows_still_hash_to_their_files` cross-references **both** of the earlier
records, which is one more than `spec_length_2_freeze_rows_still_hash_to_their_files` does — that
check compares against `FREEZE.md` only, and `FREEZE-2.md` was never cross-referenced by anything.
The gap is recorded here rather than repeated.

**In particular, the corpus is not re-frozen and is not touched.** `generated-2/`, `blind-map-2.json`,
`fixture-map-2.json`, the ledgers and the fixtures are exactly as `FREEZE-2.md` and `FREEZE.md` left
them. `PROTOCOL-3.md` revises an **instrument**; a revision that reached the corpus would not be an
instrument revision at all, and the absence of any corpus row here is what makes that checkable.

**This file is append-only.** A wrong hash means the frozen artifact changed, which is the thing that
must not happen — so it is a finding, not an edit. `FREEZE.md`'s closing section records one breach
of exactly that rule and its root cause: **the freeze happened before the checking finished.** Finish
every check the protocol names, then freeze.

## The hash column

Every hash is a `git hash-object` blob SHA, not a raw SHA-256. **Compute it with
`git hash-object --no-filters <path>`.** Without `--no-filters` the value also depends on the
invoking user's `core.autocrlf` and on any `.gitattributes` in scope, so identical bytes can hash
differently on another machine and read as a corrupted freeze when nothing had changed. Same rule,
same reason, as `FREEZE.md`, `FREEZE-2.md` and `docs/skill-evidence/arms/MANIFEST.md`.

## The `frozen at commit` column

**What `HEAD` was when the freeze was taken.** Provenance for the derivation, not a containment
claim, and deliberately **not** a hand-copied record of the commit that introduced the path — git
already answers that, and `cli/tests/skills_valid.rs` reads git for it rather than this column. A
second copy of a fact that can drift from the first would leave no check able to say which copy was
right.

For the row below the two coincide: the freeze was taken immediately after the commit that introduced
`PROTOCOL-3.md`, with nothing else in that commit. That is a fact about this row, not a rule about
the column.

## The freeze

| path | `git hash-object --no-filters` | frozen at commit | date |
|---|---|---|---|
| `docs/skill-evidence/spec-length/PROTOCOL-3.md` | `959e3f03ad31ba684b5c0c36d17bbf0ae8614606` | `2bee0ee49f8744376ce35ab7b46f72fa6d5c8675` | 2026-08-11 |

## What this file does not freeze, and why

**`retention-3/` is not frozen here, and no later task should add it.** The verdicts are the *output*
of the instrument this file freezes; freezing an output would make a re-run under `R6` — which
`PROTOCOL-3.md` §3 keeps for every class-A failure — a freeze breach rather than the remedy the
protocol names. `FREEZE-2.md` takes the same position on `retention-2/` by simply never listing it.

**`SCORING-3-NOTES.md` is not frozen here.** It is the running record of the pass, appended to as the
pass runs, and `PROTOCOL-3.md` §6 requires it to carry every amendment SHA — a file that must keep
growing cannot also be hash-frozen.
