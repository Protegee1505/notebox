#!/usr/bin/env node
// S3: summarise a deployed Worker's invocation CPU from Cloudflare analytics.
//
// The Wrangler OAuth session supplies the API token; it is never printed.
// The output contains no account id, email, request paths or headers.
//
// Usage:
//   node scripts/s3-analytics.mjs [--script notebox] [--days 7]
//                                 [--since ISO] [--until ISO]
//                                 [--json docs/measurements/s3-analytics.json]

import { execFile } from "node:child_process";
import { writeFile } from "node:fs/promises";
import { promisify } from "node:util";

const run = promisify(execFile);
const args = process.argv.slice(2);
const value = (name, fallback) => {
  const index = args.indexOf(name);
  return index >= 0 && args[index + 1] ? args[index + 1] : fallback;
};

const scriptName = value("--script", "notebox");
const days = Number(value("--days", "7"));
const jsonPath = value("--json", "");
const sinceArg = value("--since", "");
const untilArg = value("--until", "");

if (!Number.isInteger(days) || days < 1 || days > 30) {
  throw new Error("--days must be an integer between 1 and 30");
}

const cut = (date) => new Date(Math.floor(date.getTime() / 1000) * 1000).toISOString();
const until = untilArg ? new Date(untilArg) : new Date();
if (Number.isNaN(until.getTime())) throw new Error("--until is not a valid date");
const since = sinceArg
  ? new Date(sinceArg)
  : new Date(until.getTime() - days * 24 * 60 * 60 * 1000);
if (Number.isNaN(since.getTime())) throw new Error("--since is not a valid date");
if (since >= until) throw new Error("--since must be before --until");

const { stdout: authOut } = await run("npx", [
  "--yes",
  "wrangler@4.151.2",
  "auth",
  "token",
  "--json",
]);
const token = JSON.parse(authOut).token;
if (!token) throw new Error("wrangler did not return a token");

const headers = {
  authorization: `Bearer ${token}`,
  "content-type": "application/json",
};

const accountsResponse = await fetch(
  "https://api.cloudflare.com/client/v4/accounts",
  { headers },
);
if (!accountsResponse.ok) {
  throw new Error(`accounts request failed with ${accountsResponse.status}`);
}
const accounts = await accountsResponse.json();
const account = accounts.result?.[0];
if (!account?.id) throw new Error("no Cloudflare account is visible to this session");

const query = `query($account: String!, $script: String!, $since: Time!, $until: Time!) {
  viewer {
    accounts(filter: { accountTag: $account }) {
      workersInvocationsAdaptive(
        limit: 10000
        filter: { scriptName: $script, datetime_geq: $since, datetime_leq: $until }
      ) {
        sum { requests errors subrequests }
        quantiles { cpuTimeP50 cpuTimeP99 wallTimeP50 wallTimeP99 }
        dimensions { datetime status }
      }
    }
  }
}`;

const graphResponse = await fetch("https://api.cloudflare.com/client/v4/graphql", {
  method: "POST",
  headers,
  body: JSON.stringify({
    query,
    variables: {
      account: account.id,
      script: scriptName,
      since: cut(since),
      until: cut(until),
    },
  }),
});
if (!graphResponse.ok) {
  throw new Error(`analytics request failed with ${graphResponse.status}`);
}
const graph = await graphResponse.json();
if (graph.errors?.length) {
  throw new Error(`analytics query failed: ${graph.errors.map((e) => e.message).join("; ")}`);
}
const rows = graph.data?.viewer?.accounts?.[0]?.workersInvocationsAdaptive || [];
if (rows.length === 0) {
  throw new Error(`no invocations for ${scriptName} in the last ${days} days`);
}

const requests = rows.reduce((sum, row) => sum + row.sum.requests, 0);
const errors = rows.reduce((sum, row) => sum + row.sum.errors, 0);
const statuses = [...new Set(rows.map((row) => row.dimensions.status))].sort().map((status) => ({
  status,
  requests: rows
    .filter((row) => row.dimensions.status === status)
    .reduce((sum, row) => sum + row.sum.requests, 0),
  errors: rows
    .filter((row) => row.dimensions.status === status)
    .reduce((sum, row) => sum + row.sum.errors, 0),
}));
const sorted = (field) =>
  rows.map((row) => row.quantiles[field]).filter((v) => typeof v === "number").sort((a, b) => a - b);
const at = (values, fraction) =>
  values[Math.min(values.length - 1, Math.floor(values.length * fraction))] ?? null;
const bucketsOver10ms = rows.filter((row) => row.quantiles.cpuTimeP50 > 10_000).length;
const requestsOver10ms = rows
  .filter((row) => row.quantiles.cpuTimeP50 > 10_000)
  .reduce((sum, row) => sum + row.sum.requests, 0);
const cpuP50 = sorted("cpuTimeP50");
const cpuP99 = sorted("cpuTimeP99");
const wallP99 = sorted("wallTimeP99");

const summary = {
  generated_at: new Date().toISOString(),
  script: scriptName,
  window: { since: cut(since), until: cut(until) },
  requests,
  errors,
  subrequests: rows.reduce((sum, row) => sum + row.sum.subrequests, 0),
  buckets: rows.length,
  statuses,
  cpu_us: {
    bucket_p50_min: cpuP50[0] ?? null,
    bucket_p50_median: at(cpuP50, 0.5),
    bucket_p50_p95: at(cpuP50, 0.95),
    bucket_p50_max: cpuP50.at(-1) ?? null,
    bucket_p99_max: cpuP99.at(-1) ?? null,
  },
  wall_us: {
    bucket_p99_min: wallP99[0] ?? null,
    bucket_p99_median: at(wallP99, 0.5),
    bucket_p99_max: wallP99.at(-1) ?? null,
  },
  buckets_over_10ms: bucketsOver10ms,
  requests_in_buckets_over_10ms: requestsOver10ms,
};

console.log(JSON.stringify(summary, null, 2));
if (jsonPath) {
  await writeFile(jsonPath, `${JSON.stringify(summary, null, 2)}\n`, { mode: 0o644 });
  console.log(`\nWrote ${jsonPath}`);
}
