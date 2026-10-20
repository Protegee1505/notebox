#!/usr/bin/env bash
# Measure the compressed wasm cost of the note document and Loro.
#
# Builds two minimal wasm examples: one with the note document, one without,
# then reports raw, gzip and brotli sizes. Node is used for compression so the
# script works on a machine without a brotli CLI.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

cargo build -p notes-core --release --target wasm32-unknown-unknown \
  --example loro_wasm --example baseline_wasm

node - "$@" <<'NODE'
const fs = require("node:fs");
const zlib = require("node:zlib");

const files = [
  ["baseline", "target/wasm32-unknown-unknown/release/examples/baseline_wasm.wasm"],
  ["loro", "target/wasm32-unknown-unknown/release/examples/loro_wasm.wasm"],
];

const sizes = {};
for (const [name, file] of files) {
  const bytes = fs.readFileSync(file);
  const brotli = zlib.brotliCompressSync(bytes, {
    params: { [zlib.constants.BROTLI_PARAM_QUALITY]: 11 },
  });
  const gzip = zlib.gzipSync(bytes, { level: 9 });
  sizes[name] = { raw: bytes.length, brotli: brotli.length, gzip: gzip.length };
  console.log(
    `${name}: raw=${bytes.length} brotli=${brotli.length} gzip=${gzip.length}`
  );
}

console.log(
  `delta: raw=${sizes.loro.raw - sizes.baseline.raw} ` +
    `brotli=${sizes.loro.brotli - sizes.baseline.brotli} ` +
    `gzip=${sizes.loro.gzip - sizes.baseline.gzip}`
);

const budget = 1.5 * 1024 * 1024;
if (sizes.loro.brotli - sizes.baseline.brotli > budget) {
  console.error("wasm-size: FAIL: Loro adds more than 1.5 MiB brotli");
  process.exit(1);
}
console.log("wasm-size: PASS");
NODE
