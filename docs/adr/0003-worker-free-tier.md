# ADR-0003: The Worker stays on the Workers Free plan

- **Status:** accepted on desktop and browser; phone measurement open
- **Date:** 2026-10-21
- **Spike:** S3
- **Owner:** server

## Question

Does the deployed Rust Worker stay inside the Workers Free CPU and request
limits for one account, and is the client-side Argon2id login key fast enough
on desktop and in the browser?

## Context

- The Worker is `notebox` at `https://notebox.nbx-7207a015.workers.dev`,
  version `ac8e30df-e03a-45d7-83fa-b0dbeaa56434`, deployed
  `2026-10-21T08:54:30Z` from commit `e9c8e82`.
- It is bound to two D1 databases, `notebox-sync` and `notebox-blobs`, both on
  schema v1. The free plan allows ten databases; this uses two.
- The server does no password hashing, CRDT merging or image work. The device
  derives the login key with Argon2id (19 MiB, t=2, p=1) and sends only the
  result; the server stores `SHA-256(login_key)`.
- Free limits were re-read on 2026-10-21:
  [Workers limits](https://developers.cloudflare.com/workers/platform/limits/)
  and [D1 limits](https://developers.cloudflare.com/d1/platform/limits/).
  The relevant values are 100,000 requests/day and 1,000/minute, 10 ms CPU per
  request, 50 subrequests per request, 50 D1 queries per request, 100 bound
  parameters, a 2 MB row or BLOB limit, ten D1 databases, 5 GB of D1 storage
  per account and 100 Workers. Static assets are free and do not invoke the
  script.
- The plan's 10 ms figure is a per-request CPU budget. Time spent waiting on
  D1 is not counted as CPU time.

## Decision

1. **Keep the Worker on the free plan.** No paid service, no R2, no server-side
   Argon2id, CRDT merge or image processing is added.
2. **Treat 10 ms as the warm-handler budget, not as every invocation.** The
   controlled warm probe stayed at 1-2 ms, while production buckets reached
   41.8 ms with zero error 1102. The peaks are consistent with isolate startup
   and the heavier authenticated paths, but this evidence does not separate
   them. The architecture stays; the result refines the plan's wording, it does
   not change the choice of Worker.
3. **Monitor for error 1102.** If exceeded-CPU invocations become frequent
   under normal use, apply the plan's fallback: the same `notes-server` crate on
   another free host (axum). Do not optimize or move the server preemptively.
4. **Keep Argon2id at 19 MiB, t=2, p=1.** Desktop and browser are far below the
   one-second target. The phone number remains open because no Android runtime
   is available here; it is not recorded as a pass.

## Evidence

### Production traffic

Cloudflare GraphQL `workersInvocationsAdaptive` for script `notebox`, window
`2026-10-14T09:04:30Z` to `2026-10-21T09:04:30Z`:

| Metric | Result |
| --- | --- |
| Invocations | 156 |
| Errors / exceeded-CPU | 0 |
| Status | all `success` |
| Subrequests | 0 |
| CPU bucket p50 | min 0.821 ms, median 4.618 ms, p95 26.291 ms, max 41.825 ms |
| CPU bucket p99, worst | 41.825 ms |
| Buckets above 10 ms | 29 of 132 |
| Requests in those buckets | 31 of 156 |
| Wall bucket p99 | median 127.2 ms, max 720.8 ms |

This window includes the live browser sign-in and first sync and the
welcome-note updates, so it is real login and sync traffic, not only health
checks. The full sanitized response is
`docs/measurements/s3-analytics-production.json`.

The controlled warm 401 path costs 1-2 ms, so the production values above 10 ms
come from isolate startup and/or the heavier authenticated paths; this evidence
does not separate the two. None of them produced `1102` or an `exceededCpu`
outcome. That is an observation about this traffic, not a guarantee of
headroom, and it is not a licence to add server work.

### Controlled probe

`node scripts/s3-probe.mjs --count 100` sends one request per second while
`wrangler tail --format json` records the events:

- 34 `POST /v1/auth/login` attempts for a random username. Each reads the
  account row and returns `401 invalid_credentials` without a write or backoff.
- 33 `GET /v1/devices` and 33 `POST /v1/sync` requests without a token. Both
  return `401 unauthorized` before D1.
- The unique `nbx_probe` query parameter marks every request. The script keeps
  only the path, status, outcome, CPU and wall time; it deletes the raw tail and
  never stores headers or bodies.

| Metric | Result |
| --- | --- |
| Probe requests | 100 |
| Tail events matched | 100 |
| Failed requests | 0 |
| Outcomes | 100 `ok` |
| CPU | min 0 ms, p50 1 ms, p95 2 ms, max 2 ms |
| Error 1102 / 5xx | 0 |
| Expected statuses | login 401 `invalid_credentials`; devices and sync 401 `unauthorized` |

The first calibration runs at five to seven requests per second lost events:
the tail captured 14 of 20 and 28 of 100. At one request per second it captured
100 of 100. The probe therefore paces itself at one request per second. This is
still far above one person's sync rate. The sanitized
result is `docs/measurements/s3-probe.json`.

A seven-day analytics window taken after the probe reports 532 invocations and
zero errors; it includes the probe and its calibration runs.
`docs/measurements/s3-analytics.json` holds that snapshot.

### Client Argon2id

| Platform | Method | Result |
| --- | --- | --- |
| Linux desktop, release | `cargo run -p notes-sync --example argon2_bench --release`, 20 runs after a warm-up | median 38.2 ms, mean 51.7 ms, min 31.4 ms, max 92.1 ms |
| Chromium, release web build | three rejected logins in `scripts/web-wipe-login.mjs`, click to response | 178 / 119 / 116 ms, median 119 ms, including the WASM call and the local Worker round trip |
| Android phone | no runtime available (`/dev/kvm` and SDK missing) | not measured; open |

The one-second S3 target is met on desktop and in the browser. The browser
number includes the network round trip, so it is an upper bound on the
derivation itself.

### Limits and resources

The probe adds 100 requests once. The plan's heavy-day estimate remains about
3,000 requests and under 1,000 rows written per day, far below 100,000 and
5,000,000. The two D1 databases are the only Cloudflare resources this feature
uses; no new Worker, database or paid product was added.

## Consequences

- The free-tier choice stands. The plan's 10 ms line should be read as the warm
  handler budget; startup and the heavier authenticated paths can exceed it,
  and this window still had no `1102`. If `1102` starts to appear, switch to
  the axum fallback rather than adding a paid plan.
- Do not add server-side password hashing, CRDT merging or image work. The
  measured headroom is for occasional startup cost, not new CPU work.
- Keep the tail and analytics check handy after any server change:
  `node scripts/s3-probe.mjs --count 100` and
  `node scripts/s3-analytics.mjs --days 7`.
- Phone Argon2 and Android runtime checks remain blocked, so S3 is recorded as
  accepted on desktop and browser with the phone gap explicit.
- Nothing in this ADR changes the wire protocol, D1 schema or authentication.
