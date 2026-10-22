#!/usr/bin/env bash
# Restore the ephemeral build tree after a Cloud Shell restart.
#
#   scripts/cloudshell-bootstrap.sh [--wait] [--boot] [--force] [--quiet]
#   scripts/cloudshell-bootstrap.sh --force [--allow-running]
#   scripts/cloudshell-bootstrap.sh --status
#
# The live tree lives in /tmp/opencode (or NOTEBOX_BUILD_HOME). The persistent
# seeds live in ~/.cache/notebox-build (or NOTEBOX_SEED_DIR):
#
#   base.tar.xz    minimal Rust toolchain, registry cache, dx/nextest/deny/
#                  worker-build, dx tools and the npm cache
#   target.tar.xz  a pruned Cargo target tree
#
# Both are optional. A missing or corrupt seed falls back to rustup and
# cargo fetch. The script never writes build artifacts to /home and refuses to
# extract a seed that would leave less than NOTEBOX_HOME_RESERVE_MB free.
set -euo pipefail

# Never let BASH_ENV start a second bootstrap while this script holds the lock.
unset BASH_ENV

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
STORE="${NOTEBOX_BUILD_HOME:-/tmp/opencode}"
STORE_HOME="$STORE/home"
SEED="${NOTEBOX_SEED_DIR:-$HOME/.cache/notebox-build}"
READY="$STORE/.notebox-ready"
BASE="$SEED/base.tar.xz"
TARGET="$SEED/target.tar.xz"
RESERVE_MB="${NOTEBOX_HOME_RESERVE_MB:-1200}"
WAIT_SECONDS="${NOTEBOX_BOOTSTRAP_WAIT:-180}"

wait_mode=0
force=0
quiet=0
install=1
status_mode=0
allow_running=0

while [ "$#" -gt 0 ]; do
  case "$1" in
    --wait|--boot)
      wait_mode=1
      shift
      ;;
    --force)
      force=1
      shift
      ;;
    --allow-running)
      allow_running=1
      shift
      ;;
    --quiet)
      quiet=1
      shift
      ;;
    --no-install)
      install=0
      shift
      ;;
    --status)
      status_mode=1
      shift
      ;;
    -h|--help)
      sed -n '2,17p' "$0" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *)
      echo "cloudshell-bootstrap: unknown option: $1" >&2
      exit 2
      ;;
  esac
done

log() {
  local line
  line="$(date -u '+%Y-%m-%dT%H:%M:%SZ') $*"
  if [ -d "$SEED" ]; then
    printf '%s\n' "$line" >>"$SEED/bootstrap.log" 2>/dev/null || true
  fi
  if [ "$quiet" -eq 0 ]; then
    printf '%s\n' "$line" >&2
  fi
}

die() {
  log "error: $*"
  exit 1
}

free_mb() {
  df -Pm "$HOME" 2>/dev/null | awk 'NR == 2 { print $4 }'
}

tmp_free_mb() {
  df -Pm "$STORE" 2>/dev/null | awk 'NR == 2 { print $4 }'
}

file_mb() {
  stat -c %s "$1" 2>/dev/null | awk '{ print int(($1 + 1048575) / 1048576) }'
}

manifest_get() {
  local file="$1" key="$2"
  [ -f "$file" ] || return 1
  sed -n "s/^${key}=//p" "$file" | head -1
}

sha_check() {
  local archive="$1" sidecar="$2"
  [ -f "$archive" ] && [ -f "$sidecar" ] || return 1
  (cd "$(dirname "$archive")" && sha256sum -c "$(basename "$sidecar")" >/dev/null 2>&1)
}

layout_ok() {
  [ -f "$READY" ] &&
    [ -d "$STORE_HOME/cargo" ] &&
    [ -d "$STORE_HOME/rustup/toolchains" ] &&
    [ -f "$ROOT/.cargo/config.toml" ] &&
    [ -L "$ROOT/target" ] &&
    [ -d "$STORE/notebox-target" ]
}

is_busy() {
  local proc
  for proc in cargo rustc dx cargo-nextest worker-build wasm-bindgen wasm-opt esbuild; do
    pgrep -u "$(id -u)" -x "$proc" >/dev/null 2>&1 && return 0
  done
  return 1
}

show_status() {
  printf 'store: %s\nseed:  %s\n' "$STORE" "$SEED"
  printf 'home free: %s MiB\n' "$(free_mb)"
  if [ -f "$READY" ]; then
    printf 'ready: yes (%s)\n' "$(head -1 "$READY" 2>/dev/null || true)"
  else
    printf 'ready: no\n'
  fi
  for f in "$BASE" "$TARGET"; do
    if [ -f "$f" ]; then
      printf '%s: %s MiB\n' "$(basename "$f")" "$(file_mb "$f")"
    else
      printf '%s: missing\n' "$(basename "$f")"
    fi
  done
  if [ -d "$STORE/notebox-target" ]; then
    printf 'live target: %s\n' "$(du -sh "$STORE/notebox-target" 2>/dev/null | awk '{print $1}')"
  fi
  if is_busy; then
    printf 'build: running\n'
  else
    printf 'build: idle\n'
  fi
}

if [ "$status_mode" -eq 1 ]; then
  show_status
  exit 0
fi

mkdir -p "$STORE" "$SEED"

if [ -f "$SEED/bootstrap.log" ] && [ "$(stat -c %s "$SEED/bootstrap.log" 2>/dev/null || echo 0)" -gt 262144 ]; then
  tail -c 131072 "$SEED/bootstrap.log" >"$SEED/bootstrap.log.tmp" 2>/dev/null || true
  mv "$SEED/bootstrap.log.tmp" "$SEED/bootstrap.log" 2>/dev/null || true
fi

exec 9>"$STORE/.bootstrap.lock"
if ! flock -n 9; then
  if [ "$wait_mode" -eq 1 ]; then
    log "another bootstrap is running; waiting up to ${WAIT_SECONDS}s"
    waited=0
    while [ "$waited" -lt "$WAIT_SECONDS" ]; do
      if [ -f "$READY" ] && layout_ok; then
        log "ready marker appeared; continuing"
        exit 0
      fi
      sleep 1
      waited=$((waited + 1))
    done
  fi
  flock -w "$WAIT_SECONDS" 9 || die "timed out waiting for another bootstrap"
fi

if [ "$force" -eq 1 ] && [ "$allow_running" -eq 0 ] && is_busy; then
  die "a cargo/rustc/dx build is running; stop it first or pass --allow-running"
fi

if [ "$force" -eq 1 ]; then
  rm -f "$READY"
fi

# Remove leftovers from an interrupted extraction. No other bootstrap can be
# using them while this process holds the lock.
rm -rf "$STORE"/.restore-base.* "$STORE"/.restore-target.* 2>/dev/null || true

if [ "$force" -eq 0 ] && layout_ok; then
  exit 0
fi

log "preparing $STORE"

# 1. Repair symlinks and the target setting before any tool can write to /home.
"$ROOT/scripts/relocate-caches.sh" --repair "$STORE_HOME" >/dev/null 2>&1 || {
  log "warning: relocate-caches --repair failed; continuing"
}

export RUSTUP_HOME="${RUSTUP_HOME:-$STORE_HOME/rustup}"
export CARGO_HOME="${CARGO_HOME:-$STORE_HOME/cargo}"
export PATH="$CARGO_HOME/bin:$HOME/.local/bin:$PATH"

# 2. The target seed is elastic. Drop it before it makes /home uncomfortable.
if [ -f "$TARGET" ] && [ "$(free_mb)" -lt "$RESERVE_MB" ]; then
  log "home free $(free_mb) MiB is below ${RESERVE_MB} MiB; removing the target seed"
  rm -f "$TARGET" "$TARGET.sha256" "$TARGET.manifest"
fi

install_rustup() {
  local script="$STORE/rustup-init.sh"
  rm -f "$script"
  if command -v curl >/dev/null 2>&1; then
    timeout 300 curl --proto '=https' --tlsv1.2 -fsSL \
      https://sh.rustup.rs -o "$script" || return 1
  elif command -v wget >/dev/null 2>&1; then
    timeout 300 wget -qO "$script" https://sh.rustup.rs || return 1
  else
    return 1
  fi
  if ! timeout 600 sh "$script" -y --profile minimal \
      --default-toolchain none --no-modify-path; then
    rm -f "$script"
    return 1
  fi
  rm -f "$script"
  command -v rustup
}

restore_base() {
  local tmp="$STORE/.restore-base.$$"
  local extracted=0

  if [ -f "$BASE" ] && sha_check "$BASE" "$BASE.sha256"; then
    rm -rf "$tmp"
    mkdir -p "$tmp"
    log "restoring base seed ($(file_mb "$BASE") MiB)"
    if ! xz -dc -T0 "$BASE" | tar -xf - -C "$tmp" --no-same-owner; then
      rm -rf "$tmp"
      log "base seed extraction failed; discarding it"
      rm -f "$BASE" "$BASE.sha256" "$BASE.manifest"
    elif [ ! -d "$tmp/home/rustup" ] || [ ! -d "$tmp/home/cargo" ]; then
      rm -rf "$tmp"
      log "base seed is missing rustup or cargo; discarding it"
      rm -f "$BASE" "$BASE.sha256" "$BASE.manifest"
    else
      local d
      for d in rustup cargo dx dx-tools npm yarn; do
        if [ -d "$tmp/home/$d" ]; then
          rm -rf "$STORE_HOME/$d"
          mkdir -p "$(dirname "$STORE_HOME/$d")"
          mv "$tmp/home/$d" "$STORE_HOME/$d"
        fi
      done
      rm -rf "$tmp"
      extracted=1
    fi
  fi

  if [ "$extracted" -eq 1 ]; then
    return 0
  fi

  if [ -f "$BASE" ]; then
    log "base seed is missing its checksum or is corrupt; discarding it"
    rm -f "$BASE" "$BASE.sha256" "$BASE.manifest"
  fi

  if [ "$install" -eq 0 ]; then
    log "no usable base seed and --no-install was given"
    return 1
  fi

  log "installing a minimal Rust toolchain"
  mkdir -p "$STORE_HOME/rustup" "$STORE_HOME/cargo" "$STORE_HOME/cargo/bin"
  local rustup_bin
  rustup_bin="$(command -v rustup || true)"
  if [ -z "$rustup_bin" ]; then
    log "rustup is not installed; downloading the minimal installer"
    rustup_bin="$(install_rustup || true)"
  fi
  [ -n "$rustup_bin" ] || {
    log "rustup install failed; no usable toolchain"
    return 1
  }
  "$rustup_bin" set profile minimal >/dev/null 2>&1 || true
  if ! timeout 600 "$rustup_bin" toolchain install stable --profile minimal \
      --component clippy --component rustfmt \
      --target wasm32-unknown-unknown --no-self-update; then
    log "rustup install failed"
    return 1
  fi
  "$rustup_bin" default stable >/dev/null 2>&1 || true

  log "fetching the crates in Cargo.lock"
  if ! (cd "$ROOT" && timeout 600 cargo fetch --locked); then
    log "cargo fetch failed"
    return 1
  fi

  log "installing build tools (best effort)"
  export CARGO_TARGET_DIR="$STORE/tool-target"
  if ! command -v dx >/dev/null 2>&1; then
    if command -v curl >/dev/null 2>&1; then
      timeout 300 sh -c 'curl -sSL https://dioxus.dev/install.sh | bash -s v0.7.10' ||
        log "warning: dx install failed"
    fi
  fi
  if ! command -v cargo-nextest >/dev/null 2>&1; then
    timeout 600 cargo install cargo-nextest --locked ||
      log "warning: nextest install failed"
  fi
  if ! command -v cargo-deny >/dev/null 2>&1; then
    timeout 600 cargo install cargo-deny --locked ||
      log "warning: cargo-deny install failed"
  fi
  if ! command -v worker-build >/dev/null 2>&1; then
    timeout 600 cargo install worker-build --version 0.8.7 --locked ||
      log "warning: worker-build install failed"
  fi
  unset CARGO_TARGET_DIR
  return 0
}

restore_target() {
  mkdir -p "$STORE/notebox-target"
  if [ ! -f "$TARGET" ]; then
    log "no target seed; the first build will be cold"
    return 0
  fi

  local manifest="$TARGET.manifest"
  local want_rustc want_lock want_tc want_content
  want_rustc="$(manifest_get "$manifest" rustc || true)"
  want_lock="$(manifest_get "$manifest" lock || true)"
  want_tc="$(manifest_get "$manifest" toolchain || true)"
  want_content="$(manifest_get "$manifest" content_mb || true)"
  [ -n "$want_rustc" ] && [ -n "$want_lock" ] && [ -n "$want_tc" ] || {
    log "target seed has no fingerprint; skipping it"
    return 0
  }

  local have_rustc have_lock have_tc
  have_rustc="$(rustc -V 2>/dev/null || true)"
  have_lock="$(sha256sum "$ROOT/Cargo.lock" | awk '{print $1}')"
  have_tc="$(sha256sum "$ROOT/rust-toolchain.toml" | awk '{print $1}')"
  if [ "$want_rustc" != "$have_rustc" ] || [ "$want_lock" != "$have_lock" ] ||
     [ "$want_tc" != "$have_tc" ]; then
    log "target seed does not match rustc/Cargo.lock/rust-toolchain.toml; skipping it"
    return 0
  fi

  if [ -n "$want_content" ] &&
     [ "$(tmp_free_mb)" -lt "$((want_content + 512))" ]; then
    log "only $(tmp_free_mb) MiB free in $STORE; skipping target seed"
    return 0
  fi

  if ! sha_check "$TARGET" "$TARGET.sha256"; then
    log "target seed checksum failed; discarding it"
    rm -f "$TARGET" "$TARGET.sha256" "$TARGET.manifest"
    return 0
  fi

  local tmp="$STORE/.restore-target.$$"
  rm -rf "$tmp"
  mkdir -p "$tmp"
  log "restoring target seed ($(file_mb "$TARGET") MiB)"
  if ! xz -dc -T0 "$TARGET" | tar -xf - -C "$tmp" --no-same-owner; then
    rm -rf "$tmp"
    log "target seed extraction failed; the first build will be cold"
    return 0
  fi
  if [ ! -d "$tmp/notebox-target" ]; then
    rm -rf "$tmp"
    log "target seed did not contain notebox-target"
    return 0
  fi

  rm -rf "$STORE/notebox-target.old"
  if [ -e "$STORE/notebox-target" ]; then
    mv "$STORE/notebox-target" "$STORE/notebox-target.old"
  fi
  if mv "$tmp/notebox-target" "$STORE/notebox-target"; then
    rm -rf "$STORE/notebox-target.old" "$tmp"
    log "target restored"
    return 0
  fi
  rm -rf "$tmp"
  if [ -e "$STORE/notebox-target.old" ]; then
    mv "$STORE/notebox-target.old" "$STORE/notebox-target"
  fi
  log "target swap failed; the first build will be cold"
  return 0
}

if ! restore_base; then
  log "base restore failed; leaving the tree unrepaired"
  exit 1
fi

# A base seed from an older Cargo.lock can be usable but incomplete. Resolve
# offline first; only touch the network when the cache truly lacks a crate.
if ! (cd "$ROOT" && cargo metadata --locked --offline >/dev/null 2>&1); then
  log "registry cache is incomplete; fetching Cargo.lock"
  (cd "$ROOT" && timeout 600 cargo fetch --locked) ||
    log "warning: cargo fetch failed; the build may need network access"
fi

restore_target

# 3. Make sure the layout survived the base replacement.
"$ROOT/scripts/relocate-caches.sh" --repair "$STORE_HOME" >/dev/null 2>&1 || true

if [ ! -f "$ROOT/.cargo/config.toml" ]; then
  "$ROOT/scripts/relocate-caches.sh" --force "$STORE_HOME" >/dev/null 2>&1 || true
fi

{
  printf 'boot=%s\n' "$(cat /proc/sys/kernel/random/boot_id 2>/dev/null || date +%s)"
  printf 'time=%s\n' "$(date -u '+%Y-%m-%dT%H:%M:%SZ')"
  printf 'rustc=%s\n' "$(rustc -V 2>/dev/null || true)"
  printf 'base=%s\n' "$( [ -f "$BASE" ] && file_mb "$BASE" || echo missing )"
  printf 'target=%s\n' "$( [ -f "$TARGET" ] && file_mb "$TARGET" || echo missing )"
} >"$READY.tmp"
mv "$READY.tmp" "$READY"

log "ready; home free $(free_mb) MiB"
