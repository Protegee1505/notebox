#!/usr/bin/env node
// P8: links and images in the Write editor.
//
// Serves the release web build and proves that Write mode paints Markdown
// links and images, that a bare web address is painted too, that the mirror
// sits exactly over the textarea, that Ctrl+click and Ctrl+Enter open a link
// in a new tab or an image in the viewer, and that the footer chip opens the
// same token and returns focus after the viewer closes. A note with more
// tokens than the scanner cap falls back to a plain textarea. A phone profile
// repeats the tap-token-then-tap-chip path.
//
//   PLAYWRIGHT_MODULES=/path/to/node_modules \
//   node scripts/web-markup.mjs [dist-dir]

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
const screenshot = path.join(tmpdir(), "notebox-web-markup-failure.png");
const title = `Markup-${Date.now()}`;
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

async function openProfile(browser, profile, url, errors, options = {}) {
  const context = await browser.launchPersistentContext(profile, {
    headless: true,
    colorScheme: "light",
    viewport: { width: 1400, height: 900 },
    ...options,
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

async function waitForTokens(page) {
  await page
    .locator(".editor-highlights-inner")
    .waitFor({ state: "visible", timeout: 15_000 });
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
let browser;
let server;
let context;

try {
  await access(path.join(dist, "index.html"));
  const started = await serve();
  server = started.server;
  const base = started.base;
  const target = `${base}/external-target`;
  const playwright = await loadPlaywright();
  browser = playwright.chromium;

  // Desktop -----------------------------------------------------------------
  const profile = await mkdtemp(path.join(tmpdir(), "notebox-web-markup-"));
  ({ context } = await openProfile(browser, profile, base, errors));
  let page = context.pages()[0];
  await page.locator(".new-note-button").click();
  await page.waitForTimeout(400);
  const filler = "A plain filler line that lets the editor scroll.\n".repeat(40);
  await page.locator("#note-editor").fill(
    `${title}\n\n[site](${target}) and https://example.org/page\n\n${filler}`,
  );
  await page.waitForTimeout(200);
  await waitForTokens(page);

  const linkTexts = await page.locator(".md-link-token").allInnerTexts();
  results.push(
    check(
      "a Markdown link is painted",
      linkTexts.some((text) => text.includes(`[site](${target})`)),
      linkTexts.join(" | "),
    ),
  );
  results.push(
    check(
      "a bare web address is painted too",
      linkTexts.some((text) => text.includes("https://example.org/page")),
      linkTexts.join(" | "),
    ),
  );

  const paint = await page.evaluate(() => {
    const link = document.querySelector(".md-link-token");
    const editor = document.getElementById("note-editor");
    const linkStyle = getComputedStyle(link);
    const rect = link.getBoundingClientRect();
    const editorRect = editor.getBoundingClientRect();
    return {
      linkUnderline: linkStyle.borderBottomStyle,
      linkTint: linkStyle.backgroundColor,
      linkWidth: rect.width,
      editorVisible: editorRect.width > 0 && editorRect.height > 0,
    };
  });
  results.push(
    check(
      "links use a solid underline and a tint",
      paint.linkUnderline === "solid" &&
        paint.linkTint !== "rgba(0, 0, 0, 0)" &&
        paint.linkWidth > 0 &&
        paint.editorVisible,
      JSON.stringify(paint),
    ),
  );

  const mirror = await page.evaluate(() => {
    const editor = document.getElementById("note-editor");
    const highlights = document.querySelector(".editor-highlights");
    const a = editor.getBoundingClientRect();
    const b = highlights.getBoundingClientRect();
    return {
      dx: Math.abs(a.left - b.left),
      dy: Math.abs(a.top - b.top),
      dw: Math.abs(a.width - b.width),
      dh: Math.abs(a.height - b.height),
      visibility: getComputedStyle(highlights).visibility,
    };
  });
  results.push(
    check(
      "the mirror covers the textarea",
      mirror.dx < 2 &&
        mirror.dy < 2 &&
        mirror.dw < 2 &&
        mirror.dh < 2 &&
        mirror.visibility === "visible",
      JSON.stringify(mirror),
    ),
  );

  await page.evaluate(() => {
    const editor = document.getElementById("note-editor");
    editor.scrollTop = 20;
  });
  await page.waitForFunction(
    () =>
      document
        .querySelector(".editor-highlights-inner")
        ?.style.transform?.includes("-20px"),
    null,
    { timeout: 5_000 },
  );
  results.push(check("scrolling keeps the mirror in step", true));

  // Link chip ---------------------------------------------------------------
  await placeCaret(page, `[site](${target})`, 2);
  await waitForChip(page, "Open link");
  const linkChip = await page.locator("#token-open").innerText();
  results.push(
    check("the chip names the link host", linkChip.includes("127.0.0.1"), linkChip),
  );
  const linkPopupPromise = context.waitForEvent("page");
  await page.locator("#token-open").click();
  const linkPopup = await linkPopupPromise;
  await linkPopup.waitForLoadState("domcontentloaded").catch(() => {});
  results.push(
    check(
      "the chip opens the link outside the app",
      linkPopup.url().includes("/external-target"),
      linkPopup.url(),
    ),
  );
  await linkPopup.close();

  // Ctrl+click the same link ------------------------------------------------
  const linkBox = await page.locator(".md-link-token").first().boundingBox();
  const ctrlPopupPromise = context.waitForEvent("page");
  await page.keyboard.down("Control");
  await page.mouse.click(linkBox.x + 4, linkBox.y + linkBox.height / 2);
  await page.keyboard.up("Control");
  const ctrlPopup = await ctrlPopupPromise;
  await ctrlPopup.waitForLoadState("domcontentloaded").catch(() => {});
  results.push(
    check(
      "Ctrl+click opens the link",
      ctrlPopup.url().includes("/external-target"),
      ctrlPopup.url(),
    ),
  );
  await ctrlPopup.close();

  // Image attachment --------------------------------------------------------
  await page.evaluate(() => {
    const editor = document.getElementById("note-editor");
    editor.focus();
    editor.setSelectionRange(editor.value.length, editor.value.length);
  });
  await page.locator("#note-image-input").setInputFiles({
    name: "tiny.png",
    mimeType: "image/png",
    buffer: image,
  });
  await page.waitForFunction(
    () =>
      (document.getElementById("note-editor")?.value ?? "").includes("![tiny](attachments/"),
    null,
    { timeout: 30_000 },
  );
  await page.waitForTimeout(300);
  await page.locator(".md-image-token").first().waitFor({ timeout: 10_000 });
  const imagePaint = await page.evaluate(() => {
    const style = getComputedStyle(document.querySelector(".md-image-token"));
    return {
      underline: style.borderBottomStyle,
      tint: style.backgroundColor,
    };
  });
  results.push(
    check(
      "images use a dashed underline and an accent tint",
      imagePaint.underline === "dashed" && imagePaint.tint !== "rgba(0, 0, 0, 0)",
      JSON.stringify(imagePaint),
    ),
  );

  await placeCaret(page, "![tiny]", 2);
  await waitForChip(page, "View image");
  const imageChip = await page.locator("#token-open").innerText();
  results.push(
    check("the chip names the image", imageChip.includes("tiny."), imageChip),
  );
  await page.locator("#token-open").click();
  await page.locator(".attachment-gallery").waitFor({ timeout: 10_000 });
  results.push(check("the chip opens the image viewer", true));
  await page.keyboard.press("Escape");
  await page.locator(".attachment-gallery").waitFor({ state: "detached" });
  await page.waitForTimeout(150);
  const returned = await page.evaluate(() => document.activeElement?.id ?? "");
  results.push(
    check("closing the viewer returns to the editor", returned === "note-editor", returned),
  );

  // Ctrl+Enter --------------------------------------------------------------
  await placeCaret(page, "![tiny]", 2);
  await page.keyboard.press("Control+Enter");
  await page.locator(".attachment-gallery").waitFor({ timeout: 10_000 });
  results.push(check("Ctrl+Enter opens the image at the cursor", true));
  await page.keyboard.press("Escape");
  await page.locator(".attachment-gallery").waitFor({ state: "detached" });

  const beforeLength = await page.evaluate(
    () => document.getElementById("note-editor").value.length,
  );
  await placeCaret(page, `[site](${target})`, 2);
  const enterPopupPromise = context.waitForEvent("page");
  await page.keyboard.press("Control+Enter");
  const enterPopup = await enterPopupPromise;
  await enterPopup.waitForLoadState("domcontentloaded").catch(() => {});
  const afterLength = await page.evaluate(
    () => document.getElementById("note-editor").value.length,
  );
  results.push(
    check(
      "Ctrl+Enter opens the link and inserts nothing",
      enterPopup.url().includes("/external-target") && afterLength === beforeLength,
      enterPopup.url(),
    ),
  );
  await enterPopup.close();

  // Over the token cap the editor stays plain ---------------------------------
  await page.evaluate(() => {
    const editor = document.getElementById("note-editor");
    const filler = Array.from(
      { length: 2400 },
      (_, index) => `[x](https://e.co/${index})`,
    ).join(" ");
    editor.value = `cap check\n\n${filler}`;
    editor.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await page.waitForFunction(
    () => document.querySelectorAll(".editor-highlights").length === 0,
    null,
    { timeout: 15_000 },
  );
  const capState = await page.evaluate(() => ({
    highlights: document.querySelectorAll(".editor-highlights").length,
    value: document.getElementById("note-editor").value.length,
  }));
  results.push(
    check(
      "a token-dense note falls back to a plain editor",
      capState.highlights === 0 && capState.value > 40_000,
      JSON.stringify(capState),
    ),
  );

  await context.close();
  context = undefined;
  await rm(profile, { recursive: true, force: true });

  // Phone --------------------------------------------------------------------
  const phoneProfile = await mkdtemp(path.join(tmpdir(), "notebox-web-markup-phone-"));
  ({ context } = await openProfile(browser, phoneProfile, base, errors, {
    ...playwright.devices["iPhone 13"],
  }));
  page = context.pages()[0];
  const fab = page.locator(".new-note-fab");
  if (await fab.isVisible().catch(() => false)) {
    await fab.click();
  } else {
    await page.locator(".new-note-button").click();
  }
  await page.locator("#note-editor").waitFor({ timeout: 15_000 });
  await page.locator("#note-editor").fill(`${title} phone\n\nTap the https://example.org/phone link`);
  await page.waitForTimeout(250);
  await waitForTokens(page);
  const phoneBox = await page.locator(".md-link-token").first().boundingBox();
  await page.touchscreen.tap(phoneBox.x + 6, phoneBox.y + phoneBox.height / 2);
  await waitForChip(page, "Open link");
  results.push(check("a phone tap inside a token shows the chip", true));
  const phonePopupPromise = context.waitForEvent("page");
  await page.locator("#token-open").tap();
  const phonePopup = await phonePopupPromise;
  await phonePopup.waitForLoadState("domcontentloaded").catch(() => {});
  results.push(
    check(
      "the phone chip opens the link",
      phonePopup.url().includes("/phone") || phonePopup.url().includes("example.org"),
      phonePopup.url(),
    ),
  );
  await phonePopup.close();

  await page.evaluate(() => {
    const editor = document.getElementById("note-editor");
    editor.focus();
    editor.setSelectionRange(editor.value.length, editor.value.length);
  });
  await page.locator("#note-image-input").setInputFiles({
    name: "tiny.png",
    mimeType: "image/png",
    buffer: image,
  });
  await page.waitForFunction(
    () =>
      (document.getElementById("note-editor")?.value ?? "").includes("![tiny](attachments/"),
    null,
    { timeout: 30_000 },
  );
  await page.evaluate(() => {
    const editor = document.getElementById("note-editor");
    editor.scrollTop = editor.scrollHeight;
  });
  await page.waitForTimeout(300);
  const phoneImage = await page.locator(".md-image-token").last().boundingBox();
  await page.touchscreen.tap(
    phoneImage.x + 6,
    phoneImage.y + phoneImage.height / 2,
  );
  await waitForChip(page, "View image");
  await page.locator("#token-open").tap();
  await page.locator(".attachment-gallery").waitFor({ timeout: 10_000 });
  await page.keyboard.press("Escape");
  await page.locator(".attachment-gallery").waitFor({ state: "detached" });
  await page.waitForTimeout(150);
  const phoneFocus = await page.evaluate(() => document.activeElement?.id ?? "");
  results.push(
    check(
      "the phone viewer returns focus to the chip without reopening the keyboard",
      phoneFocus === "token-open",
      phoneFocus,
    ),
  );

  await page.setViewportSize({ width: 320, height: 568 });
  await page.waitForTimeout(250);
  const footer = await page.evaluate(() => {
    const status = document.querySelector(".editor-status");
    const chip = document.getElementById("token-open");
    const children = [...status.children].map((child) => ({
      name: child.id || child.className,
      right: child.getBoundingClientRect().right,
      width: child.getBoundingClientRect().width,
    }));
    return {
      client: status.clientWidth,
      scroll: status.scrollWidth,
      right: status.getBoundingClientRect().right,
      chip: chip?.getBoundingClientRect().width ?? 0,
      children,
    };
  });
  results.push(
    check(
      "the footer chip fits a 320px phone",
      footer.scroll <= footer.client &&
        footer.chip > 0 &&
        footer.children.every((child) => child.right <= footer.right + 1),
      JSON.stringify(footer),
    ),
  );

  await context.close();
  context = undefined;
  await rm(phoneProfile, { recursive: true, force: true });

  server.close();
  server = undefined;
} catch (error) {
  results.push(check("flow completed", false, String(error)));
  if (context) {
    const page = context.pages()[0];
    if (page) await page.screenshot({ path: screenshot }).catch(() => {});
  }
} finally {
  results.push(
    check("no console errors", errors.length === 0, errors.slice(0, 5).join(" | ")),
  );
  if (context) await context.close().catch(() => {});
  if (server) server.close();
  console.log(JSON.stringify({ results, errors }, null, 2));
  process.exit(results.every((result) => result.ok) ? 0 : 1);
}
