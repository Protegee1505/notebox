#!/usr/bin/env bash
# Keep Cargo targets off the small /home partition for every checkout.
#
# Source this file from ~/.config/notebox/bash-env.sh (non-interactive agent
# shells) and ~/.bashrc (interactive shells). It defines wrappers for cargo,
# dx and worker-build. Before a build it:
#   1. finds the Cargo workspace root for the current directory;
#   2. points CARGO_TARGET_DIR at /tmp/opencode for that checkout;
#   3. creates the checkout's `target` symlink if it is missing.
#
# A git worktree does not share the main checkout's untracked
# .cargo/config.toml. That is how ~/notebox-settings once wrote a 2.8 GB target
# tree into a 4.8 GB /home. This hook is the part that covers new worktrees.
#
# It must stay silent and cheap at shell start. Work happens only when cargo,
# dx or worker-build is actually invoked.

_nbx_guard_script="${BASH_SOURCE[0]:-}"
_nbx_guard_dir=""
if [ -n "$_nbx_guard_script" ]; then
  _nbx_guard_dir="$(cd "$(dirname "$_nbx_guard_script")" 2>/dev/null && pwd)" || _nbx_guard_dir=""
  if [ -n "$_nbx_guard_dir" ] && [ -r "$_nbx_guard_dir/target-path.sh" ]; then
    . "$_nbx_guard_dir/target-path.sh"
  fi
fi
unset _nbx_guard_script _nbx_guard_dir

_nbx_guard_enabled() {
  [ "${NOTEBOX_DISK_GUARD:-1}" != "0" ] || return 1
  [ -d "$(_nbx_store)" ] || return 1
  return 0
}

_nbx_guard_repo_root() {
  local manifest
  manifest="$(command cargo locate-project --workspace --message-format plain 2>/dev/null || true)"
  [ -n "$manifest" ] || return 1
  dirname "$manifest"
}

_nbx_guard_target() {
  _nbx_guard_enabled || return 0

  local root target linked
  root="$(_nbx_guard_repo_root || true)"
  [ -n "$root" ] || return 0

  case "$root" in
    "$HOME"|"$HOME"/*) ;;
    *) return 0 ;;
  esac
  _nbx_under_store "$root" && return 0

  target="$(_nbx_target_for "$root" || true)"
  [ -n "$target" ] || return 0
  _nbx_under_home "$target" && return 0

  mkdir -p "$target" 2>/dev/null || true

  if [ -L "$root/target" ]; then
    linked="$(readlink -f "$root/target" 2>/dev/null || true)"
    if [ -z "$linked" ] || _nbx_under_home "$linked"; then
      # A dangling link or a link back into /home: repoint it. The old home
      # directory (if any) is left alone; disk-guard --fix handles it.
      rm -f "$root/target" 2>/dev/null || true
      ln -s "$target" "$root/target" 2>/dev/null || true
    elif ! _nbx_under_store "$linked"; then
      # Keep a custom target directory that is already off /home.
      target="$linked"
    fi
  elif [ -e "$root/target" ]; then
    if [ "${_NBX_GUARD_WARNED:-0}" != "1" ]; then
      printf 'notebox: %s/target is a real directory on /home; run %s/scripts/disk-guard.sh --fix\n' \
        "$root" "$HOME/notebox" >&2
      _NBX_GUARD_WARNED=1
      export _NBX_GUARD_WARNED
    fi
  else
    ln -s "$target" "$root/target" 2>/dev/null || true
  fi

  export CARGO_TARGET_DIR="$target"
  export _NBX_GUARD_TARGET="$target"
  export _NBX_GUARD_ROOT="$root"
  return 0
}

if ! declare -F cargo >/dev/null 2>&1; then
  cargo() {
    _nbx_guard_target || true
    command cargo "$@"
  }
fi

if ! declare -F dx >/dev/null 2>&1; then
  dx() {
    _nbx_guard_target || true
    command dx "$@"
  }
fi

if ! declare -F worker-build >/dev/null 2>&1; then
  worker-build() {
    _nbx_guard_target || true
    command worker-build "$@"
  }
fi
