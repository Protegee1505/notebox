#!/usr/bin/env node
// P7: prove that a note, its history and its image bytes survive a browser
// restart without an account.
//
// Serves the release web build, drives a persistent Chromium profile, creates
// a local-only note with an image, closes the browser, and reopens the same
// profile. A second tab open at the same time must show the "already open"
// screen rather than writing a competing image.
//
//   PLAYWRIGHT_MODULES=/path/to/node_modules \
//   node scripts/web-persist.mjs [dist-dir]

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
    // Fall through to the conventional path when cargo is unavailable or the
    // lockfile is out of date.
  }
  return path.join(root, "target");
}

const dist =
  process.argv[2] ??
  path.join(targetDirectory(), "dx/notebox/release/web/public");
const screenshot = path.join(tmpdir(), "notebox-web-persist-failure.png");
const marker = `Persist-${Date.now()}`;
const title = `${marker} note`;
const tail = "Typed just before closing.";
const image = Buffer.from(
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==",
  "base64",
);

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

function check(name, ok, detail = "") {
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${detail ? ` — ${detail}` : ""}`);
  return { name, ok: Boolean(ok), detail };
}

async function openApp(browser, profile, url, errors) {
  const context = await browser.launchPersistentContext(profile, {
    headless: true,
    viewport: { width: 1400, height: 900 },
  });
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
  return { context, page };
}

async function storageCard(page) {
  await page.locator(".nav-row", { hasText: "Settings" }).click();
  await page.getByRole("heading", { name: "Local data" }).waitFor();
  const text = await page.locator(".storage-card").innerText();
  await page.locator(".nav-row", { hasText: "Notes" }).click();
  return text;
}

const results = [];
const errors = [];
let browser;
let profile;
let context;
try {
  await access(path.join(dist, "index.html"));
  const { server, port } = await serve();
  const url = `http://127.0.0.1:${port}`;
  const playwright = await loadPlaywright();
  browser = playwright.chromium;
  profile = await mkdtemp(path.join(tmpdir(), "notebox-web-persist-"));
  let page;

  ({ context, page } = await openApp(browser, profile, url, errors));
  const firstStorage = await storageCard(page);
  results.push(
    check(
      "settings uses browser storage",
      firstStorage.includes("Browser storage"),
      firstStorage,
    ),
  );
  results.push(
    check(
      "settings does not use memory",
      !firstStorage.includes("Browser memory"),
      firstStorage,
    ),
  );

  await page.locator(".new-note-button").click();
  await page.waitForTimeout(400);
  await page
    .locator("#note-editor")
    .fill(`${title}\n\nA local-only note with an image.`);
  await page.waitForTimeout(1_200);
  await page.locator("#note-image-input").setInputFiles({
    name: "tiny.png",
    mimeType: "image/png",
    buffer: image,
  });
  await page.locator(".attachment-row").waitFor({ timeout: 30_000 });
  await page.waitForFunction(
    () => !document.querySelector(".save-state")?.textContent?.includes("Unsaved"),
    null,
    { timeout: 30_000 },
  );
  // Let the 700 ms ticker send the image after the note save.
  await page.waitForTimeout(1_800);
  const before = await page.locator("#note-editor").inputValue();
  results.push(
    check(
      "image is referenced before restart",
      before.includes("![tiny](attachments/"),
      before.slice(0, 200),
    ),
  );
  results.push(
    check(
      "attachment is listed before restart",
      (await page.locator(".attachment-row").count()) >= 1,
    ),
  );

  // A second tab on the same origin must not open a second writer.
  const second = await context.newPage();
  await second.goto(url, { waitUntil: "networkidle" });
  await second
    .locator(".fatal-card h1", { hasText: "Notebox is already open" })
    .waitFor({ timeout: 30_000 });
  results.push(check("second tab is blocked by the store lock", true));
  await second.close();

  // Type and fire page-hide inside the 700 ms debounce: the handler must save
  // the draft and send the newer image. A synthetic event is used because
  // Playwright's context close does not dispatch page-hide.
  await page.locator("#note-editor").fill(`${before}\n\n${tail}`);
  await page.waitForTimeout(100);
  await page.evaluate(() => {
    window.dispatchEvent(new PageTransitionEvent("pagehide", { persisted: false }));
  });
  await page.waitForTimeout(1_500);
  await context.close();
  context = undefined;

  ({ context, page } = await openApp(browser, profile, url, errors));
  await page.waitForFunction(
    (expected) =>
      Array.from(document.querySelectorAll(".note-title")).some(
        (element) => element.textContent === expected,
      ),
    title,
    { timeout: 60_000 },
  );
  await page.locator(".note-card", { hasText: title }).first().click();
  await page.waitForTimeout(800);
  const after = await page.locator("#note-editor").inputValue();
  results.push(
    check(
      "note text survived the restart",
      after.includes(title) && after.includes("local-only"),
      after.slice(0, 200),
    ),
  );
  results.push(
    check(
      "edit made just before closing survived",
      after.includes(tail),
      after.slice(-120),
    ),
  );
  results.push(
    check(
      "image reference survived the restart",
      after.includes("![tiny](attachments/"),
      after.slice(0, 300),
    ),
  );
  results.push(
    check(
      "attachment row survived the restart",
      (await page.locator(".attachment-row").count()) >= 1,
    ),
  );

  await page.keyboard.press("Control+e");
  await page.locator(".editor-preview").waitFor({ timeout: 10_000 });
  await page.waitForTimeout(600);
  const images = await page.locator(".editor-preview img").count();
  const src = images
    ? await page.locator(".editor-preview img").first().getAttribute("src")
    : "";
  results.push(
    check(
      "image bytes survived the restart",
      images >= 1 && src.startsWith("data:image/webp"),
      `${images} image(s), src starts ${src.slice(0, 24)}`,
    ),
  );

  await page.getByRole("button", { name: "Note history" }).click();
  await page.locator(".history-panel").waitFor({ timeout: 10_000 });
  const versions = await page.locator(".history-version").count();
  results.push(
    check(
      "history panel has a saved version",
      versions >= 1,
      `${versions} version row(s)`,
    ),
  );
  await page.getByRole("button", { name: "Close history" }).click();

  const secondStorage = await storageCard(page);
  results.push(
    check(
      "settings still uses browser storage",
      secondStorage.includes("Browser storage") &&
        !secondStorage.includes("Browser memory"),
      secondStorage,
    ),
  );
  await page.screenshot({ path: path.join(tmpdir(), "notebox-web-persist.png") });
  await context.close();
  context = undefined;
  server.close();
} catch (error) {
  results.push(check("flow completed", false, String(error)));
  if (context) {
    const page = context.pages()[0];
    if (page) {
      await page.screenshot({ path: screenshot }).catch(() => {});
    }
  }
} finally {
  results.push(
    check("no console errors", errors.length === 0, errors.slice(0, 5).join(" | ")),
  );
  if (context) await context.close().catch(() => {});
  if (profile) await rm(profile, { recursive: true, force: true });
  console.log(JSON.stringify({ results, errors }, null, 2));
  process.exit(results.every((result) => result.ok) ? 0 : 1);
}
