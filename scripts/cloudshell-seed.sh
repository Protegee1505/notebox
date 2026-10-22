#!/usr/bin/env bash
# Create, inspect and prune the persistent Cloud Shell build seeds.
#
#   scripts/cloudshell-seed.sh status
#   scripts/cloudshell-seed.sh save --base
#   scripts/cloudshell-seed.sh save --target
#   scripts/cloudshell-seed.sh save --all
#   scripts/cloudshell-seed.sh verify
#   scripts/cloudshell-seed.sh prune --target
#
# The seeds live in ~/.cache/notebox-build (NOTEBOX_SEED_DIR). They are
# compressed copies of the live tree in /tmp/opencode: base.tar.xz holds the
# toolchain, registry cache and tools; target.tar.xz holds a pruned Cargo target
# tree. The bootstrap script restores them after a restart.
#
# Save the base seed after changing Cargo.lock, rust-toolchain.toml, dx or
# worker-build. Save the target seed after a green gate run and a web build.
# Do not snapshot during a build.
set -euo pipefail

# Never let BASH_ENV start a second bootstrap while this script holds the lock.
unset BASH_ENV

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
STORE="${NOTEBOX_BUILD_HOME:-/tmp/opencode}"
STORE_HOME="$STORE/home"
SEED="${NOTEBOX_SEED_DIR:-$HOME/.cache/notebox-build}"
BASE="$SEED/base.tar.xz"
TARGET="$SEED/target.tar.xz"
RESERVE_MB="${NOTEBOX_HOME_RESERVE_MB:-1200}"
BASE_CAP_MB="${NOTEBOX_BASE_SEED_MAX_MB:-500}"
TARGET_CAP_MB="${NOTEBOX_TARGET_SEED_MAX_MB:-1500}"
COMPRESS_THREADS="${NOTEBOX_XZ_THREADS:-0}"

quiet=0
base_wanted=0
target_wanted=0

usage() {
  sed -n '2,20p' "$0" | sed 's/^# \{0,1\}//'
}

log() {
  if [ "$quiet" -eq 0 ]; then
    printf '%s\n' "$*" >&2
  fi
}

die() {
  printf 'cloudshell-seed: %s\n' "$*" >&2
  exit 1
}

free_mb() {
  df -Pm "$HOME" 2>/dev/null | awk 'NR == 2 { print $4 }'
}

file_mb() {
  [ -f "$1" ] || { echo 0; return; }
  stat -c %s "$1" | awk '{ print int(($1 + 1048575) / 1048576) }'
}

is_busy() {
  local proc
  for proc in cargo rustc dx cargo-nextest worker-build wasm-bindgen wasm-opt esbuild; do
    pgrep -u "$(id -u)" -x "$proc" >/dev/null 2>&1 && return 0
  done
  return 1
}

require_idle() {
  if is_busy; then
    die "a cargo/rustc/dx build is running; stop it before snapshotting"
  fi
}

make_room() {
  local need_mb="$1"
  local free
  free="$(free_mb)"
  if [ "$((free - need_mb))" -ge "$RESERVE_MB" ]; then
    return 0
  fi
  if [ -f "$TARGET" ]; then
    log "removing the old target seed to make room"
    rm -f "$TARGET" "$TARGET.sha256" "$TARGET.manifest"
    free="$(free_mb)"
  fi
  if [ "$((free - need_mb))" -ge "$RESERVE_MB" ]; then
    return 0
  fi
  if [ -f "$BASE" ]; then
    log "removing the old base seed to make room"
    rm -f "$BASE" "$BASE.sha256" "$BASE.manifest"
    free="$(free_mb)"
  fi
  if [ "$((free - need_mb))" -lt "$RESERVE_MB" ]; then
    log "need ${need_mb} MiB but only ${free} MiB is free; reserve is ${RESERVE_MB} MiB"
    return 1
  fi
}

publish_file() {
  local src="$1" dst="$2"
  cp "$src" "$dst.new"
  mv "$dst.new" "$dst"
}

publish() {
  local tmpdir="$1" name="$2"
  local archive="$tmpdir/$name.tar.xz"
  local size_mb
  size_mb="$(file_mb "$archive")"
  make_room "$((size_mb + 32))" || return 1
  if [ "$name" = "base" ]; then
    publish_file "$archive" "$BASE"
    publish_file "$archive.sha256" "$BASE.sha256"
    publish_file "$tmpdir/base.manifest" "$BASE.manifest"
  else
    publish_file "$archive" "$TARGET"
    publish_file "$archive.sha256" "$TARGET.sha256"
    publish_file "$tmpdir/target.manifest" "$TARGET.manifest"
  fi
}

write_common_manifest() {
  local file="$1" content="$2" content_mb="$3"
  local manifest="${file%.tar.xz}.manifest"
  {
    printf 'format=1\n'
    printf 'created=%s\n' "$(date -u '+%Y-%m-%dT%H:%M:%SZ')"
    printf 'rustc=%s\n' "$(rustc -V 2>/dev/null || true)"
    printf 'cargo=%s\n' "$(cargo -V 2>/dev/null || true)"
    printf 'lock=%s\n' "$(sha256sum "$ROOT/Cargo.lock" | awk '{print $1}')"
    printf 'toolchain=%s\n' "$(sha256sum "$ROOT/rust-toolchain.toml" | awk '{print $1}')"
    printf 'content=%s\n' "$content"
    printf 'content_mb=%s\n' "$content_mb"
    printf 'archive_mb=%s\n' "$(file_mb "$file")"
    printf 'archive_sha=%s\n' "$(sha256sum "$file" | awk '{print $1}')"
  } >"$manifest"
}

save_base() {
  require_idle
  [ -d "$STORE_HOME/rustup" ] || die "missing $STORE_HOME/rustup; run cloudshell-bootstrap first"
  [ -d "$STORE_HOME/cargo" ] || die "missing $STORE_HOME/cargo; run cloudshell-bootstrap first"

  local tmpdir="$STORE/.seed-base.$$"
  rm -rf "$tmpdir"
  mkdir -p "$tmpdir"

  local dirs=(home/rustup home/cargo)
  local d
  for d in home/dx home/dx-tools home/npm home/yarn; do
    [ -d "$STORE/$d" ] && dirs+=("$d")
  done

  local content_mb
  content_mb="$(cd "$STORE" && du -sm "${dirs[@]}" 2>/dev/null | awk '{s += $1} END {print s + 0}')"

  log "writing base seed from ${dirs[*]} (${content_mb} MiB uncompressed)"
  if ! (cd "$STORE" && tar -cf - \
      --exclude='home/cargo/registry/src' \
      --exclude='home/cargo/registry/src/*' \
      --exclude='home/rustup/downloads' \
      --exclude='home/rustup/downloads/*' \
      --exclude='home/rustup/tmp' \
      --exclude='home/rustup/tmp/*' \
      --exclude='home/rustup/toolchains/*/share/doc' \
      --exclude='home/rustup/toolchains/*/share/doc/*' \
      --exclude='home/npm/_npx' \
      --exclude='home/npm/_npx/*' \
      --exclude='home/npm/_logs' \
      --exclude='home/npm/_logs/*' \
      "${dirs[@]}") | xz -T"$COMPRESS_THREADS" -0 -c >"$tmpdir/base.tar.xz"; then
    rm -rf "$tmpdir"
    die "base compression failed"
  fi

  local size_mb
  size_mb="$(file_mb "$tmpdir/base.tar.xz")"
  if [ "$size_mb" -gt "$BASE_CAP_MB" ]; then
    rm -rf "$tmpdir"
    die "base seed is ${size_mb} MiB, above the ${BASE_CAP_MB} MiB cap; remove ~/.npm/_cacache or dx tools, or raise NOTEBOX_BASE_SEED_MAX_MB"
  fi

  (cd "$tmpdir" && sha256sum base.tar.xz >base.tar.xz.sha256)
  write_common_manifest "$tmpdir/base.tar.xz" "base" "$content_mb"
  publish "$tmpdir" base || { rm -rf "$tmpdir"; exit 1; }
  rm -rf "$tmpdir"
  log "base seed: $(file_mb "$BASE") MiB; home free $(free_mb) MiB"
}

save_target() {
  require_idle
  [ -d "$STORE/notebox-target" ] || die "missing $STORE/notebox-target; build the workspace first"

  local tmpdir="$STORE/.seed-target.$$"
  rm -rf "$tmpdir"
  mkdir -p "$tmpdir"

  local content_mb
  content_mb="$(du -sm "$STORE/notebox-target" 2>/dev/null | awk '{print $1}')"
  local size_mb=0
  local content="full"
  local excludes=(
    --exclude='notebox-target/debug/incremental'
    --exclude='notebox-target/debug/incremental/*'
    --exclude='notebox-target/wasm32-unknown-unknown/debug/incremental'
    --exclude='notebox-target/wasm32-unknown-unknown/debug/incremental/*'
    --exclude='notebox-target/wasm-release/incremental'
    --exclude='notebox-target/wasm-release/incremental/*'
    --exclude='notebox-target/debug/examples'
    --exclude='notebox-target/debug/examples/*'
    --exclude='notebox-target/tmp'
    --exclude='notebox-target/tmp/*'
    --exclude='notebox-target/doc'
    --exclude='notebox-target/doc/*'
    --exclude='notebox-target/*/.cargo-lock'
  )

  log "writing target seed (${content_mb} MiB uncompressed)"
  if ! (cd "$STORE" && tar -cf - "${excludes[@]}" notebox-target) |
      xz -T"$COMPRESS_THREADS" -0 -c >"$tmpdir/target.tar.xz"; then
    rm -rf "$tmpdir"
    die "target compression failed"
  fi
  size_mb="$(file_mb "$tmpdir/target.tar.xz")"

  if [ "$size_mb" -gt "$TARGET_CAP_MB" ] &&
     [ -d "$STORE/notebox-target/wasm-release" ]; then
    log "full target seed is ${size_mb} MiB; retrying without wasm-release"
    rm -f "$tmpdir/target.tar.xz"
    content="no-wasm"
    if ! (cd "$STORE" && tar -cf - "${excludes[@]}" \
        --exclude='notebox-target/wasm-release' \
        --exclude='notebox-target/wasm-release/*' notebox-target) |
        xz -T"$COMPRESS_THREADS" -0 -c >"$tmpdir/target.tar.xz"; then
      rm -rf "$tmpdir"
      die "target compression failed"
    fi
    size_mb="$(file_mb "$tmpdir/target.tar.xz")"
  fi

  if [ "$size_mb" -gt "$TARGET_CAP_MB" ]; then
    rm -rf "$tmpdir"
    die "target seed is ${size_mb} MiB, above the ${TARGET_CAP_MB} MiB cap; keep only the host debug tree or raise NOTEBOX_TARGET_SEED_MAX_MB"
  fi

  (cd "$tmpdir" && sha256sum target.tar.xz >target.tar.xz.sha256)
  write_common_manifest "$tmpdir/target.tar.xz" "$content" "$content_mb"
  publish "$tmpdir" target || { rm -rf "$tmpdir"; exit 1; }
  rm -rf "$tmpdir"
  log "target seed: $(file_mb "$TARGET") MiB (${content}); home free $(free_mb) MiB"
}

manifest_get() {
  local file="$1" key="$2"
  [ -f "$file" ] || return 1
  sed -n "s/^${key}=//p" "$file" | head -1
}

seed_state() {
  local archive="$1"
  local manifest="$archive.manifest"
  local want_rustc want_lock want_tc
  if [ ! -f "$manifest" ]; then
    echo "no manifest"
    return
  fi
  want_rustc="$(manifest_get "$manifest" rustc || true)"
  want_lock="$(manifest_get "$manifest" lock || true)"
  want_tc="$(manifest_get "$manifest" toolchain || true)"
  if [ -z "$want_rustc" ] || [ -z "$want_lock" ] || [ -z "$want_tc" ]; then
    echo "no fingerprint"
    return
  fi
  if [ "$want_rustc" != "$(rustc -V 2>/dev/null || true)" ]; then
    echo "stale: rustc changed"
  elif [ "$want_lock" != "$(sha256sum "$ROOT/Cargo.lock" | awk '{print $1}')" ]; then
    echo "stale: Cargo.lock changed"
  elif [ "$want_tc" != "$(sha256sum "$ROOT/rust-toolchain.toml" | awk '{print $1}')" ]; then
    echo "stale: rust-toolchain.toml changed"
  else
    echo "current"
  fi
}

verify_seed() {
  local archive="$1" sidecar="$2" label="$3"
  if [ ! -f "$archive" ]; then
    log "$label seed: missing"
    return 1
  fi
  if [ ! -f "$sidecar" ]; then
    log "$label seed: missing $sidecar"
    return 1
  fi
  if (cd "$(dirname "$archive")" && sha256sum -c "$(basename "$sidecar")" >/dev/null 2>&1); then
    log "$label seed: OK ($(file_mb "$archive") MiB)"
    return 0
  fi
  log "$label seed: checksum mismatch"
  return 1
}

show_status() {
  printf 'store: %s\nseed:  %s\n' "$STORE" "$SEED"
  printf 'home free: %s MiB\n' "$(free_mb)"
  for f in "$BASE" "$TARGET"; do
    if [ -f "$f" ]; then
      local content state
      content="$(manifest_get "$f.manifest" content || true)"
      state="$(seed_state "$f")"
      printf '%s: %s MiB (%s, %s)\n' \
        "$(basename "$f")" "$(file_mb "$f")" "${content:-unknown}" "$state"
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
  if [ -f "$STORE/.notebox-ready" ]; then
    printf 'ready: yes (%s)\n' "$(head -1 "$STORE/.notebox-ready")"
  else
    printf 'ready: no\n'
  fi
}

action="${1:-status}"
[ "$#" -gt 0 ] && shift
while [ "$#" -gt 0 ]; do
  case "$1" in
    --base) base_wanted=1; shift ;;
    --target) target_wanted=1; shift ;;
    --all) base_wanted=1; target_wanted=1; shift ;;
    --quiet) quiet=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) die "unknown option: $1" ;;
  esac
done

mkdir -p "$SEED"

case "$action" in
  status)
    show_status
    ;;
  save)
    if [ "$base_wanted" -eq 0 ] && [ "$target_wanted" -eq 0 ]; then
      base_wanted=1
      target_wanted=1
    fi
    if [ "$base_wanted" -eq 1 ]; then
      save_base
    fi
    if [ "$target_wanted" -eq 1 ]; then
      save_target
    fi
    ;;
  verify)
    rc=0
    verify_seed "$BASE" "$BASE.sha256" base || rc=1
    verify_seed "$TARGET" "$TARGET.sha256" target || rc=1
    exit "$rc"
    ;;
  prune)
    if [ "$base_wanted" -eq 0 ] && [ "$target_wanted" -eq 0 ]; then
      target_wanted=1
    fi
    if [ "$target_wanted" -eq 1 ]; then
      rm -f "$TARGET" "$TARGET.sha256" "$TARGET.manifest"
      log "removed target seed"
    fi
    if [ "$base_wanted" -eq 1 ]; then
      rm -f "$BASE" "$BASE.sha256" "$BASE.manifest"
      log "removed base seed"
    fi
    show_status
    ;;
  *)
    usage
    exit 2
    ;;
esac
