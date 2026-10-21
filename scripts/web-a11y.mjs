#!/usr/bin/env node
// P8: WCAG A/AA audit of the release UI.
//
// Serves the release web build, drives the real app with Playwright and runs
// axe-core on every main screen in light and dark themes. It also checks the
// keyboard path: note-list arrow keys, focus return after closing a panel, and
// autofocus on Settings and Login.
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
  await page.locator("#note-editor").waitFor({ timeout: 60_000 });
  await settle(page, 600);
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
  await setTheme(page, "light");
  await audit("notes, light");

  await setTheme(page, "dark");
  await audit("notes, dark");

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
  await page.keyboard.press("Control+k");
  await page.locator("#palette-input").waitFor({ timeout: 10_000 });
  await settle(page);
  await audit("command palette, light");
  await page.keyboard.press("Escape");
  await page.waitForFunction(
    () => document.activeElement?.id === "app-root",
    null,
    { timeout: 10_000 },
  );
  results.push(check("closing the palette returns focus to the app", true));

  await setTheme(page, "dark");
  await page.keyboard.press("Control+k");
  await page.locator("#palette-input").waitFor({ timeout: 10_000 });
  await settle(page);
  await audit("command palette, dark");
  await page.keyboard.press("Escape");

  // ---- Shortcut sheet, opened from the palette. ----
  await setTheme(page, "light");
  await page.keyboard.press("Control+k");
  await page.locator("#palette-input").fill("keyboard shortcuts");
  await page.keyboard.press("Enter");
  await page.locator(".shortcuts-sheet").waitFor({ timeout: 10_000 });
  await settle(page);
  await audit("keyboard shortcuts, light");
  await page.keyboard.press("Escape");
  await page.waitForFunction(
    () => document.activeElement?.id === "app-root",
    null,
    { timeout: 10_000 },
  );
  results.push(check("closing the shortcut sheet returns focus to the app", true));

  await setTheme(page, "dark");
  await page.keyboard.press("Control+k");
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
