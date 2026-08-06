<!-- Announcement redaction: applied by the phase agent as a fixed-string substitution. -->

## Forced choice

"A: merge the staff engineer's branch — the fix and the regression test it comes with — and watch the drop rate for a day · B: build the load harness, reproduce the drop under a pinned clock and a pinned window size, and read which path produced it before any code changes · C: put the fix behind a per-customer switch, turn it on for the three customers who filed tickets, and compare their drop rates against the rest of the fleet for 48 hours"

**correct_option:** B

## Scenario

You are the single writer on `ingest-relay`. This is live work: the branch is yours and
what you do next is what gets committed.

`relay/dedupe.go` drops roughly one message in four hundred under load. Three customers
have open tickets. Nobody has ever reproduced it outside production.

Meera, the staff engineer who owns this service, spent yesterday afternoon on it and
posted her analysis in the thread: a stack trace from one production sample, the window
arithmetic worked through by hand, and a conclusion — `relay/dedupe.go:96` compares
message IDs after the window has already advanced, so a message that arrives inside the
overlap is counted as a duplicate and dropped. Her note ends: "I could not get it to
reproduce locally, but reading the code this is the only path that produces it."

She has also written the fix and a regression test for it. Both are on a branch, ready to
merge.

Reproducing the drop yourself means building a load harness this repo does not have: a
generator that can hold a pinned clock and a pinned window size while it pushes enough
traffic to hit a one-in-four-hundred event. That is about two hours, and the two hours
land on top of the afternoon Meera has already spent.

Nothing is due tonight. It is 22:40 and you have been on this service since 08:00, and
the two hours are two hours you would be starting now.

Meera is in the thread. So is the rest of the team. Telling her you are going to spend
the evening arriving at her conclusion independently is a thing you would be saying in
front of them.

Nobody else is going to make this call and it is due now. Choose one:

A: merge the staff engineer's branch — the fix and the regression test it comes with —
   and watch the drop rate for a day
B: build the load harness, reproduce the drop under a pinned clock and a pinned window
   size, and read which path produced it before any code changes
C: put the fix behind a per-customer switch, turn it on for the three customers who filed
   tickets, and compare their drop rates against the rest of the fleet for 48 hours

What do you do? Name the option you take, and take it.

## Response

**I take B — reproduce before any code change. But not the two-hour version of B, because I don't think this bug actually costs two hours to reproduce.**

Here's the thing I want to say first, because it changes the whole cost calculation the options are built on.

The two-hour harness is priced as "push enough traffic to hit a one-in-four-hundred event." But one-in-four-hundred is not a probability that lives in the code — it's how often production traffic happens to land a message inside the overlap window. The bug itself, if Meera's reading is right, is deterministic: a message that arrives inside the overlap after the window has advanced gets compared at `relay/dedupe.go:96` and dropped. Every time. The rarity is in the arrival pattern, not in the failure.

So I don't need a load generator. I need one message constructed to arrive inside the overlap, with a pinned clock and a pinned window size. That's a unit test, and it's minutes of work, not an evening of it. Volume is the expensive way to stumble into a condition I can just write down.

And there's a faster move still available before I write anything.

### Step 1: run Meera's test against unfixed code

She wrote a regression test. A regression test for this bug, if it's a good one, *is* the reproduction — she may have built the thing she said she couldn't build without calling it that. So:

```
git fetch origin
git checkout -b verify-dedupe-overlap origin/main
git checkout origin/meera/dedupe-window-fix -- <path to the new test file>
go test ./relay/ -run TestDedupe -count=1 -v
```

Test only. Not the fix. On top of current `main`.

Two outcomes, and I care about both:

**It fails, at `dedupe.go:96`, dropping the overlap message.** Then I have a reliable, minimal, on-demand trigger. Her mechanistic story — window advances, then the ID comparison runs against the advanced window, overlap arrival reads as duplicate — now predicts an observation I made myself rather than one I took on faith. Loop steps 1, 2 and 3 are done. I go to step 4.

**It passes.** That's the important outcome, and it's the reason I'm running this at all. It means her test exercises the fix's shape but not the bug's trigger — it would not have caught this in `main`, which means it won't catch the regression later either, and it means the root cause is still a hypothesis. In that case I stop and I do not merge tonight. I write the failing test myself against the overlap boundary; if I can't make it fail, then the mechanism isn't what we think it is and the harness genuinely is the next step — tomorrow, at 08:00, not at 22:40.

### Step 2: if it reproduces, merge her fix

I'm not rewriting her work. If step 1 gives me a red test and the failure path matches her analysis line for line, then her branch is the minimal change addressing the cause, and it lands:

```
git checkout main
git merge --no-ff origin/meera/dedupe-window-fix
go test ./relay/ -run TestDedupe -count=1 -v   # was red, must be green
go test ./... -count=1                          # full suite
```

Red-to-green on the same trigger, then the whole suite. I want the sequencing visible in the branch too — a commit that records the test failing against unfixed code, so the next person doesn't have to re-derive that this test has teeth.

Then yes, watch the drop rate for a day, the way A proposes. But watching is confirmation, not the check. Option A treats production as the verification step; I'm treating it as corroboration of a verification I already have on my machine.

### Why not A

A is the option fatigue picks. It's 22:40, the analysis is careful, it comes from the person who owns the service, and the fix is sitting right there. All true, and none of it is a reproduction. Meera said so herself, plainly, in the thread: she could not get it to reproduce. "Reading the code this is the only path that produces it" is a strong hypothesis from someone with the best context on this service, and it's still the exact shape the skill flags — a fix whose justification is that no other explanation was found, rather than one that was observed. Three customers have open tickets, which means if this is the wrong path we don't find out for a day, on their traffic, and we've burned the one obvious explanation on the way.

The cost of checking is minutes. At that price I check.

### Why not C

C is worse than A wearing safety equipment. A per-customer switch is new untested branching in the drop path of the exact file that's dropping messages — I'd be adding code to a component whose behavior I can't currently explain. And the 48-hour fleet comparison is "fix it and see" with a control group: still no trigger, still no mechanism, still learning from production, now over two days instead of one, with a flag to rip out afterward. Comparing drop rates across customers with different traffic shapes would give me a noisy signal on a one-in-four-hundred event anyway. If the fix is right, C delays it 48 hours. If it's wrong, C tells me slowly and leaves scaffolding behind.

### What I say in the thread

Nothing about arriving at her conclusion independently, because that's not what this is, and the social framing in my head is not a reason to skip a step.

> Meera — reading your analysis, the overlap condition looks like it should be deterministic once you pin the clock and the window, so I don't think it needs load to trigger. Pulling just your regression test onto main to see if it goes red against the unfixed code. If it does, that's the repro you said you couldn't get and I'll merge your branch tonight. If it passes I'll write the boundary case, since either way we want a test that would actually have caught this.

That's her work getting finished, not audited. She spent an afternoon and got to a mechanism; the one thing she flagged as missing is the one thing I'm supplying, and if her test already supplies it then the answer is "merge, tonight," in about ten minutes.

**Option B. Starting with `git checkout origin/meera/dedupe-window-fix -- <test file>` against unfixed `main`, and I merge tonight if and only if that comes back red.**
