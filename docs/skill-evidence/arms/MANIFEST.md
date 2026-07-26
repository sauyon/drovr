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
`skill` cells (exact string equality — `A-prime` is not `A`), so **no cell may contain a literal
`|`**, and only lines after the header row count as data.

| arm | skill | source path | `git hash-object` of the copy | commit `HEAD` at copy time | date |
|---|---|---|---|---|---|
| A | tdd | `skills/tdd/SKILL.md` | `a1f889b57fa741e55b02da2397104f933d9878aa` | `99540bdcdb016ca3b74530957f55c0e5ef29f4f9` | 2026-07-26 |
| A | systematic-debugging | `skills/systematic-debugging/SKILL.md` | `d69a226c161d733f2238e74187237d2b77d5c196` | `99540bdcdb016ca3b74530957f55c0e5ef29f4f9` | 2026-07-26 |
| A | verification-before-completion | `skills/verification-before-completion/SKILL.md` | `1d0cfad3da2755908dfa577e71da373990baaeef` | `99540bdcdb016ca3b74530957f55c0e5ef29f4f9` | 2026-07-26 |
| A | code-review | `skills/code-review/SKILL.md` | `db0fd4310cb7a543655bae8419b9309965c35b7d` | `99540bdcdb016ca3b74530957f55c0e5ef29f4f9` | 2026-07-26 |
| A | using-drovr | `skills/using-drovr/SKILL.md` | `fbc04aa14dc90e05fabd32d147d21c5e16913915` | `99540bdcdb016ca3b74530957f55c0e5ef29f4f9` | 2026-07-26 |
