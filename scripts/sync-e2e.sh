#!/usr/bin/env bash
# P3 gate: two headless sync clients against a real Worker and D1.
#
# Boots `wrangler dev --local` on a throwaway persistence directory, applies
# both migrations, then runs the ignored Rust end-to-end test. No Cloudflare
# account is needed.
#
#   ./scripts/sync-e2e.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

PORT="${PORT:-8798}"
BASE="http://127.0.0.1:${PORT}"
SETUP_TOKEN="sync-e2e-setup-token"
WRANGLER=(--yes wrangler@4.151.2)
PERSIST="$(mktemp -d "${TMPDIR:-/tmp}/notebox-sync-e2e.XXXXXX")"
LOG="$PERSIST/wrangler.log"
SERVER_PID=""

export PATH="$HOME/.cargo/bin:/tmp/opencode/bin:$PATH"

cleanup() {
  status=$?
  if [[ -n "$SERVER_PID" ]] && kill -0 "$SERVER_PID" 2>/dev/null; then
    kill -TERM -- "-$SERVER_PID" 2>/dev/null || kill "$SERVER_PID" 2>/dev/null || true
    wait "$SERVER_PID" 2>/dev/null || true
  fi
  if [[ $status -ne 0 ]]; then
    echo "--- wrangler log (tail) ---" >&2
    tail -n 40 "$LOG" >&2 || true
  fi
  rm -rf "$PERSIST"
  exit $status
}
trap cleanup EXIT

echo "== notebox client sync end-to-end =="
echo "  persist dir: $PERSIST"

for database in notebox-sync notebox-blobs; do
  if ! output=$(npx "${WRANGLER[@]}" d1 migrations apply "$database" \
    --local --persist-to "$PERSIST" 2>&1); then
    echo "$output" >&2
    exit 1
  fi
  if grep -q "ERROR" <<<"$output"; then
    echo "$output" >&2
    exit 1
  fi
done

# The Worker serves the web build from this directory (wrangler.toml). This
# sync test only needs the API, so an empty directory is enough.
mkdir -p target/dx/notebox/release/web/public

setsid npx "${WRANGLER[@]}" dev --local \
  --port "$PORT" \
  --persist-to "$PERSIST" \
  --var "SETUP_TOKEN:$SETUP_TOKEN" \
  >"$LOG" 2>&1 &
SERVER_PID=$!

for _ in $(seq 1 180); do
  if ! kill -0 "$SERVER_PID" 2>/dev/null; then
    echo "wrangler dev exited before listening" >&2
    exit 1
  fi
  if curl -fsS "$BASE/" >/dev/null 2>&1; then
    break
  fi
  sleep 1
done
if ! curl -fsS "$BASE/" >/dev/null 2>&1; then
  echo "wrangler dev did not start within 180 seconds" >&2
  exit 1
fi

NOTEBOX_SYNC_E2E_URL="$BASE" \
NOTEBOX_SYNC_E2E_SETUP_TOKEN="$SETUP_TOKEN" \
  cargo test -p notes-sync --test worker_e2e -- --ignored --nocapture
