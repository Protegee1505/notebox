# ADR-0001: Dioxus boot on web, Linux, Windows and Android

- **Status:** proposed
- **Date:** 2026-10-20
- **Spike:** S1
- **Owner:** UI

## Question

Does one Dioxus 0.7 codebase build and boot on web, Linux desktop, Windows
desktop and an Android device, at sizes that fit the budgets?

## Context

- `dioxus` pinned to `=0.7.10`; `dx` 0.7.10.
- Rust 1.99.1, `wasm32-unknown-unknown` and `aarch64-linux-android` targets.
- This machine has no X/Wayland display, no Android SDK/NDK and no `/dev/kvm`,
  so Linux boot, Windows and Android are verified in CI.
- Web was checked locally with headless Chromium 156 (Playwright).

## Decision

Keep Dioxus 0.7.10 as the single UI stack. One `notebox` binary selects its
renderer with a Cargo feature (`web`, `desktop`, `mobile`); the feature is now
explicit because the binary no longer requires `desktop`.

## Evidence

Web release build:

```text
dx build -p notes-app --platform web --release --features web
```

- CI bundle: 3 files, 2,002,086 bytes raw, **390,856 bytes brotli** (0.37 MiB).
- WASM: about 1.95 MB raw, 379 KB brotli. Budget is 3 MiB.
- Boot at 360, 900 and 1400 px in Chromium: HTTP 200, title `Notebox`, heading
  `Notebox`, zero page or console errors. Screenshots:
  `docs/screenshots/s1-web-360.png`, `-900.png`, `-1400.png`.
- `scripts/web-budget.mjs` checks the brotli budget in CI.

CI run
[39371140289](https://github.com/Protegee1505/notebox/actions/runs/39371140289)
is green for all six jobs:

- `web`: release build, brotli budget, uploaded artifact.
- `desktop`: built the Linux app and ran it for 20 seconds under Xvfb; the
  process stayed up. Desktop-portal warnings are non-fatal in the headless
  session.
- `windows`: built `notebox.exe` on `windows-latest` and uploaded it.
- `android`: compiled the headless crates with NDK r27c. The full Android app
  build and an emulator boot still need the Android SDK and Gradle.
- `gates`, `wasm`: the normal gates and the Loro wasm test.

Bootstrap CI already passed `cargo clippy -p notes-app --features desktop` and
the wasm compile of `notes-app`.

Not yet measured: desktop cold start, Windows cold start, Android cold start.
No local display or device exists for those numbers. The Android app itself
has not been built, only the headless crates it uses.

## Consequences

- S1 is met for web, Linux desktop and a Windows build. It is not met for the
  Android app or for any cold-start number; those remain open.
- `Dioxus.toml` needs a `[web.resource.dev]` table in 0.7.10; without it `dx`
  rejects the file with `missing field dev`.
- Every platform build must name its renderer feature explicitly. `README.md`
  documents the web and desktop commands.
- The bundle leaves 2.6 MiB of headroom for the editor, sync client and assets.
