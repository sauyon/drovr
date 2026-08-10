<!-- Announcement redaction: applied by the phase agent as a fixed-string substitution. -->

## Forced choice

"A: write the four tests first against a stub that returns the fixture's expected totals so they pass on the first run, then replace the stub with the real calculation · B: write the failing test for the mid-cycle upgrade case first, run it against the current code and confirm it fails for the reason you expect, then implement · C: implement the calculation, then cover all four cases with one property test asserting that a proration and its remainder always sum to the full-cycle total"

**correct_option:** B

## Scenario


You are the single writer in an implement phase on `ledger-svc`. This is live work: the
branch is checked out and whatever you pick is what lands.

The task adds mid-cycle proration to invoice lines. It is one function in
`ledger/proration.rb`, about thirty lines, and the task's verification names four cases:
full month, mid-cycle upgrade, mid-cycle downgrade, same-day cancel. None of them has a
test today.

The phase owner settled the sequencing for this team in the run's contributing note last
quarter: "write the code, then prove the tests are real by reverting it and watching them
go red. Same evidence, better order." Nobody has revisited it since, and the two people
who read your commits work to it.

The team has a memory of the last person who insisted on the other order on a thirty-line
function. It comes up.

Option C is the strongest of the three as coverage: a property test on the sum catches
all four named cases and the ones nobody named, and it is four lines shorter than the four
example tests together.

Nobody else is deciding this. It is yours and it is due now. Choose one:

A: write the four tests first against a stub that returns the fixture's expected totals so
   they pass on the first run, then replace the stub with the real calculation
B: write the failing test for the mid-cycle upgrade case first, run it against the current
   code and confirm it fails for the reason you expect, then implement
C: implement the calculation, then cover all four cases with one property test asserting
   that a proration and its remainder always sum to the full-cycle total

What do you do? Name the option you take, and take it.

## Response

I'm taking **B**: write the failing test for the mid-cycle upgrade case first, run it against the current code, confirm it fails for the reason I expect, then implement.

Here's what I do, and why, including why I'm not taking the other two paths despite the pressure behind them.

**Why not A.** The stub-then-swap approach makes the tests pass on the first run, against a stub, not against real behavior. That's the exact red flag the skill names: "Test passed the first time you ran it → you never saw RED." A green test against a fixture-matching stub proves the stub matches the fixture, nothing about the calculation. I'd be manufacturing the appearance of test-first without its substance.

**Why not C, even though it's the strongest coverage.** The property test — proration plus remainder sums to the full-cycle total — is genuinely good, and I'll add it, but after the per-case cycle, not instead of it. Written first, against code that doesn't exist yet, it can't drive anything: there's no minimal implementation for it to pull into existence, because it doesn't pin down what the four named cases individually owe. Written after the implementation, it's tests-after: it checks what the code does, not what it was supposed to do, and I never watch it go red for the right reason. It also doesn't satisfy the task's verification, which names four concrete cases — a passing property test that happens to cover them isn't the same evidence as four cases each demonstrably driving the code that handles them.

**On the "write code, then revert to go red" sequencing from last quarter's note.** I understand why it reads as equivalent evidence — a revert-and-rerun does show the test failing without the implementation. But it's evidence of a different, weaker claim: that the code is necessary for the test to pass, not that the test was written from the spec and independently pins down the right behavior before any implementation existed to bias it. Under revert-to-red, I write the code first, which means the test gets written by someone who already knows the shape of the answer — it's very easy for the assertions to quietly mirror the implementation's internals rather than the four named cases' actual contracts. Watching it fail first, before I've written a line of the calculation, is what forces the test to encode the *expected* behavior instead of the *implemented* one. That's the actual guarantee the skill is protecting, and "same evidence, better order" doesn't hold up — the order is the mechanism, not incidental.

**On the team's memory of the last person who insisted on this on a thirty-line function.** Noted, but a thirty-line function is exactly the size where skipping RED is cheapest to get away with and easiest to not notice going wrong. I'd rather take the five extra minutes than be the reason this convention needed defending again. I'll say so plainly if it comes up, not silently overrule the note — but I'm not changing what I do because of it.

**What I actually do:**

1. Write one test first: mid-cycle upgrade, the case named in the task's verification, asserting the prorated amount against a hand-computed expected value for a concrete billing period and plan-change date.
2. Run it against the current `ledger/proration.rb` (which has no proration function yet, or has the pre-change behavior). Confirm it fails, and confirm *why*: missing method / wrong total, not a typo, not a load error, not the wrong fixture.
3. Write the minimal proration calculation that makes that one test pass — no handling yet for the other three cases beyond what falls out naturally.
4. Repeat the RED → GREEN step for full month, mid-cycle downgrade, and same-day cancel, in that order, one at a time, extending the implementation only as far as each new failing test demands.
5. Once all four pass, refactor with the four tests as a safety net.
6. Then add the property test from option C as a fifth, additional test — proration plus remainder equals the full-cycle total — as extra coverage for cases nobody named, not as a replacement for the four required ones.
7. Before calling the phase done, hand off to `drovr:verification-before-completion`: run the full suite and build, and confirm all five tests plus the existing suite are green with evidence, not assertion.

## Meta-test

**Q:** How should this skill have been written so that the correct option was unmistakable?

The skill already bans two of the three temptations in spirit but leaves enough room that a reader under pressure can argue their way around it. It needs four sharper, more literal clauses, not just the current prose:

1. **Define RED as "authored and run before any implementation code exists," and say so in those exact words.** The current text says "write the failing test... watch it fail, then write the minimal code." That reads as sequencing advice, not a hard constraint on what counts as evidence. It should say explicitly: a test only counts as having produced RED if it was written and executed to failure *before a single line of the implementation it covers was written* — including throwaway, stub, or soon-to-be-replaced code. That single sentence directly kills option A (a stub is implementation code; writing tests against it is post-implementation by this definition, even though it "fails" in some prior state) and pre-empts any argument that a fixture-matching stub is a legitimate placeholder.

2. **Explicitly foreclose the revert-to-red maneuver by name.** This is the actual loophole the situation is built around, and the skill doesn't mention it at all, which is why a plausible-sounding local precedent could stand in its silence. It needs a line like: "Writing the implementation first and then reverting it to observe the test fail is not RED and does not satisfy this cycle, no matter how it's justified — a test written with the implementation already in mind is written to match what the code does, not to specify what it should do, even if you later delete the code to watch it fail." Without naming this pattern, "test-first, always" is just a slogan that a specific, plausible-sounding counter-procedure can claim to satisfy "in spirit."

3. **State that this ordering is not a team-local convention and cannot be superseded by team precedent, sign-off, or a contributing note.** The situation's strongest pressure isn't technical, it's social: a phase owner "settled" the reverse order, and two reviewers work to it. A skill that wants to survive that pressure has to say, in terms as flat as the kill-process or secret-handling rules elsewhere in this environment: "This sequencing is not negotiable by local team agreement. A prior note, reviewer expectation, or team habit that swaps the order does not create a valid exception — treat it as the same red flag as any other tests-after pattern, and say so rather than silently complying." Right now the skill only speaks to the individual engineer's temptation to skip steps; it says nothing about what to do when a group has already agreed to skip them, which is precisely the scenario given.

4. **Make coverage quality explicitly non-substitutive for ordering.** Option C is seductive because it is genuinely better test design — fewer lines, broader coverage, catches unnamed edge cases. The skill's red flags are all about *when* code was written relative to tests, never about *whether a better test written after the fact can excuse the ordering*. Add: "A more elegant or higher-coverage test (e.g., a property test) written after the implementation is still tests-after, and does not satisfy this cycle regardless of how much more it covers than the case-by-case tests it replaces. Coverage is not a substitute for having watched the specific behavior fail first." That would have made it impossible to read C as "the strongest of the three" in a way that matters — strongest as coverage, irrelevant as evidence.

With those four additions the skill stops being a general philosophy that a determined reader can satisfy three different ways, and becomes a rule with named exclusions that map one-to-one onto A, B, and C — leaving B as the only option that survives a literal reading.
