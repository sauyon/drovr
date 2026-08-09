# Tiered (cascade) code review for drovr

**Run:** `tiered-review` · **Worktree:** `.drovr/wt/tiered-review` · **Branch:** `drovr/tiered-review`

## 1. Problem

`drovr code-review run` spawns one read-only reviewer per angle (`correctness`, `security`,
`error-handling`, `type-design` — `cli/src/config.rs:436`), every one on the same expensive model,
over the whole `base..head` range. Cost scales with `angles × range size`, and the driver re-runs
the panel unconditionally on top of whatever the author already ran (`docs/known-issues.md:589`).

This run designs and measures a cascade — cheap model first, expensive model only where warranted —
against the risk specific to cascades: a cheap tier's false negative is unrecoverable if nothing
downstream ever looks at what it waved through, and a `Clean` produced that way is byte-identical to
a real one. This repo has hit that vacuous-pass class ten times (`docs/known-issues.md:526`, `:589`).
This is a measurement with a feature attached, not a feature with a measurement attached.

## 2. Invariant (binds every design)

> **A cheap-tier failure must degrade to the baseline — a full expensive review — never to less
> review than the baseline would have given.**

Stated at this altitude deliberately, since §3 determines what the cheap tier *does*, not whether
this invariant holds.

| design | cheap-tier failure degrades to |
|---|---|
| per-file / whole-change routing | that unit escalates; nothing is skipped on error |
| angle routing | that angle re-runs on the expensive tier |
| cheap-as-explorer | the expensive tier explores for itself, exactly as today |

Router unavailable, timed out, crashed, out of context, unparseable, naming a unit not in the range,
or silently omitting a unit — every one takes the degrade path. There is no code path from "the
cheap tier had a problem" to "less review happened".

Designs differ in whether they have an unrecoverable-miss class at all; §7.4's ship bar is set from
that fact per design, not uniformly.

## 3. Decision: measure cost split before choosing a design

**Decision (`decisionrule: measure-first`) — APPROVED.** The design is not chosen ahead of the
measurement. Task 0 runs first; its result selects the primary design via the pre-registered rule
below.

**Task 0 — cost instrumentation.** Instrument expensive-tier reviews on a sample of fixtures and
report the split, in both tokens and wall-clock, as `E/(E+D)` with a per-fixture distribution (not
just a mean):

- **E — exploration:** tool calls that search or read outside the diff (grep, file reads of
  unchanged files, following call sites, reading tests).
- **D — diff-reading and finding-writing:** reading the diff itself, reasoning over it, composing
  findings.

**Pre-registered decision rule:**

| measured `E/(E+D)` | primary design |
|---|---|
| ≥ 0.60 — exploration dominates | cheap-as-explorer (§4.1) |
| ≤ 0.30 — diff-reading dominates | per-file routing (§4.4) |
| 0.30 – 0.60 | angle routing (§4.2) |

Angle routing is additionally evaluated as a composable second stage under all three outcomes: it is
orthogonal to the others and its evaluation does not depend on the ratio.

## 4. Candidate designs — contracts

Each design below is implemented and measured; §3's rule selects which is primary. Per-file routing
is retained as a control arm regardless of outcome (§4.4).

### 4.1 Cheap tier as explorer / context-builder

The cheap model locates call sites, related tests, invariants, and prior art, and hands the
expensive model a context bundle; the expensive model reviews the **whole** diff with that bundle in
its seed. Nothing is skipped, so it has no unrecoverable-miss class; its failure mode is an
incomplete bundle, which degrades to the expensive tier exploring for itself.

**Ship metric: non-inferiority of end-to-end defect recall against the unaided expensive panel on
the same fixtures.** Not context completeness — a model handed a bundle that looks sufficient may
explore less than one handed nothing, which would convert a cost saving into a silent recall loss.
Context completeness (did the bundle contain the file/symbol the defect required?) is retained only
as a diagnostic for explaining a failure, never as the ship bar.

Largest architectural change of the four: needs its own read-only launch, a bundle format, and a
seed extension.

### 4.2 Route by angle

Cheap handles angles whose judgement is local; expensive keeps correctness and security. Change
shape: a per-angle `AgentLaunch` instead of one shared launch (`cfg.angles` is already the fan-out
dimension in `code_review_run`). No unrecoverable-miss class — every angle is still reviewed by
someone; failure is "the cheap tier did a worse job on that angle", bounded and measurable per angle.
Insensitive to the E/D ratio measured in §3.

"Local judgement" is measured per angle, not assumed — stratum D findings carry their angle, making
this directly testable (§5.6).

### 4.3 Whole-change triage — not a general design

One decision per change: does this need deep review at all. **Rejected as the general mechanism** —
a false clean loses an entire change's review, the highest-stakes failure available. **Retained only
as a narrow, conservative complement**, expressed as a conservative rule (not a model judgement), for
changes where near-certainty is available: dependency bumps, generated files, pure formatting.

### 4.4 Per-file routing — retained as a measured control arm

The original design, run in every branch of §3's outcome regardless of which is primary, so the spec
reports what it would have cost and missed rather than assuming it away.

**Has a structural, unrecoverable-miss class:** a router judging a file in isolation cannot see
relational defects — bugs visible only from a call site, invariant, or test in a different file. It
is under-informed by construction and its confidence is uncorrelated with what it cannot see.

**Deliverable: empirical recall ceiling for any per-file design.** For every class-3 fixture (§5.1),
compare the files the *fix* commit touched against the files the *introducing* PR changed. A defect
whose fix lands outside the introducing diff is one no per-file router could have routed correctly;
that fraction is the ceiling, computed from real defects.

## 5. Corpus

### 5.1 Ground-truth classes

| class | signal | what it proves |
|---|---|---|
| 1 | human review comment that caused a change in the same PR | review caught it pre-merge |
| 2 | **accepted** bot review comment that caused a change | same, bot-found |
| 3 | post-merge fix / revert / regression-fix of code a PR introduced | review MISSED it |

Never pooled into one recall figure — classes differ in difficulty and meaning. Class 3 is the
ceiling question and the design is weighted toward it: classes 1 and 2 are, by construction, defects
that *were* catchable by review; class 3 is what slips through entirely.

Class 2's bot-comment label is rescued from circularity by the **acceptance** filter — the label is
"a human author agreed and changed the code", not "a bot said so" — and is reported separately from
class 1.

### 5.2 Strata

| stratum | source | visibility | classes carried | notes |
|---|---|---|---|---|
| D — drovr | `skill-stickiness` run artifacts | public | drovr-specific (§5.6) | agent-found, then fixed |
| M — modular | `modularml/modular` | private | 1, 2, 3 | carries class 1 |
| Q — quite-app | `quitesh/quite-app` | private | 2, 3 | class 1 too thin (§5.4) |
| N — neovim | `neovim/neovim` | public | 1, 2, 3 | third language, reproducible off this machine |

Every stratum is probed by every router model — model access is not a stratifying factor (§6), so
the design is a full factorial `router_model × stratum × class` with no partial legs.

Neovim was chosen on diversity and reproducibility: a third codebase/language, and a public corpus
reconstructible off this machine. Measured this session: 33 inline review comments per 40 merged PRs
(`ghostty-org/ghostty` yielded 9 per 40 and is not used).

### 5.3 Class 1/2 attribution rules

- **`commits == 1` disqualifies the PR** — a single-commit PR cannot contain a comment-driven fix
  commit. Disqualifies modular 94850 (13 human comments, 16 files) and 95031 (11 comments, 11 files).
- **A later commit in the same PR must touch the commented file**, ideally the commented lines. How
  it was verified is recorded per fixture; "the comment exists" is never sufficient.
- Docs-only PRs excluded (modular 95055, 12 comments).

Best class-1/2 fixture: modular PR 94953 — 26 comments (5 bot, 21 human), 9 files, +339/−158, 6
commits. PR 94602 (13 human comments, 71 files, +6851/−108, 5 commits) is available if a large-diff
fixture is wanted; its size makes per-defect attribution harder.

### 5.4 Reviewer composition

Of the 300 most recent review comments:

| repo | bot | human | most prolific single reviewer |
|---|---|---|---|
| `modularml/modular` | 47 | 253 | `cursor[bot]`, 36 — more than any human |
| `quitesh/quite-app` | 287 | 13 | `gemini-code-assist[bot]`, 240 |

`quite-app` has essentially no human review, so it does not carry class 1. Results are reported per
repo and per class; a thin cell is recorded as thin, never merged into a neighbour.

### 5.5 Class 3 pipeline

1. Candidate fix commits: `git log -i --grep` for `revert|regression|broke` and kin.
2. `git blame` the lines the fix **deleted or replaced**, at `fix^`.
3. Require a single introducing commit — fan-out (multiple introducing commits) is rejected as not
   cleanly attributable.
4. That commit's PR at pre-merge state is the fixture; the fix commit's diff states what the defect
   was and where. Human-authored label, no invention.

Validated end-to-end on `quite-app`: fix `cb1adad` → deleted lines in
`quited/qrun/src/sip_clone.rs` → blame resolves to single commit `55b80032`.

Raw signal, matching commits in the last 2000 (overlapping, case-insensitive): modular `Revert` 28 /
`regression` 70 / `broke` 26; quite-app 29 / 36 / 28.

Attrition is reported at every stage (candidates → single-attribution → adjudicated-as-defect): of 40
class-3 candidates sampled in quite-app, only 7 touched ≤ 2 files.

### 5.6 Stratum D — scored as two stratified cells

**Decision (`corpus: both-stratified`) — APPROVED.** `D-rs` and `D-md` are measured and reported
separately, never summed into one stratum-D number. **The ship bar must pass on `D-rs` alone**;
`D-md` is reported with its own interval and cannot rescue a failing `D-rs`.

Source: `~/.local/share/drovr/runs/skill-stickiness/` — 376 per-angle findings files across 100
panels over 22 tasks, plus every `<task>-base.sha` and `<task>-review-<iter>.head`. All 100 head SHAs
and all 22 base SHAs resolve from this worktree (`git rev-parse --git-common-dir` →
`/home/sauyon/devel/drovr/.git`). 131 blocking findings: 79 `.rs` (`D-rs`), 51 `.md` (`D-md`), 1
`.json`, over 20 tasks. Findings carry their angle, which makes §4.2's per-angle premise testable.

Labels: *verified-real* (a fix landed, or it is written up in `docs/known-issues.md`) — the only set
that feeds the bar; *unverified* — reported, not scored; *rejected* — excluded with the reason
recorded.

Reviewing Rust and reviewing prose are different tasks; the cheap tier could be good at one and
useless at the other, and pooling would hide that.

## 6. Data handling

**Decision: qwen is self-hosted; there is no third-party egress and no restriction on which model
may read which stratum.** The earlier egress constraint was based on a mistaken premise and is
withdrawn in full, including the two-legs structure it forced. `provenance` keeps a single consumer
(publishing, below) rather than serving two purposes with one vestigial.

**What governs publishing, unchanged:** `sauyon/drovr` is public; `modularml/modular` and
`quitesh/quite-app` are not. Private fixtures and probe transcripts live outside any checkout
(`~/.local/share/drovr/cascade-corpus/`). Only aggregates are committed — recall, precision, counts,
analysis. No diffs, source excerpts, file paths, or defect descriptions detailed enough to
reconstruct internal design from those repos.

**Contract: `provenance: Public | Private`** is assigned at harvest from the source repo's GitHub
visibility, not from a path convention or recollection. The **write guard** refuses to write
`Private`-derived content to any path resolving inside a git checkout, after canonicalisation, so a
symlink out of the corpus directory and back into the repo does not defeat it. Shaped after
`ReviewOutcome::EmptyRange` (`cli/src/code_review.rs:94`): a fixture's body is reachable only through
an accessor requiring a token proving the destination was checked — the guard belongs in the type,
not at one call site. The write guard ships with its tests in the same change, including the
symlink-escape case. Not closed by this guard: a human running an ad-hoc command outside the harness.

## 7. The measurement

### 7.1 Arms

`router_model ∈ {qwen, sonnet}` × `stratum ∈ {D, M, Q, N}` × `class ∈ {1, 2, 3}`, design fixed by
§3's rule, with per-file routing run as a control arm alongside.

**The ship decision is per router model** — `router_agent`/`router_model` are separate config keys
(§8). A sonnet-routed cascade and a qwen-routed cascade are shipped or not independently.

### 7.2 Class 3 needs a second metric

**Decision (`class3gate: gates-conditional`) — APPROVED.** For classes 1, 2 and stratum D the
expensive tier found the defect by construction; class-3 defects were missed by review, so the
expensive tier may miss them too and a cheap tier cannot be blamed for failing where the expensive
tier would also fail. Two numbers:

- **Conditional recall** (gates) — of the class-3 defects the *expensive tier* finds, how many
  survive the cascade.
- **Expensive-tier absolute detection rate on class 3** (does not gate) — what fraction of
  review-missed defects the full panel finds at all; reported as the run's scientific ceiling.

### 7.3 Discrimination and stopping controls

Two synthetic controls bracket every metric, both free: **NULL** (everything cleared; recall 0,
saving maximal) and **FLOOD** (everything escalated; recall 1.0, saving 0). **Per stratum and per
class, a real cheap tier must land strictly inside the bracket on both axes, or those fixtures are
rebuilt before anything is scored.**

**Positive control:** re-running the expensive tier on a sample must re-find recorded class-1/2
defects at a high rate; if not, the fixtures are not reviewable in isolation and the corpus is
re-cut, not the cheap tier.

**Dev/held-out split** assigned before any cheap-tier prompt is written, at task/PR level so no diff
appears in both. Held-out is scored **once**, after the prompt is frozen and its `git hash-object`
recorded. **Decision: there is no `blind-map.json`.** The threat this run defends against is prompt
overfitting, not scorer bias (scoring is set membership with no human judgement), so the split is the
load-bearing control; this is a deliberate departure from the prior protocol, recorded as a decision.

### 7.4 Ship bar — non-uniform by failure mode

**Decision (`barshape: by-failure-mode`) — APPROVED.**

N is sized for the power the bar needs, not trimmed to fit the corpus — qwen is unlimited and may
read every stratum (§6).

Wilson 95% lower bound, N required to clear each target given misses actually observed:

| LB target | 0 misses | 1 miss | 2 misses | 3 misses | 4 misses |
|---|---|---|---|---|---|
| ≥ 0.85 | 22 | 34 | 45 | 55 | 64 |
| ≥ 0.90 | 35 | 53 | 69 | 84 | 99 |
| ≥ 0.95 | 73 | 110 | 142 | 173 | 202 |
| ≥ 0.97 | 125 | 185 | 239 | 290 | 339 |

The bar is set by failure mode, not uniformly:

- **Designs with an unrecoverable-miss class** (per-file, whole-change): a miss is a defect nobody
  ever looks at again. **Recall ≥ 0.98 with Wilson LB ≥ 0.95, target N ≥ 142 per gated cell.**
- **Designs whose failure degrades to baseline** (angle routing, cheap-as-explorer): failure is a
  cost regression, not a lost defect. **Non-inferiority against the unaided expensive panel, cascade
  recall lower bound ≥ 0.90 of the unaided panel's point estimate, target N ≥ 69.** Demanding 0.95 LB
  here buys nothing real and costs a corpus.

LB ≥ 0.97 is not used even for unrecoverable designs: N = 239 for a two-miss tolerance buys a 0.02
improvement in a bound whose real uncertainty is dominated by corpus construction (§7.5), not
sampling error.

**Order of checks:** **0.** N floor met, else null for that cell. **1.** Instrument discriminates
(§7.3). **2.** The recall bar above. **3.** Escalation/skip rate leaves a real saving. **4.** Measured
end-to-end cost < 0.80× the non-cascade panel.

**Pooling contract.** `N ≥ 142` per `(model × stratum × class)` cell would require 24 cells and is
not reachable. The N floor applies to the gated number, **pooled across strata within one
`(model × class)`** — pooling across strata buys power; pooling across classes would destroy meaning
(§5.1 forbids only the latter). Per-stratum numbers are always reported alongside; pooling is
conditional on them agreeing — **if per-stratum recall is more heterogeneous than sampling explains,
the pooled figure is not reported as the headline**, and the discrepant stratum is reported as the
finding.

**Ship decision:** bars 1 and 2 are the ship decision. Failing 3 or 4 ships the code behind its
off-by-default flag with the negative economics written up. Precision, hint quality, context
completeness, and class-3 absolute detection are reported and are **not** decision inputs.

**Freezing.** Stratum D and N fixtures are frozen by a MANIFEST in the shape of
`docs/skill-evidence/arms/MANIFEST.md`: `git hash-object --no-filters` blob SHAs, a commit cell that
must contain the blob at the recorded path, header-resolved columns, a test re-checking every row.
Strata M and Q are frozen the same way in the private corpus directory, with only the row count and
the manifest's digest committed.

**Ledger.** Every cheap-tier invocation is counted in an append-only ledger with per-stage ceilings,
arithmetic checked by a test. Retries count.

### 7.5 Stated limitations

1. Corpus construction, not model access, is now the largest limitation. Every class is defined by a
   filter (comment→fix attribution, single-blame attribution, verified-real), and each filter selects
   a reachable subset of defects. Class 3 in particular keeps only cleanly-attributable defects (§5.5
   shows 7 of 40 candidates), which are plausibly the simpler ones — the measured ceiling may be
   optimistic in a direction no interval captures.
2. Stratum D ground truth is "what an Opus/Sonnet panel found and someone fixed" — recall against the
   expensive tier, never an absolute.
3. Class 1 is "what a human caught and the author fixed"; class 2 is "what a bot nominated and a
   human accepted". Both miss what review missed entirely, which is what class 3 supplies.
4. Class 1 rests on modular and neovim only (§5.4).

## 8. Configuration and deliverables

```toml
[review.cascade]
enabled = false            # DEFAULT, and it stays off in this run regardless of result
mode = "<undecided>"       # explorer | angle | file | change — SET BY TASK 0, not before (§3).
                           # No default ships until the measurement picks one.
cheap_agent = "opencode"
cheap_model = "ko-ag/qwen3.6-35b-abliterated"
timeout_ms  = 120000       # expiry takes the degrade path of §2
```

**Cheap-tier execution: headless, not a herdr pane.** `opencode run --agent plan -m <model>`,
verified working this session. Every cold `opencode` reviewer *pane* swallows its seed and the panel
cannot converge (`docs/known-issues.md:3437`), so the pane path is closed for this backend;
`opencode run` stdout is a subprocess pipe, not the rendered terminal view
`cli/src/code_review.rs:1` refuses to scrape. Read-only via `--agent plan` with `--auto` absent, and
the `readonly_displace` protection (`cli/src/config.rs:589`) runs first.

**Deliverables.**

1. Task 0 cost instrumentation and its report.
2. Relational-defect ceiling for per-file routing (§4.4).
3. Corpus harvesters — class 1/2 attribution, class 3 blame-back — each reporting attrition.
4. Fixtures + MANIFESTs; D and N committed, M and Q private.
5. `provenance` typing, write guard, and its tests.
6. Cheap-tier implementation for the selected mode plus the per-file control arm.
7. Cascade stage in `cli/src/code_review.rs` behind `enabled = false`.
8. `docs/cascade-evidence/results.md` — controls, dev numbers, one held-out score per
   (model × stratum × class), each bar in order, ship decisions, §7.5 limitations, aggregates only.
9. Run ledger.

**Guard-with-artifact contract:** every set this run extends — config keys, the `mode` enum, the
cheap tier's output vocabulary, ledger columns, MANIFEST rows, fixture list, the class enum — has its
guarding test extended in the same change that extends it.

## 9. Scope boundaries

**Out, deliberately:** flipping the default on; changing the angle set, severity model, merge or
resume path; a second measurement instrument; fixing `docs/known-issues.md:3437` (the headless cheap
tier routes around it — the pane bug stays open and documented); any tier between qwen and opus.

`ko-ag/qwen3.6-35b-abliterated` is an **abliterated** build — safety tuning removed. Recorded because
a writeup omitting it would describe a different experiment. Under every design in §4 its output is
either a routing decision or a context bundle, never a finding that reaches a verdict.

## 10. Decisions — approved 2026-08-06, turn 1

| # | question | decision | where it binds |
|---|---|---|---|
| 1 | Unit of escalation | `measure-first` — Task 0 measures `E/(E+D)`, the pre-registered rule picks explorer / angle / per-file | §3, §4 |
| 2 | Ship bar shape | `by-failure-mode` — 0.98 / LB ≥ 0.95 (N ≥ 142) where a miss is unrecoverable; non-inferiority at LB ≥ 0.90 (N ≥ 69) where failure degrades to baseline | §7.4 |
| 3 | Class 3's role | `gates-conditional` — gates on conditional recall against the expensive tier's finds; absolute detection is reported, not gating | §7.2, §7.4 |
| 4 | Stratum D scope | `both-stratified` — `D-rs` and `D-md` scored and reported separately; the bar must pass on `D-rs` alone | §5.6 |

Decision 4 went against this run's own initial recommendation (`rs-only`); §5.6 carries the reasoning
for the choice that was made, not the one that was proposed.
