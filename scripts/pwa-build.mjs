#!/usr/bin/env node
// Add the installable-web-app files to a Dioxus release build.
//
// Dioxus hashes its JS, CSS and WASM names at build time, so the service
// worker cannot be a static file. This script copies the manifest and icons,
// lists every hashed asset for precache, writes sw.js with that list, and
// records the build identity. Run it after `dx build`.
//
//   node scripts/pwa-build.mjs [dist-dir]
//   NBX_BUILD_ID=... node scripts/pwa-build.mjs [dist-dir]

import { createHash } from "node:crypto";
import { execFileSync } from "node:child_process";
import { copyFile, readFile, readdir, stat, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const source = path.join(root, "crates/notes-app/web");

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

function readArguments() {
  const values = process.argv.slice(2);
  let dist = path.join(targetDirectory(), "dx/notebox/release/web/public");
  let buildId = process.env.NBX_BUILD_ID ?? "";
  for (let index = 0; index < values.length; index += 1) {
    const value = values[index];
    if (value === "--build-id") {
      buildId = values[index + 1] ?? "";
      index += 1;
    } else if (value.startsWith("--build-id=")) {
      buildId = value.slice("--build-id=".length);
    } else if (value === "--help" || value === "-h") {
      console.log("Usage: node scripts/pwa-build.mjs [dist-dir] [--build-id id]");
      process.exit(0);
    } else if (!value.startsWith("-")) {
      dist = path.resolve(value);
    } else {
      throw new Error(`unknown argument: ${value}`);
    }
  }
  return { dist, buildId };
}

async function walk(directory) {
  const entries = await readdir(directory, { withFileTypes: true });
  const files = [];
  for (const entry of entries) {
    const full = path.join(directory, entry.name);
    if (entry.isDirectory()) {
      files.push(...(await walk(full)));
    } else {
      files.push(full);
    }
  }
  return files;
}

function packageVersion(text) {
  const workspace = text.match(/\nversion = "([^"]+)"/);
  if (!workspace) {
    throw new Error("could not find [workspace.package] version in Cargo.toml");
  }
  return workspace[1];
}

async function main() {
  const { dist, buildId: requestedBuild } = readArguments();
  const indexPath = path.join(dist, "index.html");
  try {
    await stat(indexPath);
  } catch {
    throw new Error(`missing build: ${indexPath}. Run dx build first.`);
  }

  const index = await readFile(indexPath, "utf8");
  if (!index.includes('id="main"')) {
    throw new Error("index.html has no #main mount point");
  }
  if (!index.includes("noteboxPwa") || !index.includes("/sw.js")) {
    throw new Error(
      "index.html does not register the service worker; is the custom index in use?",
    );
  }
  if (!index.includes('rel="manifest"')) {
    throw new Error("index.html has no manifest link");
  }

  const pwaFiles = [
    "manifest.webmanifest",
    "icon-192.png",
    "icon-512.png",
    "icon-maskable-512.png",
    "icon.svg",
    "_headers",
  ];
  for (const name of pwaFiles) {
    await copyFile(path.join(source, name), path.join(dist, name));
  }

  const assetDirectory = path.join(dist, "assets");
  const assetManifest = path.join(dist, "..", ".manifest.json");
  let assets;
  try {
    const manifest = JSON.parse(await readFile(assetManifest, "utf8"));
    assets = [
      ...new Set(
        Object.values(manifest.assets ?? {})
          .flat()
          .map((asset) => asset.bundled_path)
          .filter((name) => typeof name === "string")
          .map((name) => `/assets/${name}`),
      ),
    ].sort();
  } catch {
    assets = (await walk(assetDirectory))
      .filter((file) => !file.endsWith(".map"))
      .map((file) => `/${path.relative(dist, file).split(path.sep).join("/")}`)
      .sort();
  }
  if (assets.length === 0) {
    throw new Error(`no assets in ${assetDirectory}`);
  }
  for (const url of assets) {
    await stat(path.join(dist, url));
  }

  const precache = [
    "/",
    "/index.html",
    "/manifest.webmanifest",
    "/icon-192.png",
    "/icon-512.png",
    "/icon-maskable-512.png",
    ...assets,
  ];

  const version = packageVersion(await readFile(path.join(root, "Cargo.toml"), "utf8"));
  const template = await readFile(path.join(source, "sw.js"), "utf8");
  let build = requestedBuild.trim();
  if (!/^[A-Za-z0-9._-]+$/.test(build)) {
    const hash = createHash("sha256");
    hash.update(`notebox ${version}\n`);
    hash.update(template);
    for (const url of precache) {
      hash.update(url);
      hash.update(await readFile(url === "/" ? indexPath : path.join(dist, url)));
    }
    build = hash.digest("hex").slice(0, 16);
  }

  const worker = template
    .replace('"__NBX_BUILD__"', JSON.stringify(build))
    .replace("__NBX_PRECACHE__", JSON.stringify(precache, null, 2));
  if (worker.includes("__NBX_")) {
    throw new Error("service-worker template still has a placeholder");
  }
  await writeFile(path.join(dist, "sw.js"), worker);

  await writeFile(
    path.join(dist, "version.json"),
    `${JSON.stringify({ version, build, precache: precache.length }, null, 2)}\n`,
  );

  const bytes = (
    await Promise.all(
      ["index.html", "sw.js", ...pwaFiles]
        .filter((name) => name !== "_headers")
        .map(async (name) => (await stat(path.join(dist, name))).size),
    )
  ).reduce((sum, size) => sum + size, 0);
  console.log(
    `pwa-build: ${precache.length} cached files, build ${build}, pwa files ${bytes} bytes`,
  );
  console.log(`pwa-build: PASS (${path.relative(root, dist) || dist})`);
}

main().catch((error) => {
  console.error(`pwa-build: FAIL: ${error.message}`);
  process.exit(1);
});
