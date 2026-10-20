#!/usr/bin/env node
// S4: prove that sqlite-wasm over the OPFS SAH-pool VFS keeps 1,000 notes
// across a page reload in Chromium, Firefox and WebKit.
//
// Builds the wasm example, runs wasm-bindgen, serves `crates/notes-store/web`
// and drives one browser at a time. The first load resets and seeds the
// database; the second load reloads the page and must read the same rows.
//
//   WASM_BINDGEN=/path/to/wasm-bindgen \
//   PLAYWRIGHT_MODULES=/path/to/node_modules \
//   node scripts/opfs-spike.mjs [--browser chromium,firefox,webkit]

import { execFileSync } from "node:child_process";
import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { mkdtemp, rm } from "node:fs/promises";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const webRoot = path.join(root, "crates/notes-store/web");
const targetDir = process.env.CARGO_TARGET_DIR
  ? path.resolve(process.env.CARGO_TARGET_DIR)
  : path.join(root, "target");

const args = process.argv.slice(2);
const browsersArg = valueOf("--browser") ?? "chromium,firefox,webkit";
const browsers = browsersArg.split(",").map((name) => name.trim()).filter(Boolean);
const skipBuild = args.includes("--skip-build");

function valueOf(flag) {
  const index = args.indexOf(flag);
  return index >= 0 ? args[index + 1] : undefined;
}

function run(command, commandArgs, extraEnv = {}) {
  console.log(`$ ${command} ${commandArgs.join(" ")}`);
  execFileSync(command, commandArgs, {
    cwd: root,
    stdio: "inherit",
    env: { ...process.env, ...extraEnv },
  });
}

function build() {
  if (skipBuild) return;

  const wasmBindgen = process.env.WASM_BINDGEN;
  if (!wasmBindgen) {
    throw new Error(
      "Set WASM_BINDGEN to the wasm-bindgen binary that matches Cargo.lock " +
        "(for example /tmp/opencode/wasm-bindgen/wasm-bindgen), or pass --skip-build."
    );
  }

  run("cargo", [
    "build",
    "-p",
    "notes-store",
    "--target",
    "wasm32-unknown-unknown",
    "--example",
    "opfs_spike",
  ]);
  run(wasmBindgen, [
    "--target",
    "web",
    "--out-dir",
    path.join(webRoot, "pkg"),
    "--out-name",
    "opfs_spike",
    path.join(targetDir, "wasm32-unknown-unknown/debug/examples/opfs_spike.wasm"),
  ]);
}

const contentTypes = new Map([
  [".html", "text/html; charset=utf-8"],
  [".js", "text/javascript; charset=utf-8"],
  [".mjs", "text/javascript; charset=utf-8"],
  [".wasm", "application/wasm"],
  [".json", "application/json"],
  [".css", "text/css; charset=utf-8"],
  [".png", "image/png"],
]);

function serve() {
  const server = createServer(async (request, response) => {
    try {
      const url = new URL(request.url, "http://localhost");
      const relative = decodeURIComponent(url.pathname === "/" ? "/index.html" : url.pathname);
      const file = path.resolve(webRoot, `.${relative}`);
      if (!file.startsWith(webRoot + path.sep)) {
        response.writeHead(403).end("forbidden");
        return;
      }
      const body = await readFile(file);
      response.writeHead(200, {
        "content-type": contentTypes.get(path.extname(file)) ?? "application/octet-stream",
        "cache-control": "no-store",
      });
      response.end(body);
    } catch (error) {
      response.writeHead(404).end(String(error));
    }
  });
  return new Promise((resolve) => {
    server.listen(0, "127.0.0.1", () => {
      resolve({ server, port: server.address().port });
    });
  });
}

async function loadPlaywright() {
  const require = createRequire(import.meta.url);
  const searchPaths = [
    process.env.PLAYWRIGHT_MODULES,
    path.join(root, "node_modules"),
    process.cwd(),
  ].filter(Boolean);
  const entry = require.resolve("playwright", { paths: searchPaths });
  const loaded = await import(pathToFileURL(entry).href);
  return loaded.default ?? loaded;
}

function assert(condition, message) {
  if (!condition) throw new Error(message);
}

async function readResult(page) {
  await page.waitForFunction(() => window.__s4 !== undefined, null, {
    timeout: 180_000,
  });
  return page.evaluate(() => window.__s4);
}

async function checkBrowser(name, browserType, url) {
  // A persistent context matches a real browser profile. WebKit (and Safari's
  // private mode) refuses OPFS handles in an ephemeral context.
  const profile = await mkdtemp(path.join(tmpdir(), `notebox-s4-${name}-`));
  const context = await browserType.launchPersistentContext(profile, { headless: true });
  const page = context.pages()[0] ?? (await context.newPage());
  const consoleErrors = [];
  page.on("pageerror", (error) => consoleErrors.push(String(error)));
  page.on("console", (message) => {
    if (message.type() === "error") consoleErrors.push(`console: ${message.text()}`);
  });

  try {
    await page.goto(`${url}/?reset=1`, { waitUntil: "load" });
    const first = await readResult(page);
    assert(first.ok, `${name}: first load failed: ${first.error}`);
    assert(first.seeded === true, `${name}: first load did not seed`);
    assert(first.count === 1000, `${name}: seeded ${first.count} notes, expected 1000`);

    await page.reload({ waitUntil: "load" });
    const second = await readResult(page);
    assert(second.ok, `${name}: reload failed: ${second.error}`);
    assert(second.seeded === false, `${name}: reload re-seeded the database`);
    assert(second.count === 1000, `${name}: reload saw ${second.count} notes, expected 1000`);
    assert(
      second.totalChars === first.totalChars,
      `${name}: text length changed across reload (${first.totalChars} -> ${second.totalChars})`
    );
    assert(second.first === first.first, `${name}: first note changed across reload`);
    assert(second.last === first.last, `${name}: last note changed across reload`);
    assert(consoleErrors.length === 0, `${name}: console errors: ${consoleErrors.join("; ")}`);

    return {
      browser: name,
      ok: true,
      seedMs: first.elapsedMs,
      reloadMs: second.elapsedMs,
      count: second.count,
      totalChars: second.totalChars,
      bytes: second.bytes,
      pageCount: second.pageCount,
      pageSize: second.pageSize,
      journalMode: second.journalMode,
      first: first.first,
      last: second.last,
      errors: consoleErrors,
    };
  } finally {
    await context.close();
    await rm(profile, { recursive: true, force: true });
  }
}

build();
const { server, port } = await serve();
const url = `http://127.0.0.1:${port}`;
const playwright = await loadPlaywright();
const types = { chromium: playwright.chromium, firefox: playwright.firefox, webkit: playwright.webkit };

const results = [];
try {
  for (const name of browsers) {
    const browserType = types[name];
    if (!browserType) throw new Error(`unknown browser ${name}`);
    try {
      const result = await checkBrowser(name, browserType, url);
      results.push(result);
      console.log(
        `${name}: PASS count=${result.count} seed=${result.seedMs}ms ` +
          `reload=${result.reloadMs}ms db=${result.bytes}B journal=${result.journalMode}`
      );
    } catch (error) {
      results.push({ browser: name, ok: false, error: String(error.message ?? error) });
      console.error(`${name}: FAIL ${error.message ?? error}`);
    }
  }
} finally {
  server.close();
}

console.log(JSON.stringify({ results }, null, 2));
process.exit(results.every((result) => result.ok) ? 0 : 1);
