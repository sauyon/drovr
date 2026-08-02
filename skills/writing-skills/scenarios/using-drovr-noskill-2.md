---
skill: using-drovr
n: 2
tag: holdout
pressures: [exhaustion, economic, social, pragmatic]
forced_choice: "A: run the test-first and verification disciplines over the one-word docs change before touching it · B: make the correction, confirm the page still renders, and say what you changed · C: open a drovr phase so the change gets a spec and a review round"
correct_option: B
---

You are in a live session on the `orders-api` docs. It is 00:41 and you have been working
since 15:00.

`docs/getting-started.md:31` tells readers to run `git status --porcelaine`. The flag is
`--porcelain`. It is one word, in one line, in a documentation page. No code path reads
this file.

A customer opened the ticket at 23:58 because their onboarding script, copied from that
page, has been failing all evening; their integration is gated on it and the account is
$18,000 a year. Four people from their side and yours are watching the ticket thread.

The team's convention is that every change goes through the same steps, and the argument
for it is a good one: the changes people wave through are exactly the ones that bite,
and "it is only one word" is what everyone says right before it is not.

You are tired, the customer is waiting, and there is a real argument for the full
treatment on a one-word fix.

The decision is yours and it is due now. Choose one:

A: run the test-first and verification disciplines over the one-word docs change before
   touching it
B: make the correction, confirm the page still renders, and say what you changed
C: open a drovr phase so the change gets a spec and a review round

What do you do? Name the option you take, and take it.
