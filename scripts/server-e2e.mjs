#!/usr/bin/env node
/**
 * End-to-end test against a real Worker under `wrangler dev --local`.
 *
 * It applies both D1 migrations to a throwaway directory, starts the Worker
 * with a test setup token, then drives the HTTP API the same way a device
 * would: setup, login, push/pull pages, attachment chunks, device revocation
 * and login backoff. No Cloudflare account is needed.
 *
 *   node scripts/server-e2e.mjs
 *
 * Set BASE_URL to test an already-running server instead of spawning one.
 */

import { execFile, spawn } from "node:child_process";
import { createHash, randomUUID } from "node:crypto";
import { mkdir, rm } from "node:fs/promises";
import { homedir, tmpdir } from "node:os";
import path from "node:path";
import assert from "node:assert/strict";
import { promisify } from "node:util";

const run = promisify(execFile);
const WRANGLER = ["--yes", "wrangler@4.151.2"];
const PORT = Number(process.env.PORT || 8799);
const SETUP_TOKEN = "e2e-setup-token";
const BASE = process.env.BASE_URL || `http://127.0.0.1:${PORT}`;
const PERSIST =
  process.env.NOTEBOX_E2E_DIR ||
  path.join(tmpdir(), `notebox-e2e-${process.pid}-${Date.now()}`);
const USERNAME = "e2e@notebox";
const LOGIN_KEY = Buffer.alloc(32, 7);

const env = {
  ...process.env,
  // This machine installs cargo binaries in ~/.cargo/bin without putting that
  // directory on PATH; CI gets it from the Rust action.
  PATH: `${path.join(homedir(), ".cargo", "bin")}${path.delimiter}${process.env.PATH ?? ""}`,
};

let child;
let passed = 0;

function ok(condition, message) {
  assert.ok(condition, message);
  passed += 1;
  console.log(`  ok  ${message}`);
}

function same(actual, expected, message) {
  assert.deepEqual(
    actual,
    expected,
    `${message}: got ${JSON.stringify(actual)}, expected ${JSON.stringify(expected)}`,
  );
  passed += 1;
  console.log(`  ok  ${message}`);
}

function envelope(payload) {
  return { version: 1, payload };
}

async function call(method, pathname, { body, token, setup, cookie } = {}) {
  const headers = {};
  if (body !== undefined) headers["content-type"] = "application/json";
  if (token) headers.authorization = `Bearer ${token}`;
  if (setup) headers["x-setup-token"] = setup;
  if (cookie) headers.cookie = cookie;
  const response = await fetch(`${BASE}${pathname}`, {
    method,
    headers,
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const text = await response.text();
  let json;
  try {
    json = JSON.parse(text);
  } catch {
    json = text;
  }
  return { status: response.status, headers: response.headers, body: json };
}

function payload(response) {
  return response.body?.payload;
}

function authRequestFor(username, deviceName, key = LOGIN_KEY) {
  return {
    username,
    login_key: key.toString("base64"),
    device_name: deviceName,
  };
}

function authRequest(deviceName, key = LOGIN_KEY) {
  return authRequestFor(USERNAME, deviceName, key);
}

function update(noteId, byte) {
  return {
    note_id: noteId,
    blob: Buffer.from([byte, byte + 1, byte + 2]).toString("base64"),
  };
}

async function waitForServer() {
  for (let attempt = 0; attempt < 180; attempt += 1) {
    if (child && child.exitCode !== null) {
      throw new Error(`wrangler exited with ${child.exitCode}`);
    }
    try {
      const response = await fetch(`${BASE}/`);
      if (response.ok) return;
    } catch {
      // Not listening yet.
    }
    await new Promise((resolve) => setTimeout(resolve, 1000));
  }
  throw new Error("wrangler dev did not start within 180 seconds");
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
      PERSIST,
    ],
    { env, maxBuffer: 20 * 1024 * 1024 },
  );
  if (stderr.includes("ERROR")) {
    throw new Error(`migrations for ${name} failed:\n${stderr}`);
  }
}

async function startWorker() {
  // wrangler.toml serves the Dioxus web build from this directory. The server
  // e2e runs without building the web app, so the empty directory keeps the
  // assets binding valid while every API path still reaches the Worker.
  await mkdir("target/dx/notebox/release/web/public", { recursive: true });
  child = spawn(
    "npx",
    [
      ...WRANGLER,
      "dev",
      "--local",
      "--port",
      String(PORT),
      "--persist-to",
      PERSIST,
      "--var",
      `SETUP_TOKEN:${SETUP_TOKEN}`,
    ],
    { env, detached: true, stdio: ["ignore", "pipe", "pipe"] },
  );
  let output = "";
  child.stdout.on("data", (chunk) => {
    output += chunk;
  });
  child.stderr.on("data", (chunk) => {
    output += chunk;
  });
  child.on("exit", (code) => {
    if (code !== null && code !== 0) {
      console.error(output.split("\n").slice(-40).join("\n"));
    }
  });
  await waitForServer();
}

async function stopWorker() {
  if (!child || child.exitCode !== null) return;
  try {
    process.kill(-child.pid, "SIGTERM");
  } catch {
    child.kill("SIGTERM");
  }
  await new Promise((resolve) => {
    const timer = setTimeout(resolve, 5000);
    child.once("exit", () => {
      clearTimeout(timer);
      resolve();
    });
  });
}

async function main() {
  console.log("== notebox server end-to-end ==");
  if (!process.env.BASE_URL) {
    console.log(`  persist dir: ${PERSIST}`);
    await rm(PERSIST, { recursive: true, force: true });
    await migrate("notebox-sync");
    await migrate("notebox-blobs");
    await startWorker();
  } else {
    await waitForServer();
  }

  console.log("\n-- auth --");
  const health = await call("GET", "/");
  same(health.status, 200, "health endpoint answers");

  const noToken = await call("POST", "/v1/auth/setup", {
    body: envelope(authRequest("laptop")),
  });
  same(noToken.status, 403, "setup without a token is refused");

  const wrongToken = await call("POST", "/v1/auth/setup", {
    setup: "wrong",
    body: envelope(authRequest("laptop")),
  });
  same(wrongToken.status, 403, "setup with a wrong token is refused");
  same(payload(wrongToken).code, "bad_setup_token", "setup error code");

  const created = await call("POST", "/v1/auth/setup", {
    setup: SETUP_TOKEN,
    body: envelope(authRequest("laptop")),
  });
  same(created.status, 200, "setup creates the account");
  const laptop = payload(created);
  ok(/^[0-9a-f]{64}$/.test(laptop.token), "device token is 64 hex characters");
  ok(laptop.device_id.length > 0, "setup returns a device id");

  const cookieHeaders = created.headers.getSetCookie?.() ?? [
    created.headers.get("set-cookie"),
  ];
  const cookie = cookieHeaders.find((value) => value?.startsWith("notebox_session="));
  ok(cookie?.includes("HttpOnly"), "web login sets an HttpOnly cookie");
  ok(cookie?.includes("SameSite=Strict"), "cookie is SameSite=Strict");

  const again = await call("POST", "/v1/auth/setup", {
    setup: SETUP_TOKEN,
    body: envelope(authRequest("laptop")),
  });
  same(again.status, 409, "a second setup is refused");

  const cookieList = await call("GET", "/v1/devices", {
    cookie: cookie.split(";")[0],
  });
  same(cookieList.status, 200, "the cookie authenticates like a bearer token");

  const logout = await call("POST", "/v1/auth/logout", {
    cookie: cookie.split(";")[0],
  });
  same(logout.status, 204, "logout answers without a body");
  const logoutCookies = logout.headers.getSetCookie?.() ?? [
    logout.headers.get("set-cookie"),
  ];
  const expired = logoutCookies.find((value) =>
    value?.startsWith("notebox_session="),
  );
  ok(expired?.includes("Max-Age=0"), "logout expires the session cookie");
  ok(expired?.includes("HttpOnly"), "the cleared cookie stays HttpOnly");

  const login = await call("POST", "/v1/auth/login", {
    body: envelope(authRequest("phone")),
  });
  same(login.status, 200, "login issues a second device token");
  const phone = payload(login);
  ok(phone.token !== laptop.token, "each device gets its own token");

  const unauthorized = await call("POST", "/v1/sync", {
    body: envelope({ cursor: 0, updates: [] }),
  });
  same(unauthorized.status, 401, "sync without a token is refused");

  console.log("\n-- sync --");
  const noteId = randomUUID();
  const push = await call("POST", "/v1/sync", {
    token: laptop.token,
    body: envelope({ cursor: 0, updates: [update(noteId, 1), update(noteId, 4)] }),
  });
  same(push.status, 200, "laptop pushes two updates");
  same(payload(push).updates.length, 0, "a device does not pull back its own updates");
  same(payload(push).cursor, 2, "cursor advanced past the pushed rows");

  const pull = await call("POST", "/v1/sync", {
    token: phone.token,
    body: envelope({ cursor: 0, updates: [] }),
  });
  same(pull.status, 200, "phone pulls");
  same(payload(pull).updates.length, 2, "phone sees both updates");
  same(payload(pull).updates[0].seq, 1, "updates arrive in commit order");
  same(
    payload(pull).updates[0].blob,
    Buffer.from([1, 2, 3]).toString("base64"),
    "update bytes survive the JSON envelope",
  );

  const tooMany = await call("POST", "/v1/sync", {
    token: phone.token,
    body: envelope({
      cursor: 0,
      updates: Array.from({ length: 26 }, () => update(noteId, 9)),
    }),
  });
  same(tooMany.status, 413, "a push of more than 25 updates is refused");

  // Four full batches and one short batch: 120 rows, two pages on the way back.
  for (let batch = 0; batch < 5; batch += 1) {
    const count = batch === 4 ? 20 : 25;
    const response = await call("POST", "/v1/sync", {
      token: laptop.token,
      body: envelope({
        cursor: 0,
        updates: Array.from({ length: count }, (_, index) =>
          update(noteId, (batch * 25 + index) % 250),
        ),
      }),
    });
    same(response.status, 200, `laptop pushes batch ${batch + 1} of 120`);
  }

  const firstPage = await call("POST", "/v1/sync", {
    token: phone.token,
    body: envelope({ cursor: 2, updates: [] }),
  });
  same(payload(firstPage).updates.length, 100, "first page returns 100 updates");
  same(payload(firstPage).more, true, "first page says more is waiting");
  same(payload(firstPage).cursor, 102, "first page cursor is the last read row");

  const secondPage = await call("POST", "/v1/sync", {
    token: phone.token,
    body: envelope({ cursor: payload(firstPage).cursor, updates: [] }),
  });
  same(payload(secondPage).updates.length, 20, "second page returns the rest");
  same(payload(secondPage).more, false, "second page says the log is drained");

  console.log("\n-- attachments --");
  const image = Buffer.from("a tiny webp payload for the chunk test");
  const hash = createHash("sha256").update(image).digest("hex");
  const chunk = {
    hash,
    index: 0,
    total: 1,
    size: image.length,
    mime: "image/webp",
    data: image.toString("base64"),
  };
  const uploaded = await call("PUT", `/v1/blob/${hash}`, {
    token: phone.token,
    body: envelope(chunk),
  });
  same(uploaded.status, 200, "phone uploads an attachment chunk");
  same(payload(uploaded).hash, hash, "upload acknowledges the hash");

  const reuploaded = await call("PUT", `/v1/blob/${hash}`, {
    token: laptop.token,
    body: envelope(chunk),
  });
  same(reuploaded.status, 200, "re-uploading the same chunk is idempotent");

  const downloaded = await call("GET", `/v1/blob/${hash}?index=0`, {
    token: laptop.token,
  });
  same(downloaded.status, 200, "laptop downloads the chunk");
  same(payload(downloaded).data, chunk.data, "chunk bytes round-trip through D1");
  same(payload(downloaded).mime, "image/webp", "chunk metadata round-trips");

  const missing = await call("GET", `/v1/blob/${hash}?index=1`, {
    token: laptop.token,
  });
  same(missing.status, 404, "a missing chunk is a 404");

  const anonymousUsage = await call("GET", "/v1/usage");
  same(anonymousUsage.status, 401, "usage needs a device token");
  const usage = await call("GET", "/v1/usage", { token: phone.token });
  same(usage.status, 200, "device reads attachment and history usage");
  const stored = payload(usage);
  same(stored.attachments.count, 1, "usage counts the uploaded attachment");
  same(stored.attachments.bytes, image.length, "usage reports attachment bytes");
  ok(stored.notes.count >= 120, "usage counts pushed history updates");
  ok(stored.notes.bytes > 0, "usage reports note history bytes");
  same(stored.limit_bytes, 500_000_000, "usage reports the D1 per-database limit");

  const anonymousDelete = await call("DELETE", `/v1/blob/${hash}`);
  same(anonymousDelete.status, 401, "deleting a blob needs a device token");
  const deleted = await call("DELETE", `/v1/blob/${hash}`, {
    token: laptop.token,
  });
  same(deleted.status, 204, "laptop deletes the uploaded attachment");
  const deletedAgain = await call("DELETE", `/v1/blob/${hash}`, {
    token: laptop.token,
  });
  same(deletedAgain.status, 204, "deleting the same blob is idempotent");
  const gone = await call("GET", `/v1/blob/${hash}?index=0`, {
    token: laptop.token,
  });
  same(gone.status, 404, "a deleted chunk is gone");

  console.log("\n-- devices --");
  const devices = await call("GET", "/v1/devices", { token: phone.token });
  same(devices.status, 200, "device list loads");
  same(payload(devices).devices.length, 2, "both devices are listed");
  const current = payload(devices).devices.find((device) => device.current);
  same(current.id, phone.device_id, "the current device is marked");

  const revoked = await call("DELETE", `/v1/devices/${phone.device_id}`, {
    token: laptop.token,
  });
  same(revoked.status, 204, "laptop revokes the phone");
  const revokedAgain = await call("GET", "/v1/devices", { token: phone.token });
  same(revokedAgain.status, 401, "a revoked token stops working immediately");

  console.log("\n-- login backoff --");
  const wrongKey = Buffer.alloc(32, 8);
  const failed = await call("POST", "/v1/auth/login", {
    body: envelope(authRequest("tablet", wrongKey)),
  });
  same(failed.status, 401, "a wrong login key is rejected");
  const blocked = await call("POST", "/v1/auth/login", {
    body: envelope(authRequest("tablet")),
  });
  same(blocked.status, 429, "the next attempt is rate-limited");
  const retryAfter = Number(blocked.headers.get("retry-after"));
  ok(retryAfter >= 1, "the response says how long to wait");
  await new Promise((resolve) => setTimeout(resolve, Math.min(retryAfter, 5) * 1000 + 250));
  const recovered = await call("POST", "/v1/auth/login", {
    body: envelope(authRequest("tablet")),
  });
  same(recovered.status, 200, "login works after the backoff expires");

  console.log("\n-- credentials --");
  const newUsername = "e2e@notebox-new";
  const newKey = Buffer.alloc(32, 9);
  const credentials = (currentKey, username, nextKey) => ({
    current_login_key: currentKey.toString("base64"),
    new_username: username,
    new_login_key: nextKey.toString("base64"),
  });

  const wrongCurrent = await call("POST", "/v1/auth/credentials", {
    token: laptop.token,
    body: envelope(credentials(wrongKey, newUsername, newKey)),
  });
  same(wrongCurrent.status, 403, "a wrong current password is refused");
  same(payload(wrongCurrent).code, "wrong_password", "wrong-password has its own code");
  const afterWrong = await call("GET", "/v1/devices", { token: laptop.token });
  same(payload(afterWrong).devices.length, 2, "a wrong password changes no device");

  const changed = await call("POST", "/v1/auth/credentials", {
    token: laptop.token,
    body: envelope(credentials(LOGIN_KEY, newUsername, newKey)),
  });
  same(changed.status, 200, "the laptop changes the username and password key");
  same(payload(changed).username, newUsername, "the response carries the trimmed username");
  const afterChange = await call("GET", "/v1/devices", { token: laptop.token });
  same(payload(afterChange).devices.length, 1, "only the current device survives");
  const tabletAfter = await call("GET", "/v1/devices", { token: recovered.token });
  same(tabletAfter.status, 401, "the other device is signed out");

  const newLogin = await call("POST", "/v1/auth/login", {
    body: envelope(authRequestFor(newUsername, "new-phone", newKey)),
  });
  same(newLogin.status, 200, "the new username and key log in");
  const oldLogin = await call("POST", "/v1/auth/login", {
    body: envelope(authRequestFor(USERNAME, "old-phone", LOGIN_KEY)),
  });
  same(oldLogin.status, 401, "the old username and key no longer log in");

  console.log(`\nPASS: ${passed} checks`);
}

let failure;
try {
  await main();
} catch (error) {
  failure = error;
  console.error(`\nFAIL: ${error.stack || error}`);
} finally {
  await stopWorker();
  if (!process.env.BASE_URL && process.env.KEEP_E2E_DIR !== "1") {
    await rm(PERSIST, { recursive: true, force: true });
  }
}
if (failure) process.exit(1);
