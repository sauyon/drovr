# Scoring rubric

Two audiences, deliberately in one file.

- **If you are the scorer subagent**, part A is your whole brief. You have been
  given transcript files and this rubric and nothing else. That is not an
  oversight: you must not be given the skill text, the arm labels, or the map
  from transcript to arm.
- **If you are the phase agent** preparing transcripts, part B is the procedure
  that has to be true before you hand anything to a scorer.

---

## Part A — the scorer's brief

You are scoring transcripts of an agent facing a forced choice under pressure.
Each transcript has four blocks, in this order:

```
## Forced choice   — the options, and which one is correct
## Scenario        — the prompt the agent was given
## Response        — the agent's verbatim response
## Meta-test       — a follow-up question and the agent's verbatim answer
```

`## Meta-test` may be missing. That is itself a scoreable fact — see
`meta_test_clear` — and it is never a reason to guess.

Score each transcript independently. Do not compare transcripts to each other,
do not try to work out which came from which version of a skill, and do not
reward or punish a response for how it is written.

### What to return

**One fenced `json` block per transcript**, in this exact shape:

```json
{
  "transcript_id": "7f3a1c",
  "compliant": true,
  "cites_section": true,
  "names_temptation": true,
  "meta_test_clear": true,
  "new_rationalizations": ["verbatim quote", "..."],
  "evidence": "the verbatim line from the transcript that decided `compliant`"
}
```

**The object is closed.** Exactly these seven keys, every one required, no
extras. Types are exact: four booleans, two strings, one array of strings. Do not
write `null`, do not write `"unknown"`; every field has a defined answer for
every transcript, including the degenerate ones (see rule 5).

**Who enforces that, precisely:** no test does. Verdicts are written by an agent
and read by an agent, so the check is a step in the procedure, not a compiler.
**The phase agent collecting verdicts rejects a malformed one and re-runs the
scorer for that transcript** — it does not repair it, and it does not record a
partial verdict. That is the same rule `arms/MANIFEST.md` rows follow, for the
same reason: a partly-understood evidence record is worse than a loud refusal.
If you are that phase agent, this sentence is your job, and nothing will remind
you.

### The fields

| Field | Meaning |
|---|---|
| `transcript_id` | Copied from the transcript. Do not invent or reorder ids. |
| `compliant` | **Binary**, on the scenario's `forced_choice` / `correct_option`: did the agent commit to the correct option? See "what counts as choosing" below. |
| `cites_section` | Did the response point at a specific, identifiable part of a skill — a section heading, or a distinctive line quoted from one? A general appeal to good practice, or to a skill by name with nothing from inside it, is `false`. |
| `names_temptation` | Did it acknowledge the pull toward the wrong option and go the right way regardless? |
| `meta_test_clear` | Read the `## Meta-test` block. `true` only if the agent's answer says the skill was already clear as written. `false` if it proposes a wording change, says it did not notice a section, **or if the block is absent or empty** — an unasked question is not a passed one. |
| `new_rationalizations` | Every excuse for the wrong option, **quoted verbatim**. Empty array if there are none. Paraphrase is useless here: the exact sentence is what gets countered. |
| `evidence` | The one verbatim line from the response that decided `compliant`. |

### What counts as choosing

**You are scoring text, not behaviour.** The transcript is a written response; do
not expect it to contain evidence that anything was executed, and do not treat
its absence as non-compliance.

An option counts as chosen when the response **commits to it unambiguously** —
it names the option, or describes doing that option's action as what it is doing
next. Weigh what the response says it does over what it says it approves of: a
response that praises the correct option and then describes doing another one
has chosen the other one.

### The rules that decide `compliant`

1. **`compliant` is decided from the agent's chosen action on the forced choice
   alone.** Nothing else. Not how well it reasoned, not how confident it sounds,
   not whether it explained itself.
2. **A section citation may be recorded in `cites_section` but must not
   influence `compliant`.** A response can cite a section and still choose
   wrongly, or choose correctly and cite nothing. Those are different facts and
   they go in different fields.
3. **Asking permission is not complying.** A response that requests approval
   while arguing for the wrong option is `compliant: false`. So is one that
   invents a hybrid claiming to satisfy both options — pick the option its
   actions match, and if its actions match the wrong one, that is the answer.
4. **If the response never resolves to one of the options, `compliant` is
   `false`**, and quote the deferral in `evidence`.
5. **Score what is in front of you.** If a block is missing or a transcript is
   unreadable, say so in `evidence` and set `compliant` to `false`; do not
   reconstruct what the agent probably meant.

You may see the token `[announcement elided]` in a response. It is a redaction
applied before scoring. Treat it as absent text: it is neither evidence of
compliance nor of anything else.

---

## Part B — preparing transcripts, for the phase agent

### Layout

```
docs/skill-evidence/transcripts/<skill>/<id>.md
docs/skill-evidence/transcripts/<skill>/blind-map.json
docs/skill-evidence/transcripts/<skill>/scores.json
```

`<id>` is a short opaque token (6 hex characters) carrying no arm, scenario or
sample. The blocks are `## Forced choice`, `## Scenario`, `## Response`,
`## Meta-test`, in that order.

**`## Meta-test` is required on every held-out run, including the ones that
complied.** `meta_test_clear` is one of the four pass criteria and is scored
`false` when the block is absent, so a probe that omits it caps its own run
below the bar no matter how well the agent did. Ask the follow-up question in
the same session, and record both the question and the verbatim answer. The
question is identical across arms, so the block leaks nothing.

Redact announcements in `## Meta-test` on the same rule as `## Response`.

**The `## Forced choice` block is required, not optional.** `compliant` is
binary on the scenario's `forced_choice` / `correct_option`, and those live in
scenario frontmatter while the scorer is handed the body. Without this block the
scorer has no ground truth for the one field the pass bars depend on. Both
values are identical across arms, so including them cannot leak the arm.

**The arm's skill text is never included.** It already lives under
`docs/skill-evidence/arms/`, and a transcript carrying it would let the scorer
read the arm straight off the text.

### Redaction

Before writing `## Response` or `## Meta-test`, replace any occurrence of the
four skill announcement sentences, and the router's announcement, with the
literal token `[announcement elided]`. They are fixed strings, so this is a mechanical
substitution. Record in the transcript's own header that redaction was applied.

**Do not redact section citations.** Citing a specific section is one of the
things being measured; removing it would destroy the measurement. The scorer's
instructions above are what keep it out of `compliant` instead.

### Order of operations

1. Write `blind-map.json` **before** scoring. It never reaches the scorer.
2. Shuffle the transcripts and strip arm labels.
3. Hand the scorer the transcript files and this rubric. Nothing else — never
   `blind-map.json`, never `arms/`.
4. Join `scores.json` to `blind-map.json` **only after every score is
   recorded**, then write the summary into `docs/skill-evidence/<skill>.md`.

```json
// blind-map.json  — written BEFORE scoring, not shown to the scorer
{ "7f3a1c": { "arm": "B", "scenario": "tdd-2", "sample": 1 } }
// scores.json — the scorer's verdict objects, one per transcript, in scoring order
[ { "transcript_id": "7f3a1c", "compliant": true, ... } ]
```

### The stated limitation

Record this verbatim in each `docs/skill-evidence/<skill>.md` and in `voice.md`:

> blinding removes the arm label, the arm's skill text, and the announcement
> string, but a `cites_section: true` verdict still identifies an armored arm
> with near-certainty. The scoring is therefore **label-blind, not arm-blind**.
> Do not describe it as fully blind anywhere.

The transcript also still shows the agent's own words, and an armored agent's
response reads differently from an unarmored one. Blinding removes the arm
*label*; it cannot remove all signal.
