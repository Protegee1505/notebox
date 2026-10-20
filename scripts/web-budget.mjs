#!/usr/bin/env node
// Brotli first-load budget gate for the Dioxus web bundle.
//
// Usage: node scripts/web-budget.mjs target/dx/notebox/release/web/public

import fs from "node:fs";
import path from "node:path";
import zlib from "node:zlib";

const root = process.argv[2];
if (!root || !fs.existsSync(root)) {
  console.error(`web-budget: missing build directory: ${root ?? "<none>"}`);
  process.exit(2);
}

const LIMIT_BYTES = 3 * 1024 * 1024;

function walk(dir) {
  return fs.readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const full = path.join(dir, entry.name);
    return entry.isDirectory() ? walk(full) : [full];
  });
}

let raw = 0;
let brotli = 0;
for (const file of walk(root)) {
  const bytes = fs.readFileSync(file);
  raw += bytes.length;
  brotli += zlib.brotliCompressSync(bytes, {
    params: { [zlib.constants.BROTLI_PARAM_QUALITY]: 11 },
  }).length;
}

const mib = (bytes) => (bytes / 1024 / 1024).toFixed(2);
console.log(
  `web-budget: raw=${raw} bytes brotli=${brotli} bytes (${mib(brotli)} MiB), limit 3.00 MiB`
);

if (brotli > LIMIT_BYTES) {
  console.error(
    `web-budget: FAIL: brotli ${mib(brotli)} MiB exceeds the 3 MiB budget`
  );
  process.exit(1);
}

console.log("web-budget: PASS");
