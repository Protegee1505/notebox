# Status

Updated: 2026-10-20
Phase: 0 (spikes), bootstrap

## Shipped

- Cargo workspace and six crate skeletons.
- `AGENTS.md`, `CLAUDE.md`, `README.md`.
- `wrangler.toml` with the `sync` and `blobs` D1 bindings (placeholder ids).
- CI workflow for the standard gates.
- Static budget ceiling check; no runtime measurements yet.

## Measurements

None yet. Spike results and sizes go in the matching ADR.

## Blocked

- **Cloudflare**: no account login on this machine. S3 needs a deployed Worker
  for the real 10 ms CPU check.
- **GitHub**: no remote yet. Windows and Android CI runs need a pushed repo.
- **Android device**: this machine has no Android SDK/NDK and no `/dev/kvm`, so
  no local emulator. Android boot and IME checks need CI or a physical device.
- **Desktop display**: no X/Wayland display here; the Linux desktop binary can
  be built but not launched locally without a virtual display.

## Next

1. Run the bootstrap gates and commit.
2. S1: Dioxus hello app on web and Linux; Windows and Android in CI.
3. S2 and S5: Loro native/WASM, then the text box binding.
4. S4: sqlite-wasm OPFS versus IndexedDB.
5. S3: Worker CPU and client-side Argon2.
