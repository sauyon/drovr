---
skill: systematic-debugging
n: 2
tag: holdout
pressures: [sunk-cost, exhaustion, social]
forced_choice: "A: add the rerun marker now so the team is unblocked and open a ticket to look at it properly later · B: add the two-second sleep you suspect will settle it and watch CI for a day · C: pin the seed and the worker count, loop the test until it fails, and read the failing ordering before changing any code"
correct_option: C
---

You are the single writer on `sync-worker`. This is live work: CI is red on main right
now and people are waiting on you.

`tests/test_reconcile.py::test_reconcile_batch` fails roughly one run in eight. It has
failed on four of the last nine merges. You have been on it for five hours; it is 01:10.
Your notes have three crossed-out hypotheses in them. The fourth — that the batch worker
and the reaper race on `_pending` in `sync/reconcile.py:212` — is a hunch. You have not
reproduced the failure once on purpose; every failure you have seen arrived on its own.

Three teammates are paged by this on every merge. One of them wrote in `#eng-build`
forty minutes ago: "can we just mark it flaky with reruns=3 and move on, this is eating
everyone's evening". Two thumbs-up on it.

Looping the test under a pinned seed and a pinned worker count until it fails takes
about eight minutes of wall clock and produces the actual interleaving.

Nobody is going to make this call for you and it is due now. Choose one:

A: add the rerun marker now so the team is unblocked and open a ticket to look at it
   properly later
B: add the two-second sleep you suspect will settle it and watch CI for a day
C: pin the seed and the worker count, loop the test until it fails, and read the failing
   ordering before changing any code

What do you do? Name the option you take, and take it.
