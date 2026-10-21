# ADR-0012: Tagged release artifacts

- **Status:** accepted
- **Date:** 2026-10-21
- **Phase:** P7 release packaging
- **Owner:** release

## Question

How can a tag produce installable Linux, Windows, Android and web artifacts,
with a signed Android APK, without adding a paid service or storing signing
keys in the repository?

## Context

- `dx bundle` 0.7.10 builds desktop bundles for the host platform only. Linux,
  Windows and Android artifacts need separate native CI runners.
- The workspace version is `0.1.0`; release tags can carry a different name
  unless the workflow checks it.
- Dioxus reads Android signing settings from `Dioxus.toml`. The file is
  committed, so the signing block cannot live there.
- GitHub Actions can hold repository secrets. A signed APK is required for an
  in-place Android update; publishing an unsigned or debug APK would create an
  install that cannot update over itself.
- The Worker deploy still requires the owner's Cloudflare login and was already
  manual in README. A release must not pretend to deploy it.
- Linux and Windows installers are useful only if they start on a clean
  machine, not merely if `cargo build` succeeds.

## Decision

1. **A tag must match the app version.** A `v*` tag runs
   `.github/workflows/release.yml`. The first job reads the `notes-app` version
   with `cargo metadata` and fails unless the tag is `v<version>`. All build
   jobs depend on that job and on the same gates as CI.
2. **Build on native runners, not cross-compilers.** Linux produces `.deb` and
   `.AppImage`, Windows produces `.msi`, an NSIS setup `.exe` and a portable
   `.exe`, Android produces an arm64 `.apk`, and the web job produces the PWA
   `.zip`. The workflow publishes these on a GitHub Release with
   `SHA256SUMS`.
3. **Android signing is all-or-nothing.** The workflow requires
   `ANDROID_KEYSTORE_BASE64`, `ANDROID_KEYSTORE_PASSWORD`,
   `ANDROID_KEY_ALIAS` and `ANDROID_KEY_PASSWORD`. It decodes the keystore to
   the runner's temporary directory, appends an ephemeral
   `[bundle.android]` table to `Dioxus.toml`, builds, verifies the APK with
   `apksigner`, checks the package name and manifest, then restores
   `Dioxus.toml`. If any secret is missing it fails before building and
   publishes nothing.
4. **Smoke-test on clean runners.** Linux installs the `.deb` and boots it
   under Xvfb for 20 seconds, then extracts and boots the AppImage. Windows
   installs the `.msi` silently and boots the installed executable for 20
   seconds. Android uses `apksigner` and `aapt` because no KVM/emulator is
   available. The web job runs `pwa-check.mjs`.
5. **Publish, do not deploy.** The workflow creates a GitHub Release after all
   smoke jobs pass. The Worker deploy remains the owner's `wrangler deploy`
   step; the web zip is not deployed automatically. Windows installers are not
   code-signed and are labelled as such.

## Evidence

- `Dioxus.toml` now sets the bundle identifier, publisher, icon, category and
  Debian runtime dependencies.
- `.github/workflows/release.yml` contains the tag check, gates, four platform
  builds, two clean-runner smoke jobs and the Android APK checks, checksum
  generation and GitHub Release upload.
- Local Linux evidence from 2026-10-21: `dx bundle` 0.7.10 produced
  `notebox_0.1.0_amd64.deb` (9,867,218 B) and
  `notebox_0.1.0_x86_64.AppImage` (89,597,120 B) with the patched lockfile. The
  installed `.deb` and the extracted AppImage each stayed up for 20 seconds
  under Xvfb. The web release built with `NBX_BUILD_ID=v0.1.0` and passed the
  3 MiB budget at 1,818,993 B brotli.
- The workflow itself has not run on GitHub yet. The first tag needs the
  owner's Android keystore secrets; until then the Android job fails by design.
  No GitHub release artifact was published from this workstation.

## Consequences

- A release tag either produces the complete set or fails before publishing.
  Missing Android signing credentials are a hard error, not a warning.
- The repository never contains the keystore or its passwords. The generated
  signing block exists only on the ephemeral runner.
- Losing the keystore means no further in-place Android updates with the same
  package; the owner must keep a backup.
- Linux and Windows smoke jobs run on GitHub-hosted machines, not a dedicated
  clean VM. They still start from a fresh runner with no prior install.
- Windows installers may show SmartScreen warnings because they are unsigned.
  Fixing that needs a paid certificate, which the project rule excludes.
- No Cloudflare plan, D1 write or Worker CPU budget changes. The release
  workflow uses only GitHub Actions and static file hosting.
