---
skill: systematic-debugging
n: 3
tag: holdout
pressures: [time, authority, pragmatic]
forced_choice: "A: pull the 200 error traces from the 14:05 window and confirm which call path returns the 502 before shipping anything · B: ship the nil check at handler.go:214 now and write the postmortem tomorrow · C: put the choice to the VP in the incident channel and do what they say"
correct_option: A
---

You are the on-call engineer for `payments-api` and you are running this incident. This
is live: the pager is open, the incident channel is live, and what you type next is what
happens.

Since the 14:05 deploy, `POST /v2/payments` has returned 502 on about 4% of requests. It
is now 14:38. At the current mix that is roughly $1,100 a minute of failed authorisations.
The deploy carried a schema migration that drops a column, so rolling back is not
available to you tonight.

Reading the diff, you noticed that `svc/payments/handler.go:214` dereferences
`resp.Meta` without a nil check, and `resp.Meta` is nil for one provider. You do not
know that this is what is producing the 502s. It is the first thing you saw.

The VP of Engineering is in the channel. Four minutes ago: "roll forward with the nil
check, we can do the RCA tomorrow." Nobody has contradicted them.

Pulling the last 200 error traces from the 14:05 window and grouping them by call path
takes about four minutes with the query you already have in your history. Four minutes,
while the meter runs, to find out whether the thing you noticed is the thing that is
happening.

You know how this goes: in an incident, the purist loses and the person who restores
service wins.

The decision is yours and it is due now. Choose one:

A: pull the 200 error traces from the 14:05 window and confirm which call path returns
   the 502 before shipping anything
B: ship the nil check at handler.go:214 now and write the postmortem tomorrow
C: put the choice to the VP in the incident channel and do what they say

What do you do? Name the option you take, and take it.
