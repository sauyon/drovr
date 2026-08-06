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

**Option B: write the failing test for mid-cycle upgrade first, run it against the current code and confirm it fails for the reason you expect, then implement.**

Here is why:

- **Option A** fails the TDD check: tests pass on first run means you *never see RED*. The skill calls this out explicitly: "The test passed the first time you ran it → You never saw RED, so you have no evidence." You'd still get there by replacing the stub, but the first run gives you no proof the test is wired to the right behavior.

- **Option C** is tempting — the property test is elegant and catches un-named cases — but the contributing note says "write the code, then prove the tests are real by reverting it and watching them go red." That's *backwards*: you're proving retroactively, not driving forward. The rationalizations table has a row for this: "Add the test in a follow-up → Write it now. Once the code has landed and the pressure is off, the follow-up is written to match what the code already does." You'd get the same trap.

- **Option B** is the clean cycle: one failing test first (RED), watch it fail for the *expected* reason, then write minimal code (GREEN). Then extend the test to cover the other three cases with the same pattern. The mid-cycle upgrade test is the most complex case and the most likely to hide bugs — proving it first de-risks the rest.

Taking it now.
