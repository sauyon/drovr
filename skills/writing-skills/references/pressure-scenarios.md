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

Every scenario file carries this frontmatter, and nothing else parses it today —
the measurement phases read it directly:

```yaml
---
skill: tdd                 # one of the five, or `using-drovr` for the noskill class
n: 1
tag: dev                   # `dev` | `holdout`; exactly one `dev` per skill, two `holdout`
pressures: [time, sunk-cost, authority]   # >= 3, from the seven above
forced_choice: "A: ship it now · B: write the failing test first · C: ask the human"
correct_option: B
---
```

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
