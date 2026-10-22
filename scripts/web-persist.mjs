#!/usr/bin/env node
// P7: prove that a note, its history and its image bytes survive a browser
// restart without an account.
//
// Serves the release web build, drives a persistent Chromium profile, creates
// a local-only note with an image, closes the browser, and reopens the same
// profile. The note and the last screen must come back by themselves. A second
// tab open at the same time must show the "already open" screen rather than
// writing a competing image. The final pass turns delete confirmation off and
// checks that the trash action no longer asks.
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
// A second, different image gives the attachment gallery two items to move
// between and one item to delete.
const secondImage = await readFile(
  path.join(root, "crates/notes-app/web/icon-192.png"),
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
    colorScheme: "light",
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
  const offline = page.getByRole("button", { name: /Try it offline/ });
  if (await offline.isVisible().catch(() => false)) {
    await offline.click();
  }
  await page.locator(".workspace").waitFor({ state: "visible", timeout: 60_000 });
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
  await page.locator(".status-attachments").waitFor({ timeout: 30_000 });
  results.push(
    check(
      "the attachment panel starts closed",
      (await page.locator(".attachment-strip").count()) === 0,
    ),
  );
  await page.locator(".status-attachments").click();
  await page.locator(".attachment-row").waitFor({ timeout: 30_000 });
  await page.locator("#note-image-input").setInputFiles({
    name: "badge.png",
    mimeType: "image/png",
    buffer: secondImage,
  });
  await page.waitForFunction(
    () => document.querySelectorAll(".attachment-row").length >= 2,
    null,
    { timeout: 30_000 },
  );
  await page.waitForFunction(
    () => (document.getElementById("note-editor")?.value.match(/attachments\//g) ?? []).length >= 2,
    null,
    { timeout: 30_000 },
  );
  await page.waitForFunction(
    () => !document.querySelector(".save-state")?.textContent?.includes("Unsaved"),
    null,
    { timeout: 30_000 },
  );
  // Let the 700 ms ticker send the images after the note save.
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
  const cardImage = page
    .locator(".note-card", { hasText: title })
    .first()
    .locator(".note-media img");
  await cardImage.waitFor({ timeout: 30_000 });
  const cardSrc = (await cardImage.getAttribute("src")) ?? "";
  results.push(
    check(
      "the note card shows the image before restart",
      cardSrc.startsWith("data:image/"),
      cardSrc.slice(0, 40),
    ),
  );

  // Display choices are local preferences, not note data. A reload must keep
  // them, and they must never appear in the synced note itself. The first click
  // must resolve System against the light OS and land on dark, not no-op.
  const systemTheme = await page.locator("#app-root").getAttribute("data-theme");
  await page.getByRole("button", { name: "Toggle theme" }).click();
  await page.waitForFunction(
    () => document.getElementById("app-root")?.getAttribute("data-theme") === "dark",
    null,
    { timeout: 10_000 },
  );
  const firstClickTheme = await page.locator("#app-root").getAttribute("data-theme");
  results.push(
    check(
      "one theme click leaves System for the opposite theme",
      systemTheme === "system" && firstClickTheme === "dark",
      `${systemTheme} -> ${firstClickTheme}`,
    ),
  );
  await page.getByRole("button", { name: "List view" }).click();
  await page.waitForTimeout(300);

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
  const restoredTheme = await page.locator("#app-root").getAttribute("data-theme");
  const restoredList = await page.locator(".note-list").getAttribute("class");
  results.push(check("theme survives a restart", restoredTheme === "dark", restoredTheme ?? ""));
  results.push(
    check(
      "layout survives a restart",
      (restoredList ?? "").includes("note-rows"),
      restoredList ?? "",
    ),
  );
  await page.waitForFunction(
    (expected) =>
      Array.from(document.querySelectorAll(".note-title")).some(
        (element) => element.textContent === expected,
      ),
    title,
    { timeout: 60_000 },
  );
  await page.waitForFunction(
    (expected) => document.getElementById("note-editor")?.value.includes(expected),
    title,
    { timeout: 30_000 },
  );
  const after = await page.locator("#note-editor").inputValue();
  results.push(
    check(
      "the last note reopens without choosing it again",
      after.includes(title),
      after.slice(0, 120),
    ),
  );
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
  await page.locator(".status-attachments").click();
  await page.locator(".attachment-strip").waitFor({ timeout: 10_000 });
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
      images >= 2 && src.startsWith("data:image/webp"),
      `${images} image(s), src starts ${src.slice(0, 24)}`,
    ),
  );

  // The rendered image is the viewer's entry point. Clicking it and pressing
  // Enter both open the same per-note gallery, and closing returns focus to the
  // image the reader came from.
  const previewImage = page
    .locator(".editor-preview img.attachment-preview-image")
    .first();
  await previewImage.waitFor({ timeout: 10_000 });
  const openedHash = await previewImage.getAttribute("data-attachment-hash");
  await previewImage.click();
  await page.locator(".attachment-gallery").waitFor({ timeout: 10_000 });
  const openedAt = await page.locator(".gallery-position").innerText();
  results.push(
    check(
      "a rendered image opens its attachment viewer",
      openedAt.includes("of 2"),
      openedAt,
    ),
  );
  await page.keyboard.press("Escape");
  await page.locator(".attachment-gallery").waitFor({ state: "detached" });
  await page.waitForFunction(
    (hash) =>
      document.activeElement?.getAttribute("data-attachment-hash") === hash,
    openedHash,
    { timeout: 5_000 },
  );
  results.push(check("closing the viewer returns focus to the image", true));

  await previewImage.focus();
  await page.keyboard.press("Enter");
  await page.locator(".attachment-gallery").waitFor({ timeout: 10_000 });
  await page.keyboard.press("ArrowRight");
  await page.waitForFunction(
    () => document.querySelector(".gallery-position")?.textContent?.includes("2 of 2"),
    null,
    { timeout: 5_000 },
  );
  await page.keyboard.press("Tab");
  const focusInside = await page.evaluate(() =>
    Boolean(document.activeElement?.closest(".attachment-gallery")),
  );
  results.push(
    check(
      "arrow keys step the viewer and Tab stays inside it",
      focusInside,
      focusInside ? "focus trapped" : "focus escaped",
    ),
  );
  await page.keyboard.press("Escape");
  await page.locator(".attachment-gallery").waitFor({ state: "detached" });

  // Browse all reopens the same gallery from the attachment strip. Download
  // one file, then delete it and check the note reference and local count.
  if ((await page.locator("#attachment-gallery-button").count()) === 0) {
    await page.locator(".status-attachments").click();
  }
  await page.locator("#attachment-gallery-button").click();
  await page.locator(".attachment-gallery").waitFor({ timeout: 10_000 });
  const downloadPromise = page.waitForEvent("download");
  await page
    .locator(".gallery-actions")
    .getByRole("button", { name: "Download" })
    .click();
  const download = await downloadPromise;
  const downloaded = download.suggestedFilename();
  results.push(
    check(
      "download saves the selected attachment",
      /tiny|badge/.test(downloaded),
      downloaded,
    ),
  );

  const refsBefore = (after.match(/attachments\//g) ?? []).length;
  await page.locator("#gallery-delete").click();
  await page.locator(".gallery-confirm").waitFor({ timeout: 5_000 });
  const confirmFocus = await page.evaluate(() => document.activeElement?.id);
  results.push(
    check(
      "delete confirmation starts on Cancel",
      confirmFocus === "gallery-delete-cancel",
      confirmFocus ?? "no focus",
    ),
  );
  await page.keyboard.press("Escape");
  await page.locator(".gallery-confirm").waitFor({ state: "detached" });
  await page.waitForFunction(
    () => document.activeElement?.id === "gallery-delete",
    null,
    { timeout: 5_000 },
  );
  await page.locator("#gallery-delete").click();
  await page.getByRole("button", { name: "Delete permanently" }).click();
  await page.waitForFunction(
    () => document.querySelector(".gallery-position")?.textContent?.includes("1 of 1"),
    null,
    { timeout: 15_000 },
  );
  await page.keyboard.press("Escape");
  await page.locator(".attachment-gallery").waitFor({ state: "detached" });
  await page.waitForFunction(
    () => document.activeElement?.id === "status-attachments-button",
    null,
    { timeout: 5_000 },
  );
  results.push(check("closing the strip-opened viewer restores the footer button", true));

  await page.keyboard.press("Control+e");
  await page.locator("#note-editor").waitFor({ timeout: 10_000 });
  const refsAfter = (
    (await page.locator("#note-editor").inputValue()).match(/attachments\//g) ?? []
  ).length;
  results.push(
    check(
      "deleting removes one note reference and keeps the rest",
      refsAfter === refsBefore - 1,
      `${refsBefore} -> ${refsAfter}`,
    ),
  );

  // Delete the last copy. There is no app-wide file library, so Remove from
  // note is disabled and the viewer must close and hand focus back to the app
  // root once the footer attachment button disappears with the last file.
  if ((await page.locator("#attachment-gallery-button").count()) === 0) {
    await page.locator(".status-attachments").click();
  }
  await page.locator("#attachment-gallery-button").click();
  await page.locator(".attachment-gallery").waitFor({ timeout: 10_000 });
  const removeLast = page
    .locator(".gallery-actions")
    .getByRole("button", { name: "Remove from note" });
  results.push(
    check(
      "the last copy cannot be removed from the note alone",
      await removeLast.isDisabled(),
    ),
  );
  await page.locator("#gallery-delete").click();
  await page.locator(".gallery-confirm").waitFor({ timeout: 5_000 });
  await page.getByRole("button", { name: "Delete permanently" }).click();
  await page.locator(".attachment-gallery").waitFor({ state: "detached" });
  await page.waitForFunction(
    () => document.activeElement?.id === "app-root",
    null,
    { timeout: 5_000 },
  );
  const refsFinal = (
    (await page.locator("#note-editor").inputValue()).match(/attachments\//g) ?? []
  ).length;
  results.push(
    check(
      "deleting the last copy closes the viewer and focuses the app",
      refsFinal === 0,
      `${refsFinal} reference(s) left; focus fell back to the app root`,
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
  results.push(
    check(
      "settings drops the final deleted attachment from local storage",
      /Images & files\s+0\b/.test(secondStorage),
      secondStorage,
    ),
  );
  await page.screenshot({ path: path.join(tmpdir(), "notebox-web-persist.png") });

  // Leave Settings open; the next launch must return to it, then Back must
  // reveal the note that was open before the restart.
  await page.locator(".nav-row", { hasText: "Settings" }).click();
  await page.getByRole("heading", { name: "Local data" }).waitFor();
  await context.close();
  context = undefined;

  ({ context, page } = await openApp(browser, profile, url, errors));
  await page.locator(".settings-pane").waitFor({ timeout: 60_000 });
  results.push(check("the last screen reopens on restart", true));
  await page.getByRole("button", { name: "Back to notes" }).click();
  await page.waitForFunction(
    (expected) => document.getElementById("note-editor")?.value.includes(expected),
    title,
    { timeout: 30_000 },
  );
  const finalDraft = await page.locator("#note-editor").inputValue();
  results.push(
    check(
      "Back from the restored Settings opens the restored note",
      finalDraft.includes(title),
      finalDraft.slice(0, 120),
    ),
  );

  // The opt-out must remove the prompt, not only persist a pressed button.
  await page.locator(".nav-row", { hasText: "Settings" }).click();
  await page.getByRole("heading", { name: "Local data" }).waitFor();
  await page
    .getByRole("group", { name: "Confirm before moving notes to trash" })
    .getByRole("button", { name: "Off" })
    .click();
  await page.waitForTimeout(200);
  await page.getByRole("button", { name: "Back to notes" }).click();
  await page.locator("#note-editor").waitFor();
  await page.getByRole("button", { name: "Move note to trash" }).click();
  await page.locator(".toast", { hasText: "Moved to trash" }).waitFor();
  const prompts = await page.locator("#delete-confirm").count();
  results.push(
    check(
      "turning delete confirmation off skips the prompt",
      prompts === 0,
      `${prompts} prompt(s)`,
    ),
  );

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
