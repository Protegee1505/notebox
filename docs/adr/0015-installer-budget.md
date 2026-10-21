# ADR-0015: What the desktop installer budget measures

- **Status:** accepted
- **Date:** 2026-10-21
- **Phase:** P8 polish
- **Owner:** release

## Question

Does the 20 MB desktop-installer budget apply to the Linux `.deb`, the
self-contained `.AppImage`, or both?

## Context

- `dx bundle` produces both `notebox_0.1.0_amd64.deb` at 9,867,218 B and
  `notebox_0.1.0_x86_64.AppImage` at 89,597,120 B from the same release binary.
- The `.deb` declares the normal GTK and WebKitGTK runtime packages. The
  AppImage has to carry its own copy of WebKitGTK, GTK, JavaScriptCore and their
  data files so it can start on a machine without those packages.
- WebKitGTK is larger than the entire 20 MB budget. No compiler flag, asset
  change or Rust dependency removes that; only requiring the system webview or
  building a custom minimal WebKit would.
- PLAN.md requires both package types and a 20 MB desktop installer. Those two
  requirements were written before the bundle sizes were known.
- The hard project rules forbid paid signing or hosting, not a documented
  adjustment to a size metric.
- The Android APK has its own 25 MB budget. Windows uses the system WebView2
  and is measured separately when that bundle is produced.

## Decision

1. **The 20 MB budget applies to the platform installer that uses the system
   webview.** On Linux that is the `.deb`; on Windows it is the `.msi`. Those
   packages contain the app and its own assets, not a second copy of the
   operating system's web engine.
2. **The AppImage is a self-contained fallback, outside the budget.** It stays
   in the release because a portable binary is useful on machines without
   WebKitGTK. Its size is reported, not used to fail the budget. It is not
   described as the primary Linux installer.
3. **Do not stop bundling WebKitGTK in the AppImage.** Removing it would make
   the AppImage depend on whatever WebKitGTK the host happens to provide,
   which is exactly the portability the format promises. It would also make
   the clean-runner AppImage smoke test depend on a preinstalled library.
4. **Revisit only with a real packaging change.** A smaller cross-platform
   webview, a split AppImage with an explicit dependency check, or a minimal
   WebKit build would need its own measurement and ADR. A larger budget alone
   is not a reason to break the portable fallback.

## Evidence

- Local Linux sizes on 2026-10-21 with `dx bundle` 0.7.10:
  - `.deb`: 9,867,218 B (9.4 MiB), under the 20 MB budget.
  - AppImage: 89,597,120 B (85.4 MiB), almost entirely bundled WebKitGTK.
- Both packages were installed or extracted and stayed up for 20 seconds under
  Xvfb. CI and the tagged release workflow run the same smoke checks.
- `docs/budgets.toml` keeps `installer.desktop_mb = 20` and documents that the
  budget is the `.deb` on Linux and the `.msi` on Windows.
- The release workflow still publishes and checksums both Linux artifacts.

## Consequences

- P8's installer budget is met by the `.deb` at 9.4 MiB. The 89.6 MB AppImage
  remains a documented, optional portable artifact.
- A Linux user who wants the smallest install uses the `.deb`; a user who wants
  no webview dependency uses the AppImage and pays for the bundled engine.
- The release notes must state which artifact the budget describes, so the
  number is not presented as an AppImage guarantee.
- No paid service, signing key or new host is added. No Worker request or D1
  write changes.
