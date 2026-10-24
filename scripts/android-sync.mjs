#!/usr/bin/env node
// Drive the debug app on an emulator through a real native sign-in and sync.
//
// A local Worker answers on 8788. A loopback proxy on 8787 bootstraps the
// account from the first login the app sends, so the host never has to derive
// Argon2id. `adb reverse` makes that proxy reachable at 127.0.0.1:8787 on the
// device, which is the only cleartext origin the generated network security
// config allows.

import { execFileSync, spawn, spawnSync } from "node:child_process";
import { appendFileSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { homedir, tmpdir } from "node:os";
import { join, resolve } from "node:path";

const PACKAGE = process.env.PACKAGE || "app.notebox";
const ACTIVITY = process.env.ACTIVITY || "dev.dioxus.main.MainActivity";
const WORKER_PORT = Number(process.env.WORKER_PORT || 8788);
const PROXY_PORT = Number(process.env.PROXY_PORT || 8787);
const CDP_PORT = Number(process.env.CDP_PORT || 9222);
const SETUP_TOKEN = process.env.SETUP_TOKEN || "android-e2e-setup-token";
const ARTIFACTS_DIR = resolve(process.env.ARTIFACTS_DIR || ".");
const APK = process.env.APK || "";
const USERNAME = process.env.E2E_USERNAME || "android-e2e";
const PASSWORD = process.env.E2E_PASSWORD || "android emulator drill password";
const KEEP_E2E_DIR = process.env.KEEP_E2E_DIR === "1";
const WRANGLER = ["--yes", "wrangler@4.151.2"];
const WORKER_BASE = `http://127.0.0.1:${WORKER_PORT}`;
const PROTOCOL_VERSION = 1;
const MARKER = `android-sync-${Date.now().toString(36)}`;
const NOTE_TEXT = `Android sync drill\n\n${MARKER}`;

const logFile = join(ARTIFACTS_DIR, "android-sync.log");
const workerLog = join(ARTIFACTS_DIR, "android-sync-worker.log");
const proxyLog = join(ARTIFACTS_DIR, "android-sync-proxy.log");
const reportFile = join(ARTIFACTS_DIR, "android-sync-report.json");

let worker = null;
let workerExit = null;
let proxyServer = null;
let cdp = null;
let persist = "";

const state = {
  setupStatus: null,
  loginStatus: null,
  syncRequests: 0,
  syncCompleted: 0,
  capture: null,
};

function log(message) {
  const line = `[android-sync] ${message}`;
  console.log(line);
  try {
    appendFileSync(logFile, `${line}\n`);
  } catch {
    // The artifacts directory is created before the first log in main().
  }
}

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function fail(message) {
  throw new Error(message);
}

function adb(args, options = {}) {
  return execFileSync("adb", args, {
    encoding: "utf8",
    timeout: options.timeout || 120_000,
    maxBuffer: options.maxBuffer || 32 * 1024 * 1024,
    stdio: ["ignore", "pipe", "pipe"],
  }).trim();
}

function preflight() {
  try {
    adb(["version"]);
  } catch (error) {
    fail(`adb is not available: ${error.message}`);
  }
  if (typeof WebSocket === "undefined") {
    fail("this Node has no global WebSocket; run Node 22 or newer");
  }
  const online = adb(["devices"])
    .split("\n")
    .slice(1)
    .map((line) => line.trim())
    .filter((line) => /\tdevice$/.test(line));
  if (online.length !== 1) {
    fail(`expected exactly one online device, found ${online.length}: ${online.join(", ")}`);
  }
  log(`device: ${online[0]}`);
}

function prepareApp() {
  if (APK) {
    log(`installing ${APK}`);
    const installed = adb(["install", "-r", APK], { timeout: 300_000 });
    if (!installed.includes("Success")) {
      fail(`apk install did not report Success: ${installed}`);
    }
  }
  log("clearing app data");
  const cleared = adb(["shell", "pm", "clear", PACKAGE]);
  if (!cleared.includes("Success")) {
    fail(`pm clear did not report Success: ${cleared}`);
  }
}

function runChecked(command, args, env) {
  const result = spawnSync(command, args, {
    encoding: "utf8",
    env,
    timeout: 240_000,
    maxBuffer: 32 * 1024 * 1024,
  });
  if (result.status !== 0) {
    fail(
      `${command} ${args.join(" ")} failed (${result.status}):\n` +
        `${(result.stderr || "").slice(-2000)}\n${(result.stdout || "").slice(-2000)}`,
    );
  }
  return result;
}

function workerEnv() {
  const cargoHome = process.env.CARGO_HOME || join(homedir(), ".cargo");
  return {
    ...process.env,
    PATH: `${join(cargoHome, "bin")}:${process.env.PATH}`,
  };
}

function startWorker() {
  persist = mkdtempSync(join(tmpdir(), "notebox-android-sync-"));
  mkdirSync("target/dx/notebox/release/web/public", { recursive: true });
  const env = workerEnv();

  log(`applying migrations in ${persist}`);
  for (const database of ["notebox-sync", "notebox-blobs"]) {
    runChecked(
      "npx",
      [...WRANGLER, "d1", "migrations", "apply", database, "--local", "--persist-to", persist],
      env,
    );
  }

  log(`starting wrangler dev on ${WORKER_PORT}`);
  worker = spawn(
    "npx",
    [
      ...WRANGLER,
      "dev",
      "--local",
      "--ip",
      "127.0.0.1",
      "--port",
      String(WORKER_PORT),
      "--persist-to",
      persist,
      "--var",
      `SETUP_TOKEN:${SETUP_TOKEN}`,
    ],
    { env, detached: true, stdio: ["ignore", "pipe", "pipe"] },
  );
  worker.stdout.on("data", (chunk) => appendFileSync(workerLog, chunk));
  worker.stderr.on("data", (chunk) => appendFileSync(workerLog, chunk));
  worker.on("error", (error) => {
    workerExit = -1;
    appendFileSync(workerLog, `spawn error: ${error.message}\n`);
  });
  worker.on("exit", (code) => {
    workerExit = code;
  });
}

async function waitForWorker(timeoutMs = 600_000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (workerExit !== null) {
      fail(`wrangler exited with ${workerExit}; see ${workerLog}`);
    }
    try {
      const response = await fetch(`${WORKER_BASE}/`, { signal: AbortSignal.timeout(3000) });
      if (response.ok) {
        log("worker is ready");
        return;
      }
    } catch {
      // Not listening yet.
    }
    await sleep(1000);
  }
  fail(`wrangler dev did not answer within ${Math.round(timeoutMs / 1000)} seconds`);
}

async function forward(base, path, method, headers, body) {
  const forwarded = { ...headers };
  for (const name of [
    "host",
    "connection",
    "content-length",
    "transfer-encoding",
    "accept-encoding",
    "keep-alive",
  ]) {
    delete forwarded[name];
  }
  const response = await fetch(`${base}${path}`, {
    method,
    headers: forwarded,
    body: body && body.length > 0 ? body : undefined,
    redirect: "manual",
    signal: AbortSignal.timeout(60_000),
  });
  const bytes = Buffer.from(await response.arrayBuffer());
  const responseHeaders = {};
  for (const [name, value] of response.headers.entries()) {
    if (!["content-encoding", "content-length", "transfer-encoding", "connection"].includes(name)) {
      responseHeaders[name] = value;
    }
  }
  return { status: response.status, headers: responseHeaders, body: bytes };
}

async function bootstrapAccount(body) {
  if (state.setupStatus !== null) return;
  let payload;
  try {
    payload = JSON.parse(body.toString("utf8")).payload;
  } catch {
    fail("the app sent a login body that is not JSON");
  }
  if (!payload?.username || !payload?.login_key) {
    fail("the login body has no username or login_key");
  }
  state.capture = {
    username: payload.username,
    login_key: payload.login_key,
    device_name: payload.device_name,
  };
  log(`bootstrapping the account for ${payload.username}`);
  const setup = await forward(
    WORKER_BASE,
    "/v1/auth/setup",
    "POST",
    { "content-type": "application/json", "x-setup-token": SETUP_TOKEN },
    body,
  );
  state.setupStatus = setup.status;
  if (setup.status === 409) {
    log("the account already exists; continuing with login");
    return;
  }
  if (setup.status < 200 || setup.status >= 300) {
    fail(`account setup returned ${setup.status}: ${setup.body.toString("utf8").slice(0, 300)}`);
  }
  log(`account created (${setup.status})`);
}

function startProxy() {
  proxyServer = createServer(async (request, response) => {
    const chunks = [];
    for await (const chunk of request) chunks.push(chunk);
    const body = Buffer.concat(chunks);
    const path = request.url || "/";

    try {
      appendFileSync(proxyLog, `${request.method} ${path} -> ${body.length} bytes\n`);
      if (request.method === "POST" && path.startsWith("/v1/auth/login")) {
        await bootstrapAccount(body);
      }
      const upstream = await forward(WORKER_BASE, path, request.method, request.headers, body);
      if (request.method === "POST" && path.startsWith("/v1/auth/login")) {
        state.loginStatus = upstream.status;
      }
      if (request.method === "POST" && path.startsWith("/v1/sync")) {
        state.syncRequests += 1;
        if (upstream.status >= 200 && upstream.status < 300) state.syncCompleted += 1;
      }
      response.writeHead(upstream.status, upstream.headers);
      response.end(upstream.body);
    } catch (error) {
      appendFileSync(proxyLog, `ERROR ${path}: ${error.message}\n`);
      response.writeHead(502, { "content-type": "application/json" });
      response.end(JSON.stringify({ error: error.message }));
    }
  });
  return new Promise((resolve, reject) => {
    proxyServer.once("error", reject);
    proxyServer.listen(PROXY_PORT, "127.0.0.1", resolve);
  });
}

async function launchApp() {
  adb(["shell", "am", "force-stop", PACKAGE]);
  log(`starting ${PACKAGE}/${ACTIVITY}`);
  const output = adb(["shell", "am", "start", "-W", "-n", `${PACKAGE}/${ACTIVITY}`], {
    timeout: 180_000,
  });
  log(output.split("\n").filter(Boolean).slice(-3).join(" | "));

  const deadline = Date.now() + 60_000;
  let pid = "";
  while (Date.now() < deadline) {
    pid = adb(["shell", "pidof", PACKAGE]).split(/\s+/)[0] || "";
    if (pid) break;
    await sleep(500);
  }
  if (!pid) fail("the app process did not start");
  log(`app pid ${pid}`);
  return pid;
}

async function waitForDevtoolsSocket(pid, timeoutMs = 60_000) {
  const name = `webview_devtools_remote_${pid}`;
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    let sockets = "";
    try {
      sockets = adb(["shell", "cat", "/proc/net/unix"]);
    } catch {
      // Retry.
    }
    if (sockets.includes(name)) {
      log(`devtools socket ${name}`);
      return;
    }
    await sleep(500);
  }
  fail(`no ${name} socket; is the WebView debugging hook present?`);
}

async function waitForTarget(timeoutMs = 60_000) {
  const deadline = Date.now() + timeoutMs;
  let last = "";
  while (Date.now() < deadline) {
    for (const path of ["/json/list", "/json"]) {
      try {
        const response = await fetch(`http://127.0.0.1:${CDP_PORT}${path}`, {
          signal: AbortSignal.timeout(3000),
        });
        if (!response.ok) {
          last = `${path} -> ${response.status}`;
          continue;
        }
        const targets = await response.json();
        const page = targets.find((target) => target.type === "page" && target.webSocketDebuggerUrl);
        if (page) {
          log(`CDP target ${page.url || "(no url)"}`);
          return page;
        }
        last = `${path} -> ${targets.length} targets`;
      } catch (error) {
        last = `${path} -> ${error.message}`;
      }
    }
    await sleep(500);
  }
  fail(`no WebView CDP target: ${last}`);
}

function connectCdp(url) {
  return new Promise((resolve, reject) => {
    const socket = new WebSocket(url);
    const timer = setTimeout(() => reject(new Error("WebSocket connect timed out")), 15_000);
    socket.addEventListener("open", () => {
      clearTimeout(timer);
      resolve(socket);
    });
    socket.addEventListener("error", () => {
      clearTimeout(timer);
      reject(new Error("WebSocket connect failed"));
    });
  });
}

class Devtools {
  constructor(socket) {
    this.socket = socket;
    this.nextId = 0;
    this.pending = new Map();
    socket.addEventListener("message", (event) => {
      const message = JSON.parse(event.data);
      if (!message.id || !this.pending.has(message.id)) return;
      const { resolve, reject, timer } = this.pending.get(message.id);
      this.pending.delete(message.id);
      clearTimeout(timer);
      if (message.error) reject(new Error(`${message.error.message} (${message.error.code})`));
      else resolve(message.result);
    });
  }

  send(method, params = {}) {
    const id = ++this.nextId;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new Error(`CDP ${method} timed out`));
      }, 180_000);
      this.pending.set(id, { resolve, reject, timer });
      this.socket.send(JSON.stringify({ id, method, params }));
    });
  }

  close() {
    try {
      this.socket.close();
    } catch {
      // Already closed.
    }
  }
}

const PAGE_HELPERS = `
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const waitFor = async (fn, ms, label) => {
  const end = Date.now() + ms;
  let last = null;
  while (Date.now() < end) {
    try {
      const value = fn();
      if (value) return value;
      last = value;
    } catch (error) {
      last = error.message;
    }
    await sleep(250);
  }
  throw new Error('timed out waiting for ' + label + ': ' + JSON.stringify(last));
};
const byText = (selector, text) =>
  Array.from(document.querySelectorAll(selector)).find((element) => element.textContent.includes(text));
const setValue = (element, value) => {
  if (!element) throw new Error('missing input');
  const prototype = element.tagName === 'TEXTAREA' ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
  Object.getOwnPropertyDescriptor(prototype, 'value').set.call(element, value);
  element.dispatchEvent(new Event('input', { bubbles: true }));
  element.dispatchEvent(new Event('change', { bubbles: true }));
};
`;

async function page(body) {
  const result = await cdp.send("Runtime.evaluate", {
    expression: `(async () => {${PAGE_HELPERS}${body}})()`,
    awaitPromise: true,
    returnByValue: true,
    userGesture: true,
  });
  if (result.exceptionDetails) {
    const description =
      result.exceptionDetails.exception?.description || result.exceptionDetails.text || "unknown";
    fail(`page script failed: ${description}`);
  }
  return result.result?.value;
}

async function signIn() {
  log("filling the login form");
  const filled = await page(`
    await waitFor(() => document.querySelector('#login-server'), 120000, 'login screen');
    const server = document.querySelector('#login-server');
    const username = document.querySelector('#login-username');
    const password = document.querySelector('input[type=password]');
    const device = document.querySelector('.login-form input[placeholder="This device"]');
    setValue(server, ${JSON.stringify(`http://127.0.0.1:${PROXY_PORT}`)});
    await sleep(200);
    setValue(username, ${JSON.stringify(USERNAME)});
    await sleep(200);
    setValue(password, ${JSON.stringify(PASSWORD)});
    await sleep(200);
    setValue(device, 'Android E2E');
    await sleep(500);
    document.querySelector('.login-form').requestSubmit();
    return { username: username.value, device: device.value };
  `);
  log(`submitted ${filled.username} on ${filled.device}`);

  const result = await page(`
    const result = await waitFor(() => {
      const pill = document.querySelector('.status-pill');
      if (pill) {
        const text = pill.textContent.trim();
        if (['Synced', 'Problem', 'Offline', 'Sign in', 'Local'].includes(text)) return text;
      }
      const error = document.querySelector('.form-error');
      return error ? 'login error: ' + error.textContent.trim() : null;
    }, 120000, 'login result');
    if (result !== 'Synced') throw new Error(result);
    return result;
  `);
  log(`login reached ${result}`);
}

async function createNote() {
  log(`creating note ${MARKER}`);
  const result = await page(`
    const fab = await waitFor(() => document.querySelector('.new-note-fab'), 30000, 'new note button');
    fab.click();
    const editor = await waitFor(() => document.querySelector('#note-editor'), 30000, 'note editor');
    await sleep(500);
    setValue(editor, ${JSON.stringify(NOTE_TEXT)});
    await sleep(800);
    const value = document.querySelector('#note-editor')?.value || '';
    return { value: value, hasMarker: value.includes(${JSON.stringify(MARKER)}) };
  `);
  if (!result.hasMarker) fail("the editor did not keep the typed note");
  return result;
}

async function syncNow() {
  log("requesting Sync now");
  await page(`
    const account = await waitFor(() => document.querySelector('#account-button'), 15000, 'account button');
    account.click();
    await waitFor(() => document.querySelector('#account-menu'), 15000, 'account menu');
    const action = await waitFor(() => {
      const candidate = byText('#account-menu .account-action', 'Sync now');
      return candidate && !candidate.disabled ? candidate : null;
    }, 60000, 'enabled Sync now');
    action.click();
    return true;
  `);
}

async function waitForSynced(timeoutMs = 120_000) {
  const status = await page(`
    const status = await waitFor(() => {
      const pill = document.querySelector('.status-pill');
      const text = pill ? pill.textContent.trim() : null;
      return ['Synced', 'Problem', 'Offline', 'Sign in'].includes(text) ? text : null;
    }, ${timeoutMs}, 'Synced');
    return status;
  `);
  if (status !== "Synced") fail(`sync ended in ${status}`);
}

async function waitUntil(predicate, timeoutMs, label) {
  const deadline = Date.now() + timeoutMs;
  let last = null;
  while (Date.now() < deadline) {
    try {
      const value = await predicate();
      if (value) return value;
      last = value;
    } catch (error) {
      last = error.message;
    }
    await sleep(1000);
  }
  fail(`timed out waiting for ${label}: ${JSON.stringify(last)}`);
}

async function workerCall(path, payload, headers = {}) {
  const response = await fetch(`${WORKER_BASE}${path}`, {
    method: "POST",
    headers: { "content-type": "application/json", ...headers },
    body: JSON.stringify({ version: PROTOCOL_VERSION, payload }),
    signal: AbortSignal.timeout(60_000),
  });
  const text = await response.text();
  if (!response.ok) fail(`${path} returned ${response.status}: ${text.slice(0, 300)}`);
  return JSON.parse(text).payload;
}

function readSession() {
  const text = adb(["exec-out", "run-as", PACKAGE, "cat", "files/session.json"]);
  const session = JSON.parse(text);
  if (!session.username || !session.device_id || !session.token) {
    fail("session.json is incomplete");
  }
  log(`session belongs to ${session.username} on device ${session.device_id}`);
  return session;
}

async function proveServer(session) {
  if (!state.capture) fail("no login request was captured");
  if (session.username !== state.capture.username) {
    fail(`session user ${session.username} does not match the login ${state.capture.username}`);
  }
  const reader = await workerCall("/v1/auth/login", {
    username: state.capture.username,
    login_key: state.capture.login_key,
    device_name: "e2e-reader",
  });
  log(`reader device ${reader.device_id} signed in`);
  const pull = await waitUntil(
    async () => {
      const response = await workerCall(
        "/v1/sync",
        { cursor: 0, updates: [] },
        { authorization: `Bearer ${reader.token}` },
      );
      return response.updates.length > 0 ? response : null;
    },
    120_000,
    "the note to reach another device",
  );
  const noteIds = [...new Set(pull.updates.map((update) => update.note_id))];
  log(`reader pulled ${pull.updates.length} update(s) at cursor ${pull.cursor}`);
  if (pull.cursor <= 0) fail("the server cursor did not advance");
  if (noteIds.length === 0) fail("the reader saw no note id");
  return { reader, pull, noteIds };
}

function captureEvidence() {
  try {
    const png = execFileSync("adb", ["exec-out", "screencap", "-p"], {
      encoding: "buffer",
      maxBuffer: 32 * 1024 * 1024,
      timeout: 60_000,
    });
    writeFileSync(join(ARTIFACTS_DIR, "android-sync.png"), png);
  } catch (error) {
    log(`screenshot failed: ${error.message}`);
  }
  try {
    const logcat = adb(["logcat", "-d"], { timeout: 120_000, maxBuffer: 64 * 1024 * 1024 });
    writeFileSync(join(ARTIFACTS_DIR, "android-sync-logcat.txt"), logcat);
  } catch (error) {
    log(`logcat failed: ${error.message}`);
  }
}

function writeReport(report) {
  try {
    writeFileSync(reportFile, `${JSON.stringify(report, null, 2)}\n`);
  } catch (error) {
    log(`report write failed: ${error.message}`);
  }
}

async function cleanup() {
  if (cdp) cdp.close();
  if (proxyServer) {
    // adb reverse can hold an idle keep-alive socket, which would otherwise
    // keep close() waiting until the job timeout.
    proxyServer.closeAllConnections?.();
    await new Promise((resolve) => proxyServer.close(resolve));
  }
  if (worker && worker.pid && workerExit === null) {
    try {
      process.kill(-worker.pid, "SIGKILL");
    } catch {
      try {
        worker.kill("SIGKILL");
      } catch {
        // Already gone.
      }
    }
  }
  for (const args of [["reverse", "--remove", `tcp:${PROXY_PORT}`], ["forward", "--remove", `tcp:${CDP_PORT}`]]) {
    try {
      adb(args);
    } catch {
      // Nothing to remove.
    }
  }
  if (persist && !KEEP_E2E_DIR) {
    try {
      rmSync(persist, { recursive: true, force: true });
    } catch {
      // Best effort.
    }
  }
}

async function main() {
  mkdirSync(ARTIFACTS_DIR, { recursive: true });
  for (const file of [logFile, workerLog, proxyLog]) writeFileSync(file, "");
  log(`== notebox emulator sync drill ==`);
  preflight();
  prepareApp();
  startWorker();
  await waitForWorker();
  await startProxy();
  log(`proxy on 127.0.0.1:${PROXY_PORT}`);
  adb(["reverse", `tcp:${PROXY_PORT}`, `tcp:${PROXY_PORT}`]);

  const pid = await launchApp();
  await waitForDevtoolsSocket(pid);
  adb(["forward", `tcp:${CDP_PORT}`, `localabstract:webview_devtools_remote_${pid}`]);
  const target = await waitForTarget();
  const socket = await connectCdp(target.webSocketDebuggerUrl);
  cdp = new Devtools(socket);
  await cdp.send("Runtime.enable");
  log("CDP attached");

  await signIn();
  await createNote();
  const before = state.syncCompleted;
  await syncNow();
  await waitUntil(() => state.syncCompleted > before, 120_000, "a completed device sync");
  log(`device completed ${state.syncCompleted} sync(s)`);
  await waitForSynced();
  const sync = { requests: state.syncRequests, completed: state.syncCompleted };
  const session = readSession();
  const proof = await proveServer(session);
  const editor = await page(`
    const value = document.querySelector('#note-editor')?.value || '';
    return { value: value, hasMarker: value.includes(${JSON.stringify(MARKER)}) };
  `);
  if (!editor.hasMarker) fail("the note marker disappeared after sync");

  captureEvidence();
  writeReport({
    ok: true,
    marker: MARKER,
    login: {
      username: state.capture.username,
      device: "Android E2E",
      setupStatus: state.setupStatus,
      loginStatus: state.loginStatus,
    },
    session: { username: session.username, deviceId: session.device_id, hasToken: true },
    deviceSync: { requests: state.syncRequests, completed: state.syncCompleted },
    reader: {
      deviceId: proof.reader.device_id,
      updates: proof.pull.updates.length,
      cursor: proof.pull.cursor,
      noteIds: proof.noteIds,
    },
    note: { hasMarker: editor.hasMarker, value: editor.value },
    sync,
  });
  log("emulator sync drill passed");
}

let exitCode = 0;
try {
  await main();
} catch (error) {
  exitCode = 1;
  log(`FAILED: ${error.stack || error.message}`);
  captureEvidence();
  writeReport({
    ok: false,
    marker: MARKER,
    error: error.message,
    state: {
      setupStatus: state.setupStatus,
      loginStatus: state.loginStatus,
      syncRequests: state.syncRequests,
      syncCompleted: state.syncCompleted,
      capturedLogin: Boolean(state.capture),
    },
  });
} finally {
  await cleanup();
}
process.exit(exitCode);
