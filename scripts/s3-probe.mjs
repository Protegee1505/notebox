#!/usr/bin/env node
// S3: send harmless requests to the deployed Worker while `wrangler tail`
// records the Worker's CPU time and outcome.
//
// The probe uses no account token and writes no D1 rows:
//
//   * 34 login attempts for a random username (one D1 read each, no backoff),
//   * 33 device-list requests without a token (401 before D1),
//   * 33 sync requests without a token (401 before D1).
//
// A unique `nbx_probe` query parameter marks every request, so the tail can be
// filtered without keeping any of the owner's traffic. The raw tail is written
// to a mode-0600 temporary file and deleted after the summary is read; headers
// and bodies are never written to the summary.
//
// Usage:
//   node scripts/s3-probe.mjs [--url URL] [--worker notebox] [--count 100]
//                             [--json docs/measurements/s3-probe.json]
//
// Requires Node 20+, `npx`, `jq` and an authenticated Wrangler session.

import { spawn, execFile } from "node:child_process";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { randomBytes } from "node:crypto";
import { tmpdir } from "node:os";
import path from "node:path";
import { promisify } from "node:util";

const runFile = promisify(execFile);
const WRANGLER = ["--yes", "wrangler@4.151.2"];

const args = process.argv.slice(2);
const value = (name, fallback) => {
  const index = args.indexOf(name);
  return index >= 0 && args[index + 1] ? args[index + 1] : fallback;
};

const base = (
  value("--url", process.env.NOTEBOX_WORKER_URL || "https://notebox.nbx-7207a015.workers.dev")
).replace(/\/+$/, "");
const worker = value("--worker", "notebox");
const count = Number(value("--count", "100"));
const interval = Number(value("--interval", "1000"));
const jsonPath = value("--json", "");
const run = randomBytes(6).toString("hex");
const loginKey = randomBytes(32).toString("base64");
const username = `probe-${run}@example.invalid`;

if (!Number.isInteger(count) || count < 1 || count > 200) {
  throw new Error("--count must be an integer between 1 and 200");
}

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const probeUrl = (kind, index) =>
  `${base}${kind}?nbx_probe=${run}-${index}`;

let tail;
let rawDir;
let rawPath;
let raw = "";

async function stopTail() {
  if (!tail || tail.exitCode !== null) return;
  tail.kill("SIGINT");
  await Promise.race([
    new Promise((resolve) => tail.once("exit", resolve)),
    sleep(5_000).then(() => tail.kill("SIGTERM")),
  ]);
  await Promise.race([
    new Promise((resolve) => tail.once("exit", resolve)),
    sleep(5_000),
  ]);
}

async function waitForTail() {
  for (let attempt = 0; attempt < 20; attempt += 1) {
    if (tail.exitCode !== null) {
      throw new Error(`wrangler tail exited with ${tail.exitCode}`);
    }
    const marker = `${run}-warmup-${attempt}`;
    await fetch(`${base}/v1/devices?nbx_probe=${marker}`, {
      signal: AbortSignal.timeout(10_000),
    }).catch(() => {});
    await sleep(1_000);
    if (raw.includes(`nbx_probe=${marker}`)) return;
  }
  throw new Error("wrangler tail did not deliver the warm-up request");
}

async function one(kind, index) {
  const url = probeUrl(kind, index);
  const started = performance.now();
  let status = 0;
  let code = "";
  try {
    const response =
      kind === "/v1/devices"
        ? await fetch(url, { signal: AbortSignal.timeout(10_000) })
        : await fetch(url, {
            method: "POST",
            headers: { "content-type": "application/json" },
            signal: AbortSignal.timeout(10_000),
            body:
              kind === "/v1/auth/login"
                ? JSON.stringify({
                    version: 1,
                    payload: {
                      username,
                      login_key: loginKey,
                      device_name: "S3 probe",
                    },
                  })
                : JSON.stringify({ version: 1, payload: { cursor: 0, updates: [] } }),
          });
    status = response.status;
    const body = await response.json().catch(() => null);
    code = body?.payload?.code || "";
  } catch (error) {
    code = `network: ${error.message}`;
  }
  return {
    kind,
    index,
    status,
    code,
    client_ms: Math.round(performance.now() - started),
  };
}

async function main() {
  rawDir = await mkdtemp(path.join(tmpdir(), "notebox-s3-probe-"));
  rawPath = path.join(rawDir, "tail.json");

  tail = spawn(
    "npx",
    [...WRANGLER, "tail", worker, "--format", "json"],
    {
      env: { ...process.env, NO_COLOR: "1" },
      stdio: ["ignore", "pipe", "pipe"],
    },
  );
  tail.stdout.on("data", (chunk) => {
    raw += chunk.toString();
  });
  let stderr = "";
  tail.stderr.on("data", (chunk) => {
    stderr += chunk.toString();
  });

  const warmupStart = performance.now();
  await waitForTail();
  const warmupMs = Math.round(performance.now() - warmupStart);

  const kinds = ["/v1/auth/login", "/v1/devices", "/v1/sync"];
  const results = [];
  for (let index = 0; index < count; index += 1) {
    const kind = kinds[index % kinds.length];
    results.push(await one(kind, index));
    await sleep(interval);
    process.stdout.write(
      `\r${index + 1}/${count} ${kind} ${results.at(-1).status} ${results.at(-1).code}`.padEnd(80),
    );
  }
  process.stdout.write("\n");

  // Tail buffers events briefly; give it time to flush before closing.
  await sleep(8_000);
  await stopTail();
  await writeFile(rawPath, raw, { mode: 0o600 });

  const filter = `.[] | select((.event.request.url // "") | contains("nbx_probe=${run}-")) | {
    kind: ((.event.request.url // "") | split("?")[0]),
    query: ((.event.request.url // "") | split("?")[1] // ""),
    status: (.event.response.status // 0),
    outcome: (.outcome // ""),
    cpu_ms: (.cpuTime // 0),
    wall_ms: (.wallTime // 0),
    exceptions: ((.exceptions // []) | length),
    logs: ((.logs // []) | length),
    truncated: (.truncated // false)
  }`;

  const { stdout } = await runFile("jq", ["-s", "-c", filter, rawPath], {
    maxBuffer: 64 * 1024 * 1024,
  });
  const events = stdout
    .trim()
    .split("\n")
    .filter(Boolean)
    .map((line) => JSON.parse(line))
    .filter((event) => event.query.startsWith(`nbx_probe=${run}-`));

  const measured = events.filter((event) => {
    const marker = event.query.slice(`nbx_probe=${run}-`.length);
    return /^\d+$/.test(marker);
  });
  const byIndex = new Map();
  for (const event of measured) {
    const index = Number(event.query.slice(`nbx_probe=${run}-`.length));
    // A request can appear twice when a retry is logged; keep the last event.
    byIndex.set(index, event);
  }

  const rows = results.map((result) => {
    const event = byIndex.get(result.index) || {};
    return {
      ...result,
      cpu_ms: event.cpu_ms ?? null,
      wall_ms: event.wall_ms ?? null,
      outcome: event.outcome ?? "missing",
    };
  });

  const missing = rows.filter((row) => row.cpu_ms === null).length;
  const failures = rows.filter(
    (row) =>
      row.status !== 401 ||
      row.code !== (row.kind === "/v1/auth/login" ? "invalid_credentials" : "unauthorized") ||
      row.outcome !== "ok" ||
      row.cpu_ms === null,
  );
  const cpu = rows
    .map((row) => row.cpu_ms)
    .filter((value) => typeof value === "number")
    .sort((a, b) => a - b);
  const pick = (fraction) => cpu[Math.min(cpu.length - 1, Math.floor(cpu.length * fraction))] ?? null;
  const summary = {
    generated_at: new Date().toISOString(),
    worker,
    url: base,
    requests: rows.length,
    warmup_ms: warmupMs,
    missing_tail_events: missing,
    failures: failures.length,
    outcomes: Object.fromEntries(
      [...new Set(rows.map((row) => row.outcome))].sort().map((outcome) => [
        outcome,
        rows.filter((row) => row.outcome === outcome).length,
      ]),
    ),
    cpu_ms: {
      min: cpu[0] ?? null,
      p50: pick(0.5),
      p95: pick(0.95),
      max: cpu.at(-1) ?? null,
    },
    wall_ms: {
      max: Math.max(...rows.map((row) => row.wall_ms ?? 0)),
    },
    by_kind: Object.fromEntries(
      kinds.map((kind) => {
        const kindRows = rows.filter((row) => row.kind === kind);
        return [
          kind,
          {
            requests: kindRows.length,
            statuses: [...new Set(kindRows.map((row) => row.status))].sort(),
            codes: [...new Set(kindRows.map((row) => row.code))].sort(),
          },
        ];
      }),
    ),
  };

  console.log(JSON.stringify(summary, null, 2));
  if (jsonPath) {
    await writeFile(jsonPath, `${JSON.stringify(summary, null, 2)}\n`, { mode: 0o644 });
    console.log(`\nWrote ${jsonPath}`);
  }
  if (failures.length > 0) {
    console.error(`\n${failures.length} probe request(s) failed:`);
    for (const row of failures.slice(0, 5)) console.error(JSON.stringify(row));
    process.exitCode = 1;
  }
  if (stderr.trim()) {
    console.error(`\nwrangler tail stderr:\n${stderr.trim()}`);
  }
}

try {
  await main();
} finally {
  await stopTail().catch(() => {});
  if (rawDir) await rm(rawDir, { recursive: true, force: true }).catch(() => {});
}
