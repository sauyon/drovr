# Arm snapshot manifest

Every measurement arm in the `skill-stickiness` run is a frozen copy of the five methodology skill
files, taken at the moment that arm's text existed on disk. The live `skills/*/SKILL.md` files move
out from under each arm as the fixes land — arm A in particular is unrecoverable without a checkout
once fix 1 rewrites the `description:` lines it measures — so the snapshots under `arms/<arm>/` are
what every probe run pastes into its subagent prompt, and this table is what makes "byte-exact"
checkable.

A snapshot is the **whole file including frontmatter**: the `description:` line is itself under test.

**This table is append-only.** Each snapshotting task adds its rows and rewrites nothing.

The hash column is a `git hash-object` blob SHA, not a raw SHA-256. **Compute it with
`git hash-object --no-filters <path>`.** Without `--no-filters` the value also depends on the
invoking user's `core.autocrlf` and on any `.gitattributes` in scope, so the same bytes can hash
differently on another machine — which would read as arm corruption when nothing had changed. There
is no `.gitattributes` in this repo today, so the two forms agree on every value below; the flag is
what keeps them agreeing.

`cli/tests/skills_valid.rs::arm_a_snapshots_match_manifest` re-checks the arm A rows on every test
run; later tasks re-check their own arm before using its text. Rows are matched on their `arm` and
`skill` cells (exact string equality — `A-prime` is not `A`), and only lines after the header row
count as data.

**The header row is the schema, and the schema is closed.** `parse_manifest` resolves each column by
its header text — never by position — so the columns may be reordered, but they may not be renamed,
dropped, duplicated, **or added to**: any of those is a parse error, not a silently rebound or
silently ignored field. A seventh column would carry evidence no reader ever sees, so adding one
means editing `REQUIRED_COLUMNS` and `ManifestRow` in `cli/tests/skills_valid.rs` too — deliberately,
not as a side effect of appending a row. Header cells are compared with
backticks dropped, whitespace collapsed, and case folded, so `` `git hash-object` of the copy ``
matches the key `git hash-object of the copy`. A row is recognized as the header only if it carries
**all six** columns, so an illustrative fragment like `| arm | skill |` in this preamble is passed
over rather than mistaken for the real thing — but a *complete* six-column table above the data
table would be, so do not write one.

Further rules follow from the parser being strict for the whole file at once:

- **No cell may contain a literal `|`** — there is no escape handling.
- **Every data row must have all six cells; the hash and commit cells must each be a 40-hex git
  object id.** Use the full SHA, not the short form. A malformed row for *any* arm fails the parse,
  which fails the arm A tripwire too. Append carefully.
- **`(arm, skill)` is the key — one row per pair.** A second row for a pair already present is
  rejected, so re-snapshotting an arm means correcting its row, not appending another. (This is the
  one place the append-only rule yields: a duplicate would leave two hashes claiming the same
  identity, which is worse than an edit with a reason.)
- **The `skill` cell must own the `source path`** — the skill name has to be either the path's file
  stem or its parent directory. `skills/tdd/SKILL.md` qualifies for `tdd` (parent), and
  `…/voice/V0.md` qualifies for `V0` (stem), so both the per-skill arms and the voice arm fit one
  rule. A row recording one skill's file under another skill's name will not parse.
- **Run `cargo test --test skills_valid` the moment you append a row**, not at the end of your task.
  It is under a second, and it turns "the manifest is corrupt" into a one-line diagnosis naming your
  row instead of a mystery failure inherited by whoever comes next.
- **No line after the table may begin with `|`.** Once the header is seen, every such line is read
  as a data row and is checked for the six cells. A short row is an error, never a silent drop —
  a dropped row would read as "that arm was never snapshotted" instead of "this file is corrupt".

| arm | skill | source path | `git hash-object` of the copy | commit `HEAD` at copy time | date |
|---|---|---|---|---|---|
| A | tdd | `skills/tdd/SKILL.md` | `a1f889b57fa741e55b02da2397104f933d9878aa` | `99540bdcdb016ca3b74530957f55c0e5ef29f4f9` | 2026-07-26 |
| A | systematic-debugging | `skills/systematic-debugging/SKILL.md` | `d69a226c161d733f2238e74187237d2b77d5c196` | `99540bdcdb016ca3b74530957f55c0e5ef29f4f9` | 2026-07-26 |
| A | verification-before-completion | `skills/verification-before-completion/SKILL.md` | `1d0cfad3da2755908dfa577e71da373990baaeef` | `99540bdcdb016ca3b74530957f55c0e5ef29f4f9` | 2026-07-26 |
| A | code-review | `skills/code-review/SKILL.md` | `db0fd4310cb7a543655bae8419b9309965c35b7d` | `99540bdcdb016ca3b74530957f55c0e5ef29f4f9` | 2026-07-26 |
| A | using-drovr | `skills/using-drovr/SKILL.md` | `fbc04aa14dc90e05fabd32d147d21c5e16913915` | `99540bdcdb016ca3b74530957f55c0e5ef29f4f9` | 2026-07-26 |
