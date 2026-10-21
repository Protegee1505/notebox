# Measurements

Sanitized evidence for the budgets in `PLAN.md` and the S3 free-tier decision in
[ADR-0003](../adr/0003-worker-free-tier.md). These files hold aggregate numbers
only: no account ids, emails, request headers, bodies or note content.

| File | What it records |
| --- | --- |
| `s3-analytics-production.json` | Cloudflare GraphQL `workersInvocationsAdaptive` for `notebox` over the seven days ending 2026-10-21T09:04:30Z, before the probe: 156 invocations, zero errors, CPU bucket percentiles |
| `s3-probe.json` | 100 paced requests to the deployed Worker while `wrangler tail` recorded CPU and outcome: 100/100 events, zero failures, expected 401 codes |
| `s3-analytics.json` | The same analytics query after the probe and its calibration runs; includes that extra traffic, so it is not a pure production snapshot |

Regenerate them with:

```bash
node scripts/s3-probe.mjs --count 100 --json docs/measurements/s3-probe.json
node scripts/s3-analytics.mjs --days 7 --json docs/measurements/s3-analytics.json
```

The probe requires Node 20+, `npx`, `jq` and an authenticated Wrangler session.
It deletes its raw tail after reading the sanitized fields. Pin a production
snapshot with `--until` when a probe must not be included, as was done for
`s3-analytics-production.json`.
