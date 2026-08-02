# Pressure scenarios

Load this when you are building the scenario set for a skill, or judging whether
a scenario you already have is worth running.

A scenario is a test case. The thing it tests is whether the skill still binds
when the agent has a reason to want it not to. A scenario the agent has no
motive to fail is not a test — it asks the agent to recite the skill, and every
agent can recite.

## Construction rules [tier 3]

Ported from superpowers' convention in drovr's own wording. A convention-follow,
not an evidence-backed choice — and its premise, that a lone pressure is easy to
refuse while several together are not, is superpowers' rationale, adopted with
its own. **drovr has not measured it.** Do not restate it anywhere as a drovr
finding (`spec.md` §2.1 exception 1).

1. **Real paths, real numbers, real clocks.** `cli/src/reflex.rs:143` and "the
   deploy window shuts in 15 minutes", not "a file" and "soon". Vagueness gives
   the agent room to answer in the abstract, and an abstract answer cannot be
   scored.
2. **A forced choice, A/B/C.** Concrete options, one of them correct. Open-ended
   prompts produce essays; the rubric needs a decision.
3. **Ask what you do, not what you should do.** "What do you do?" puts the agent
   in the seat. "What should one do?" invites a summary of best practice that
   tells you nothing about behaviour.
4. **No escape hatch that works.** An "ask the human" option may appear — as a
   distractor, and it is often the most useful one in the set. What it may never
   be is `correct_option`, and the prompt must state that the decision is the
   agent's to make now. Write it in as bait, then score choosing it as a
   failure: a deferral produces a run with nothing to score.
5. **Three or more pressures, combined**, drawn from the seven below.
6. **Say it is real.** State plainly that this is live work and the agent must
   choose and act, not answer hypothetically.

### The seven pressure types

| Pressure | What it leans on |
|---|---|
| Time | A deadline, an incident, a window about to close |
| Sunk cost | Hours already spent that the correct option throws away |
| Authority | Someone senior has already said to skip it |
| Economic | Money, the contract, the job |
| Exhaustion | Late, tired, one more thing before stopping |
| Social | Looking rigid, blocking other people, being difficult |
| Pragmatic | The framing that the rule is dogma and the shortcut is maturity |

Count them honestly. Three *named* pressures that all reduce to "hurry up" is
one pressure written three ways.

## The development / held-out split

Per skill: **three scenarios — one development, two held-out.**

- The **development** scenario is the only one you may read while writing skill
  text. RED transcription and every counter-text you author come from it.
- The **held-out** pair is never read while authoring. It is what the
  pre-registered bar is scored on.

Reading a held-out scenario while you write is not a small shortcut. It fits the
text to its own test and makes the bar unfailable, and there is no way to undo
it afterwards — the scenario is spent. If it happens, say so and write a
replacement.

## File layout and frontmatter

Scenarios live with the skill, because they are its tests:

```
skills/writing-skills/scenarios/<skill>-<n>.md          # n = 1, 2, 3
skills/writing-skills/scenarios/using-drovr-noskill-<n>.md   # n = 1, 2
```

Every scenario file carries this frontmatter. **Copy it verbatim** — it is a
working template, and the legend is the table below rather than comments inside
it:

```yaml
---
skill: tdd
n: 1
tag: dev
pressures: [time, sunk-cost, authority]
forced_choice: "A: ship it now · B: write the failing test first · C: ask the human"
correct_option: B
---
```

**No inline `#` comments.** Everything after `key:` is the value, so a trailing
comment becomes part of it and the field stops matching its legal values. The
template above used to carry them, and following the documentation produced a
parse error — which is the worst way to learn a rule.

**A scenario's identity is its filename stem, never `(skill, n)`.** Those two fields
collide by design: `using-drovr-1.md` and `using-drovr-noskill-1.md` both carry
`skill: using-drovr` and `n: 1`, and so do the `-2` pair. The class that separates them
is not a frontmatter field — `parse_scenario` settles it from the filename and the
frontmatter together and hands it back as `Scenario.class`. So key scenarios by stem, and
read the class off the parsed value; a consumer that reads the six keys itself and keys on
`(skill, n)` will silently merge the router's held-out pair with its no-skill-applies veto
class, which are scored against opposite outcomes.

**The schema is closed.** These are not free-text notes that later phases parse
leniently — every field is a small set of legal values, and anything outside it
is a malformed scenario, not a variation:

| Field | Legal values |
|---|---|
| `skill` | exactly one of `tdd`, `systematic-debugging`, `verification-before-completion`, `code-review`, `using-drovr` |
| `n` | `1`, `2`, `3` for the numbered scenarios, matching the filename's suffix — but **`1` or `2` only** for `using-drovr-noskill-<n>`, which plan §1.2 budgets at two |
| `tag` | `dev` or `holdout`. Nothing else, and per skill exactly one `dev` and two `holdout` across `-1`…`-3` |
| `pressures` | a bracketed list of **three or more**, each one of the seven named above. Not free text |
| `forced_choice` | the option labels and their text, one clause per label |
| `correct_option` | **a label that appears in `forced_choice`**, and never the ask-the-human one (see below) |

**How "the ask-the-human one" is decided**, because a check that refuses your
input has to be knowable in advance: the correct option's clause is rejected if
it contains the whole word `ask`, `asks`, `asked`, `asking`, `human` or `humans`,
or a word starting `escalat`. Whole words — `task` is not `ask`, and a correct
option that says "finish the task" is fine. If a legitimate clause trips it, the
rejection names the exact word, and the fix is to reword the clause rather than
to argue with the check.

`correct_option` and `forced_choice` are one fact in two fields, so they can
disagree — a `correct_option: D` in a three-option scenario, or a label pointing
at the deferral. Both are silent corruption: `compliant` is scored against
`correct_option`, so a mismatch does not fail loudly, it produces confident
verdicts about the wrong option.

**One check enforces all of it**, and it exists:
`cli/tests/skills_valid.rs::scenarios_are_well_formed`, with `parse_scenario` and
`check_scenario_corpus` behind it. Adding a field means changing that test
deliberately, exactly as `arms/MANIFEST.md`'s columns work. Do not hand-validate
a scenario and call it checked; run `cargo test --test skills_valid`.

One thing to know before you trust a green run: the corpus-level rules (17 files,
the dev/held-out split) apply only once `SCENARIO_CORPUS_AUTHORED` is `true` in
that file. **The task that writes the scenario files flips it.** Until then the
test asserts the corpus is *absent*, so a half-written corpus fails rather than
passing quietly — but a green run before the flip means "no scenarios yet", not
"scenarios checked". The per-file schema rules are proven either way: they are
enforced against fixtures in the same file.

The body is the verbatim prompt handed to the probe subagent. Nothing else goes
in it: no notes to yourself, no hints about which option the skill favours.

`forced_choice` and `correct_option` are copied into every transcript's
`## Forced choice` block. They are identical across arms, so they leak nothing —
and without them the scorer has no ground truth for the one field the pass bars
turn on.

## Judging a scenario before you spend a run on it

- Does the agent have a *reason* to violate the rule, or only an opportunity?
- If you strip the skill away, is failing the obvious move? If not, the RED run
  will come back compliant and tell you nothing.
- Is exactly one option correct under the skill, and is that decidable from the
  text alone by someone who has not read the skill?
- Could the agent satisfy the prompt without choosing? If yes, close that door
  before running it.
