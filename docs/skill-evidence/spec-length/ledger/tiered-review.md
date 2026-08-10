# Key-point ledger — `tiered-review`

Derived from `../fixtures/tiered-review.spec.md` and **nothing else**. No prompt text — no arm, no
phase prompt, no plan — was read to decide a row. See `../FREEZE.md` for why that is the whole
point, and for this file's hash.

A row is **load-bearing**: an implementer holding a candidate spec that omitted it would build
something materially different, or would have to stop and ask. Rationale, motivation, history, and
examples illustrating a point already stated are not rows. `kind` is one of `decision`,
`interface`, `constraint`, `scope`. Ids are stable forever — a later task may not renumber them.

**This is the held-out fixture** (see `../PRE-REGISTRATION.md` when T8 writes it): an arm must clear
`tui-dc-picker` and `skill-stickiness` before this one is scored.

**Closed list: 84 rows.**

| id | kind | item |
|---|---|---|
| tiered-review-01 | constraint | Any cheap-tier failure — unavailable, timed out, crashed, out of context, unparseable, naming a unit outside the range, or silently omitting a unit — must degrade to a full expensive review, never to less review than the non-cascade baseline. |
| tiered-review-02 | decision | Task 0 instruments expensive-tier reviews to measure the exploration-vs-diff cost ratio `E/(E+D)` on a sample of fixtures, and no design is committed to before that number exists. |
| tiered-review-03 | interface | `E` counts tool calls that search or read outside the diff (grep, reads of unchanged files, call sites, tests) and `D` counts reading the diff, reasoning over it, and composing findings. |
| tiered-review-04 | constraint | The `E/(E+D)` split is reported in both tokens and wall-clock, as a per-fixture distribution rather than only a mean. |
| tiered-review-05 | decision | The pre-registered rule picks the primary design from the measured ratio: `≥ 0.60` selects cheap-as-explorer, `≤ 0.30` selects per-file routing, and 0.30–0.60 selects angle routing. |
| tiered-review-06 | decision | Angle routing is additionally evaluated as a composable second stage in all three branches of the decision rule. |
| tiered-review-07 | interface | In explorer mode the cheap model produces a context bundle (call sites, related tests, invariants, prior art) placed in the expensive model's seed, and the expensive model then reviews the whole diff. |
| tiered-review-08 | decision | The explorer design's ship metric is non-inferiority of end-to-end defect recall against the unaided expensive panel on the same fixtures, with context completeness kept only as a diagnostic and never as the bar. |
| tiered-review-09 | decision | In angle mode correctness and security stay on the expensive tier while angles whose judgement is local move to the cheap tier. |
| tiered-review-10 | interface | Angle mode is implemented as a per-angle `AgentLaunch` rather than one shared launch across `cfg.angles`. |
| tiered-review-11 | constraint | The premise that an angle is cheapenable must be measured per angle against stratum D findings, never assumed. |
| tiered-review-12 | decision | Whole-change triage is used only as a narrow conservative complement on near-certain changes such as dependency bumps, generated files, and pure formatting, never as the general design. |
| tiered-review-13 | constraint | That whole-change triage is expressed as a conservative deterministic rule rather than a model judgement. |
| tiered-review-14 | decision | Per-file routing is kept in the run as a measured control arm whatever design the decision rule selects. |
| tiered-review-15 | interface | The relational-defect ceiling is computed by comparing, for every class-3 fixture, the files the fix commit touched against the files the introducing PR changed; the fraction landing outside is the empirical recall ceiling for any per-file design. |
| tiered-review-16 | interface | Ground truth uses three classes: class 1 is a human review comment that caused a change in the same PR, class 2 is an accepted bot review comment that caused a change, class 3 is a post-merge fix, revert, or regression-fix of code a PR introduced. |
| tiered-review-17 | constraint | The three classes are never pooled into one recall figure. |
| tiered-review-18 | constraint | Class 2 admits a comment only when a human author accepted it and changed the code. |
| tiered-review-19 | interface | The corpus has four strata: D from `skill-stickiness` run artifacts, M from `modularml/modular`, Q from `quitesh/quite-app`, and N from `neovim/neovim`. |
| tiered-review-20 | constraint | Every stratum is probed by every router model, making the design a full `router_model × stratum × class` factorial with no separate legs. |
| tiered-review-21 | constraint | A PR with `commits == 1` is disqualified as a class-1 or class-2 fixture. |
| tiered-review-22 | constraint | Class-1/2 attribution requires a later commit in the same PR that touches the commented file, ideally the commented lines. |
| tiered-review-23 | constraint | How attribution was verified is recorded per fixture, and the mere existence of a comment is never accepted as attribution. |
| tiered-review-24 | constraint | Docs-only PRs are excluded from class-1/2 fixtures. |
| tiered-review-25 | scope | Stratum Q cannot carry class 1, so class 1 rests on strata M and N only. |
| tiered-review-26 | constraint | Results are reported per repo as well as per class. |
| tiered-review-27 | constraint | A thin cell is recorded as thin and is never merged into a neighbouring cell. |
| tiered-review-28 | interface | Class-3 candidates are harvested by `git log -i --grep` over revert/regression/broke terms, then `git blame` of the lines the fix deleted or replaced, evaluated at `fix^`. |
| tiered-review-29 | constraint | A class-3 candidate is rejected unless blame resolves to a single introducing commit. |
| tiered-review-30 | interface | The class-3 fixture is the introducing commit's PR at pre-merge state, with the fix commit's diff supplying the human-authored defect label. |
| tiered-review-31 | constraint | Harvesting reports attrition at every stage: candidates, then single-attribution, then adjudicated-as-defect. |
| tiered-review-32 | decision | Stratum D is scored as two stratified cells, `D-rs` and `D-md`, measured and reported separately and never summed into one stratum-D number. |
| tiered-review-33 | constraint | The ship bar must pass on `D-rs` alone, and `D-md` cannot rescue a failing `D-rs`. |
| tiered-review-34 | interface | Stratum D findings carry one of three labels: verified-real, unverified, or rejected with its reason recorded. |
| tiered-review-35 | constraint | Only verified-real findings feed the ship bar. |
| tiered-review-36 | constraint | Private-repo fixtures and probe transcripts live outside any checkout, at `~/.local/share/drovr/cascade-corpus/`. |
| tiered-review-37 | constraint | Only aggregates — recall, precision, counts, analysis — are committed from private strata: no diffs, source excerpts, file paths, or defect descriptions detailed enough to reconstruct internal design. |
| tiered-review-38 | interface | Every fixture carries a `provenance` field of `Public` or `Private`, assigned at harvest from the source repo's GitHub visibility rather than from a path convention. |
| tiered-review-39 | constraint | The write guard refuses to write `Private`-derived content to any path that resolves inside a git checkout after canonicalisation, so a symlink out of the corpus and back into the repo does not defeat it. |
| tiered-review-40 | decision | The guard is made unrepresentable-by-type: a fixture's body is reachable only through an accessor requiring a token proving the destination was checked. |
| tiered-review-41 | decision | No egress guard ships, and `provenance` keeps exactly one consumer. |
| tiered-review-42 | constraint | The write guard ships with its tests in the same change, including the symlink-escape case. |
| tiered-review-43 | interface | The measurement arms are `router_model ∈ {qwen, sonnet}` × `stratum ∈ {D, M, Q, N}` × `class ∈ {1, 2, 3}`, with per-file routing run alongside as a control arm. |
| tiered-review-44 | decision | The ship decision is made per router model, so the router agent and router model are separate config keys. |
| tiered-review-45 | decision | Class 3 gates on conditional recall — of the class-3 defects the expensive tier finds, how many survive the cascade. |
| tiered-review-46 | constraint | The expensive tier's absolute detection rate on class 3 is reported but gates nothing. |
| tiered-review-47 | interface | Two synthetic controls bracket the metric: NULL clears everything (recall 0, saving maximal) and FLOOD escalates everything (recall 1.0, saving 0). |
| tiered-review-48 | constraint | Per stratum and per class the real cheap tier must land strictly inside the NULL/FLOOD bracket on both recall and saving, or those fixtures are rebuilt before anything is scored. |
| tiered-review-49 | constraint | A positive control re-runs the expensive tier on a sample and must re-find recorded class-1/2 defects at a high rate, otherwise the corpus is re-cut rather than the cheap tier blamed. |
| tiered-review-50 | constraint | The dev/held-out split is assigned at task/PR level before any cheap-tier prompt is written, so no diff appears in both. |
| tiered-review-51 | constraint | The held-out set is scored exactly once, after the prompt is frozen and its `git hash-object` recorded. |
| tiered-review-52 | decision | There is no `blind-map.json`; the dev/held-out split is the sole load-bearing control because scoring is set membership with no human judgement. |
| tiered-review-53 | decision | Designs with an unrecoverable-miss class (per-file, whole-change) must reach recall ≥ 0.98 with Wilson 95% lower bound ≥ 0.95, targeting N ≥ 142 per gated cell. |
| tiered-review-54 | decision | Designs whose failure degrades to baseline (angle routing, cheap-as-explorer) are gated on non-inferiority against the unaided expensive panel, with the cascade's recall lower bound ≥ 0.90 of the unaided panel's point estimate, targeting N ≥ 69. |
| tiered-review-55 | constraint | The bars are evaluated in order: 0 N floor, 1 instrument discriminates, 2 recall bar, 3 escalation/skip rate leaves a real saving, 4 measured cost. |
| tiered-review-56 | constraint | A cell that fails the N floor is reported as null for that cell. |
| tiered-review-57 | interface | Bar 4 requires measured end-to-end cost below 0.80× the non-cascade panel. |
| tiered-review-58 | decision | Bars 1 and 2 constitute the ship decision, while failing bar 3 or 4 still ships the code behind its off-by-default flag with the negative economics written up. |
| tiered-review-59 | constraint | Precision, hint quality, and context completeness are reported but are not decision inputs. |
| tiered-review-60 | constraint | The N floor applies to the gated number pooled across strata within a single `(model × class)` cell, not to each `(model × stratum × class)` cell. |
| tiered-review-61 | constraint | Per-stratum numbers are always reported alongside any pooled figure. |
| tiered-review-62 | constraint | If per-stratum recall is more heterogeneous than sampling explains, the pooled figure is not reported as the headline and the discrepant stratum is reported as the finding. |
| tiered-review-63 | interface | Stratum D and N fixtures are frozen by a MANIFEST shaped like `docs/skill-evidence/arms/MANIFEST.md`, carrying `git hash-object --no-filters` blob SHAs, a commit cell that must contain the blob at the recorded path, header-resolved columns, and a test re-checking every row. |
| tiered-review-64 | constraint | Strata M and Q are frozen the same way inside the private corpus directory, with only the row count and the manifest's digest committed. |
| tiered-review-65 | interface | Every cheap-tier invocation, retries included, is counted in an append-only ledger with per-stage ceilings whose arithmetic is checked by a test. |
| tiered-review-66 | interface | Configuration lives under `[review.cascade]` with the keys `enabled`, `mode`, `cheap_agent`, `cheap_model`, and `timeout_ms`. |
| tiered-review-67 | constraint | `enabled` defaults to `false` and stays off for the whole run regardless of the result. |
| tiered-review-68 | interface | `mode` is an enum of `explorer`, `angle`, `file`, and `change`. |
| tiered-review-69 | constraint | No default value for `mode` ships until Task 0's measurement selects one. |
| tiered-review-70 | interface | `cheap_agent` is `opencode` and `cheap_model` is `ko-ag/qwen3.6-35b-abliterated`. |
| tiered-review-71 | interface | `timeout_ms` is 120000. |
| tiered-review-72 | decision | The cheap tier runs headless via `opencode run --agent plan -m <model>` rather than as a herdr pane. |
| tiered-review-73 | constraint | The cheap tier is kept read-only by running `--agent plan` with `--auto` absent. |
| tiered-review-74 | constraint | The `readonly_displace` protection (`cli/src/config.rs:589`) runs before the cheap tier launches. |
| tiered-review-75 | interface | A cascade stage is added to `cli/src/code_review.rs` behind `enabled = false`. |
| tiered-review-76 | interface | `docs/cascade-evidence/results.md` reports the controls, dev numbers, one held-out score per `(model × stratum × class)`, each bar in order, the ship decisions, the stated limitations, and aggregates only. |
| tiered-review-77 | constraint | Every set this run extends — config keys, the `mode` enum, the cheap tier's output vocabulary, ledger columns, MANIFEST rows, the fixture list, the class enum — has its guarding test extended in the same change. |
| tiered-review-78 | scope | Changing the angle set, severity model, merge path, or resume path is out of scope. |
| tiered-review-79 | scope | Building a second measurement instrument is out of scope. |
| tiered-review-80 | scope | Fixing the pane bug the spec cites in `docs/known-issues.md` is out of scope; it stays open and documented while the headless cheap tier routes around it. |
| tiered-review-81 | scope | Any model tier between qwen and opus is out of scope. |
| tiered-review-82 | constraint | Under every design the cheap tier's output is only a routing decision or a context bundle, never a finding that reaches a verdict. |
| tiered-review-83 | constraint | The writeup records that `ko-ag/qwen3.6-35b-abliterated` is an abliterated build with safety tuning removed. |
| tiered-review-84 | constraint | The plan phase inherits the four approved decisions without re-litigating them, and anything they do not cover is a new question for a new gate. |

## Derivation notes

Recorded so a later reader can see what was weighed, not only what survived. These are the deriving
subagent's own exclusions, kept verbatim in substance:

- The §1 problem statement, the "cheap tier's false negative is unrecoverable" framing, and all
  For/Against argumentation in §4 were excluded as motivation and rationale, not buildable items.
- Specific evidence numbers were excluded as illustrative support for decisions already rowed: the
  reviewer-composition table (§5.4), the raw grep counts and the `cb1adad`/`55b80032` validation
  (§5.5), the stratum-D counts (§5.6), the neovim measurement, and the named PR numbers.
- The Wilson N-vs-misses table and the "why not 0.97" paragraph (§7.4) were excluded as sizing
  rationale behind rows 53 and 54.
- §7.5's four stated limitations were excluded as reporting content; their operative halves (class
  1 rests on M and N; the verified-real filter) already appear as rows 25 and 35.
- §10's decision table was excluded entirely: all four rows restate §3.1, §7.4, §7.2, and §5.6.
- "Flipping the default on is out of scope" (§9) was excluded as a restatement of row 67.

**One conflict inside the control spec, noted and not resolved here.** §7.1 names the config keys
`router_agent`/`router_model` while §8's block shows `cheap_agent`/`cheap_model`. Row 44 states the
requirement (agent and model are separate keys) without picking a spelling, and row 66 records §8's.
Resolving it would be editing the fixture, which the freeze forbids.

**Row 80 paraphrases a line-numbered citation in the control spec.** The fixture cites
`docs/known-issues.md` by line; this repo's rule is to cite it by heading, and a line number
recorded here would rot at the next merge without ever having been checkable. The row therefore
records the substance — the pane bug stays open — rather than the fixture's line number. The
fixture itself is frozen byte-for-byte and still carries the original citation.
