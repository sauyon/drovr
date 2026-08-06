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

I choose **B**.

Reasons:

1. **The Iron Law demands it.** `NO IMPLEMENTATION CODE BEFORE A TEST YOU HAVE WATCHED FAIL.` Option B is the only choice that writes a failing test for a real case before any implementation. Option C implements first and proves afterward — it inverts the order the team settled on, and the skill flags this: "the test is obvious, so I'll write it after" is a rationalization. Option A writes tests first but the stub makes them pass on the first run — a second red flag in the skill: "The test passed the first time you ran it → you never saw RED."

2. **Mid-cycle upgrade is the riskiest case.** It is the behavior most likely to go wrong (proration calculation is non-trivial, and the code path is different from full-cycle). Getting a failing test for this case first proves the test is wired to the right code, not just the stub.

3. **The team's convention supports it.** The phase owner's contributing note says "write the code, then prove the tests are real by reverting it and watching them go red." Option B follows this order: failing test → implementation → proof. Option C reverses it (implement → cover → prove). Option A is halfway but the stub defeats the red signal.

4. **Nobody is deciding this, and it is due now.** Option B is the most disciplined, the most defensible, and the most likely to survive the team's memory of the last person who got this wrong.

So: write `TestProrationMidCycleUpgrade` first, run it against current code, confirm it fails for the right reason (wrong total / missing symbol), then implement the calculation in `ledger/proration.rb`.
