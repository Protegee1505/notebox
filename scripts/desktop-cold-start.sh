#!/usr/bin/env bash
# P8: measure Linux desktop cold start to an editable note.
#
# Boots a private Xvfb display, launches the release binary with an isolated
# XDG data directory and waits for the editor's NBX_READY_FILE marker. The
# first run also seeds the sample notes; the warm runs use that database.
#
#   ./scripts/desktop-cold-start.sh
#   NBX_RUNS=5 ./scripts/desktop-cold-start.sh
#
# Set NBX_BINARY to measure an existing binary. The release build is not part
# of the measurement.

set -euo pipefail

cd "$(dirname "$0")/.."

binary="${NBX_BINARY:-target/release/notebox}"
runs="${NBX_RUNS:-3}"
budget_ms="${NBX_COLD_START_BUDGET_MS:-600}"
work="$(mktemp -d)"
xvfb_pid=""
display=""

cleanup() {
  if [[ -n "$xvfb_pid" ]] && kill -0 "$xvfb_pid" 2>/dev/null; then
    kill "$xvfb_pid" 2>/dev/null || true
    wait "$xvfb_pid" 2>/dev/null || true
  fi
  if [[ "${NBX_KEEP:-0}" != "1" ]]; then
    rm -rf "$work"
  else
    printf 'artifacts kept in %s\n' "$work"
  fi
}
trap cleanup EXIT

if [[ ! -x "$binary" ]]; then
  printf 'building %s (release)\n' "$binary"
  cargo build --release -p notes-app --no-default-features --features desktop
fi

for number in $(seq 90 120); do
  if [[ ! -e "/tmp/.X11-unix/X$number" && ! -e "/tmp/.X$number-lock" ]]; then
    Xvfb ":$number" -screen 0 1280x900x24 -nolisten tcp \
      >"$work/xvfb.log" 2>&1 &
    xvfb_pid=$!
    sleep 0.4
    if kill -0 "$xvfb_pid" 2>/dev/null; then
      display=":$number"
      break
    fi
    wait "$xvfb_pid" 2>/dev/null || true
    xvfb_pid=""
  fi
done
if [[ -z "$display" ]]; then
  printf 'could not start Xvfb\n' >&2
  cat "$work/xvfb.log" >&2 || true
  exit 1
fi

data="$work/data"
config="$work/config"
cache="$work/cache"
mkdir -p "$data" "$config" "$cache"

measure() {
  local label="$1"
  local ready="$work/ready-$label"
  local log="$work/$label.log"
  rm -f "$ready"
  local started
  started="$(date +%s%N)"
  DISPLAY="$display" \
    XDG_DATA_HOME="$data" \
    XDG_CONFIG_HOME="$config" \
    XDG_CACHE_HOME="$cache" \
    GDK_BACKEND=x11 \
    LIBGL_ALWAYS_SOFTWARE=1 \
    WEBKIT_DISABLE_COMPOSITING_MODE=1 \
    NBX_READY_FILE="$ready" \
    setsid dbus-run-session -- "$binary" >"$log" 2>&1 &
  local pid=$!
  local deadline=$((SECONDS + 20))
  while [[ ! -s "$ready" ]]; do
    if ! kill -0 "$pid" 2>/dev/null; then
      printf '%s: app exited before the editor was ready\n' "$label" >&2
      cat "$log" >&2 || true
      exit 1
    fi
    if ((SECONDS > deadline)); then
      printf '%s: timed out waiting for the editor\n' "$label" >&2
      cat "$log" >&2 || true
      kill -- "-$pid" 2>/dev/null || true
      exit 1
    fi
    sleep 0.01
  done
  local ready_ms
  ready_ms="$(cat "$ready")"
  local elapsed=$((ready_ms - started / 1000000))
  kill -- "-$pid" 2>/dev/null || true
  wait "$pid" 2>/dev/null || true
  printf '%s %s\n' "$label" "$elapsed"
}

first="$(measure first-run)"
printf 'first run (seeds sample notes): %s ms\n' "$first"

warm=()
for run in $(seq 1 "$runs"); do
  value="$(measure "warm-$run" | awk '{print $2}')"
  warm+=("$value")
  printf 'warm run %s: %s ms\n' "$run" "$value"
done

median="$(
  printf '%s\n' "${warm[@]}" | sort -n | awk '
    { values[NR] = $1 }
    END {
      if (NR % 2) print values[(NR + 1) / 2];
      else print int((values[NR / 2] + values[NR / 2 + 1]) / 2);
    }
  '
)"
printf 'warm median: %s ms (budget %s ms)\n' "$median" "$budget_ms"
if ((median >= budget_ms)); then
  printf 'FAIL desktop cold start\n' >&2
  exit 1
fi
printf 'PASS desktop cold start\n'
