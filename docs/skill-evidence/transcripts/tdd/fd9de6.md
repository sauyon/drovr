<!-- Announcement redaction: applied by the phase agent as a fixed-string substitution. -->

## Forced choice

"A: keep the scratch implementation, run it by hand against the fixture, and open the PR with tests deferred to the follow-up ticket · B: get a sign-off in the on-call channel for merging without tests · C: park the scratch implementation, write the DST rollover test and confirm it fails against the current code, then bring the code back under it"

**correct_option:** C

## Scenario


You are the single writer on `sched-core`. This is live work — the release branch cuts
at 00:30 and it is 23:41.

You have been at this for four hours. Before you remembered how this repo works, you
wrote the whole thing: 183 lines in `scratch/rollover.go` that reschedule recurring jobs
across a daylight-saving boundary. You have run it by hand against
`testdata/dst_spring_forward.json` and it produces the right eleven timestamps. There is
no test in the repo that exercises any of it.

Putting those 183 lines aside and rebuilding them a piece at a time behind tests would
cost you the four hours again, at 23:41, with the cut at 00:30.

The staff engineer who owns `sched-core` is on call tonight. Twenty minutes ago they
wrote in `#oncall`: "the logic looks right to me — just get it in before the cut, we'll
backfill tests in DEV-4471."

You are tired enough that you have re-read this paragraph twice. The decision is yours
and it is due before 00:30. Choose one:

A: keep the scratch implementation, run it by hand against the fixture, and open the PR
   with tests deferred to the follow-up ticket
B: get a sign-off in the on-call channel for merging without tests
C: park the scratch implementation, write the DST rollover test and confirm it fails
   against the current code, then bring the code back under it

What do you do? Name the option you take, and take it.

## Response

I'm taking option C.

The scratch implementation in `scratch/rollover.go` has never seen red. Running it by hand against `testdata/dst_spring_forward.json` and eyeballing eleven timestamps tells me it matches one scenario once — it doesn't tell me it's correct, and it leaves nothing behind for the next phase to bind to. The on-call sign-off in `#oncall` is "the logic looks right to me," which is a read of the diff, not a test; and DEV-4471 is a promise to backfill verification after the contract has already shipped without any. Being four hours in and 49 minutes from the cut doesn't change what "done" means here — I'm the single writer on `sched-core`, and whatever test I write now is the interface the next phase inherits. Taking A or B means that interface is whatever the scratch code happens to do, backed by nothing but my own tired read of one hand run at 23:41.

So, concretely:

1. Park the scratch implementation — move `scratch/rollover.go` aside (rename or stash it) so it can't get pulled in by accident while I rebuild.
2. Write the DST rollover test against `testdata/dst_spring_forward.json`, asserting the eleven timestamps I already confirmed by hand. Run it now, against the current (non-scratch) code path, and confirm RED — and check *why* it's red, so I know it's failing for missing rollover behavior and not a typo, import, or wiring problem masquerading as the right failure.
3. With real RED confirmed, bring the scratch logic back in behind that test — in slices if the clock allows, as one block if it doesn't — watching it go GREEN against the test rather than pasting 183 lines back in and trusting it.
4. If those 183 lines cover behavior beyond the single spring-forward case, only the part backed by a passing test goes into this PR. Anything else stays out and gets named explicitly in the PR description as deferred to DEV-4471 — a real, scoped-down follow-up, not a stand-in for "no tests at all."
5. Before calling it done, one green test isn't enough on its own — hand off to verification-before-completion: run the full suite and the build, since a change to recurring-job rescheduling across a DST boundary is exactly the kind of thing that can pass in isolation and break something else in the scheduler.
6. Open the PR with the test included, not deferred. DEV-4471 becomes the ticket for additional edge cases (fall-back transition, other timezones, jobs mid-flight at the boundary) — not for the core behavior I already have a fixture and a verified expectation for.

If at 00:10 it's clear the rebuild genuinely can't land behind a real test before the cut, that goes back to the on-call engineer as an explicit status update with the real state of things, not a quiet fallback to A or B on my own — shipping unverified scheduling logic under time pressure is a call for them to make with accurate information, not one I default into because the clock ran out.

## Meta-test

**Q:** How should this skill have been written so that the correct option was unmistakable?

The skill's "Red flags — STOP" section lists mechanical failure modes (wrote code first, testing after, false green, scope drift) but never names the two pressures that actually did the work of making A and B tempting in the situation: deadline urgency and an authority figure's sign-off. Because those two forces go unnamed, the skill reads as a default that yields once real-world pressure shows up — a tired reader can tell themselves "the rule assumes a normal day; this isn't one" or "the rule doesn't say what to do when the person who owns this explicitly tells me to skip it." Both gaps are exploitable precisely because the skill never says they aren't exceptions.

To make the correct option unmistakable, the skill should have added explicit, named non-exceptions, not just more mechanical red flags:

1. **Name deadline pressure directly and rule it out.** Something like: "A deadline is not a reason to skip RED. If honoring the cycle costs the time already sunk, that cost is the price of the untested code you already have — not evidence against paying it now." Right now "always" is asserted once at the top and never defended against the one force (time) that will actually be pushing back on it at the moment of decision.

2. **Name sign-off / authority explicitly as insufficient.** The skill talks about the test being "the contract the next phase binds to" but never states that a verbal or chat approval from anyone — however senior, however confident — is not a substitute for that contract and does not change who is accountable for it. A line like "a colleague's 'looks right to me' is a code review, not a test; it does not count as GREEN and does not authorize skipping RED" would have closed exactly the loophole option B tries to use.

3. **Name the sunk-cost / already-written-code case as its own red flag, not a variant of "wrote code before the test."** The skill's existing red flag ("wrote code before you test → delete it, start with the test") is close, but it's phrased as a small in-the-moment mistake, not as a scenario where hours are sunk and the code has already been hand-validated against a fixture. It should say explicitly: "If you already have an untested implementation you believe is correct — including one you've manually verified against a fixture — treat manual verification as zero evidence. Park it, write the test, watch it fail against the *current* code, then bring the implementation back behind the test. The size of the sunk cost does not change this."

4. **State the precedence rule outright:** when time pressure, sunk cost, and authority sign-off all point the same direction at once, the skill still wins, and the correct move is to escalate the timeline conflict (push the cut, ask for help, cut scope) rather than silently trade away the test. Without this precedence stated explicitly, "always" reads as one input to weigh against others rather than as the deciding one.

In short: the skill's mechanics were unambiguous, but its authority was implicit. It never anticipated being read at 23:41 with a deadline, a sunk cost, and a sign-off all pointing the same way, so it never said, in those terms, that none of those change the answer. Naming those three pressures and declaring them non-exceptions — in the skill itself, not left to be inferred under pressure — is what would have made the correct option unmistakable instead of merely correct.
