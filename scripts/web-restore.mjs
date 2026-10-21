#!/usr/bin/env node
// P8: prove a backup round trip through the browser UI.
//
// Profile A creates a notebook, a pinned note with a tag and an image, and a
// trashed note, then exports a zip. Profile B is a fresh browser profile, as if
// the device had been wiped. It imports the zip and checks that the notes,
// notebook, tag, trash state, image bytes and version history all come back.
// Importing the same zip a second time must not duplicate anything.
//
//   PLAYWRIGHT_MODULES=/path/to/node_modules \
//   node scripts/web-restore.mjs [dist-dir]

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
const failureShot = path.join(tmpdir(), "notebox-web-restore-failure.png");
const marker = `Restore-${Date.now()}`;
const title = `${marker} note`;
const body = `${title}\n\nA pinned note with a tag and an image.`;
const trashedTitle = `${marker} trashed`;
const notebookName = `${marker} notebook`;
const tag = "restore-tag";
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
      const data = await readFile(file);
      response.writeHead(200, {
        "content-type":
          contentTypes.get(path.extname(file)) ?? "application/octet-stream",
        "cache-control": "no-store",
      });
      response.end(data);
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
    acceptDownloads: true,
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

async function openSettings(page) {
  await page.locator(".nav-row", { hasText: "Settings" }).click();
  await page.getByRole("heading", { name: "Local data" }).waitFor();
}

async function chooseBackup(page, zipPath) {
  await openSettings(page);
  const chooser = page.waitForEvent("filechooser");
  await page.getByRole("button", { name: "Restore from backup" }).click();
  await (await chooser).setFiles(zipPath);
  await page.locator(".restore-confirm").waitFor({ timeout: 30_000 });
  return page.locator(".restore-confirm").innerText();
}

async function chooseBackupFromPalette(page, zipPath) {
  await page.keyboard.press("Control+k");
  await page.locator("#palette-input").waitFor({ timeout: 10_000 });
  await page.locator("#palette-input").fill("restore from backup");
  const chooser = page.waitForEvent("filechooser");
  await page
    .locator(".palette-row", { hasText: "Restore from backup" })
    .click();
  await (await chooser).setFiles(zipPath);
  await page.locator(".restore-confirm").waitFor({ timeout: 30_000 });
  return page.locator(".restore-confirm").innerText();
}

async function confirmRestore(page) {
  await page.getByRole("button", { name: "Restore backup" }).click();
  await page
    .locator(".toast", { hasText: "Restored" })
    .waitFor({ timeout: 60_000 });
  return page.locator(".toast").innerText();
}

async function markerCards(page) {
  return page.locator(".note-card", { hasText: title }).count();
}

const results = [];
const errors = [];
let browser;
let profileA;
let profileB;
let downloadDir;
let context;
try {
  await access(path.join(dist, "index.html"));
  const { server, port } = await serve();
  const url = `http://127.0.0.1:${port}`;
  const playwright = await loadPlaywright();
  browser = playwright.chromium;
  profileA = await mkdtemp(path.join(tmpdir(), "notebox-restore-a-"));
  profileB = await mkdtemp(path.join(tmpdir(), "notebox-restore-b-"));
  downloadDir = await mkdtemp(path.join(tmpdir(), "notebox-restore-zip-"));
  const zipPath = path.join(downloadDir, "notebox-backup.zip");
  let page;

  // ---- The old device: make a backup worth restoring. ----
  ({ context, page } = await openApp(browser, profileA, url, errors));
  await page.getByRole("button", { name: "New notebook" }).click();
  await page.getByRole("textbox", { name: "New notebook name" }).fill(notebookName);
  await page.keyboard.press("Enter");
  await page.locator(".nav-row", { hasText: notebookName }).waitFor();

  await page.locator(".new-note-button").click();
  await page.waitForTimeout(400);
  await page.locator("#note-editor").fill(body);
  await page.waitForTimeout(500);
  await page.locator(".tag-input").fill(tag);
  await page.keyboard.press("Enter");
  await page.locator(".editor-tag", { hasText: tag }).waitFor();
  await page.locator(".notebook-select").selectOption({ label: notebookName });
  await page.getByRole("button", { name: "Pin note" }).click();
  await page.getByRole("button", { name: "Unpin note" }).waitFor();
  await page.locator("#note-image-input").setInputFiles({
    name: "restore.png",
    mimeType: "image/png",
    buffer: image,
  });
  await page.locator(".attachment-row").waitFor({ timeout: 30_000 });
  await page.waitForFunction(
    () => !document.querySelector(".save-state")?.textContent?.includes("Unsaved"),
    null,
    { timeout: 30_000 },
  );
  await page.waitForTimeout(1_500);

  await page.locator(".new-note-button").click();
  await page.waitForTimeout(400);
  await page.locator("#note-editor").fill(`${trashedTitle}\n\nKept in the trash.`);
  await page.waitForTimeout(800);
  await page.getByRole("button", { name: "Move note to trash" }).click();
  await page.locator(".toast", { hasText: "Moved to trash" }).waitFor();

  await openSettings(page);
  const download = page.waitForEvent("download");
  await page.getByRole("button", { name: "Export everything" }).click();
  await (await download).saveAs(zipPath);
  const zip = await readFile(zipPath);
  results.push(
    check(
      "backup is a non-empty zip",
      zip.length > 100 && zip.subarray(0, 2).toString() === "PK",
      `${zip.length} bytes`,
    ),
  );
  await context.close();
  context = undefined;

  // ---- The wiped device: a brand new profile with no notes of its own. ----
  ({ context, page } = await openApp(browser, profileB, url, errors));
  const firstText = await chooseBackup(page, zipPath);
  results.push(
    check(
      "confirmation names the archive and its history",
      firstText.includes("version history") &&
        firstText.includes("attachment"),
      firstText,
    ),
  );
  const toast = await confirmRestore(page);
  results.push(
    check(
      "restore reports what came back",
      toast.includes("Restored") && toast.includes("notebook"),
      toast,
    ),
  );

  await page.locator(".sidebar-nav .nav-row", { hasText: "Notes" }).click();
  await page.locator(".note-card", { hasText: title }).first().waitFor({
    timeout: 30_000,
  });
  results.push(
    check("restored note appears exactly once", (await markerCards(page)) === 1),
  );
  results.push(
    check(
      "restored notebook appears",
      (await page.locator(".nav-row", { hasText: notebookName }).count()) === 1,
    ),
  );

  await page.locator(".note-card", { hasText: title }).first().click();
  await page.waitForTimeout(600);
  const text = await page.locator("#note-editor").inputValue();
  results.push(
    check(
      "note text came back",
      text.includes(title) && text.includes("pinned note"),
      text.slice(0, 200),
    ),
  );
  results.push(
    check(
      "tag came back",
      (await page.locator(".editor-tag", { hasText: tag }).count()) === 1,
    ),
  );
  results.push(
    check(
      "notebook assignment came back",
      (await page.locator(".notebook-select").inputValue()) !== "",
    ),
  );
  results.push(
    check(
      "pin came back",
      (await page.getByRole("button", { name: "Unpin note" }).count()) === 1,
    ),
  );
  results.push(
    check(
      "attachment came back",
      (await page.locator(".attachment-row").count()) === 1,
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
      "image bytes came back",
      images >= 1 && src.startsWith("data:image/webp"),
      `${images} image(s), src starts ${src.slice(0, 24)}`,
    ),
  );
  await page.keyboard.press("Control+e");

  await page.getByRole("button", { name: "Note history" }).click();
  await page.locator(".history-panel").waitFor({ timeout: 10_000 });
  const versions = await page.locator(".history-version").count();
  const historyText = await page.locator(".history-panel").innerText();
  results.push(
    check(
      "history timeline came from the archive",
      versions >= 1 && /Device [0-9a-f]{4}/.test(historyText),
      `${versions} version row(s): ${historyText.slice(0, 160)}`,
    ),
  );
  results.push(
    check(
      "history is not relabelled as this device",
      !historyText.includes("This device"),
      historyText.slice(0, 160),
    ),
  );
  await page.getByRole("button", { name: "Close history" }).click();

  await page
    .locator(".sidebar-nav .nav-row", { hasText: "Trash" })
    .click();
  await page
    .locator(".note-card", { hasText: trashedTitle })
    .waitFor({ timeout: 10_000 });
  results.push(check("trashed note came back in the trash", true));

  // Import the same archive again from the command palette: ids must update,
  // not duplicate, and both entry points must reach the same picker.
  const secondText = await chooseBackupFromPalette(page, zipPath);
  results.push(
    check(
      "command palette opens the restore picker",
      secondText.includes("version history"),
      secondText,
    ),
  );
  await confirmRestore(page);
  await page.locator(".sidebar-nav .nav-row", { hasText: "Notes" }).click();
  await page.locator(".note-card", { hasText: title }).first().waitFor();
  results.push(
    check(
      "importing twice does not duplicate the note",
      (await markerCards(page)) === 1,
    ),
  );
  await page.locator(".note-card", { hasText: title }).first().click();
  await page.getByRole("button", { name: "Note history" }).click();
  await page.locator(".history-panel").waitFor();
  results.push(
    check(
      "importing twice does not duplicate history",
      (await page.locator(".history-version").count()) === versions,
    ),
  );
  await page.getByRole("button", { name: "Close history" }).click();

  await page.screenshot({ path: path.join(tmpdir(), "notebox-web-restore.png") });
  await context.close();
  context = undefined;
  server.close();
} catch (error) {
  results.push(check("flow completed", false, String(error)));
  if (context) {
    const page = context.pages()[0];
    if (page) {
      await page.screenshot({ path: failureShot }).catch(() => {});
    }
  }
} finally {
  results.push(
    check("no console errors", errors.length === 0, errors.slice(0, 5).join(" | ")),
  );
  if (context) await context.close().catch(() => {});
  for (const directory of [profileA, profileB, downloadDir]) {
    if (directory) await rm(directory, { recursive: true, force: true });
  }
  console.log(JSON.stringify({ results, errors }, null, 2));
  process.exit(results.every((result) => result.ok) ? 0 : 1);
}
