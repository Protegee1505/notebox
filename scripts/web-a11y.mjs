#!/usr/bin/env node
// P8: WCAG A/AA audit of the release UI.
//
// Serves the release web build, drives the real app with Playwright and runs
// axe-core on every main screen in light and dark themes. It also checks the
// keyboard path: note-list arrow keys, Tab trapped inside each dialog, focus
// return after closing a panel, and autofocus on Settings, Login and dialogs.
//
//   PLAYWRIGHT_MODULES=/path/to/node_modules \
//   node scripts/web-a11y.mjs [dist-dir]
//
// The modules directory must contain playwright and axe-core (4.10.x).

import { createServer } from "node:http";
import { access, readFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const dist =
  process.argv[2] ??
  path.join(root, "target/dx/notebox/release/web/public");
const failureShot = path.join(tmpdir(), "notebox-a11y-failure.png");
const WCAG_TAGS = ["wcag2a", "wcag2aa", "wcag21a", "wcag21aa", "wcag22aa"];

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
      await access(path.join(candidate, "axe-core", "axe.min.js"));
      return candidate;
    } catch {
      // Try the next candidate.
    }
  }
  throw new Error(
    "playwright and axe-core are not installed; set PLAYWRIGHT_MODULES to their node_modules directory",
  );
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

async function settle(page, ms = 250) {
  await page.waitForTimeout(ms);
}

async function setTheme(page, theme) {
  for (let attempt = 0; attempt < 3; attempt += 1) {
    const current = await page.locator("#app-root").getAttribute("data-theme");
    if (current === theme) return;
    await page.getByRole("button", { name: "Toggle theme" }).click();
    await settle(page);
  }
  throw new Error(`could not switch to the ${theme} theme`);
}

function axeSummary(violations) {
  return violations
    .map((violation) => {
      const targets = violation.nodes
        .slice(0, 3)
        .map((node) => node.target.join(" "))
        .join(", ");
      return `${violation.id} (${violation.impact}): ${targets}`;
    })
    .join("; ");
}

async function activeCardIndex(page) {
  return page.evaluate(() => {
    const cards = [...document.querySelectorAll(".note-card")];
    return cards.indexOf(document.activeElement);
  });
}

async function tabStay(page, selector, key = "Tab", presses = 40) {
  for (let press = 0; press < presses; press += 1) {
    await page.keyboard.press(key);
    await page.waitForTimeout(25);
    const inside = await page.evaluate((target) => {
      const root = document.querySelector(target);
      return Boolean(root && root.contains(document.activeElement));
    }, selector);
    if (!inside) return { ok: false, press: press + 1 };
  }
  return { ok: true, press: presses };
}

const results = [];
const errors = [];
let browser;
let context;
let server;
try {
  await access(path.join(dist, "index.html"));
  const modules = await moduleDirectory();
  const axePath = path.join(modules, "axe-core", "axe.min.js");
  const playwright = await loadPlaywright(modules);
  const started = await serve();
  server = started.server;

  browser = await playwright.chromium.launch({ headless: true });
  context = await browser.newContext({
    viewport: { width: 1400, height: 900 },
    colorScheme: "light",
  });
  const page = await context.newPage();
  page.on("pageerror", (error) => errors.push(String(error)));
  page.on("console", (message) => {
    if (message.type() === "error" && !message.text().includes("_dioxus")) {
      errors.push(`console: ${message.text()}`);
    }
  });
  await page.goto(started.base, { waitUntil: "networkidle" });
  await page.getByRole("heading", { name: "Welcome to Notebox" }).waitFor({ timeout: 60_000 });
  results.push(check("first launch opens the welcome login screen", true));
  const firstRunBack = await page.getByRole("button", { name: "Back to notes" }).count();
  results.push(check("first launch hides the login back button", firstRunBack === 0));
  await page.getByRole("button", { name: /Try it offline/ }).click();
  await page.locator(".empty-list").waitFor({ timeout: 10_000 });
  results.push(
    check(
      "offline mode starts without sample notes",
      (await page.locator(".note-card").count()) === 0,
    ),
  );
  await page.getByRole("button", { name: "Add welcome tour" }).click();
  await page.locator(".note-card").nth(2).waitFor({ timeout: 60_000 });
  await page.locator("#note-editor").waitFor({ timeout: 60_000 });
  await settle(page, 600);
  await page.locator("#note-editor").click();
  await page.keyboard.press("Control+k");
  await page.waitForFunction(
    () => document.activeElement?.id === "global-search",
    null,
    { timeout: 10_000 },
  );
  results.push(check("Ctrl K focuses note search", true));

  await page.locator("#global-search").fill("pragmatic");
  await page.locator(".note-card").first().waitFor({ timeout: 10_000 });
  const marks = await page.locator(".note-card .search-mark").allTextContents();
  results.push(
    check(
      "search highlights a matching result",
      marks.includes("Pragmatic"),
      marks.join("|"),
    ),
  );
  await page.locator(".note-card").first().click();
  await page.locator("#find-input").waitFor({ timeout: 10_000 });
  const openedFind = await page.locator("#find-input").inputValue();
  results.push(
    check(
      "opening a search hit prefills Find with the body match",
      openedFind.toLowerCase() === "pragmatic",
      openedFind,
    ),
  );
  await page.keyboard.press("Escape");
  await page.locator("#find-input").waitFor({ state: "hidden", timeout: 10_000 });

  await page.locator("#global-search").fill("personal");
  await page.waitForFunction(
    () => document.querySelectorAll(".note-card").length === 1,
    null,
    { timeout: 10_000 },
  );
  const tagMarks = await page.locator(".note-tag .search-mark").allTextContents();
  results.push(
    check(
      "search matches a tag even when the body lacks the word",
      tagMarks.includes("personal"),
      tagMarks.join("|"),
    ),
  );
  await page.locator(".note-card").first().click();
  const tagOnlyFind = await page.locator("#find-input").count();
  results.push(check("a tag-only hit leaves Find closed", tagOnlyFind === 0));

  await page.locator("#global-search").fill("");
  await page.waitForFunction(
    () => document.querySelectorAll(".note-card").length === 3,
    null,
    { timeout: 10_000 },
  );
  await page.locator("#note-editor").click();
  await page.addScriptTag({ path: axePath });

  async function audit(name) {
    const violations = await page.evaluate(async (tags) => {
      const report = await axe.run(document, {
        runOnly: { type: "tag", values: tags },
      });
      return report.violations.map((violation) => ({
        id: violation.id,
        impact: violation.impact,
        help: violation.help,
        nodes: violation.nodes.map((node) => ({
          target: node.target,
          summary: node.failureSummary,
        })),
      }));
    }, WCAG_TAGS);
    if (violations.length > 0) {
      console.log(`\n${name}:`);
      for (const violation of violations) {
        console.log(`  [${violation.impact}] ${violation.id}: ${violation.help}`);
        for (const node of violation.nodes.slice(0, 5)) {
          console.log(`    ${node.target.join(" ")}`);
        }
      }
    }
    results.push(
      check(
        `WCAG A/AA on ${name}`,
        violations.length === 0,
        axeSummary(violations),
      ),
    );
  }

  // ---- Notes, both themes, and the list's keyboard contract. ----
  const viewportMeta = await page.evaluate(
    () => document.querySelector('meta[name="viewport"]')?.content ?? "",
  );
  results.push(
    check(
      "viewport asks the keyboard to shrink the layout",
      viewportMeta.includes("interactive-widget=resizes-content"),
      viewportMeta,
    ),
  );

  await setTheme(page, "light");
  await audit("notes, light");

  await setTheme(page, "dark");
  await audit("notes, dark");

  const desktopFab = await page.locator(".new-note-fab").isVisible();
  results.push(
    check("the floating new-note button stays off desktop", !desktopFab),
  );

  // A soft keyboard exposes whatever page area sits below the app. html, body
  // and the static crash panel carry the app's theme, not the light default.
  const darkCanvas = await page.evaluate(() => ({
    attribute: document.documentElement.getAttribute("data-theme"),
    html: getComputedStyle(document.documentElement).backgroundColor,
    body: getComputedStyle(document.body).backgroundColor,
    app: getComputedStyle(document.getElementById("app-root")).backgroundColor,
  }));
  results.push(
    check(
      "the document canvas follows the dark theme",
      darkCanvas.attribute === "dark" &&
        darkCanvas.html === darkCanvas.app &&
        darkCanvas.body === darkCanvas.app &&
        darkCanvas.app === "rgb(21, 24, 21)",
      JSON.stringify(darkCanvas),
    ),
  );

  await setTheme(page, "light");
  const cards = page.locator(".note-card");
  await cards.first().focus();
  await page.keyboard.press("ArrowDown");
  const down = await activeCardIndex(page);
  results.push(check("Arrow Down moves focus to the next note", down === 1, `index ${down}`));
  await page.keyboard.press("Home");
  const home = await activeCardIndex(page);
  results.push(check("Home moves focus to the first note", home === 0, `index ${home}`));
  await page.keyboard.press("Enter");
  await settle(page);
  const selected = await page.evaluate(
    () => document.activeElement?.getAttribute("aria-selected") === "true",
  );
  results.push(check("Enter selects the focused note", selected));

  // ---- History panel, both themes, and focus return. ----
  await page.getByRole("button", { name: "Note history" }).click();
  await page.locator(".history-panel").waitFor({ timeout: 10_000 });
  await settle(page);
  await audit("history, light");
  const historyTab = await tabStay(page, ".history-panel");
  const historyBack = await tabStay(page, ".history-panel", "Shift+Tab", 5);
  results.push(
    check(
      "Tab stays inside the history panel",
      historyTab.ok && historyBack.ok,
      historyTab.ok && historyBack.ok
        ? ""
        : `escaped after ${historyTab.ok ? historyBack.press : historyTab.press} presses`,
    ),
  );
  await page.keyboard.press("Escape");
  await page.waitForFunction(
    () => document.activeElement?.id === "app-root",
    null,
    { timeout: 10_000 },
  );
  results.push(check("closing history returns focus to the app", true));

  await setTheme(page, "dark");
  await page.getByRole("button", { name: "Note history" }).click();
  await page.locator(".history-panel").waitFor({ timeout: 10_000 });
  await settle(page);
  await audit("history, dark");
  await page.keyboard.press("Escape");
  await page.waitForFunction(
    () => document.activeElement?.id === "app-root",
    null,
    { timeout: 10_000 },
  );

  // ---- Command palette, both themes. ----
  await setTheme(page, "light");
  await page.keyboard.press("Control+p");
  await page.locator("#palette-input").waitFor({ timeout: 10_000 });
  await settle(page);
  await audit("command palette, light");
  const paletteTab = await tabStay(page, ".palette");
  const paletteBack = await tabStay(page, ".palette", "Shift+Tab", 5);
  results.push(
    check(
      "Tab stays inside the command palette",
      paletteTab.ok && paletteBack.ok,
      paletteTab.ok && paletteBack.ok
        ? ""
        : `escaped after ${paletteTab.ok ? paletteBack.press : paletteTab.press} presses`,
    ),
  );
  await page.keyboard.press("Escape");
  await page.waitForFunction(
    () => document.activeElement?.id === "app-root",
    null,
    { timeout: 10_000 },
  );
  results.push(check("closing the palette returns focus to the app", true));

  await setTheme(page, "dark");
  await page.keyboard.press("Control+p");
  await page.locator("#palette-input").waitFor({ timeout: 10_000 });
  await settle(page);
  await audit("command palette, dark");
  await page.keyboard.press("Escape");

  // ---- Shortcut sheet, opened from the palette. ----
  await setTheme(page, "light");
  await page.keyboard.press("Control+p");
  await page.locator("#palette-input").fill("keyboard shortcuts");
  await page.keyboard.press("Enter");
  await page.locator(".shortcuts-sheet").waitFor({ timeout: 10_000 });
  await settle(page);
  await audit("keyboard shortcuts, light");
  const sheetTab = await tabStay(page, ".shortcuts-sheet");
  const sheetBack = await tabStay(page, ".shortcuts-sheet", "Shift+Tab", 5);
  results.push(
    check(
      "Tab stays inside the shortcut sheet",
      sheetTab.ok && sheetBack.ok,
      sheetTab.ok && sheetBack.ok
        ? ""
        : `escaped after ${sheetTab.ok ? sheetBack.press : sheetTab.press} presses`,
    ),
  );
  await page.keyboard.press("Escape");
  await page.waitForFunction(
    () => document.activeElement?.id === "app-root",
    null,
    { timeout: 10_000 },
  );
  results.push(check("closing the shortcut sheet returns focus to the app", true));

  await setTheme(page, "dark");
  await page.keyboard.press("Control+p");
  await page.locator("#palette-input").fill("keyboard shortcuts");
  await page.keyboard.press("Enter");
  await page.locator(".shortcuts-sheet").waitFor({ timeout: 10_000 });
  await settle(page);
  await audit("keyboard shortcuts, dark");
  await page.keyboard.press("Escape");
  await page.waitForFunction(
    () => document.activeElement?.id === "app-root",
    null,
    { timeout: 10_000 },
  );

  // ---- Trash view, both themes. ----
  await setTheme(page, "light");
  await page.getByRole("button", { name: "Move note to trash" }).click();
  await page.locator(".toast", { hasText: "Moved to trash" }).waitFor();
  await page.locator(".sidebar-nav .nav-row", { hasText: "Trash" }).click();
  await page.locator(".note-card").first().waitFor();
  await settle(page);
  await audit("trash, light");

  await setTheme(page, "dark");
  await audit("trash, dark");
  await setTheme(page, "light");

  // ---- Settings, both themes, and autofocus. ----
  await page.locator(".sidebar-nav .nav-row", { hasText: "Notes" }).click();
  await page.locator(".nav-row", { hasText: "Settings" }).click();
  await page.getByRole("heading", { name: "Local data" }).waitFor();
  await settle(page);
  const settingsFocus = await page.evaluate(
    () => document.activeElement?.getAttribute("aria-label") === "Back to notes",
  );
  results.push(check("Settings focuses its back button", settingsFocus));
  await audit("settings, light");

  await setTheme(page, "dark");
  await audit("settings, dark");

  // ---- Login form and its first field. ----
  await setTheme(page, "light");
  await page.getByRole("button", { name: "Sign in to sync" }).click();
  await page.getByRole("heading", { name: "Sign in" }).waitFor();
  await settle(page);
  const loginFocus = await page.evaluate(
    () => document.activeElement?.getAttribute("autocomplete") === "username",
  );
  results.push(check("Login focuses the username field", loginFocus));
  await audit("login, light");
  await page.getByRole("button", { name: "Back to notes" }).click();
  await page.locator("#note-editor").waitFor();

  await page.locator(".nav-row", { hasText: "Settings" }).click();
  await page.getByRole("heading", { name: "Local data" }).waitFor();
  await setTheme(page, "dark");
  await page.getByRole("button", { name: "Sign in to sync" }).click();
  await page.getByRole("heading", { name: "Sign in" }).waitFor();
  await settle(page);
  await audit("login, dark");
  await page.getByRole("button", { name: "Back to notes" }).click();
  await page.locator("#note-editor").waitFor();
  await setTheme(page, "light");

  // ---- Mobile layout, both themes. ----
  await page.setViewportSize({ width: 390, height: 844 });
  await settle(page, 500);
  await audit("notes, mobile light");
  await page.getByRole("button", { name: "Open navigation" }).click();
  await page.locator(".sidebar.is-open").waitFor();
  await page.locator(".nav-row", { hasText: "Settings" }).click();
  await page.getByRole("heading", { name: "Local data" }).waitFor();
  await settle(page);
  await audit("settings, mobile light");

  await page.getByRole("button", { name: "Back to notes" }).click();
  await page.locator("#note-editor").waitFor();
  await setTheme(page, "dark");
  await audit("notes, mobile dark");
  await page.getByRole("button", { name: "Open navigation" }).click();
  await page.locator(".sidebar.is-open").waitFor();
  await page.locator(".nav-row", { hasText: "Settings" }).click();
  await page.getByRole("heading", { name: "Local data" }).waitFor();
  await settle(page);
  await audit("settings, mobile dark");

  // ---- Phone regression: long image titles, narrow chrome, one-tap Back. ----
  await page.getByRole("button", { name: "Back to notes" }).click();
  await page.locator("#note-editor").waitFor();
  await page.getByRole("button", { name: "Back to notes" }).click();
  await page.locator(".workspace.show-list").waitFor();
  const phoneFab = page.locator(".new-note-fab");
  results.push(
    check(
      "the phone note list shows a floating new-note button",
      await phoneFab.isVisible(),
    ),
  );
  await phoneFab.click();
  await page.locator("#note-editor").waitFor({ timeout: 10_000 });
  results.push(check("the floating button opens a new note", true));
  await page.getByRole("button", { name: "Back to notes" }).click();
  await page.locator(".workspace.show-list").waitFor();
  await page.getByRole("button", { name: "Open navigation" }).click();
  await page.locator(".sidebar.is-open").waitFor();
  await page.locator(".new-note-button").click();
  await page.locator("#note-editor").waitFor();
  const imageNote = `![image](data:image/png;base64,${"A".repeat(600)})`;
  await page.locator("#note-editor").fill(imageNote);
  await settle(page, 1200);
  await page.getByRole("button", { name: "Back to notes" }).click();
  await page.locator(".workspace.show-list").waitFor();
  await settle(page, 250);

  const imageTitle = page
    .locator(".note-card .note-title")
    .filter({ hasText: "image" })
    .first();
  const shownTitle = (await imageTitle.textContent())?.trim() ?? "";
  results.push(
    check(
      "image note shows a short title, not its data URL",
      shownTitle.length > 0 &&
        shownTitle.length < 40 &&
        !shownTitle.includes("data:image"),
      shownTitle,
    ),
  );

  for (const width of [390, 360, 320]) {
    await page.setViewportSize({ width, height: 780 });
    await settle(page, 300);
    const layout = await page.evaluate(() => {
      const list = document.querySelector(".note-list");
      const cards = [...document.querySelectorAll(".note-card")];
      return {
        documentWidth: document.documentElement.scrollWidth,
        viewport: window.innerWidth,
        listWidth: list?.clientWidth ?? 0,
        listScrollWidth: list?.scrollWidth ?? 0,
        listRight: list ? Math.ceil(list.getBoundingClientRect().right) : 0,
        cardRights: cards.map((card) =>
          Math.ceil(card.getBoundingClientRect().right),
        ),
      };
    });
    results.push(
      check(
        `note cards fit a ${width}px phone list`,
        layout.viewport === width &&
          layout.documentWidth <= width + 1 &&
          layout.listScrollWidth <= layout.listWidth + 1 &&
          layout.cardRights.every((right) => right <= layout.listRight + 1),
        JSON.stringify(layout),
      ),
    );
  }

  // The note list is also its scroll container. A short viewport makes it
  // overflow; the grid rows must still size to their cards, or flex children
  // shrink and the title clamp slices glyphs.
  await page.setViewportSize({ width: 360, height: 420 });
  await settle(page, 350);
  const overflowingCards = await page.evaluate(() => {
    const list = document.querySelector(".note-list");
    const cards = [...document.querySelectorAll(".note-card")];
    return {
      scrolls: Boolean(list && list.scrollHeight > list.clientHeight + 1),
      titles: cards.map((card) => {
        const title = card.querySelector(".note-title");
        return {
          text: title?.textContent?.trim().slice(0, 30) ?? "",
          height: title?.getBoundingClientRect().height ?? 0,
          line: title
            ? Number.parseFloat(getComputedStyle(title).lineHeight)
            : 0,
        };
      }),
    };
  });
  results.push(
    check(
      "cards keep full title lines when the phone list scrolls",
      overflowingCards.scrolls &&
        overflowingCards.titles.length > 0 &&
        overflowingCards.titles.every(
          (title) => title.line > 0 && title.height >= title.line - 0.5,
        ),
      JSON.stringify(overflowingCards),
    ),
  );

  // The compact list is a flex scroll container; rows must not shrink when
  // their content is taller than the viewport.
  await page.setViewportSize({ width: 1024, height: 420 });
  await settle(page, 300);
  await page.getByRole("button", { name: "List view" }).click();
  await settle(page, 300);
  const overflowingRows = await page.evaluate(() => {
    const list = document.querySelector(".note-list");
    return {
      scrolls: Boolean(list && list.scrollHeight > list.clientHeight + 1),
      rows: [...document.querySelectorAll(".note-card")].map((card) => {
        const title = card.querySelector(".note-title");
        return {
          height: card.getBoundingClientRect().height,
          titleHeight: title?.getBoundingClientRect().height ?? 0,
          line: title
            ? Number.parseFloat(getComputedStyle(title).lineHeight)
            : 0,
        };
      }),
    };
  });
  results.push(
    check(
      "note rows keep their title line when the list scrolls",
      overflowingRows.scrolls &&
        overflowingRows.rows.length > 0 &&
        overflowingRows.rows.every(
          (row) =>
            row.height > 0 && row.line > 0 && row.titleHeight >= row.line - 0.5,
        ),
      JSON.stringify(overflowingRows),
    ),
  );
  await page.getByRole("button", { name: "Card grid" }).click();
  await settle(page, 250);
  await page.setViewportSize({ width: 320, height: 780 });
  await settle(page, 300);

  const imageCard = page
    .locator(".note-card")
    .filter({ has: page.locator(".note-title", { hasText: /^image$/ }) })
    .first();
  await imageCard.click();
  await page.locator("#note-editor").waitFor();
  await settle(page, 250);

  const chrome = await page.evaluate(() => {
    const box = (element) => {
      if (!element) return null;
      const rect = element.getBoundingClientRect();
      return { left: rect.left, right: rect.right, width: rect.width };
    };
    const header = document.querySelector(".editor-header");
    const toolbar = document.querySelector(".editor-toolbar");
    const quote = [...toolbar.querySelectorAll("button")].find(
      (button) => button.getAttribute("aria-label") === "Quote",
    );
    return {
      viewport: window.innerWidth,
      headerWidth: header.clientWidth,
      headerScrollWidth: header.scrollWidth,
      back: box(document.querySelector(".back-button")),
      actions: box(document.querySelector(".editor-actions")),
      save: box(document.querySelector(".save-state")),
      toolbarWidth: toolbar.clientWidth,
      toolbarScrollWidth: toolbar.scrollWidth,
      quote: box(quote),
    };
  });
  results.push(
    check(
      "editor header fits a 320px phone",
      chrome.viewport === 320 &&
        chrome.headerScrollWidth <= chrome.headerWidth + 1 &&
        chrome.back?.left >= 0 &&
        chrome.actions?.right <= chrome.viewport + 1 &&
        chrome.save?.right <= chrome.viewport + 1,
      JSON.stringify(chrome),
    ),
  );
  results.push(
    check(
      "formatting toolbar scrolls instead of hiding controls",
      chrome.toolbarScrollWidth > chrome.toolbarWidth &&
        (chrome.quote?.width ?? 0) > 0,
      JSON.stringify({
        client: chrome.toolbarWidth,
        scroll: chrome.toolbarScrollWidth,
        quote: chrome.quote,
      }),
    ),
  );

  const quote = page.getByRole("button", { name: "Quote" });
  await quote.scrollIntoViewIfNeeded();
  const quoteBox = await quote.boundingBox();
  results.push(
    check(
      "Quote button is reachable at 320px",
      Boolean(
        quoteBox &&
          quoteBox.x >= -1 &&
          quoteBox.x + quoteBox.width <= 321,
      ),
      JSON.stringify(quoteBox),
    ),
  );

  // A soft keyboard shrinks the visible area without shrinking dvh on every
  // browser, so the shell follows --app-height: the app, its footer and the
  // tag field must stay inside the shrunken viewport with no page scroll.
  const keyboardShell = await page.evaluate(async () => {
    const root = document.documentElement;
    const previous = root.style.getPropertyValue("--app-height");
    root.style.setProperty("--app-height", "320px");
    await new Promise((resolve) =>
      requestAnimationFrame(() => requestAnimationFrame(resolve)),
    );
    const app = document.getElementById("app-root").getBoundingClientRect();
    const footer = document
      .querySelector(".editor-status")
      .getBoundingClientRect();
    // Fixed elements (the off-canvas sidebar, the scrim) are anchored to the
    // viewport and never extend the scrollable page, so their children are
    // excluded from "how far down does the document actually reach".
    const fixedNested = (element) => {
      for (
        let node = element;
        node && node !== document.body;
        node = node.parentElement
      ) {
        if (getComputedStyle(node).position === "fixed") return true;
      }
      return false;
    };
    const state = {
      height: Math.round(app.height),
      bottom: Math.round(app.bottom),
      footerBottom: Math.round(footer.bottom),
      bodyScrollHeight: document.body.scrollHeight,
      deepest: Math.round(
        Math.max(
          0,
          ...[...document.body.querySelectorAll("*")]
            .filter((element) => !fixedNested(element))
            .map((element) => element.getBoundingClientRect().bottom),
        ),
      ),
      background: getComputedStyle(document.documentElement).backgroundColor,
    };
    if (previous) root.style.setProperty("--app-height", previous);
    else root.style.removeProperty("--app-height");
    return state;
  });
  results.push(
    check(
      "a keyboard-sized viewport keeps the app and its tag field on screen",
      keyboardShell.height === 320 &&
        keyboardShell.bottom <= 321 &&
        keyboardShell.footerBottom <= 321 &&
        keyboardShell.bodyScrollHeight <= 321 &&
        keyboardShell.deepest <= 321 &&
        keyboardShell.background !== "rgb(255, 255, 255)",
      JSON.stringify(keyboardShell),
    ),
  );

  const tagEntry = page.locator(".tag-entry");
  const entryBox = await tagEntry.boundingBox();
  results.push(
    check(
      "a phone editor offers the add-tag field",
      (await tagEntry.isVisible()) &&
        Boolean(
          entryBox && entryBox.x >= -1 && entryBox.x + entryBox.width <= 321,
        ),
      JSON.stringify(entryBox),
    ),
  );

  await page.locator(".tag-input").fill("phone-tag");
  await settle(page, 150);
  await page.getByRole("button", { name: "Add tag" }).click();
  await page
    .locator(".editor-tag", { hasText: "phone-tag" })
    .waitFor({ timeout: 5_000 });
  results.push(check("the add-tag button tags the note on a phone", true));

  // The chip row folds away while the note body scrolls so the text gets the
  // height back, and returns when the note is back at its start.
  await page
    .locator("#note-editor")
    .fill(
      Array.from(
        { length: 40 },
        (_, index) => `Line ${index + 1} keeps the phone editor scrolling.`,
      ).join("\n"),
    );
  await settle(page, 300);
  const scrolledTop = await page.evaluate(async () => {
    const body = document.getElementById("note-editor");
    body.scrollTop = body.scrollHeight;
    await new Promise((resolve) =>
      requestAnimationFrame(() => requestAnimationFrame(resolve)),
    );
    return Math.round(body.scrollTop);
  });
  await settle(page, 300);
  const collapsedRow = await page.evaluate(() => {
    const row = document.querySelector(".editor-tags");
    return {
      height: row ? Number(row.getBoundingClientRect().height.toFixed(1)) : -1,
      visibility: row ? getComputedStyle(row).visibility : "missing",
    };
  });
  const chipHidden = !(await page
    .locator(".editor-tag", { hasText: "phone-tag" })
    .isVisible());
  results.push(
    check(
      "tag chips collapse when the note body scrolls",
      scrolledTop > 0 &&
        collapsedRow.height < 1 &&
        collapsedRow.visibility === "hidden" &&
        chipHidden,
      JSON.stringify({ scrolledTop, ...collapsedRow, chipHidden }),
    ),
  );

  const backToTop = await page.evaluate(async () => {
    const body = document.getElementById("note-editor");
    body.scrollTop = 0;
    await new Promise((resolve) =>
      requestAnimationFrame(() => requestAnimationFrame(resolve)),
    );
    return Math.round(body.scrollTop);
  });
  await settle(page, 300);
  const returnedRow = await page.locator(".editor-tags").boundingBox();
  const chipBack = await page
    .locator(".editor-tag", { hasText: "phone-tag" })
    .isVisible();
  results.push(
    check(
      "tag chips return at the top of the note",
      backToTop === 0 && chipBack && (returnedRow?.height ?? 0) > 8,
      JSON.stringify({ backToTop, row: returnedRow, chipBack }),
    ),
  );

  await page.getByRole("button", { name: "Remove tag phone-tag" }).click();
  await settle(page, 150);
  results.push(
    check(
      "the tag chip removes the tag again",
      (await page.locator(".editor-tag", { hasText: "phone-tag" }).count()) === 0,
    ),
  );

  await page.getByRole("button", { name: "Note colour" }).click();
  await page.locator(".color-popover").waitFor();
  const menuBox = await page.locator(".color-popover").boundingBox();
  results.push(
    check(
      "colour menu stays inside a 320px viewport",
      Boolean(
        menuBox &&
          menuBox.x >= 0 &&
          menuBox.x + menuBox.width <= 321 &&
          menuBox.y >= 0 &&
          menuBox.y + menuBox.height <= 781,
      ),
      JSON.stringify(menuBox),
    ),
  );

  const backButton = page.getByRole("button", { name: "Back to notes" });
  const backBox = await backButton.boundingBox();
  if (backBox) {
    await page.mouse.click(
      backBox.x + backBox.width / 2,
      backBox.y + backBox.height / 2,
    );
    await settle(page, 350);
  }
  const workspaceClass =
    (await page.locator(".workspace").getAttribute("class")) ?? "";
  results.push(
    check(
      "one tap on Back leaves the editor with the colour menu open",
      workspaceClass.includes("show-list"),
      workspaceClass,
    ),
  );

  await page.getByRole("button", { name: "Open settings" }).click();
  await page.getByRole("heading", { name: "Local data" }).waitFor();
  await page.getByRole("button", { name: "Back to notes" }).click();
  await page.locator("#note-editor").waitFor();
  await page.getByRole("button", { name: "Back to notes" }).click();
  await page.locator(".workspace.show-list").waitFor();
  await page.getByRole("button", { name: "Open navigation" }).click();
  await page.locator(".sidebar.is-open").waitFor();
  results.push(check("phone Settings and navigation open", true));
  // At 320 px the open drawer covers the scrim's centre, so tap the exposed
  // strip on the right rather than letting Playwright aim at the hidden middle.
  const scrimBox = await page.locator(".scrim").boundingBox();
  if (scrimBox) {
    await page.mouse.click(
      scrimBox.x + scrimBox.width - 12,
      scrimBox.y + scrimBox.height / 2,
    );
  }
  await settle(page, 200);

  await page.screenshot({ path: path.join(tmpdir(), "notebox-a11y.png") });
} catch (error) {
  results.push(check("accessibility run completed", false, String(error)));
  if (context) {
    const page = context.pages()[0];
    if (page) await page.screenshot({ path: failureShot }).catch(() => {});
  }
} finally {
  results.push(
    check("no console errors", errors.length === 0, errors.slice(0, 5).join(" | ")),
  );
  if (context) await context.close().catch(() => {});
  if (browser) await browser.close().catch(() => {});
  if (server) server.close();
  console.log(JSON.stringify({ results, errors }, null, 2));
  process.exit(results.every((result) => result.ok) ? 0 : 1);
}
