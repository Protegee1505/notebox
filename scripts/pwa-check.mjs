#!/usr/bin/env node
// P7: prove the web build installs, works offline and updates deliberately.
//
// Serves a copy of the release output (so the checked artifact is not changed
// by the simulated second build), then drives Chromium through:
//   - manifest, icons and service-worker installability
//   - a local-only note, an offline reload and an offline edit
//   - a second build waiting for the update banner, then activation on Reload
//
//   PLAYWRIGHT_MODULES=/path/to/node_modules \
//   node scripts/pwa-check.mjs [dist-dir]
//
// Set NBX_PWA_SCREENSHOTS=1 to refresh docs/screenshots/p7-pwa-*.

import { execFileSync } from "node:child_process";
import { createServer } from "node:http";
import { access, cp, copyFile, mkdir, mkdtemp, readFile, rm } from "node:fs/promises";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const distArgument = process.argv[2];
const screenshotDirectory = path.join(root, "docs/screenshots");
const wantScreenshots = process.env.NBX_PWA_SCREENSHOTS === "1";

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
    // Fall through to the conventional path when cargo is unavailable.
  }
  return path.join(root, "target");
}

const dist = path.resolve(
  distArgument ??
    path.join(targetDirectory(), "dx/notebox/release/web/public"),
);
const marker = `PWA-${Date.now()}`;
const title = `${marker} note`;
const offlineTail = "Typed while offline.";
const contentTypes = new Map([
  [".html", "text/html; charset=utf-8"],
  [".js", "text/javascript; charset=utf-8"],
  [".mjs", "text/javascript; charset=utf-8"],
  [".wasm", "application/wasm"],
  [".json", "application/json"],
  [".webmanifest", "application/manifest+json"],
  [".css", "text/css; charset=utf-8"],
  [".png", "image/png"],
  [".svg", "image/svg+xml"],
  [".ico", "image/x-icon"],
]);

function check(name, ok, detail = "") {
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${detail ? ` — ${detail}` : ""}`);
  return { name, ok: Boolean(ok), detail };
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

async function prepareBuild() {
  const temporary = await mkdtemp(path.join(tmpdir(), "notebox-pwa-"));
  const publicDirectory = path.join(temporary, "web/public");
  await cp(dist, publicDirectory, { recursive: true });
  const manifest = path.join(dist, "..", ".manifest.json");
  try {
    await access(manifest);
    await copyFile(manifest, path.join(temporary, "web/.manifest.json"));
  } catch {
    // pwa-build falls back to scanning the copied assets.
  }
  return { temporary, publicDirectory };
}

function serve(publicDirectory, requests) {
  const server = createServer(async (request, response) => {
    try {
      const url = new URL(request.url, "http://localhost");
      requests.push(url.pathname);
      const relative = decodeURIComponent(
        url.pathname === "/" ? "/index.html" : url.pathname,
      );
      const file = path.resolve(publicDirectory, `.${relative}`);
      if (!file.startsWith(publicDirectory + path.sep)) {
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
      resolve({ server, port: server.address().port });
    });
  });
}

async function openApp(context, url, errors) {
  const page = context.pages()[0] ?? (await context.newPage());
  page.on("pageerror", (error) => errors.push(String(error)));
  page.on("console", (message) => {
    if (message.type() === "error" && !message.text().includes("_dioxus")) {
      errors.push(`console: ${message.text()}`);
    }
  });
  await page.goto(url, { waitUntil: "networkidle" });
  await page
    .locator("#note-editor")
    .waitFor({ state: "visible", timeout: 60_000 });
  await page.waitForFunction(
    () => Boolean(navigator.serviceWorker?.controller),
    null,
    { timeout: 45_000 },
  );
  return page;
}

async function openNote(page, name) {
  const card = page.locator(".note-card", { hasText: name }).first();
  await card.waitFor({ state: "visible", timeout: 30_000 });
  await card.click();
  await page.waitForFunction(
    (expected) =>
      document.querySelector("#note-editor")?.value.includes(expected) ?? false,
    name,
    { timeout: 30_000 },
  );
}

async function capture(page, results, width, theme) {
  if (!wantScreenshots) return;
  await mkdir(screenshotDirectory, { recursive: true });
  await page.setViewportSize({ width, height: 900 });
  await page.evaluate(() => window.scrollTo(0, 0));
  await page.waitForTimeout(250);
  const file = path.join(screenshotDirectory, `p7-pwa-${width}-${theme}.png`);
  await page.screenshot({ path: file, fullPage: false });
  results.push(check(`screenshot ${width} ${theme}`, true, file));
}

const results = [];
const errors = [];
const requests = [];
let browser;
let temporary;
let server;
let context;
try {
  await access(path.join(dist, "index.html"));
  await access(path.join(dist, "sw.js"));
  await access(path.join(dist, "manifest.webmanifest"));
  const manifest = JSON.parse(
    await readFile(path.join(dist, "manifest.webmanifest"), "utf8"),
  );
  results.push(
    check(
      "manifest is standalone and root-scoped",
      manifest.display === "standalone" &&
        manifest.start_url === "/" &&
        manifest.scope === "/",
      `${manifest.display} ${manifest.start_url} ${manifest.scope}`,
    ),
  );
  const iconSizes = (manifest.icons ?? []).map((icon) => icon.sizes);
  results.push(
    check(
      "manifest has 192 and 512 icons",
      iconSizes.includes("192x192") && iconSizes.includes("512x512"),
      iconSizes.join(", "),
    ),
  );
  const worker = await readFile(path.join(dist, "sw.js"), "utf8");
  results.push(
    check(
      "worker precaches the app shell",
      worker.includes('"/index.html"') &&
        worker.includes('"/assets/') &&
        !worker.includes("__NBX_"),
    ),
  );

  const prepared = await prepareBuild();
  temporary = prepared.temporary;
  const served = await serve(prepared.publicDirectory, requests);
  server = served.server;
  const url = `http://127.0.0.1:${served.port}`;
  const playwright = await loadPlaywright();
  browser = await playwright.chromium.launch({ headless: true });
  context = await browser.newContext({
    viewport: { width: 1400, height: 900 },
    colorScheme: "light",
  });
  let page = await openApp(context, url, errors);
  const initialCaches = await page.evaluate(() => caches.keys());
  results.push(check("service worker controls the first load", true));

  const session = await context.newCDPSession(page);
  await session.send("Page.enable");
  const installability = await session.send("Page.getInstallabilityErrors");
  results.push(
    check(
      "Chromium reports no installability errors",
      installability.installabilityErrors.length === 0,
      JSON.stringify(installability.installabilityErrors),
    ),
  );

  await page.locator(".new-note-button").click();
  await page.waitForFunction(
    () => document.querySelector("#note-editor")?.value === "",
    null,
    { timeout: 30_000 },
  );
  await page.locator("#note-editor").fill(`${title}\n\nA local-only note.`);
  await page.waitForFunction(
    (expected) =>
      Array.from(document.querySelectorAll(".note-title")).some(
        (element) => element.textContent === expected,
      ),
    title,
    { timeout: 30_000 },
  );
  await page.waitForFunction(
    () => !document.querySelector(".save-state")?.textContent?.includes("Unsaved"),
    null,
    { timeout: 30_000 },
  );
  await page.waitForTimeout(1_200);

  await context.setOffline(true);
  await page.reload({ waitUntil: "domcontentloaded", timeout: 45_000 });
  await page
    .locator("#note-editor")
    .waitFor({ state: "visible", timeout: 60_000 });
  await openNote(page, title);
  const offlineText = await page.locator("#note-editor").inputValue();
  results.push(
    check(
      "offline reload keeps the local note",
      offlineText.includes(title),
      offlineText.slice(0, 120),
    ),
  );
  await page.locator("#note-editor").fill(`${offlineText}\n\n${offlineTail}`);
  await page.waitForFunction(
    () => !document.querySelector(".save-state")?.textContent?.includes("Unsaved"),
    null,
    { timeout: 30_000 },
  );
  await page.waitForTimeout(1_200);
  await page.reload({ waitUntil: "domcontentloaded", timeout: 45_000 });
  await page
    .locator("#note-editor")
    .waitFor({ state: "visible", timeout: 60_000 });
  await openNote(page, title);
  const afterOfflineEdit = await page.locator("#note-editor").inputValue();
  results.push(
    check(
      "an edit made offline survives a second offline reload",
      afterOfflineEdit.includes(offlineTail),
      afterOfflineEdit.slice(-120),
    ),
  );

  await context.setOffline(false);
  await page.waitForTimeout(500);
  execFileSync(
    process.execPath,
    [path.join(root, "scripts/pwa-build.mjs"), prepared.publicDirectory, "--build-id", "pwa-check-b"],
    { cwd: root, stdio: "inherit" },
  );
  await page.evaluate(async () => {
    const registration = await navigator.serviceWorker.getRegistration();
    await registration.update();
  });
  await page.waitForFunction(() => window.noteboxPwa?.ready === true, null, {
    timeout: 45_000,
  });
  await page.locator(".update-banner").waitFor({ state: "visible" });
  const banner = await page.locator(".update-banner").innerText();
  results.push(
    check(
      "a waiting build shows the update banner",
      banner.includes("A new version of Notebox is ready") &&
        banner.includes("Reload"),
      banner.replaceAll("\n", " "),
    ),
  );

  await capture(page, results, 1400, "light");
  await capture(page, results, 900, "light");
  await capture(page, results, 360, "light");
  await page.setViewportSize({ width: 1400, height: 900 });
  await page.getByRole("button", { name: "Toggle theme" }).click();
  await page.getByRole("button", { name: "Toggle theme" }).click();
  await page.waitForTimeout(250);
  await capture(page, results, 1400, "dark");
  await capture(page, results, 900, "dark");
  await capture(page, results, 360, "dark");
  await page.setViewportSize({ width: 1400, height: 900 });

  await Promise.all([
    page.waitForNavigation({ waitUntil: "domcontentloaded" }).catch(() => null),
    page.getByRole("button", { name: "Reload" }).click(),
  ]);
  await page
    .locator("#note-editor")
    .waitFor({ state: "visible", timeout: 60_000 });
  await openNote(page, title);
  const caches = await page.evaluate(() => caches.keys());
  results.push(
    check(
      "Reload activates the new worker and drops the old cache",
      caches.includes("notebox-pwa-check-b") &&
        initialCaches.every((cache) => !caches.includes(cache)),
      caches.join(", "),
    ),
  );
  const afterUpdate = await page.locator("#note-editor").inputValue();
  results.push(
    check(
      "the note is still open after the update reload",
      afterUpdate.includes(offlineTail),
      afterUpdate.slice(-120),
    ),
  );

  const apiRequests = requests.filter((request) => request.startsWith("/v1/"));
  results.push(
    check(
      "offline and update flows make no Worker API requests",
      apiRequests.length === 0,
      apiRequests.join(", "),
    ),
  );
  results.push(check("no page or console errors", errors.length === 0, errors.join("; ")));
} catch (error) {
  results.push(check("flow completed", false, String(error)));
} finally {
  if (context) await context.close().catch(() => {});
  if (browser) await browser.close().catch(() => {});
  if (server) server.close();
  if (temporary) await rm(temporary, { recursive: true, force: true });
}

const failed = results.filter((result) => !result.ok);
console.log(JSON.stringify({ results, errors }, null, 2));
if (failed.length > 0) {
  process.exit(1);
}
