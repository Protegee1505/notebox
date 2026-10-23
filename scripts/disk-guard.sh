#!/usr/bin/env bash
# Keep every Cargo target off /home and repair worktree target links.
#
#   scripts/disk-guard.sh --status          list checkouts and target locations
#   scripts/disk-guard.sh --check            fail if a target resolves into /home
#   scripts/disk-guard.sh --check --strict   also enforce the 1200 MiB reserve
#   scripts/disk-guard.sh --ensure           create/repair safe target symlinks
#   scripts/disk-guard.sh --fix              move a target found in /home
#   scripts/disk-guard.sh ... --root PATH     limit the operation to one checkout
#
# `--fix` refuses to move anything while cargo/rustc/dx is running. It moves a
# real target into the ephemeral store (preserving the build cache) and leaves
# a symlink behind. Boot runs --fix; web-build.sh runs --ensure + --check.
#
# This is Cloud Shell-specific. When the /tmp/opencode store is absent (CI,
# a normal workstation) the script exits 0 so it cannot break other builds.
set -euo pipefail
unset BASH_ENV

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
. "$ROOT/scripts/target-path.sh"

STORE="$(_nbx_store)"
RESERVE_MB="${NOTEBOX_HOME_RESERVE_MB:-1200}"
mode="status"
quiet=0
strict=0
only_root=""

usage() {
  sed -n '2,16p' "$0" | sed 's/^# \{0,1\}//'
}

log() {
  if [ "$quiet" -eq 0 ]; then
    printf 'disk-guard: %s\n' "$*" >&2
  fi
}

die() {
  printf 'disk-guard: %s\n' "$*" >&2
  exit 1
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --status) mode="status"; shift ;;
    --check) mode="check"; shift ;;
    --ensure) mode="ensure"; shift ;;
    --fix) mode="fix"; shift ;;
    --strict) strict=1; shift ;;
    --quiet) quiet=1; shift ;;
    --root)
      [ "$#" -ge 2 ] || die "--root needs a directory"
      only_root="$(readlink -f "$2" 2>/dev/null || true)"
      [ -n "$only_root" ] || die "--root directory is not accessible: $2"
      shift 2
      ;;
    -h|--help) usage; exit 0 ;;
    *) die "unknown option: $1" ;;
  esac
done

if [ "${NOTEBOX_DISK_GUARD:-1}" = "0" ]; then
  exit 0
fi

if [ ! -d "$STORE" ] && [ "${NOTEBOX_DISK_GUARD_FORCE:-0}" != "1" ]; then
  if [ "$mode" = "status" ]; then
    printf 'disk-guard: %s is absent; Cloud Shell target guard inactive\n' "$STORE"
  fi
  exit 0
fi

free_home_mb() {
  df -Pm "$HOME" 2>/dev/null | awk 'NR == 2 { print $4 }'
}

free_store_mb() {
  df -Pm "$STORE" 2>/dev/null | awk 'NR == 2 { print $4 }'
}

dir_mb() {
  local dir="$1"
  [ -d "$dir" ] || { echo 0; return; }
  du -sm "$dir" 2>/dev/null | awk '{ print $1 + 0 }'
}

is_busy() {
  local proc
  for proc in cargo rustc dx cargo-nextest worker-build wasm-bindgen wasm-opt esbuild; do
    pgrep -u "$(id -u)" -x "$proc" >/dev/null 2>&1 && return 0
  done
  return 1
}

declare -a repos=()
declare -A seen=()

add_repo() {
  local dir="${1%/}"
  [ -n "$dir" ] || return 0
  [ -d "$dir" ] || return 0
  [ -f "$dir/Cargo.toml" ] || return 0
  case "$dir" in
    "$HOME"|"$HOME"/*) ;;
    *) return 0 ;;
  esac
  if [ -n "$only_root" ] && [ "$(readlink -f "$dir" 2>/dev/null || true)" != "$only_root" ]; then
    return 0
  fi
  _nbx_under_store "$dir" && return 0
  [ -n "${seen[$dir]:-}" ] && return 0
  seen[$dir]=1
  repos+=("$dir")
  return 0
}

discover_repos() {
  local dir line

  if command -v git >/dev/null 2>&1 &&
     { [ -d "$HOME/notebox/.git" ] || [ -f "$HOME/notebox/.git" ]; }; then
    while IFS= read -r line; do
      case "$line" in
        "worktree "*) add_repo "${line#worktree }" ;;
      esac
    done < <(git -C "$HOME/notebox" worktree list --porcelain 2>/dev/null || true)
  fi

  # Separate clones that sit directly under the home directory.
  for dir in "$HOME"/*/; do
    dir="${dir%/}"
    if [ -e "$dir/.git" ] || [ -L "$dir/.git" ]; then
      add_repo "$dir"
    fi
  done

  # OpenChamber-created worktrees, even if the main worktree list is stale.
  for dir in "$HOME"/.local/share/opencode/worktree/*/*/; do
    dir="${dir%/}"
    add_repo "$dir"
  done

  add_repo "$HOME/notebox"
  if [ -n "$only_root" ]; then
    add_repo "$only_root"
  fi
}

target_state() {
  local repo="$1" target="$repo/target" resolved
  if [ -L "$target" ]; then
    resolved="$(readlink -f "$target" 2>/dev/null || true)"
    if [ -z "$resolved" ]; then
      echo "dangling"
    elif _nbx_under_home "$resolved"; then
      echo "home-symlink"
    elif [ -e "$resolved" ] && [ ! -d "$resolved" ]; then
      echo "other"
    else
      echo "safe-symlink"
    fi
  elif [ -d "$target" ]; then
    echo "home-dir"
  elif [ -e "$target" ]; then
    echo "other"
  else
    echo "absent"
  fi
}

show_status() {
  local repo state resolved size
  printf 'home free: %s MiB (reserve %s MiB)\n' "$(free_home_mb)" "$RESERVE_MB"
  for repo in "${repos[@]}"; do
    state="$(target_state "$repo")"
    case "$state" in
      safe-symlink)
        resolved="$(readlink -f "$repo/target" 2>/dev/null || true)"
        size="$(dir_mb "$resolved")"
        printf '%s: symlink -> %s (%s MiB)\n' "$repo" "$resolved" "$size"
        ;;
      home-dir)
        printf '%s: DIRECTORY on /home (%s MiB)\n' "$repo" "$(dir_mb "$repo/target")"
        ;;
      home-symlink)
        resolved="$(readlink -f "$repo/target" 2>/dev/null || true)"
        printf '%s: symlink into /home -> %s\n' "$repo" "$resolved"
        ;;
      absent)
        printf '%s: no target yet\n' "$repo"
        ;;
      *)
        printf '%s: %s\n' "$repo" "$state"
        ;;
    esac
  done
}

check_targets() {
  local repo state failures=0
  for repo in "${repos[@]}"; do
    state="$(target_state "$repo")"
    case "$state" in
      home-dir)
        printf 'disk-guard: %s/target is a real directory on /home; run scripts/disk-guard.sh --fix\n' \
          "$repo" >&2
        failures=1
        ;;
      home-symlink)
        printf 'disk-guard: %s/target points into /home; run scripts/disk-guard.sh --fix\n' \
          "$repo" >&2
        failures=1
        ;;
      other)
        printf 'disk-guard: %s/target is not a directory; remove it manually\n' "$repo" >&2
        failures=1
        ;;
    esac
  done

  if [ "$strict" -eq 1 ]; then
    local free
    free="$(free_home_mb)"
    if [ -z "$free" ] || [ "$free" -lt "$RESERVE_MB" ]; then
      printf 'disk-guard: /home has only %s MiB free; the reserve is %s MiB\n' \
        "${free:-0}" "$RESERVE_MB" >&2
      failures=1
    fi
  fi
  return "$failures"
}

link_target() {
  local repo="$1" safe="$2"
  if [ -e "$repo/target" ] || [ -L "$repo/target" ]; then
    return 1
  fi
  mkdir -p "$safe"
  ln -s "$safe" "$repo/target"
  log "linked $repo/target -> $safe"
  return 0
}

move_home_target() {
  local repo="$1" safe="$2" target="$repo/target"
  local size_mb store_free

  if [ "$(readlink -f "$target" 2>/dev/null || true)" = "$(readlink -f "$safe" 2>/dev/null || true)" ]; then
    return 0
  fi

  size_mb="$(dir_mb "$target")"
  store_free="$(free_store_mb)"
  if [ -z "$store_free" ] || [ "$store_free" -lt "$((size_mb + 512))" ]; then
    log "not enough space in $STORE for $target (${size_mb} MiB)"
    return 1
  fi

  if [ -L "$safe" ]; then
    log "$safe is a symlink; resolve it manually before moving $target"
    return 1
  fi
  if [ -d "$safe" ] && [ -n "$(find "$safe" -mindepth 1 -maxdepth 1 -print -quit 2>/dev/null)" ]; then
    # A newer build already populated the safe path (for example the shell
    # guard redirected a second agent while the first build was still using the
    # home target). Keep the old home cache in the store for inspection and
    # point the checkout at the live safe cache.
    local quarantine="${safe}.home-$(date +%Y%m%d%H%M%S)"
    local n=1
    while [ -e "$quarantine" ]; do
      quarantine="${safe}.home-$(date +%Y%m%d%H%M%S)-$n"
      n=$((n + 1))
    done
    mkdir -p "$(dirname "$quarantine")"
    if ! mv "$target" "$quarantine"; then
      log "could not move $target -> $quarantine"
      return 1
    fi
    if ! ln -s "$safe" "$target"; then
      log "moved $target to $quarantine but could not create the symlink"
      return 1
    fi
    log "kept the old home target at $quarantine; linked $repo/target -> $safe"
    return 0
  fi
  if [ -d "$safe" ]; then
    if ! rmdir "$safe" 2>/dev/null; then
      log "could not remove the empty directory $safe"
      return 1
    fi
  fi

  mkdir -p "$(dirname "$safe")"
  if ! mv "$target" "$safe"; then
    log "could not move $target -> $safe"
    return 1
  fi
  if ! ln -s "$safe" "$target"; then
    log "moved $target but could not create the symlink"
    return 1
  fi
  log "moved $target -> $safe"
  return 0
}

repair_targets() {
  local move="$1" repo state safe rc=0 busy=0
  if is_busy; then
    busy=1
  fi

  for repo in "${repos[@]}"; do
    state="$(target_state "$repo")"
    safe="$(_nbx_target_for "$repo" 2>/dev/null || true)"
    [ -n "$safe" ] || continue

    case "$state" in
      absent)
        link_target "$repo" "$safe" || rc=1
        ;;
      safe-symlink)
        local resolved
        resolved="$(readlink -f "$repo/target" 2>/dev/null || true)"
        if [ -n "$resolved" ] && [ ! -d "$resolved" ]; then
          mkdir -p "$resolved" 2>/dev/null || rc=1
          log "recreated missing target directory $resolved"
        fi
        ;;
      dangling)
        rm -f "$repo/target" 2>/dev/null || true
        link_target "$repo" "$safe" || rc=1
        ;;
      home-symlink)
        if [ "$move" -eq 1 ]; then
          if [ "$busy" -eq 1 ]; then
            log "a build is running; not repointing $repo/target"
            rc=1
          else
            rm -f "$repo/target" 2>/dev/null || true
            link_target "$repo" "$safe" || rc=1
          fi
        else
          log "$repo/target points into /home; run --fix when idle"
          rc=1
        fi
        ;;
      home-dir)
        if [ "$move" -eq 1 ]; then
          if [ "$busy" -eq 1 ]; then
            log "a build is running; not moving $repo/target"
            rc=1
          else
            move_home_target "$repo" "$safe" || rc=1
          fi
        else
          log "$repo/target is a real directory on /home; run --fix when idle"
          rc=1
        fi
        ;;
      *)
        log "$repo/target is not a directory; remove it manually"
        rc=1
        ;;
    esac
  done
  return "$rc"
}

discover_repos

case "$mode" in
  status)
    show_status
    ;;
  check)
    check_targets
    ;;
  ensure)
    repair_targets 0
    ;;
  fix)
    repair_targets 1
    ;;
  *)
    die "unknown mode: $mode"
    ;;
esac
