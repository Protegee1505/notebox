# Status

Updated: 2026-10-20
Phase: 0 (spikes)

## Shipped

- Cargo workspace and six crate skeletons; CI gates run on every push.
- `notes-core`: `NoteDocument` on Loro 1.16.2 with create, edit, snapshot
  export/import, update export/import and time travel (S2).
- `notes-core`: Unicode-safe prefix/suffix text-box binding with 1,000-edit
  convergence and remote-cursor tests (S5).
- Web release build via `dx` 0.7.10; brotli budget script and a headless
  Chromium boot check at 360, 900 and 1400 px (S1 web).
- CI jobs for wasm tests and size, web build/budget, Linux desktop boot under
  Xvfb, Windows build and Android NDK compile. Run
  [39371140289](https://github.com/Protegee1505/notebox/actions/runs/39371140289)
  is green across all six jobs.
- ADR-0001, ADR-0002 and ADR-0005 record the spike evidence.
- Development disk: Cargo target, Rust/Cargo/npm/yarn/dx caches and Playwright
  browsers now live under `/tmp/opencode`; `scripts/relocate-caches.sh` repairs
  the layout and `scripts/budget-check` fails below 512 MiB free on `/home`.
  See `docs/DISK.md`.

## Measurements

| Check | Result |
| --- | --- |
| Web bundle (release, brotli) | 390,856 bytes (0.37 MiB); wasm about 379 KB |
| Loro wasm delta (brotli) | 712,105 bytes (0.68 MiB), budget 1.5 MiB |
| 10,000 edits | 46,000 chars; snapshot 998 B; updates 46,106 B |
| Native tests | 17 passed |
| Loro wasm smoke (Node) | 1 passed |
| 3-replica convergence | 120 random edits, drops and random sync order |
| Text-box convergence | 1,000 randomized whole-value edits |
| Linux desktop boot (CI) | stayed up 20 s under Xvfb |
| Windows desktop build (CI) | `notebox.exe` built and uploaded |
| Android headless compile (CI) | NDK r27c, four crates |

## Blocked

- **Cloudflare**: no `wrangler login` on this machine. S3 needs a deployed
  Worker for the 10 ms CPU check and the Argon2 timing.
- **Android runtime**: no `/dev/kvm` and no Android SDK, so S2/S5 cannot be run
  on a device or emulator locally. Compile is verified with NDK r27c and in CI;
  the full Android app has not been built.
- **Cold starts**: Linux desktop boot is verified under Xvfb, but the 600 ms
  cold-start number is not measured. Windows is build-only; Android has no
  boot or cold-start number.

## Next

1. Run S2/S5 on an Android emulator with `/dev/kvm` (CI or a physical device) to
   close the runtime gap, including Gboard composition.
2. S4: sqlite-wasm OPFS versus IndexedDB in Chromium, Firefox and Safari.
3. S3: after `wrangler login`, deploy the Worker and measure CPU and Argon2.
4. Start P1 once S2 is accepted: note metadata, store trait, search, history
   API and the full 3-replica property test.
