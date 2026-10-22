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
| `~/.cargo` | `/tmp/opencode/home/cargo` | registry cache, index, binaries |
| `~/.rustup` | `/tmp/opencode/home/rustup` | minimal toolchain and targets |
| `~/.npm` | `/tmp/opencode/home/npm` | npm and `npx` caches |
| `~/.cache/yarn` | `/tmp/opencode/home/yarn` | Yarn cache |
| `~/.dx` | `/tmp/opencode/home/dx` | Dioxus CLI data |
| `~/.local/share/.dx` | `/tmp/opencode/home/dx-tools` | Dioxus tools (wasm-bindgen, esbuild, wasm-opt) |
| `~/.cache/ms-playwright` | `/tmp/opencode/pw-browsers` | Chromium for web smoke tests |
| `~/.cargo/config.toml` | local, untracked | pins `build.target-dir` outside home |

`scripts/relocate-caches.sh` creates or repairs this layout. It is idempotent.
`--repair` repairs dangling links and missing `/tmp` targets even when `/home`
has plenty of free space; `--force` also relocates a real cache directory that
is still in `/home`.

```bash
./scripts/relocate-caches.sh --repair
```

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

## Restore flow

On boot, `/home/kinfuny/.customize_environment` runs
`scripts/cloudshell-bootstrap.sh --boot --quiet`. It first appends a
`boot-hook start` line with the boot id to `~/.cache/notebox-build/boot.log`,
then writes a `boot-hook finish` line with the exit code and ready state. The
script:

1. repairs the symlinks in `$HOME` before any tool can write to `/home`;
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
- `scripts/budget-check` keeps a **512 MiB** hard floor and is part of the gate.
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

If the base seed is corrupt, the bootstrap removes it and reinstalls the
toolchain. If only the target seed is corrupt, it is removed and the first
build is cold; rerun `save --target` afterwards. If `/home` is too full to
restore the target seed, the bootstrap skips it and continues with the base
seed.

The project source and `Cargo.lock` are in git and are not affected by a
restart. Keep at least 1 GiB free on `/home` for editor state, shell history and
the git index.
