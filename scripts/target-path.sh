#!/usr/bin/env bash
# Shared Cargo target-path policy for the Cloud Shell disk guards.
#
# This file is source-only. It is used by scripts/build-guard.sh (the shell
# hook that wraps cargo/dx) and scripts/disk-guard.sh (the boot/gate sweep).
# /home is a 4.8 GB partition here; Cargo target trees belong on the overlay
# disk under /tmp/opencode, never under $HOME.

# The ephemeral build store. Keep this in sync with cloudshell-bootstrap.sh.
_nbx_store() {
  printf '%s\n' "${NOTEBOX_BUILD_HOME:-/tmp/opencode}"
}

_nbx_under_home() {
  [ -n "${1:-}" ] || return 1
  case "$1" in
    "$HOME"|"$HOME"/*) return 0 ;;
  esac
  return 1
}

_nbx_under_store() {
  local store
  store="$(_nbx_store)"
  [ -n "${1:-}" ] || return 1
  case "$1" in
    "$store"|"$store"/*) return 0 ;;
  esac
  return 1
}

# _nbx_target_for ROOT
#
# Print the target directory to use for the checkout at ROOT. Prefer an
# existing safe symlink or an explicit CARGO_TARGET_DIR so a warm build cache
# is not thrown away. Otherwise derive a stable per-checkout path.
_nbx_target_for() {
  local root="${1:-}" store base suffix hash target
  store="$(_nbx_store)"
  [ -n "$root" ] || return 1

  if [ -L "$root/target" ]; then
    target="$(readlink -f "$root/target" 2>/dev/null || true)"
    if [ -n "$target" ] && ! _nbx_under_home "$target"; then
      printf '%s\n' "$target"
      return 0
    fi
  fi

  # Respect an explicit CARGO_TARGET_DIR, but not the value this guard exported
  # for a previous checkout in the same shell.
  if [ -n "${CARGO_TARGET_DIR:-}" ] &&
     [ "${CARGO_TARGET_DIR:-}" != "${_NBX_GUARD_TARGET:-}" ]; then
    target="$(readlink -f "$CARGO_TARGET_DIR" 2>/dev/null || true)"
    if [ -n "$target" ] && ! _nbx_under_home "$target"; then
      printf '%s\n' "$target"
      return 0
    fi
  fi

  base="$(basename "$root")"
  case "$base" in
    notebox)
      target="${NOTEBOX_TARGET_DIR:-$store/notebox-target}"
      if _nbx_under_home "$target"; then
        target="$store/notebox-target"
      fi
      printf '%s\n' "$target"
      return 0
      ;;
    notebox-*)
      suffix="${base#notebox-}"
      printf '%s\n' "$store/notebox-target-$suffix"
      return 0
      ;;
  esac

  suffix="$(printf '%s' "$base" | tr -c 'A-Za-z0-9._-' '-')"
  hash="$(printf '%s' "$root" | sha256sum 2>/dev/null | cut -c1-8)"
  [ -n "$hash" ] || hash="$(printf '%s' "$root" | cksum 2>/dev/null | tr ' ' '-')"
  printf '%s/targets/%s-%s\n' "$store" "$suffix" "$hash"
}
