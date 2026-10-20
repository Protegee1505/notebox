# Development disk layout

This workspace runs on a machine where `/home` is a **4.8 GB** partition and
`/` (the overlay disk) has much more space. Building Loro and Dioxus in the
default locations filled `/home` and broke a build with `No space left on
device`. Build outputs and caches must stay off `/home`.

## Current layout

| Path | Where it lives | Why |
| --- | --- | --- |
| `~/notebox/target` | `/tmp/opencode/notebox-target` | Cargo and `dx` artifacts, several GB |
| `~/.cargo` | `/tmp/opencode/home/cargo` | registry, sources, binaries |
| `~/.rustup` | `/tmp/opencode/home/rustup` | toolchains and targets |
| `~/.npm` | `/tmp/opencode/home/npm` | npm and `npx` caches |
| `~/.cache/yarn` | `/tmp/opencode/home/yarn` | Yarn cache |
| `~/.dx` | `/tmp/opencode/home/dx` | Dioxus CLI install |
| `~/.local/share/.dx` | `/tmp/opencode/home/dx-tools` | Dioxus tools (wasm-bindgen, esbuild, wasm-opt) |
| `~/.cache/ms-playwright` | `/tmp/opencode/pw-browsers` | Chromium for web smoke tests |
| `~/.cargo/config.toml` | local, untracked | pins `build.target-dir` outside home |

`scripts/relocate-caches.sh` creates or repairs this layout. It is idempotent
and does nothing when `/home` has more than 2 GiB free, unless `--force` is
passed:

```bash
./scripts/relocate-caches.sh --force
```

The script also writes the untracked `.cargo/config.toml` and adds it to
`.git/info/exclude`, so it cannot be committed by accident.

## Guard

`scripts/budget-check` is part of the required gate. It now fails when the
home partition has less than **512 MiB** free:

```text
home partition /home/kinfuny has only 120 MiB free;
move build caches off it (see docs/DISK.md)
```

This turns a future disk-full condition into an early, readable failure instead
of a linker error halfway through a build.

## If `/tmp/opencode` is cleared

The overlay paths are treated as ephemeral. If `/tmp/opencode/home` disappears,
the symlinks in `$HOME` become dangling. Recreate the targets before running a
build:

```bash
mkdir -p /tmp/opencode/home/{cargo,rustup,npm,yarn,dx,dx-tools} \
         /tmp/opencode/notebox-target /tmp/opencode/pw-browsers
```

If the Rust toolchain itself was stored there, reinstall it with `rustup`, then
rerun `scripts/relocate-caches.sh --force`. The same applies to `dx` and the
Playwright browser. The project source and `Cargo.lock` are in git and are not
affected.

## Checking

```bash
df -h /home
ls -ld ~/.cargo ~/.rustup ~/.npm ~/notebox/target
./scripts/budget-check
```

Keep at least 1 GiB free on `/home` for editor state, shell history and the git
index. The build itself should use almost none of it.
