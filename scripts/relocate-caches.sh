#!/usr/bin/env bash
# Keep Cargo, Rust, npm, yarn, worker-build, dx and Playwright caches off a small
# /home partition. This workspace's home partition is 4.8 GB while the overlay
# disk has tens of GB free.
#
# Usage:
#   scripts/relocate-caches.sh [--repair] [--force] [STORE]
#
# STORE defaults to /tmp/opencode/home. Without --repair or --force the script
# does nothing when /home still has more than 2 GiB free, so it is safe to keep
# in a shell profile or a setup step.
#
# --repair  recreate missing /tmp targets and repair dangling symlinks even
#           when /home has plenty of free space. It may move a real cache
#           directory to STORE when that directory is the only copy, so a
#           boot after /tmp was cleared cannot leave build caches in /home.
# --force   relocate even when /home is not tight (implies --repair).
set -euo pipefail

# Never let BASH_ENV start a second bootstrap while this script holds the lock.
unset BASH_ENV

force=0
repair=0
while [ "$#" -gt 0 ]; do
  case "$1" in
    --force)
      force=1
      repair=1
      shift
      ;;
    --repair)
      repair=1
      shift
      ;;
    -h|--help)
      sed -n '2,16p' "$0" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    --*)
      echo "relocate-caches: unknown option: $1" >&2
      exit 2
      ;;
    *)
      break
      ;;
  esac
done

store="${1:-${NOTEBOX_BUILD_HOME:-/tmp/opencode}/home}"

case "$store" in
  /*) ;;
  *)
    echo "relocate-caches: STORE must be an absolute path: $store" >&2
    exit 2
    ;;
esac

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
target_store="${NOTEBOX_TARGET_DIR:-${NOTEBOX_BUILD_HOME:-$(dirname "$store")}/notebox-target}"
pw_store="${NOTEBOX_PLAYWRIGHT_DIR:-${NOTEBOX_BUILD_HOME:-$(dirname "$store")}/pw-browsers}"

case "$target_store" in
  /*) ;;
  *)
    echo "relocate-caches: NOTEBOX_TARGET_DIR must be absolute: $target_store" >&2
    exit 2
    ;;
esac

mkdir -p "$store" "$target_store"
store=$(cd "$store" && pwd)
target_store=$(cd "$target_store" && pwd)

# A boot after /tmp was cleared leaves symlinks that resolve to nothing. Even
# when /home has free space, those must be repaired before a build starts.
needs_repair=0
for src in \
  "$HOME/.cargo" "$HOME/.rustup" "$HOME/.npm" "$HOME/.dx" \
  "$HOME/.local/share/.dx" "$HOME/.cache/yarn" "$HOME/.config/yarn" \
  "$HOME/.cache/worker-build" "$HOME/.cache/ms-playwright" "$root/target"; do
  if [ -L "$src" ] && [ ! -e "$src" ]; then
    needs_repair=1
  elif [ ! -e "$src" ] && [ ! -L "$src" ]; then
    needs_repair=1
  fi
done

home_avail=$(df -Pm "$HOME" | awk 'NR == 2 { print $4 }')
if [ "$force" -eq 0 ] && [ "$repair" -eq 0 ] && [ "$needs_repair" -eq 0 ] &&
   [ "${home_avail:-0}" -gt 2048 ]; then
  echo "relocate-caches: $HOME has ${home_avail} MiB free; layout is healthy (use --repair to check it anyway)"
  exit 0
fi

link_dir() {
  local src="$1"
  local dst="$2"

  mkdir -p "$(dirname "$src")"

  if [ -L "$src" ]; then
    local target
    target=$(readlink "$src")
    if [ -e "$src" ]; then
      echo "relocate-caches: already linked $src -> $target"
      return
    fi
    if [ "$target" != "$dst" ]; then
      echo "relocate-caches: repointing dangling $src -> $dst (was $target)"
      rm -f "$src"
      mkdir -p "$dst"
      ln -s "$dst" "$src"
      return
    fi
    echo "relocate-caches: recreating missing target $target for $src"
    mkdir -p "$target"
    return
  fi

  if [ -d "$src" ]; then
    if [ -d "$dst" ] && [ -n "$(find "$dst" -mindepth 1 -maxdepth 1 -print -quit 2>/dev/null)" ]; then
      echo "relocate-caches: both $src and $dst exist; merge them manually" >&2
      exit 1
    fi
    if [ -d "$dst" ]; then
      rmdir "$dst"
    fi
    mv "$src" "$dst"
    echo "relocate-caches: moved $src -> $dst"
  elif [ -e "$src" ]; then
    echo "relocate-caches: $src is not a directory; refusing to move it" >&2
    exit 1
  else
    mkdir -p "$dst"
    echo "relocate-caches: created $dst"
  fi

  ln -s "$dst" "$src"
  echo "relocate-caches: linked $src -> $dst"
}

link_dir "$HOME/.cargo" "$store/cargo"
link_dir "$HOME/.rustup" "$store/rustup"
link_dir "$HOME/.npm" "$store/npm"
link_dir "$HOME/.dx" "$store/dx"
link_dir "$HOME/.local/share/.dx" "$store/dx-tools"
link_dir "$HOME/.cache/yarn" "$store/yarn"
link_dir "$HOME/.config/yarn" "$store/yarn-config"
link_dir "$HOME/.cache/worker-build" "$store/worker-build"

# Browser downloads are large and are only needed for local web checks. Create
# the target even when it is empty so Playwright can install into it later.
mkdir -p "$HOME/.cache"
if [ -L "$HOME/.cache/ms-playwright" ] && [ ! -e "$HOME/.cache/ms-playwright" ]; then
  rm -f "$HOME/.cache/ms-playwright"
fi
if [ ! -L "$HOME/.cache/ms-playwright" ]; then
  if [ -e "$HOME/.cache/ms-playwright" ]; then
    if [ -d "$HOME/.cache/ms-playwright" ] &&
       [ -z "$(find "$HOME/.cache/ms-playwright" -mindepth 1 -maxdepth 1 -print -quit 2>/dev/null)" ]; then
      rmdir "$HOME/.cache/ms-playwright"
    elif [ "$repair" -eq 1 ]; then
      mkdir -p "$pw_store"
      if [ -z "$(find "$pw_store" -mindepth 1 -maxdepth 1 -print -quit 2>/dev/null)" ]; then
        rmdir "$pw_store"
        mv "$HOME/.cache/ms-playwright" "$pw_store"
      else
        echo "relocate-caches: merge $HOME/.cache/ms-playwright manually" >&2
        exit 1
      fi
    else
      echo "relocate-caches: merge $HOME/.cache/ms-playwright manually" >&2
      exit 1
    fi
  fi
  mkdir -p "$pw_store"
  ln -s "$pw_store" "$HOME/.cache/ms-playwright"
  echo "relocate-caches: linked $HOME/.cache/ms-playwright -> $pw_store"
fi

# Cargo's target directory is per checkout, not a cache directory. Keep it on
# the large disk too and make the setting local to this machine.
if [ -L "$root/target" ]; then
  if [ "$(readlink "$root/target")" != "$target_store" ]; then
    echo "relocate-caches: $root/target points elsewhere: $(readlink "$root/target")" >&2
    exit 1
  fi
elif [ -d "$root/target" ]; then
  if [ -n "$(find "$root/target" -mindepth 1 -maxdepth 1 -print -quit 2>/dev/null)" ]; then
    if [ -n "$(find "$target_store" -mindepth 1 -maxdepth 1 -print -quit 2>/dev/null)" ]; then
      echo "relocate-caches: both $root/target and $target_store exist; merge them manually while no build is running" >&2
      exit 1
    fi
    rmdir "$target_store"
    mv "$root/target" "$target_store"
    echo "relocate-caches: moved $root/target -> $target_store"
  fi
  rmdir "$root/target" 2>/dev/null || true
  ln -s "$target_store" "$root/target"
  echo "relocate-caches: linked $root/target -> $target_store"
else
  ln -s "$target_store" "$root/target"
  echo "relocate-caches: linked $root/target -> $target_store"
fi

mkdir -p "$root/.cargo"
config="$root/.cargo/config.toml"
config_ok=0
if [ -f "$config" ] && grep -Fq "target-dir = \"$target_store\"" "$config"; then
  config_ok=1
fi
if [ "$config_ok" -eq 0 ]; then
  if [ -f "$config" ]; then
    if grep -Fq "Generated by scripts/relocate-caches.sh" "$config"; then
      rm -f "$config"
    elif [ "$repair" -eq 1 ]; then
      mv "$config" "$config.bak"
      echo "relocate-caches: moved the existing $config to $config.bak" >&2
    else
      echo "relocate-caches: $config exists and points elsewhere; leaving it alone" >&2
    fi
  fi
  if [ ! -f "$config" ]; then
    cat > "$config" <<EOF
# Machine-local: keep Cargo build artifacts off a small /home partition.
# Generated by scripts/relocate-caches.sh; untracked via .git/info/exclude.
[build]
target-dir = "$target_store"
EOF
    echo "relocate-caches: wrote $config"
  fi
fi

exclude="$root/.git/info/exclude"
if [ -f "$exclude" ] && ! grep -qx '.cargo/config.toml' "$exclude"; then
  printf '\n# Machine-local build paths\n.cargo/config.toml\n' >> "$exclude"
fi

df -h "$HOME" | tail -1
echo "relocate-caches: done"
