#!/usr/bin/env node
// P8: large-note budget in the release web UI.
//
// Creates a note of at least 100 KB, saves it, measures how long it takes to
// open again, and measures input-to-next-frame latency while typing into it.
// A sample above 34 ms means at least two 60 Hz frames were missed; the run
// fails if any sample does. The note is then saved, reloaded and opened again
// to prove the text survived.
//
//   PLAYWRIGHT_MODULES=/path/to/node_modules \
//   node scripts/web-perf.mjs [dist-dir]

import { createServer } from "node:http";
import { access, readFile } from "node:fs/promises";
import { createRequire } from "node:module";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const dist =
  process.argv[2] ??
  path.join(root, "target/dx/notebox/release/web/public");
const NOTE_BYTES = 100 * 1024;
const OPEN_BUDGET_MS = 50;
const DROPPED_FRAME_MS = 34;
const OPEN_ROUNDS = 5;
const TYPED = "the quick brown fox jumps over the lazy dog and keeps writing ";

const contentTypes = new Map([
  [".html", "text/html; charset=utf-8"],
  [".js", "text/javascript; charset=utf-8"],
  [".mjs", "text/javascript; charset=utf-8"],
  [".wasm", "application/wasm"],
  [".json", "application/json"],
  [".css", "text/css; charset=utf-8"],
  [".png", "image/png"],
  [".webp", "image/webp"],
  [".svg", "image/svg+xml"],
  [".ico", "image/x-icon"],
]);

function serve() {
  const server = createServer(async (request, response) => {
    try {
      const url = new URL(request.url, "http://localhost");
      const relative = decodeURIComponent(
        url.pathname === "/" ? "/index.html" : url.pathname,
      );
      const file = path.resolve(dist, `.${relative}`);
      if (!file.startsWith(dist + path.sep)) {
        response.writeHead(403).end("forbidden");
        return;
      }
      const data = await readFile(file);
      response.writeHead(200, {
        "content-type":
          contentTypes.get(path.extname(file)) ?? "application/octet-stream",
        "cache-control": "no-store",
      });
      response.end(data);
    } catch {
      response.writeHead(404).end("not found");
    }
  });
  return new Promise((resolve) => {
    server.listen(0, "127.0.0.1", () => {
      resolve({ server, base: `http://127.0.0.1:${server.address().port}` });
    });
  });
}

async function moduleDirectory() {
  const candidates = [
    process.env.PLAYWRIGHT_MODULES,
    path.join(root, "node_modules"),
    process.cwd(),
  ].filter(Boolean);
  for (const candidate of candidates) {
    try {
      await access(path.join(candidate, "playwright"));
      return candidate;
    } catch {
      // Try the next candidate.
    }
  }
  throw new Error("playwright is not installed; set PLAYWRIGHT_MODULES");
}

async function loadPlaywright(modules) {
  const require = createRequire(import.meta.url);
  const entry = require.resolve("playwright", { paths: [modules] });
  const loaded = await import(pathToFileURL(entry).href);
  return loaded.default ?? loaded;
}

function check(name, ok, detail = "") {
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${detail ? ` — ${detail}` : ""}`);
  return { name, ok: Boolean(ok), detail };
}

function percentile(samples, fraction) {
  if (samples.length === 0) return 0;
  const sorted = [...samples].sort((a, b) => a - b);
  const index = Math.min(
    sorted.length - 1,
    Math.ceil(sorted.length * fraction) - 1,
  );
  return sorted[index];
}

const results = [];
const errors = [];
let browser;
let context;
let server;
try {
  await access(path.join(dist, "index.html"));
  const modules = await moduleDirectory();
  const playwright = await loadPlaywright(modules);
  const started = await serve();
  server = started.server;

  browser = await playwright.chromium.launch({ headless: true });
  context = await browser.newContext({
    viewport: { width: 1400, height: 900 },
  });
  const page = await context.newPage();
  page.on("pageerror", (error) => errors.push(String(error)));
  page.on("console", (message) => {
    if (message.type() === "error" && !message.text().includes("_dioxus")) {
      errors.push(`console: ${message.text()}`);
    }
  });

  const navigationStart = Date.now();
  await page.goto(started.base, { waitUntil: "networkidle" });
  const offline = page.getByRole("button", { name: /Try it offline/ });
  if (await offline.isVisible().catch(() => false)) {
    await offline.click();
  }
  await page.getByRole("button", { name: "Add welcome tour" }).click();
  await page.locator("#note-editor").waitFor({ timeout: 60_000 });
  const webColdStart = Date.now() - navigationStart;
  await page.waitForTimeout(500);

  // Build a note just over 100 KB: a title line, then numbered prose. The
  // first line becomes the card title.
  const header = "Large note benchmark\n\n";
  const body = [];
  let size = Buffer.byteLength(header, "utf8");
  let line = 1;
  while (size < NOTE_BYTES) {
    const row = `${line}. The quick brown fox jumps over the lazy dog. 0123456789 abcdefghijklmnopqrstuvwxyz.\n`;
    body.push(row);
    size += Buffer.byteLength(row, "utf8");
    line += 1;
  }
  const text = header + body.join("");

  await page.locator(".new-note-button").click();
  await page.locator("#note-editor").waitFor();
  await page.locator("#note-editor").fill(text);
  await page.keyboard.press("Control+s");
  await page.waitForFunction(
    () =>
      !document.querySelector(".save-state")?.textContent?.includes("Unsaved"),
    null,
    { timeout: 30_000 },
  );
  await page.waitForTimeout(400);
  const editorLength = await page
    .locator("#note-editor")
    .evaluate((element) => element.value.length);
  results.push(
    check(
      "the editor holds the 100 KB note",
      editorLength === text.length,
      `${editorLength} characters, ${size} bytes`,
    ),
  );

  const card = () =>
    page
      .locator(".note-card", { hasText: "Large note benchmark" })
      .first();

  // Leave the note, then measure the click-to-populated time. Calling click()
  // in the page avoids Playwright actionability time in the measurement.
  const openTimes = [];
  for (let round = 0; round < OPEN_ROUNDS; round += 1) {
    await page
      .locator(".note-card", { hasText: "Welcome to Notebox" })
      .first()
      .click();
    await page.waitForFunction(
      () =>
        document
          .getElementById("note-editor")
          ?.value?.startsWith("Welcome to Notebox"),
      null,
      { polling: "raf" },
    );
    await page.evaluate(() => {
      const target = [...document.querySelectorAll(".note-card")].find(
        (element) => element.textContent.includes("Large note benchmark"),
      );
      window.__noteboxOpenStart = performance.now();
      target.click();
    });
    await page.waitForFunction(
      (length) => {
        const editor = document.getElementById("note-editor");
        if (editor && editor.value.length === length) {
          window.__noteboxOpenEnd = performance.now();
          return true;
        }
        return false;
      },
      text.length,
      { polling: "raf", timeout: 30_000 },
    );
    const elapsed = await page.evaluate(
      () => window.__noteboxOpenEnd - window.__noteboxOpenStart,
    );
    openTimes.push(elapsed);
  }
  const medianOpen = percentile(openTimes, 0.5);
  results.push(
    check(
      `opening the 100 KB note is under ${OPEN_BUDGET_MS} ms`,
      medianOpen < OPEN_BUDGET_MS,
      `median ${medianOpen.toFixed(1)} ms; ${openTimes.map((value) => value.toFixed(1)).join(", ")} ms`,
    ),
  );

  // Type at a human pace and record how long each input waits for its next
  // frame. Headless Chromium schedules frames at about 60 Hz.
  await page.locator("#note-editor").evaluate((element) => {
    element.focus();
    element.setSelectionRange(element.value.length, element.value.length);
    window.__noteboxFrames = [];
    window.__noteboxInput = () => {
      const start = performance.now();
      requestAnimationFrame(() => {
        window.__noteboxFrames.push(performance.now() - start);
      });
    };
    element.addEventListener("input", window.__noteboxInput);
  });
  const payload = TYPED.repeat(2).slice(0, 120);
  await page.keyboard.type(payload, { delay: 8 });
  await page.waitForTimeout(300);
  const samples = await page.evaluate(() => {
    const editor = document.getElementById("note-editor");
    if (editor && window.__noteboxInput) {
      editor.removeEventListener("input", window.__noteboxInput);
    }
    return window.__noteboxFrames ?? [];
  });
  const dropped = samples.filter((value) => value > DROPPED_FRAME_MS).length;
  const p95 = percentile(samples, 0.95);
  const max = samples.length > 0 ? Math.max(...samples) : 0;
  results.push(
    check(
      "typing in the 100 KB note drops no frames",
      samples.length > 0 && dropped === 0,
      `${samples.length} inputs, p95 ${p95.toFixed(1)} ms, max ${max.toFixed(1)} ms, ${dropped} over ${DROPPED_FRAME_MS} ms`,
    ),
  );
  const typedLength = await page
    .locator("#note-editor")
    .evaluate((element) => element.value.length);
  results.push(
    check(
      "every typed character reached the editor",
      typedLength === text.length + payload.length,
      `${typedLength} characters, expected ${text.length + payload.length}`,
    ),
  );

  // Save, reload and open the note again to prove the large text survives.
  await page.keyboard.press("Control+s");
  await page.waitForFunction(
    () =>
      !document.querySelector(".save-state")?.textContent?.includes("Unsaved"),
    null,
    { timeout: 30_000 },
  );
  // The OPFS image is written on a 700 ms tick. Give the worker time to finish
  // the larger image before navigating; pagehide is the safety net, not the
  // normal path.
  await page.waitForTimeout(2_000);
  await page.reload({ waitUntil: "networkidle" });
  await page.locator("#note-editor").waitFor({ timeout: 60_000 });
  await page.waitForTimeout(500);
  await card().click();
  await page.waitForFunction(
    (length) => {
      const editor = document.getElementById("note-editor");
      return editor && editor.value.length === length;
    },
    text.length + payload.length,
    { polling: "raf", timeout: 30_000 },
  );
  results.push(
    check(
      "the 100 KB note survives a reload",
      true,
      `web cold start ${webColdStart} ms`,
    ),
  );
  results.push(
    check(
      "web cold start is recorded",
      webColdStart > 0,
      `${webColdStart} ms to an editable note (desktop budget is separate)`,
    ),
  );
} catch (error) {
  results.push(check("large-note run completed", false, String(error)));
} finally {
  results.push(
    check("no console errors", errors.length === 0, errors.slice(0, 5).join(" | ")),
  );
  if (context) await context.close().catch(() => {});
  if (browser) await browser.close().catch(() => {});
  if (server) server.close();
  const summary = {
    openMs: results.find((result) => result.name.startsWith("opening"))?.detail,
    typing: results.find((result) => result.name.startsWith("typing"))?.detail,
    coldStart: results.find((result) => result.name.includes("cold start"))?.detail,
    results,
    errors,
  };
  console.log(JSON.stringify(summary, null, 2));
  process.exit(results.every((result) => result.ok) ? 0 : 1);
}
