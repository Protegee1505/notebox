# ADR-0022: Cloud Android build and emulator

- **Status:** proposed
- **Date:** 2026-10-24
- **Phase:** P4/P8 Android completion
- **Owner:** app and release

## Question

Can the full Dioxus Android app be compiled, installed and booted entirely on
free GitHub-hosted runners, so the storage-limited laptop and Cloud Shell need
no Android Studio, SDK or NDK? What fails first if it cannot?

## Context

- The repository is public. Standard GitHub-hosted Linux runners are free for
  public repositories and provide 4 vCPU, 16 GB RAM and a 14 GB SSD. GitHub
  supports hardware-accelerated Android virtualization on these runners, but
  the disk is the binding constraint.
- This workspace has no `ANDROID_HOME`, `ANDROID_SDK_ROOT` or `NDK_HOME`, no
  `adb`, and no `/dev/kvm`. It has roughly 3.5 GB free on a 4.8 GB `/home`
  partition. Installing the SDK and NDK here would break the disk budget.
- Dioxus 0.7.10 uses a generated Gradle project (Gradle 9.1.0, AGP 8.7.0,
  Kotlin 2.0.20, JDK 17). The repository has only ever compiled four headless
  crates for `aarch64-linux-android`; it has never built `notes-app` or produced
  a debug APK.
- The release workflow can build a signed arm64 APK, but it is opt-in behind
  `RELEASE_ANDROID=true` and needs the owner's keystore. There is no routine
  debug APK and no emulator test.
- The current native storage path uses `$HOME`/`XDG_DATA_HOME`, and export uses
  `$HOME/Downloads`; Phase 1 replaces both with Android app-private storage and
  SAF. This ADR only establishes the build host.

## Decision

1. **GitHub Actions is the Android build farm.** The new `Android` workflow
   builds two debug APKs on standard `ubuntu-latest` runners: arm64 for a phone
   and x86_64 for the emulator. Cloud Shell remains the editor and runs the
   existing host gates; it never installs the SDK or NDK.
2. **Use the stock Dioxus Gradle path first.** JDK 17, Android platform 34,
   build-tools 34.0.0, NDK r27c and `dx` 0.7.10 are installed per run. If the
   generated Gradle 9.1/AGP 8.7 project fails, patch the generated wrapper to
   Gradle 8.9 and run Gradle directly. Do not upgrade Dioxus for this.
3. **Keep the debug APK installable across runs.** A small `debug-key` job
   caches and publishes the standard Android debug keystore, so matrix jobs use
   the same certificate. The key is not a secret; it is not committed.
4. **Run a separate emulator job.** It downloads the x86_64 APK, enables KVM,
   boots an API 34 `google_apis` emulator, installs, launches, checks the
   resumed activity and logcat, force-stops and relaunches. Build and emulator
   are separate jobs to fit in the 14 GB runner disk.
5. **Leave release signing where it is.** The existing tag-only, opt-in signed
   APK job remains the only release path. This ADR adds no signing secret and
   does not make Android a release gate.

## Evidence

Pending the first CI run. The workflow records:

- the debug APK path, size and SHA-256 for each ABI;
- `apksigner` and `aapt` output, including the package name, native code and
  debuggable flag;
- emulator boot, install, launch, resumed activity, screenshot and logcat;
- the runner disk headroom after the build.

The first run must prove the Gradle toolchain and emulator boot before the
workflow is treated as accepted. If it fails, record the failure and the
fallback result here.

## Consequences

- The laptop and Cloud Shell stay free of Android Studio and the SDK. The
  public-repo Actions allowance covers the build; if the repository becomes
  private, the 2,000-minute monthly allowance and 10 GB cache limit apply.
- Two Android builds run per push/PR. They are independent and can be cached,
  but the 14 GB runner disk means the emulator job must not also build Rust.
- A debug APK from CI is not a release artifact. Installing a later release
  APK over it requires the usual signature migration: export, uninstall,
  install release, restore.
- The workflow does not touch the Worker, D1 or Cloudflare limits.
