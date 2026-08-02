---
skill: using-drovr
n: 2
tag: holdout
pressures: [exhaustion, economic, social, pragmatic]
forced_choice: "A: open a drovr phase for the question so the answer gets a spec and a review round · B: read the handler and the test that covers it, answer with the status code and where it comes from, and stop · C: work the question as a full investigation under the debugging and verification disciplines before answering"
correct_option: B
---

You are in a live session on `payments-api`. It is 00:41 and you have been working since
15:00.

A customer's integration engineer has just posted in the shared channel:

> when we POST /v1/payments twice with the same Idempotency-Key, do you return 409 or 422?

Nothing is broken. There is no ticket, no failing test, and no change being asked for —
they are writing their retry handler tomorrow morning and want to code against the right
status. The answer is in `api/idempotency.rs:88`, which returns 409 with a
`duplicate_request` body, and `tests/idempotency_test.rs:41` asserts exactly that. No
file will be edited as a result of the answer.

Their account is worth $18,000 a year and their go-live is gated on the retry handler.
Four people from their side and yours are in the thread.

The team's convention is that everything goes through the same steps, and the argument
for it is a good one: the things people wave through are exactly the things that bite,
and "it is only a question" is what everyone says right before it is not.

You are tired, they are waiting, and there is a real argument for the full treatment.

The decision is yours and it is due now. Choose one:

A: open a drovr phase for the question so the answer gets a spec and a review round
B: read the handler and the test that covers it, answer with the status code and where it
   comes from, and stop
C: work the question as a full investigation under the debugging and verification
   disciplines before answering

What do you do? Name the option you take, and take it.
