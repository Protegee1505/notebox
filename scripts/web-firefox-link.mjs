#!/usr/bin/env node
// Firefox regression: opening a link from Write mode must not stop the app.
//
// Firefox runs queued microtasks inside window.open. A pending Dioxus render
// then re-enters the event dispatch while Dioxus already borrows its element
// arena, which panics. This drives the real release build and checks
// Ctrl+click, Ctrl+Enter and the "Open link" chip.
//
//   PLAYWRIGHT_MODULES=/path/to/node_modules \
//   node scripts/web-firefox-link.mjs [dist-dir]

import { execFileSync } from "node:child_process";
import { createServer } from "node:http";
import { access, mkdtemp, readFile, rm } from "node:fs/promises";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

function targetDirectory() {
  if (process.env.CARGO_TARGET_DIR) {
    return path.resolve(process.env.CARGO_TARGET_DIR);
  }
  try {
    const metadata = JSON.parse(
      execFileSync(
        "cargo",
        ["metadata", "--no-deps", "--locked", "--format-version", "1"],
        { cwd: root, encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] },
      ),
    );
    if (typeof metadata.target_directory === "string") {
      return metadata.target_directory;
    }
  } catch {
    // Fall through to the conventional path.
  }
  return path.join(root, "target");
}

const dist =
  process.argv[2] ??
  path.join(targetDirectory(), "dx/notebox/release/web/public");
const title = `Firefox link ${Date.now()}`;

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
      const body = await readFile(file);
      response.writeHead(200, {
        "content-type":
          contentTypes.get(path.extname(file)) ?? "application/octet-stream",
        "cache-control": "no-store",
      });
      response.end(body);
    } catch (error) {
      response.writeHead(404).end(String(error));
    }
  });
  return new Promise((resolve) => {
    server.listen(0, "127.0.0.1", () => {
      resolve({ server, base: `http://127.0.0.1:${server.address().port}` });
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

function check(name, ok, detail = "") {
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${detail ? ` — ${detail}` : ""}`);
  return { name, ok: Boolean(ok), detail };
}

async function placeCaret(page, needle, offset = 2) {
  await page.locator("#note-editor").focus();
  await page.evaluate(
    ({ needle, offset }) => {
      const editor = document.getElementById("note-editor");
      const at = editor.value.indexOf(needle);
      if (at < 0) throw new Error(`not found: ${needle}`);
      editor.setSelectionRange(at + offset, at + offset);
      editor.dispatchEvent(new Event("select", { bubbles: true }));
    },
    { needle, offset },
  );
}

async function waitForChip(page, text) {
  await page.locator("#token-open").waitFor({ state: "visible", timeout: 10_000 });
  await page.waitForFunction(
    (text) => document.getElementById("token-open")?.textContent?.includes(text),
    text,
    { timeout: 10_000 },
  );
}

const results = [];
const errors = [];
const profile = await mkdtemp(path.join(tmpdir(), "notebox-firefox-link-"));
let server;
let context;

try {
  await access(path.join(dist, "index.html"));
  const started = await serve();
  server = started.server;
  const base = started.base;
  const target = `${base}/external-target`;
  const playwright = await loadPlaywright();

  context = await playwright.firefox.launchPersistentContext(profile, {
    headless: true,
    colorScheme: "light",
    viewport: { width: 1400, height: 900 },
  });
  const page = context.pages()[0] ?? (await context.newPage());
  const pageErrors = [];
  page.on("pageerror", (error) => pageErrors.push(String(error)));

  await page.goto(base, { waitUntil: "networkidle" });
  const offline = page.getByRole("button", { name: /Try it offline/ });
  if (await offline.isVisible().catch(() => false)) {
    await offline.click();
  }
  await page.locator(".workspace").waitFor({ state: "visible", timeout: 60_000 });

  await page.locator(".new-note-button").click();
  await page.locator("#note-editor").waitFor({ timeout: 15_000 });
  await page
    .locator("#note-editor")
    .fill(`${title}\n\n[site](${target}) and https://example.org/page`);
  await page
    .locator(".editor-highlights-inner")
    .waitFor({ state: "visible", timeout: 15_000 });

  async function openFrom(action, label) {
    const popupPromise = context.waitForEvent("page", { timeout: 10_000 });
    await action();
    let popup;
    try {
      popup = await popupPromise;
    } catch {
      results.push(check(label, false, "no new tab"));
      return;
    }
    await popup.waitForLoadState("domcontentloaded").catch(() => {});
    const url = popup.url();
    // The crash panel can appear a tick after the tap.
    await page.waitForTimeout(1_000);
    const crash = await page
      .locator("#notebox-crash")
      .isVisible()
      .catch(() => false);
    results.push(
      check(label, url.includes("/external-target") && !crash, `${url} crash=${crash}`),
    );
    await popup.close();
  }

  const linkBox = await page.locator(".md-link-token").first().boundingBox();
  await openFrom(async () => {
    await page.keyboard.down("Control");
    await page.mouse.click(linkBox.x + 4, linkBox.y + linkBox.height / 2);
    await page.keyboard.up("Control");
  }, "Ctrl+click opens a link without stopping the app");

  await placeCaret(page, "[site]", 2);
  await openFrom(
    () => page.keyboard.press("Control+Enter"),
    "Ctrl+Enter opens a link without stopping the app",
  );

  await placeCaret(page, "[site]", 2);
  await waitForChip(page, "Open link");
  await openFrom(
    () => page.locator("#token-open").click(),
    "the Open link chip opens a link without stopping the app",
  );

  results.push(
    check(
      "Firefox reported no runtime error",
      pageErrors.length === 0,
      pageErrors.join(" | "),
    ),
  );
} catch (error) {
  errors.push(String(error));
} finally {
  if (context) await context.close().catch(() => {});
  if (server) server.close();
  await rm(profile, { recursive: true, force: true });
}

console.log(JSON.stringify({ results, errors }, null, 2));
process.exit(
  errors.length === 0 && results.length > 0 && results.every((result) => result.ok)
    ? 0
    : 1,
);
