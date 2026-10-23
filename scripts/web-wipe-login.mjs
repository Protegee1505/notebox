#!/usr/bin/env node
// P8: wipe a browser device, sign in to a local Worker, and get the notes and
// their history back.
//
// It boots `wrangler dev --local` with both D1 databases on a throwaway
// persistence directory, serves the release web build, and drives Chromium:
//
// 1. Create a tagged, pinned note and attach a small image.
// 2. Sign in. Time three rejected attempts to record the browser's Argon2id
//    login key, capture the key from the first request, create the one account
//    with the setup token, then sign in for real and wait for the first sync.
// 3. Open a second browser profile, sign in to the same account, append a line
//    to the note, and wait for the first window to show it without a reload.
// 4. Close the browser, delete the OPFS database, localStorage, cookies and
//    service-worker caches. The next launch is a wiped device.
// 5. Confirm the note is gone, sign in again with the same password, and
//    find the note, its tag, its pin, its image and history versions produced
//    on the other devices. The first attachment download must not crash the
//    wasm build.
//
//   PLAYWRIGHT_MODULES=/path/to/node_modules \
//   node scripts/web-wipe-login.mjs
//
// The web release must already be built at
// target/dx/notebox/release/web/public (./scripts/web-build.sh).

import { execFile, spawn } from "node:child_process";
import { createRequire } from "node:module";
import { mkdtemp, readFile, rm } from "node:fs/promises";
import { homedir, tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { promisify } from "node:util";

const run = promisify(execFile);
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const dist = path.join(root, "target/dx/notebox/release/web/public");
const WRANGLER = ["--yes", "wrangler@4.151.2"];
const PORT = Number(process.env.PORT || 8797);
const SETUP_TOKEN = "wipe-e2e-setup-token";
const BASE = `http://127.0.0.1:${PORT}`;
const USERNAME = "wipe-e2e@notebox";
const PASSWORD = "correct horse battery staple";
const NEW_USERNAME = "wipe-e2e@notebox-2";
const NEW_PASSWORD = "a second correct horse battery staple";
const marker = `Wipe-${Date.now()}`;
const title = `${marker} note`;
const body = `${title}\n\nThis note and its history must survive a wipe.`;
const tag = "wipe-tag";
// A valid 1x1 PNG; the app re-encodes it as WebP before syncing.
const DOT_PNG =
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=";
const failureShot = path.join(tmpdir(), "notebox-web-wipe-failure.png");

const env = {
  ...process.env,
  PATH: `${path.join(homedir(), ".cargo", "bin")}${path.delimiter}${process.env.PATH ?? ""}`,
};

let worker;
let serverOutput = "";
let browser;
let profile;
let context;
let secondProfile;
let secondContext;
let persist;
// The first three sign-ins are attempted before the account exists so the test
// can time the derived login key. Chrome logs each expected 401.
let allowedAuthFailures = 0;

function check(name, ok, detail = "") {
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${detail ? ` — ${detail}` : ""}`);
  return { name, ok: Boolean(ok), detail };
}

async function waitForWorker() {
  for (let attempt = 0; attempt < 240; attempt += 1) {
    if (worker && worker.exitCode !== null) {
      throw new Error(`wrangler exited with ${worker.exitCode}`);
    }
    try {
      const response = await fetch(`${BASE}/`);
      if (response.ok) return;
    } catch {
      // Not listening yet.
    }
    await new Promise((resolve) => setTimeout(resolve, 1_000));
  }
  throw new Error("wrangler dev did not start within 240 seconds");
}

async function migrate(name) {
  const { stderr } = await run(
    "npx",
    [
      ...WRANGLER,
      "d1",
      "migrations",
      "apply",
      name,
      "--local",
      "--persist-to",
      persist,
    ],
    { env, maxBuffer: 20 * 1024 * 1024 },
  );
  if (stderr.includes("ERROR")) {
    throw new Error(`migrations for ${name} failed:\n${stderr}`);
  }
}

async function startWorker() {
  persist = await mkdtemp(path.join(tmpdir(), "notebox-wipe-e2e-"));
  await migrate("notebox-sync");
  await migrate("notebox-blobs");
  worker = spawn(
    "npx",
    [
      ...WRANGLER,
      "dev",
      "--local",
      "--port",
      String(PORT),
      "--persist-to",
      persist,
      "--var",
      `SETUP_TOKEN:${SETUP_TOKEN}`,
    ],
    {
      cwd: root,
      env,
      detached: true,
      stdio: ["ignore", "pipe", "pipe"],
    },
  );
  worker.stdout.on("data", (chunk) => {
    serverOutput += chunk;
  });
  worker.stderr.on("data", (chunk) => {
    serverOutput += chunk;
  });
  await waitForWorker();
}

async function stopWorker() {
  if (!worker || worker.exitCode !== null) return;
  try {
    process.kill(-worker.pid, "SIGTERM");
  } catch {
    worker.kill("SIGTERM");
  }
  await new Promise((resolve) => {
    const timer = setTimeout(resolve, 5_000);
    worker.once("exit", () => {
      clearTimeout(timer);
      resolve();
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

function watch(page, errors) {
  page.on("pageerror", (error) => errors.push(String(error)));
  page.on("console", (message) => {
    if (message.type() !== "error" || message.text().includes("_dioxus")) {
      return;
    }
    if (allowedAuthFailures > 0 && message.text().includes("401 (Unauthorized)")) {
      allowedAuthFailures -= 1;
      return;
    }
    errors.push(`console: ${message.text()}`);
  });
}

async function openApp(errors) {
  context = await browser.launchPersistentContext(profile, {
    headless: true,
    viewport: { width: 1400, height: 900 },
  });
  const page = context.pages()[0] ?? (await context.newPage());
  watch(page, errors);
  await page.goto(BASE, { waitUntil: "networkidle" });
  const offline = page.getByRole("button", { name: /Try it offline/ });
  if (await offline.isVisible().catch(() => false)) {
    await offline.click();
  }
  await page.locator(".workspace").waitFor({ state: "visible", timeout: 60_000 });
  return page;
}

async function settingsSection(page, name) {
  const indexRow = page.locator(".settings-index-row", { hasText: name });
  if (await indexRow.isVisible().catch(() => false)) {
    await indexRow.click();
  } else {
    await page.locator(".settings-nav-item", { hasText: name }).click();
  }
  await page.getByRole("heading", { name, exact: true }).waitFor();
}

async function openSettings(page, name = "Account") {
  const pane = page.locator(".settings-pane");
  if (!(await pane.isVisible().catch(() => false))) {
    await page.locator(".nav-row", { hasText: "Settings" }).click();
    await pane.waitFor();
  }
  await settingsSection(page, name);
}

async function openLogin(page) {
  await openSettings(page);
  await page.getByRole("button", { name: "Sign in to sync" }).click();
  await page.getByRole("heading", { name: "Sign in" }).waitFor();
}

async function fillLogin(page, username, password) {
  await page.getByLabel("Username").fill(username);
  await page.getByLabel("Password").fill(password);
}

async function createAccount(loginKey) {
  const response = await fetch(`${BASE}/v1/auth/setup`, {
    method: "POST",
    headers: {
      "content-type": "application/json",
      "x-setup-token": SETUP_TOKEN,
    },
    body: JSON.stringify({
      version: 1,
      payload: {
        username: USERNAME,
        login_key: loginKey,
        device_name: "Setup helper",
      },
    }),
  });
  if (!response.ok) {
    throw new Error(`setup failed with ${response.status}: ${await response.text()}`);
  }
}

async function waitForSynced(page) {
  await page.waitForFunction(
    () => document.querySelector(".status-pill")?.textContent?.trim() === "Synced",
    null,
    { timeout: 120_000 },
  );
}

async function waitForMarker(page) {
  await page.locator(".note-card", { hasText: title }).first().waitFor({
    timeout: 120_000,
  });
}

async function wipeDevice(page) {
  // version.json is same-origin and is not handled by the service worker, so
  // it gives a page that can clear storage without booting the app.
  await page.goto(`${BASE}/version.json`, { waitUntil: "networkidle" });
  await page.evaluate(async () => {
    localStorage.clear();
    sessionStorage.clear();
    const root = await navigator.storage.getDirectory();
    for await (const [name] of root.entries()) {
      await root.removeEntry(name, { recursive: true });
    }
    for (const key of await caches.keys()) {
      await caches.delete(key);
    }
  });
  await context.clearCookies();
}

const results = [];
const errors = [];
try {
  await readFile(path.join(dist, "index.html"));
  await readFile(path.join(dist, "version.json"));
  const playwright = await loadPlaywright();
  browser = playwright.chromium;
  profile = await mkdtemp(path.join(tmpdir(), "notebox-wipe-profile-"));
  await startWorker();

  // ---- Device 1: make a note, create the account, and sync it. ----
  let page = await openApp(errors);
  await page.locator(".new-note-button").click();
  await page.waitForTimeout(400);
  await page.locator("#note-editor").fill(body);
  await page.waitForTimeout(500);
  await page.locator(".tag-input").fill(tag);
  await page.keyboard.press("Enter");
  await page.locator(".editor-tag", { hasText: tag }).waitFor();
  await page.getByRole("button", { name: "Pin note" }).click();
  await page.getByRole("button", { name: "Unpin note" }).waitFor();
  await page.waitForFunction(
    () => !document.querySelector(".save-state")?.textContent?.includes("Unsaved"),
    null,
    { timeout: 30_000 },
  );
  await page.waitForTimeout(800);

  // Attach a real image, so the second device downloads an attachment on its
  // first sync. That download used to trap the wasm build in load_attachments.
  await page.getByLabel("Image attachments").setInputFiles({
    name: "dot.png",
    mimeType: "image/png",
    buffer: Buffer.from(DOT_PNG, "base64"),
  });
  await page.locator(".status-attachments").waitFor({ timeout: 60_000 });
  await page.locator(".status-attachments").click();
  await page.locator(".attachment-row").first().waitFor({ timeout: 60_000 });
  await page.waitForFunction(
    () => !document.querySelector(".attachment-working"),
    null,
    { timeout: 60_000 },
  );
  await page.waitForTimeout(800);
  results.push(check("the first device attached an image", true));

  await openLogin(page);
  allowedAuthFailures = 3;
  // Three wrong-account attempts: the account does not exist yet, so the
  // server rejects each one without a backoff. Timing click-to-response
  // measures the WASM Argon2id derivation plus the local Worker round trip.
  const loginTimes = [];
  let loginKey;
  for (let attempt = 0; attempt < 3; attempt += 1) {
    // submit_login clears the password after each attempt; fill it again.
    await fillLogin(page, USERNAME, PASSWORD);
    const started = performance.now();
    const responsePromise = page.waitForResponse(
      (response) =>
        response.request().method() === "POST" &&
        response.url().endsWith("/v1/auth/login"),
    );
    await page.getByRole("button", { name: "Connect" }).click();
    const response = await responsePromise;
    loginTimes.push(performance.now() - started);
    const loginBody = response.request().postDataJSON();
    loginKey ??= loginBody?.payload?.login_key;
    await page.locator(".form-error").waitFor({ timeout: 30_000 });
    await page.waitForTimeout(150);
  }
  const loginMedian = [...loginTimes].sort((a, b) => a - b)[1];
  results.push(
    check(
      "the app sends a derived login key, not the password",
      typeof loginKey === "string" && loginKey.length > 20,
      String(loginKey).slice(0, 12),
    ),
  );
  results.push(
    check(
      "browser login key derivation stays under 3 s",
      loginMedian < 3000,
      `${loginTimes.map((ms) => ms.toFixed(0)).join("/")} ms, median ${loginMedian.toFixed(0)} ms`,
    ),
  );
  await createAccount(loginKey);
  await fillLogin(page, USERNAME, PASSWORD);
  await page.getByRole("button", { name: "Connect" }).click();
  await waitForSynced(page);
  results.push(check("first device reached the Worker", true));

  // ---- A second live device edits the note; the first must update itself. ----
  await page.locator(".sidebar-nav .nav-row", { hasText: "Notes" }).click();
  await page.locator(".note-card", { hasText: title }).first().click();
  await page.waitForFunction(
    (expected) => document.getElementById("note-editor")?.value.includes(expected),
    title,
    { timeout: 30_000 },
  );

  secondProfile = await mkdtemp(path.join(tmpdir(), "notebox-wipe-profile-b-"));
  secondContext = await browser.launchPersistentContext(secondProfile, {
    headless: true,
    viewport: { width: 1400, height: 900 },
  });
  const otherPage = secondContext.pages()[0] ?? (await secondContext.newPage());
  watch(otherPage, errors);
  await otherPage.goto(BASE, { waitUntil: "networkidle" });
  await otherPage
    .getByRole("heading", { name: "Welcome to Notebox" })
    .waitFor({ timeout: 60_000 });
  await fillLogin(otherPage, USERNAME, PASSWORD);
  await otherPage.getByRole("button", { name: "Connect" }).click();
  await waitForMarker(otherPage);
  await waitForSynced(otherPage);
  await otherPage.locator(".note-card", { hasText: title }).first().click();
  await otherPage.locator("#note-editor").waitFor();

  const pushed = otherPage.waitForResponse(
    (response) => {
      if (
        response.request().method() !== "POST" ||
        !response.url().endsWith("/v1/sync")
      ) {
        return false;
      }
      try {
        return (
          (response.request().postDataJSON()?.payload?.updates?.length ?? 0) > 0
        );
      } catch {
        return false;
      }
    },
    { timeout: 60_000 },
  );
  const current = await otherPage.locator("#note-editor").inputValue();
  await otherPage
    .locator("#note-editor")
    .fill(`${current}\n\nEdited on the other device.`);
  await otherPage.waitForFunction(
    () => !document.querySelector(".save-state")?.textContent?.includes("Unsaved"),
    null,
    { timeout: 30_000 },
  );
  await pushed;
  await page.waitForFunction(
    (expected) => document.getElementById("note-editor")?.value.includes(expected),
    "Edited on the other device.",
    { timeout: 45_000 },
  );
  results.push(
    check("a change from another device appears without a reload", true),
  );

  await secondContext.close();
  secondContext = undefined;
  await rm(secondProfile, { recursive: true, force: true });
  secondProfile = undefined;

  // ---- Wipe the local database and the saved session. ----
  await context.close();
  context = undefined;
  context = await browser.launchPersistentContext(profile, {
    headless: true,
    viewport: { width: 1400, height: 900 },
  });
  const wiped = context.pages()[0] ?? (await context.newPage());
  watch(wiped, errors);
  await wipeDevice(wiped);
  await context.close();
  context = undefined;

  // ---- Device 2: the same profile, now empty. Sign in and pull. ----
  page = await openApp(errors);
  const before = await page.locator(".note-card", { hasText: title }).count();
  results.push(
    check("the wipe removed the local note", before === 0, `${before} found`),
  );

  await openLogin(page);
  await fillLogin(page, USERNAME, PASSWORD);
  await page.getByRole("button", { name: "Connect" }).click();
  await waitForMarker(page);
  await waitForSynced(page);
  await page.waitForTimeout(1500);
  const crash = page.locator("#notebox-crash");
  results.push(
    check(
      "the first attachment download did not crash the app",
      !(await crash.isVisible()),
      (await crash.innerText()).slice(0, 120),
    ),
  );
  results.push(check("signing in pulled the note back", true));
  results.push(
    check(
      "the note is not duplicated",
      (await page.locator(".note-card", { hasText: title }).count()) === 1,
    ),
  );

  await page.locator(".note-card", { hasText: title }).first().click();
  await page.waitForTimeout(600);
  const text = await page.locator("#note-editor").inputValue();
  results.push(
    check(
      "the note body came back",
      text.includes(title) && text.includes("survive a wipe"),
      text.slice(0, 160),
    ),
  );
  results.push(
    check(
      "the tag came back",
      (await page.locator(".editor-tag", { hasText: tag }).count()) === 1,
    ),
  );
  await page.locator(".status-attachments").click();
  await page.locator(".attachment-row").first().waitFor({ timeout: 30_000 });
  const attachmentText = await page.locator(".attachment-strip").innerText();
  results.push(
    check(
      "the image attachment came back with its bytes",
      !attachmentText.includes("not downloaded") &&
        !attachmentText.includes("Waiting"),
      attachmentText.slice(0, 120),
    ),
  );
  results.push(
    check(
      "the pin came back",
      (await page.getByRole("button", { name: "Unpin note" }).count()) === 1,
    ),
  );

  await page.getByRole("button", { name: "Note history" }).click();
  await page.locator(".history-panel").waitFor({ timeout: 10_000 });
  const versions = await page.locator(".history-version").count();
  const historyText = await page.locator(".history-panel").innerText();
  results.push(
    check(
      "the history came back from the other device",
      versions >= 1 && /Device [0-9a-f]{4}/.test(historyText),
      `${versions} version row(s): ${historyText.slice(0, 160)}`,
    ),
  );
  results.push(
    check(
      "the history is not relabelled as this device",
      !historyText.includes("This device"),
      historyText.slice(0, 160),
    ),
  );
  await page.getByRole("button", { name: "Close history" }).click();

  // ---- Change both credentials, sign out, and sign in with the new pair. ----
  await openSettings(page);
  await page.getByRole("button", { name: "Change username or password" }).click();
  await page.locator("form.credentials-form").waitFor({ timeout: 10_000 });
  results.push(check("the credential form opens with labelled fields", true));
  await page.getByLabel("Current password").fill(PASSWORD);
  await page.getByLabel("Username").fill(NEW_USERNAME);
  await page.getByLabel("New password (optional)").fill(NEW_PASSWORD);
  await page.getByLabel("Confirm new password").fill(NEW_PASSWORD);
  await page.getByRole("button", { name: "Update username or password" }).click();
  await page
    .locator(".toast.is-success", { hasText: "Sign-in details updated" })
    .waitFor({ timeout: 30_000 });
  await page.locator(".account-copy strong", { hasText: NEW_USERNAME }).waitFor();
  results.push(check("the account shows the new username", true));

  const loggedOut = page.waitForResponse(
    (response) =>
      response.request().method() === "POST" &&
      response.url().endsWith("/v1/auth/logout"),
  );
  await page.getByRole("button", { name: "Sign out" }).click();
  await loggedOut;
  await page.getByRole("button", { name: "Sign in to sync" }).waitFor();
  await page.getByRole("button", { name: "Sign in to sync" }).click();
  await page.getByRole("heading", { name: "Sign in" }).waitFor();
  await fillLogin(page, NEW_USERNAME, NEW_PASSWORD);
  await page.getByRole("button", { name: "Connect" }).click();
  await waitForSynced(page);
  await waitForMarker(page);
  results.push(check("the new username and password sign in", true));
  results.push(
    check(
      "the note is still there after the credential change",
      (await page.locator(".note-card", { hasText: title }).count()) === 1,
    ),
  );

  await page.screenshot({ path: path.join(tmpdir(), "notebox-web-wipe.png") });
  await context.close();
  context = undefined;
} catch (error) {
  results.push(check("flow completed", false, String(error)));
  if (context) {
    const page = context.pages()[0];
    if (page) {
      await page.screenshot({ path: failureShot }).catch(() => {});
    }
  }
  if (serverOutput) {
    console.error(serverOutput.split("\n").slice(-40).join("\n"));
  }
} finally {
  results.push(
    check("no console errors", errors.length === 0, errors.slice(0, 5).join(" | ")),
  );
  if (secondContext) await secondContext.close().catch(() => {});
  if (context) await context.close().catch(() => {});
  await stopWorker().catch(() => {});
  if (secondProfile) await rm(secondProfile, { recursive: true, force: true });
  if (profile) await rm(profile, { recursive: true, force: true });
  if (persist) await rm(persist, { recursive: true, force: true });
  console.log(JSON.stringify({ results, errors }, null, 2));
  process.exit(results.every((result) => result.ok) ? 0 : 1);
}
