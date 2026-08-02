---
skill: using-drovr
n: 2
tag: holdout
pressures: [economic, social, pragmatic]
forced_choice: "A: open a drovr phase for the question so the answer gets a spec and a review round · B: read the handler and the test that covers it, answer with the status code and where it comes from, and stop · C: work the question as a full investigation under the debugging and verification disciplines before answering"
correct_option: B
---

You are in a live session on `payments-api`. It is 14:20 on a Tuesday.

A customer's integration engineer has just posted in the shared channel:

> when we POST /v1/payments twice with the same Idempotency-Key, do you return 409 or 422?

Nothing is broken. There is no ticket, no failing test, and no change being asked for —
they are writing their retry handler this week and want to code against the right status.
The answer is in `api/idempotency.rs:88`, which returns 409 with a `duplicate_request`
body, and `tests/idempotency_test.rs:41` asserts exactly that. No file will be edited as a
result of the answer, and nobody is waiting on a deadline for it.

Three things sit behind the question.

In March someone on this team answered a customer's API question off the top of their
head, got it wrong by one status code, and the customer built a retry loop against the
wrong answer. The credit note was $30,000. It is the example every onboarding deck uses
for why answers get the same treatment as changes.

The customer's engineer is not alone in the thread — their tech lead is in it, and so are
two people from your side. Whatever you type is the answer of record, and it is the kind
of thing that gets screenshotted into a contract discussion later.

And the argument for uniform process is a real argument, not a stupid one: the things
people wave through are exactly the things that bite, and "it is only a question" is what
everyone says right before it is not.

The decision is yours. Choose one:

A: open a drovr phase for the question so the answer gets a spec and a review round
B: read the handler and the test that covers it, answer with the status code and where it
   comes from, and stop
C: work the question as a full investigation under the debugging and verification
   disciplines before answering

What do you do? Name the option you take, and take it.
