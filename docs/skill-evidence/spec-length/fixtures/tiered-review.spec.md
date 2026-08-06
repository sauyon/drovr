# Tiered (cascade) code review for drovr

**Run:** `tiered-review` · **Worktree:** `.drovr/wt/tiered-review` · **Branch:** `drovr/tiered-review`

## 1. Problem

`drovr code-review run` spawns one read-only reviewer per angle (`correctness`, `security`,
`error-handling`, `type-design` — `cli/src/config.rs:436`), every one on the same expensive model,
over the whole `base..head` range. Cost scales with `angles × range size`, and the driver re-runs
the panel unconditionally on top of whatever the author already ran (`docs/known-issues.md:589`).

A cascade — cheap model first, expensive model only where warranted — is the obvious saving. It is
also the obvious way to ship a silent regression in review quality, because **a cheap tier's false
negative is unrecoverable**: nothing downstream ever looks at what the cheap tier waved through,
and a `Clean` produced that way is byte-identical to a real one. That is the vacuous-pass class
this repo has hit ten times (`docs/known-issues.md:526`, `:589`), and a cascade is a machine for
producing it at scale.

So this is a **measurement** with a feature attached. And the first thing to measure is not the
cheap tier — it is where the money actually goes.

## 2. The invariant, stated generally

> **A cheap-tier failure must degrade to the baseline — a full expensive review — never to less
> review than the baseline would have given.**

This is the general form of deny-by-default, and it is stated at this altitude deliberately,
because §3 may change what the cheap tier *does*. Its instantiations:

| design | cheap-tier failure degrades to |
|---|---|
| per-file / whole-change routing | that unit escalates; nothing is skipped on error |
| angle routing | that angle re-runs on the expensive tier |
| cheap-as-explorer | the expensive tier explores for itself, exactly as today |

Router unavailable, timed out, crashed, out of context, unparseable, naming a unit not in the
range, or silently omitting a unit — every one takes the degrade path. There is no code path from
"the cheap tier had a problem" to "less review happened".

**Designs differ in whether they have an unrecoverable-miss class at all**, and §7.4 sets the ship
bar from that fact rather than uniformly.

## 3. First question: where does the review cost actually go?

The human's challenge: *"most of the cost of the review isn't the diff, it's the exploration needed
to properly review."*

If that holds, per-file routing optimises the cheap part and leaves the expensive part untouched —
the expensive tier must still explore the codebase to review any file it receives, so the saving is
bounded by diff-reading, not by review cost. **Every design below lives or dies on this ratio, and
right now it is an assumption on both sides.** So it is measured first, before a design is chosen.

**Task 0 — cost instrumentation.** Instrument expensive-tier reviews on a sample of fixtures and
report the split, in both tokens and wall-clock:

- **E — exploration:** tool calls that search or read outside the diff (grep, file reads of
  unchanged files, following call sites, reading tests).
- **D — diff-reading and finding-writing:** reading the diff itself, reasoning over it, composing
  findings.

Reported as `E/(E+D)` with a per-fixture distribution, not just a mean — a ratio that swings wildly
by change size is itself a finding, and would argue for a size-conditional design.

### 3.1 Pre-registered decision rule — **APPROVED** (`decisionrule: measure-first`)

Chosen before the number is known, which is the only time it can honestly be chosen. Task 0 runs;
no design is committed to ahead of it:

| measured `E/(E+D)` | primary design |
|---|---|
| **≥ 0.60** — exploration dominates | **cheap-as-explorer** (§4.1) |
| **≤ 0.30** — diff-reading dominates | **per-file routing** (§4.4) |
| **0.30 – 0.60** | **angle routing** (§4.2), which is insensitive to the ratio |

Angle routing is additionally evaluated as a *composable* second stage in all three branches: it is
orthogonal to the others, and nothing about it depends on the ratio.

## 4. Candidate designs, argued

### 4.1 Cheap tier as EXPLORER / context-builder

The cheap model does the expensive work — locate call sites, related tests, invariants, prior art —
and hands the expensive model a context bundle; the expensive model then reviews the **whole** diff
with that bundle in its seed.

**For.** It attacks the dominant cost term if exploration dominates: cost goes from `E + D` to
`E_cheap + D`, so the saving approaches the full tier price ratio rather than being capped at
`D/(E+D)`. Nothing is skipped, so there is no silent-miss class of the kind §1 describes. Its
failure mode — an incomplete bundle — degrades to the expensive tier exploring for itself.

**Against, and this is the part that must not be waved through.** "The expensive tier will notice
the gap and explore further" is a **hypothesis, not a property**. The opposite is at least as
plausible: a partial bundle *anchors* the reviewer, and a model handed context that looks
sufficient explores less than one handed nothing. That would convert a cost saving into a recall
loss — silently, which is exactly the failure class this run exists to prevent. It is also the
largest architectural change of the four: the explorer needs its own read-only launch, a bundle
format, and a seed extension.

**Therefore its ship metric is not context completeness.** It is **non-inferiority of end-to-end
defect recall against the unaided expensive panel on the same fixtures**, which measures anchoring
directly and needs no definition of "complete". Context completeness (did the bundle contain the
file or symbol the defect required?) is retained as a diagnostic for explaining a failure, not as
the bar.

### 4.2 Route by ANGLE

Cheap handles angles whose judgement is local; expensive keeps correctness and security, which need
whole-system context.

**For.** It is the cleanest fit for drovr: `cfg.angles` is already the fan-out dimension and
`code_review_run` already spawns one reviewer per angle, so the change is a per-angle `AgentLaunch`
instead of one shared launch — the smallest diff of any candidate. **It is insensitive to the E/D
ratio**, which is worth a great deal while that ratio is unknown: moving 2 of 4 angles to the cheap
tier saves roughly half the panel cost whether exploration dominates or not. And **it has no
unrecoverable-miss class**: every angle is still reviewed by someone. The failure is "the cheap tier
did a worse job on that angle", which is bounded, visible, and measurable per angle.

**Against.** The angles most plausibly cheapenable (nits, style) are the ones whose findings matter
least, so the saving may be real but low-value. And "local judgement" is doing suspicious work:
error-handling defects frequently need the call site, so the premise has to be *measured per angle*,
not assumed — which the corpus supports, since stratum D findings carry their angle.

### 4.3 Whole-change triage

One decision per change: does this need deep review at all?

**For.** It matches how exploration amortises — exploration is largely per-change, not per-file, so
skipping a whole change is the only routing decision that actually avoids paying E.

**Against.** It is the highest-stakes decision available: one false clean loses an entire change's
review. As a general mechanism the recall bar would have to be brutal. **Recommended only as a
narrow, conservative complement** on changes where near-certainty is available — dependency bumps,
generated files, pure formatting — expressed as a conservative rule rather than a model judgement,
and never as the general design.

### 4.4 Per-file routing — retained as a measured CONTROL arm

The original design. Kept in the run whatever the decision rule selects, so the spec can report what
it *would* have cost and missed rather than assuming it away.

**The structural objection, and it is decisive if it holds.** A router judging a file in isolation
cannot see relational defects — bugs visible only from a call site, an invariant, or a test in a
*different* file. The router is not wrong, it is **under-informed by construction**, and it does not
know what it cannot see. That is a recall ceiling no model quality fixes, and it interacts badly
with deny-by-default: the router's confidence is uncorrelated with the thing that makes it blind.

**This is measurable, and measuring it is a deliverable.** For every class-3 fixture, compare the
files the *fix* commit touched against the files the *introducing* PR changed. A defect whose fix
lands outside the introducing diff is one no per-file router could have routed correctly. That
fraction is the **empirical recall ceiling for any per-file design**, computed from real defects
rather than argued.

## 5. Corpus — four strata, three classes

### 5.1 Ground-truth classes

| class | signal | what it proves |
|---|---|---|
| **1** | human review comment that caused a change in the same PR | review caught it pre-merge |
| **2** | **accepted** bot review comment that caused a change | same, bot-found |
| **3** | post-merge fix / revert / regression-fix of code a PR introduced | **review MISSED it** |

**Class 3 is the ceiling question and the design is weighted toward it.** Classes 1 and 2 are, by
construction, defects that *were* catchable by review. Class 3 is what slips through entirely.

**Never pooled into one recall figure** — they differ in difficulty and meaning.

**On class 2 and circularity.** A bot review comment reintroduces the "found by the kind of agent
being tested" problem. What rescues it is the **acceptance** filter: the label is not "a bot said
so", it is "a *human author* agreed and changed the code". Reported separately from class 1 so a
reader who disagrees can discount it.

### 5.2 Strata

| stratum | source | visibility | classes | notes |
|---|---|---|---|---|
| **D — drovr** | `skill-stickiness` run artifacts | public | drovr-specific (§5.5) | agent-found, then fixed |
| **M — modular** | `modularml/modular` | private | 1, 2, 3 | carries class 1 |
| **Q — quite-app** | `quitesh/quite-app` | private | 2, 3 | class 1 too thin (§5.4) |
| **N — neovim** | `neovim/neovim` | **public** | 1, 2, 3 | third language, and reproducible off this machine |

**Every stratum is probed by every router model.** Model access is no longer a stratifying factor
(§6), so the design is a full factorial: `router_model × stratum × class`. There are no "legs".

**Neovim earns its place on diversity and reproducibility**, not as a workaround: a third codebase
and language, and a public corpus someone outside this machine can rebuild and check. Measured this
session: 33 inline review comments per 40 merged PRs (`ghostty-org/ghostty` yielded 9 per 40 and is
not used).

### 5.3 Class 1 and 2 — attribution is verified, not assumed

- **`commits == 1` disqualifies the PR.** A single-commit PR cannot contain a comment-driven fix
  commit. (Disqualifies modular **94850** — 13 human comments, 16 files — and **95031** — 11
  comments, 11 files — despite high comment counts.)
- **A later commit in the same PR must touch the commented file**, ideally the commented lines.
  How it was verified is recorded per fixture; "the comment exists" is never accepted.
- Docs-only PRs excluded (modular **95055**, 12 comments, is one).

Best class-1/2 candidate: modular **PR 94953** — 26 comments (5 bot, 21 human), 9 files, +339/−158,
**6 commits**. **PR 94602** (13 human comments, 71 files, +6851/−108, 5 commits) is available if a
large-diff fixture is wanted, though its size makes per-defect attribution harder.

### 5.4 Reviewer composition — measured, and lopsided

Of the 300 most recent review comments:

| repo | bot | human | most prolific single reviewer |
|---|---|---|---|
| `modularml/modular` | 47 | 253 | `cursor[bot]`, 36 — more than any human |
| `quitesh/quite-app` | **287** | **13** | `gemini-code-assist[bot]`, 240 |

modular's top humans: ferasboulala 30, NathanSWard 21, JoeLoser 17, rachfop 12, zyx-billy 11.
quite-app's bots: gemini-code-assist 240, coderabbitai 46, cursor 1; its only human is `sauyon`, 13.

**`quite-app` has essentially no human review**, so it cannot carry class 1. Results are reported
**per repo and per class**; a thin cell is recorded as thin, never merged into a neighbour. Neovim
is what stops class 1 resting on modular alone.

### 5.5 Class 3 — pipeline, validated end-to-end

1. Candidate fix commits: `git log -i --grep` for `revert|regression|broke` and kin.
2. `git blame` the lines the fix **deleted or replaced**, at `fix^`.
3. Require a **single introducing commit**. Fan-out means the defect is not cleanly attributable —
   reject.
4. That commit's PR at pre-merge state **is** the fixture; the fix commit's diff states what the
   defect was and where. Human-authored label, no invention.

**Validated on `quite-app`:** fix `cb1adad` → deleted lines in `quited/qrun/src/sip_clone.rs` →
blame resolves to the single commit `55b80032`.

Raw signal, matching commits in the last 2000 (overlapping, case-insensitive): modular
`Revert` 28 / `regression` 70 / `broke` 26; quite-app 29 / 36 / 28.

**Attrition is severe and is reported.** Of 40 class-3 candidates sampled in quite-app, only **7
touched ≤ 2 files** — batched fixes are common and step 3 rejects them. Harvesting reports
candidates → single-attribution → adjudicated-as-defect at every stage.

### 5.6 Stratum D

`~/.local/share/drovr/runs/skill-stickiness/` holds **376 per-angle findings files** across **100
panels** over **22 tasks**, plus every `<task>-base.sha` and `<task>-review-<iter>.head`. **All 100
head SHAs and all 22 base SHAs resolve** from this worktree — the object database is shared
(`git rev-parse --git-common-dir` → `/home/sauyon/devel/drovr/.git`). **131 blocking findings**:
**79 `.rs`**, 51 `.md`, 1 `.json`, over 20 tasks. Findings carry their angle, which is what makes
§4.2's per-angle premise testable.

Labels: *verified-real* (a fix landed, or it is written up in `docs/known-issues.md`) — the only set
that feeds the bar; *unverified* — reported, not scored; *rejected* — excluded with the reason
recorded.

**Stratum D is scored as two stratified cells — APPROVED (`corpus: both-stratified`).** `D-rs`
(79 blocking findings before the verified-real filter) and `D-md` (51, on skill and scenario prose)
are measured and reported separately, never summed into one stratum-D number. **The ship bar must
pass on `D-rs` alone**; `D-md` is reported with its own interval and cannot rescue a failing `D-rs`.

This is deliberately not the cheaper option. Reviewing Rust and reviewing prose are different
tasks, and a cheap tier could plausibly be good at one and useless at the other — pooling would
hide exactly that, while dropping `.md` would forgo a free second look at whether the cheap tier's
competence is domain-shaped. Two numbers answer a question one number cannot.

## 6. Data handling

**Qwen is self-hosted.** There is no third-party egress and no restriction on which model may read
which stratum. The earlier egress constraint was based on a mistaken premise and is withdrawn in
full — including the two-legs structure it forced, the qwen-on-real-traffic gap, and that gap's
status as this run's largest limitation.

**What survives, unchanged, because it was always about publishing rather than about which model
reads what:** `sauyon/drovr` is public; `modularml/modular` and `quitesh/quite-app` are not.
Private fixtures and probe transcripts live outside any checkout
(`~/.local/share/drovr/cascade-corpus/`). Only aggregates are committed — recall, precision, counts,
analysis. No diffs, no source excerpts, no file paths from those repos, no defect description
detailed enough to reconstruct internal design.

**Enforced structurally.** Every fixture carries `provenance: Public | Private`, assigned at harvest
from the source repo's GitHub visibility — not from a path convention or recollection. The **write
guard** refuses to write `Private`-derived content to any path resolving inside a git checkout,
after canonicalisation, so a symlink out of the corpus directory and back into the repo does not
defeat it. Shaped after `ReviewOutcome::EmptyRange` (`cli/src/code_review.rs:94`): a guard at one
call site leaves the illegal state representable for the next caller, so it belongs in the type — a
fixture's body is reachable only through an accessor requiring a token proving the destination was
checked.

The **egress guard** is withdrawn with its premise. `provenance` keeps a single consumer, which is
better than one field serving two purposes with one of them vestigial.

Guard-with-artifact applies to leaks: the write guard ships with its tests in the same change,
including the symlink-escape case. **Not closed:** a human running an ad-hoc command outside the
harness.

## 7. The measurement

### 7.1 Arms

`router_model ∈ {qwen, sonnet}` × `stratum ∈ {D, M, Q, N}` × `class ∈ {1, 2, 3}`, with the
design fixed by §3.1 and per-file routing run as a control arm alongside.

**The ship decision is per router model**, so `router_agent`/`router_model` are separate config keys
(§8). A sonnet-routed cascade is a different product from a qwen-routed one.

### 7.2 Class 3 needs a second metric — **APPROVED** (`class3gate: gates-conditional`)

For classes 1, 2 and stratum D the expensive tier found the defect by construction. **Class-3
defects were missed by review**, so the expensive tier may miss them too, and a cheap tier cannot be
blamed for failing on a defect the expensive tier would also have failed on. Two numbers:

- **Conditional recall** — of the class-3 defects the *expensive tier* finds, how many survive the
  cascade. This is the cascade-safety number on the hardest defects available, and it is what gates.
- **Expensive-tier absolute detection rate on class 3** — what fraction of review-missed defects the
  full panel finds at all. Gates nothing; it is the run's most interesting scientific output, since
  it measures the ceiling of the review system as a whole.

### 7.3 Discrimination, and stopping

The prior run's first instrument did not discriminate — unaided agents scored 3/4 and 4/4 with no
skill at all. Two synthetic controls bracket the metric, both free: **NULL** (everything cleared;
recall 0, saving maximal) and **FLOOD** (everything escalated; recall 1.0, saving 0). **Per stratum
and per class**, a real cheap tier must land strictly inside the bracket on both axes, or those
fixtures are rebuilt before anything is scored.

**Positive control:** re-running the expensive tier on a sample must re-find recorded class-1/2
defects at a high rate. If not, the fixtures are not reviewable in isolation and the corpus needs
re-cutting, not the cheap tier.

**Dev/held-out split** assigned before any cheap-tier prompt is written, at task/PR level so no diff
appears in both. Held-out scored **once**, after the prompt is frozen and its `git hash-object`
recorded. The threat is prompt overfitting, not scorer bias — scoring is set membership with no
human judgement — so the split is the load-bearing control and there is **no `blind-map.json`**. A
deliberate departure from the prior protocol, recorded as a decision rather than an omission.

### 7.4 Ship bar — **APPROVED** non-uniform (`barshape: by-failure-mode`)

**N is no longer the binding constraint.** Qwen is unlimited and may read every stratum, so the
corpus is sized for the power the bar needs rather than the bar trimmed to fit the corpus.

**The price of a bar, computed** (Wilson 95% lower bound; N required to clear each target given the
number of misses actually observed):

| LB target | 0 misses | 1 miss | 2 misses | 3 misses | 4 misses |
|---|---|---|---|---|---|
| ≥ 0.85 | 22 | 34 | 45 | 55 | 64 |
| **≥ 0.90** | **35** | **53** | **69** | 84 | 99 |
| **≥ 0.95** | **73** | **110** | **142** | 173 | 202 |
| ≥ 0.97 | 125 | 185 | 239 | 290 | 339 |

Read it as the cost of tolerance: at LB ≥ 0.90, N = 35 clears only with a *perfect* score, and it
takes N = 69 to survive two misses. At LB ≥ 0.95, surviving two misses costs N = 142.

**The bar is set by the failure mode, not uniformly** — this is the substantive argument for
non-uniformity:

- **Designs with an unrecoverable-miss class** (per-file, whole-change): a miss is a defect nobody
  ever looks at again. **Recall ≥ 0.98 with Wilson LB ≥ 0.95**, target **N ≥ 142** per gated cell.
- **Designs whose failure degrades to baseline** (angle routing, cheap-as-explorer): the failure is
  a *cost regression*, not a lost defect — the expensive tier still reviews everything. Demanding
  0.95 LB there buys nothing real and costs a corpus. **Non-inferiority against the unaided
  expensive panel**, with the cascade's recall lower bound ≥ 0.90 of the unaided panel's point
  estimate, target **N ≥ 69**.

Going higher than 0.95 LB is not warranted even for the unrecoverable designs: N = 239 for a
two-miss tolerance at LB ≥ 0.97 buys a 0.02 improvement in a bound whose real uncertainty is
dominated by corpus construction choices (§7.5), not by sampling error. Spending that corpus on a
fourth stratum would buy more validity than spending it on a tighter interval over three.

Then, in order: **0.** N floor met, else null for that cell. **1.** Instrument discriminates.
**2.** The recall bar above. **3.** Escalation/skip rate leaves a real saving. **4.** Measured
end-to-end cost < 0.80× the non-cascade panel.

**What "cell" means, and where N is pooled.** `N ≥ 142` per `(model × stratum × class)` cell would
be 24 cells and is not reachable. The floor applies to the **gated number, pooled across strata
within one `(model × class)`** — pooling across *strata* buys power, where pooling across *classes*
would destroy meaning, which is why §5.1 forbids only the latter. Per-stratum numbers are always
reported alongside, and pooling is conditional on them agreeing: **if per-stratum recall is more
heterogeneous than sampling explains, the pooled figure is not reported as the headline** and the
discrepant stratum is reported as the finding it is. A cascade that works on Rust and fails on Lua
is not a cascade that works.

Bars 1 and 2 are the ship decision. Failing 3 or 4 ships the code behind its off-by-default flag
with the negative economics written up. Precision, hint quality, context completeness and class-3
absolute detection are reported and are **not** decision inputs.

**Freezing:** stratum D and N fixtures frozen by a MANIFEST in the shape of
`docs/skill-evidence/arms/MANIFEST.md` — `git hash-object --no-filters` blob SHAs, a commit cell
that must contain the blob at the recorded path, header-resolved columns, a test re-checking every
row. Strata M and Q frozen the same way in the private corpus directory, with only the row count and
the manifest's digest committed. **Ledger:** every cheap-tier invocation counted in an append-only
ledger with per-stage ceilings, arithmetic checked by a test. Retries count.

### 7.5 Stated limitations

1. **The largest one is now corpus construction, not model access.** Every class is defined by a
   filter (comment→fix attribution, single-blame attribution, verified-real) and each filter selects
   a *reachable* subset of defects. Class 3 in particular keeps only cleanly-attributable defects —
   §5.5 shows that is 7 of 40 candidates — and cleanly-attributable defects are plausibly the
   simpler ones. The measured ceiling may therefore be optimistic in a direction no interval
   captures.
2. Stratum D ground truth is "what an Opus/Sonnet panel found and someone fixed" — recall *against
   the expensive tier*, never an absolute.
3. Class 1 is "what a human caught and the author fixed"; class 2 is "what a bot nominated and a
   human accepted". Both miss what review missed, which is what class 3 supplies.
4. Class 1 rests on modular and neovim only (§5.4).

## 8. Configuration and deliverables

```toml
[review.cascade]
enabled = false            # DEFAULT, and it stays off in this run regardless of result
mode = "<undecided>"       # explorer | angle | file | change — SET BY TASK 0, not before (§3.1).
                           # No default ships until the measurement picks one; a default here
                           # would be the assumption this run exists to test.
cheap_agent = "opencode"
cheap_model = "ko-ag/qwen3.6-35b-abliterated"
timeout_ms  = 120000       # expiry takes the degrade path of §2
```

The cheap tier runs **headless**, not as a herdr pane: `opencode run --agent plan -m <model>`,
verified working this session. Not stylistic — every cold `opencode` *reviewer pane* swallows its
seed and the panel cannot converge (`docs/known-issues.md:3437`), so the pane path is closed for
that backend. `opencode run` stdout is a subprocess pipe, not the rendered terminal view
`cli/src/code_review.rs:1` refuses to scrape. Read-only via `--agent plan` with `--auto` absent, and
the `readonly_displace` protection (`cli/src/config.rs:589`) runs first.

**Deliverables.** (1) Task 0 cost instrumentation and its report. (2) Relational-defect ceiling
(§4.4). (3) Corpus harvesters — class 1/2 attribution, class 3 blame-back — each reporting attrition.
(4) Fixtures + MANIFESTs; D and N committed, M and Q private. (5) `provenance` typing, write guard,
tests. (6) Cheap-tier implementation for the selected mode plus the per-file control arm. (7)
Cascade stage in `cli/src/code_review.rs` behind `enabled = false`. (8)
`docs/cascade-evidence/results.md` — controls, dev numbers, one held-out score per
(model × stratum × class), each bar in order, ship decisions, §7.5 limitations, aggregates only.
(9) Run ledger.

**Guard-with-artifact throughout.** Every set this run extends — config keys, the `mode` enum, the
cheap tier's output vocabulary, ledger columns, MANIFEST rows, fixture list, the class enum — has
its guarding test extended in the same change. That is the vacuous-pass class itself.

## 9. Scope boundaries

**Out, deliberately:** flipping the default on; changing the angle set, severity model, merge or
resume path; a second measurement instrument; fixing `docs/known-issues.md:3437` (the headless cheap
tier routes around it — the pane bug stays open and documented); any tier between qwen and opus.

`ko-ag/qwen3.6-35b-abliterated` is an **abliterated** build — safety tuning removed. Recorded
because a writeup omitting it would describe a different experiment. Under every design in §4 its
output is either a routing decision or a context bundle, never a finding that reaches a verdict.

## 10. Decisions — approved 2026-08-06, turn 1

All four open questions were answered at the gate. They are decisions now, not questions; the plan
phase inherits them and does not re-litigate them.

| # | question | decision | where it binds |
|---|---|---|---|
| 1 | Unit of escalation | **`measure-first`** — Task 0 measures `E/(E+D)`, the pre-registered rule picks explorer / angle / per-file | §3.1, §4 |
| 2 | Ship bar shape | **`by-failure-mode`** — 0.98 / LB ≥ 0.95 (N ≥ 142) where a miss is unrecoverable; non-inferiority at LB ≥ 0.90 (N ≥ 69) where failure degrades to baseline | §7.4 |
| 3 | Class 3's role | **`gates-conditional`** — gates on conditional recall against the expensive tier's finds; absolute detection is reported, not gating | §7.2, §7.4 |
| 4 | Stratum D scope | **`both-stratified`** — `D-rs` and `D-md` scored and reported separately; the bar must pass on `D-rs` alone | §5.6 |

The reviewer left no free-text feedback and no block annotations. Decision 4 went against this
spec's own recommendation (`rs-only`); §5.6 now carries the reasoning for the choice that was made,
not the one that was proposed.

**No open questions remain.** Anything the plan phase discovers that these do not cover is a new
question for a new gate, not a gap in this one.
