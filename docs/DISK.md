# Development disk layout

This workspace runs on a machine where `/home` is a **4.8 GB** partition and
`/` (the overlay disk) has much more space. Building Loro and Dioxus in the
default locations filled `/home` and broke a build with `No space left on
device`. Build outputs and caches must stay off `/home`.

`/tmp/opencode` is also ephemeral: it is cleared whenever the Cloud Shell VM
restarts. The persistent compressed seeds below make the first build after a
restart fast without writing build artifacts to `/home`.

## Layout

| Path | Where it lives | Why |
| --- | --- | --- |
| `~/notebox/target` | `/tmp/opencode/notebox-target` | Cargo and `dx` artifacts, several GB |
| `<worktree>/target` | `/tmp/opencode/notebox-target-<name>` or `/tmp/opencode/targets/<name>-<hash>` | every git worktree and clone; a worktree does not inherit `.cargo/config.toml` |
| `~/.cargo` | `/tmp/opencode/home/cargo` | registry cache, index, binaries |
| `~/.rustup` | `/tmp/opencode/home/rustup` | minimal toolchain and targets |
| `~/.npm` | `/tmp/opencode/home/npm` | npm and `npx` caches |
| `~/.cache/yarn` | `/tmp/opencode/home/yarn` | Yarn cache |
| `~/.config/yarn` | `/tmp/opencode/home/yarn-config` | Yarn global cache |
| `~/.cache/worker-build` | `/tmp/opencode/home/worker-build` | Worker wasm build cache |
| `~/.dx` | `/tmp/opencode/home/dx` | Dioxus CLI data |
| `~/.local/share/.dx` | `/tmp/opencode/home/dx-tools` | Dioxus tools (wasm-bindgen, esbuild, wasm-opt) |
| `~/.cache/ms-playwright` | `/tmp/opencode/pw-browsers` | Chromium for web smoke tests |
| `~/.cargo/config.toml` | local, untracked | pins `build.target-dir` outside home; main checkout only, not inherited by worktrees |

`scripts/relocate-caches.sh` creates or repairs this layout. It is idempotent.
`--repair` repairs dangling links and missing `/tmp` targets even when `/home`
has plenty of free space; `--force` also relocates a real cache directory that
is still in `/home`. The base seed leaves out the worker-build and Yarn global
caches; they refill on demand after a restart.

```bash
./scripts/relocate-caches.sh --repair
```

## Worktrees and clones

A git worktree shares tracked files, not the main checkout's untracked
`.cargo/config.toml`. On 2026-10-23 a build in `~/notebox-settings` therefore
used its own `target/`; that directory reached **2.8 GiB** and filled the
4.8 GiB `/home` partition while the main checkout's target was safely in
`/tmp`. The build failed with `No space left on device`.

Three layers now cover worktrees and clones:

- `scripts/build-guard.sh` is sourced by `~/.config/notebox/bash-env.sh`
  (non-interactive agent shells) and `~/.bashrc` (interactive shells). It wraps
  `cargo`, `dx` and `worker-build`; before the first build it finds the Cargo
  workspace root, creates the checkout's `target` symlink if missing, and
  exports `CARGO_TARGET_DIR` under `/tmp/opencode`.
- `scripts/disk-guard.sh` sweeps the main worktree list, direct child clones and
  OpenChamber worktrees. `--ensure` creates or repairs safe links, `--check`
  fails if any target resolves into `/home`, and `--fix` moves a real target
  into `/tmp` (preserving the cache) and leaves a symlink. If the safe path
  already has a live cache, the old home tree is kept at
  `<safe>.home-<timestamp>` instead of merging two targets.
- `.cargo/config.toml` remains the durable setting for the main checkout.

`--fix` refuses to move anything while `cargo`, `rustc`, `dx`, `nextest` or
`worker-build` is running. Run it when the build is idle:

```bash
./scripts/disk-guard.sh --status
./scripts/disk-guard.sh --check
./scripts/disk-guard.sh --fix
./scripts/disk-guard.sh --check --root ~/notebox   # one checkout
```

The boot hook runs `--fix` before its early-exit path. `scripts/web-build.sh`
runs `--ensure` and `--check --strict` for its own checkout, and
`scripts/budget-check` runs the global `--check` as part of the gate.

## Persistent seeds

The seeds live in `~/.cache/notebox-build` (`NOTEBOX_SEED_DIR`):

| Seed | Contains | Cap |
| --- | --- | --- |
| `base.tar.xz` | minimal Rust toolchain, registry cache/index, `dx`, `cargo-nextest`, `cargo-deny`, `worker-build`, dx tools, npm cache | 500 MiB |
| `target.tar.xz` | pruned Cargo target tree (no incremental, examples, tmp or doc) | 1.5 GiB |

Both have a `.sha256` sidecar and a `.manifest` fingerprint. Measured on
2026-10-22: the base archive is **460 MiB** and the full target archive is
**1000 MiB** (6046 MiB uncompressed). The base seed deliberately leaves out
`cargo/registry/src`, Rust docs, rustup downloads, `npm/_npx` and
`npm/_logs`; Cargo and `npx` refill those on demand. Playwright browsers are
not in either seed; the first browser suite after a restart downloads them
into `/tmp/opencode/pw-browsers`. With both seeds, `/home` kept **1860 MiB**
free.

On 2026-10-23 the settings worktree target alone was **2.8 GiB** (2.0 GiB
under `debug/deps`, 352 MiB incremental), which filled the 4.8 GiB partition.
The same build left **360 MiB** in `~/.cache/worker-build` and **282 MiB** in
`~/.config/yarn`; the cache repair did not know those two paths. After moving
the target to `/tmp/opencode/notebox-target-settings`, relocating both caches
and repairing the links, `/home` was back to **13% used (4.0 GiB free)** before
the seeds were restored.

## Restore flow

On boot, `/home/kinfuny111/.customize_environment` runs
`scripts/cloudshell-bootstrap.sh --boot --quiet`. It first appends a
`boot-hook start` line with the boot id and whether `/tmp/opencode` was already
present to `~/.cache/notebox-build/boot.log`, then writes a `boot-hook finish`
line with the exit code and ready state. The script:

1. repairs the symlinks in `$HOME` and runs `scripts/disk-guard.sh --fix`
   before any tool can write to `/home`;
2. drops `target.tar.xz` first if `/home` free space is below
   `NOTEBOX_HOME_RESERVE_MB` (default **1200 MiB**);
3. extracts the base seed into `/tmp/opencode/home`, then the target seed into
   `/tmp/opencode/notebox-target`, using temporary directories and atomic
   renames;
4. verifies the target seed's rustc, `Cargo.lock` and `rust-toolchain.toml`
   fingerprint before using it;
5. writes `/tmp/opencode/.notebox-ready`.

The start line is the proof that the boot path ran; the `BASH_ENV` path is a
fallback for a first shell when it did not. A controlled reboot after changing
the hook is the only way to test the boot path itself.

The same script is available from a shell. Non-interactive agent shells get it
through `BASH_ENV=~/.config/notebox/bash-env.sh`, and interactive shells source
it from the OCS shell hook. If another restore is already running, `--wait`
waits for it instead of competing.

A measured restore on 2026-10-22 took **1m21s** for both seeds (about 25s for
the base and 53s for the target). With the target restored, the first
`cargo nextest run --workspace` took **53s** (3.6s compile plus 51.4s of tests)
and `./scripts/web-build.sh` took **23s**. From a base-only restore, the same
nextest run took **4m13s** and the web build took **6m41s**.

A real Cloud Shell restart on the same day restored automatically through the
boot hook/`BASH_ENV` in **1m19s**. The first nextest run took **36s**. The first
web build took **84s** because the restart removed system `clang`; the script
installed it on demand and the build itself took 21s, so later web builds are
back to about 20s.

```bash
./scripts/cloudshell-bootstrap.sh --status
./scripts/cloudshell-bootstrap.sh --force   # re-extract both seeds; refuses during a build
./scripts/cloudshell-bootstrap.sh --force --allow-running  # deliberate override
```

A missing or corrupt base seed is not fatal. The bootstrap removes it and falls
back to `rustup toolchain install`, `cargo fetch --locked` and best-effort tool
installs; the first build is then cold. A target seed with a bad checksum is
removed, and one that does not match the rustc, `Cargo.lock` or
`rust-toolchain.toml` fingerprint is skipped rather than trusted.

## Saving the seeds

Save them manually after the gates and the web build. Do not snapshot while
`cargo`, `rustc`, `dx`, `nextest` or `worker-build` is running; the save script
refuses if it detects one.

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace
cargo deny check
scripts/budget-check
./scripts/web-build.sh

./scripts/cloudshell-seed.sh save --base    # after a toolchain or Cargo.lock change
./scripts/cloudshell-seed.sh save --target  # after a green build
./scripts/cloudshell-seed.sh save --all     # both
./scripts/cloudshell-seed.sh verify
./scripts/cloudshell-seed.sh status         # current or stale: rustc/Cargo.lock/toolchain
```

The web release build needs `clang` for the bundled SQLite wasm build. It is a
system package, not part of the seed. `scripts/web-build.sh` installs it
on demand with non-interactive `sudo`/`apt-get` when it is missing; set
`NOTEBOX_SKIP_CLANG_INSTALL=1` to turn that off, or install it yourself:

```bash
sudo apt-get install -y clang
```

## Space policy

- Keep **1200 MiB** free on `/home`; the bootstrap and seed saves enforce it.
- `scripts/budget-check` keeps a **1200 MiB** hard floor, checks every
  worktree's target with `scripts/disk-guard.sh --check`, and is part of the
  gate.
- Every Cargo target must be a symlink or an explicit `CARGO_TARGET_DIR` under
  `/tmp/opencode`; `scripts/build-guard.sh` handles new worktrees.
- The target seed is elastic: `cloudshell-seed.sh prune --target` removes it
  first when space is tight. The base seed is kept because it makes the next
  restart usable offline.
- Never point a build cache back into `/home`, and do not commit
  `.cargo/config.toml`.

```bash
./scripts/cloudshell-seed.sh status
./scripts/cloudshell-seed.sh prune --target
df -h /home
```

## Recovery

If `/tmp/opencode` was cleared and the boot hook did not run:

```bash
./scripts/cloudshell-bootstrap.sh --force
```

The force path refuses while a build is running. Stop the build first; use
`--allow-running` only when replacing the target is intentional.

If a worktree target landed in `/home` (for example after a build in a new
clone), stop the build and run `./scripts/disk-guard.sh --fix`. It moves the
target to `/tmp/opencode` and replaces it with a symlink. If a build is still
running, the guard leaves it alone and reports the checkout.

If the base seed is corrupt, the bootstrap removes it and reinstalls the
toolchain. If only the target seed is corrupt, it is removed and the first
build is cold; rerun `save --target` afterwards. If `/home` is too full to
restore the target seed, the bootstrap skips it and continues with the base
seed.

The project source and `Cargo.lock` are in git and are not affected by a
restart. Keep at least 1.2 GiB free on `/home` for editor state, shell history
and the git index.
