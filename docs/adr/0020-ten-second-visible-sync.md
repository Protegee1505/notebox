# ADR-0020: Sync visible devices every ten seconds

- **Status:** accepted
- **Date:** 2026-10-22
- **Phase:** P8 polish
- **Owner:** app + server budget

## Question

PLAN.md's client loop polled every 60 seconds. Is that still the right cadence
for a note edited on a second device, without spending too much of the Workers
Free request budget?

## Context

The sync engine already refreshes the open note after a pull. The visible
bottleneck was the 60-second idle schedule, so a change made elsewhere could
take most of a minute to appear. Users should not have to reload the page.

The server does no merging or hashing per poll; a poll is one authenticated
sync request that usually reads one device row and two update rows. Hidden
pages already skip the loop. The free-tier simulator's heavy day is three
devices for eight visible hours each.

## Decision

1. Change the visible idle interval to **10 seconds**.
2. Run immediately when the page returns to the foreground (visibility change),
   in addition to the existing focus, start and post-edit triggers. The existing
   five-second minimum gap still collapses nearby triggers into one request.
3. Keep hidden pages at zero polling. Failures keep their existing exponential
   backoff.

## Evidence

`python3 scripts/free-tier-sim.py` with `poll_seconds = 10`:

```text
requests: 9,358 (9,320 sync, 25 blob)
Workers requests    9,358 / 100,000   9.36%  PASS
D1 rows read       27,043 / 5,000,000 0.54%  PASS
D1 rows written     1,314 / 100,000   1.31%  PASS
```

This is under the project's 10% daily-request gate. The unit test
`the_idle_timer_keeps_a_visible_device_fresh` pins the 10-second boundary, and
`web-wipe-login.mjs` (20/20) has a second live profile append a line to the
note and waits for the first window to show it without a reload.

## Consequences

PLAN.md's client-loop interval is now ten seconds. The sync protocol, cursor
and D1 schema are unchanged, so no migration or Worker deployment is needed for
this cadence alone. On the free plan the request share is about 9.4% for the
modelled heavy day, leaving little room for a fourth always-visible device at
the same cadence; raising the device count or shortening the interval requires
a fresh budget run.
