# ADR-0022: Cloud Android build and emulator

- **Status:** accepted
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
   boots an API 34 AOSP (`default`) emulator, installs, launches, checks the
   resumed activity and logcat, force-stops and relaunches. Build and emulator
   are separate jobs to fit in the 14 GB runner disk. The `google_apis` image
   was tried first; its Google Play Services process crashed during boot and
   Android killed the app through the GMS fonts provider, so the lighter AOSP
   image is the gate.
5. **Use the runner's preinstalled SDK.** `android-actions/setup-android@v3`
   now fails because its default `sdkmanager tools` package no longer exists.
   The workflow locates the SDK, installs platform 34/build-tools 34.0.0 and
   keeps the same path for the release job.
6. **Leave release signing where it is.** The existing tag-only, opt-in signed
   APK job remains the only release path. This ADR adds no signing secret and
   does not make Android a release gate.

## Evidence

Accepted on run [40058399384](https://github.com/Protegee1505/notebox/actions/runs/40058399384)
(2026-10-24), total 5m44s:

- `debug-key` 5s; arm64 build 3m33s; x86_64 build 3m05s; emulator 2m.
- arm64 APK 111 MiB, SHA-256
  `2ae874932c7067d69ad800df5d5f8c09927e3bf09f8f6b108b03ffa0d6a485c3`,
  `native-code: 'arm64-v8a'`.
- x86_64 APK 114 MiB, SHA-256
  `89b7c901799944756aa4b9fb49589a9f4517d9ea2de60c3ded33685b9b7902d5`,
  `native-code: 'x86_64'`.
- Both APKs: `package: name='app.notebox'`, `application-debuggable`.
- Debug certificate SHA-256:
  `B6:DC:6A:87:8D:8E:28:89:C2:0A:91:B5:9D:A7:F8:D7:13:A0:A4:F1:75:34:51:B9:47:62:06:24:17:9E:DD:CB`.
- Emulator: API 34, AOSP x86_64; cold launch `TotalTime: 1534`, relaunch
  `TotalTime: 897`; resumed activity contained
  `app.notebox/dev.dioxus.main.MainActivity`; zero `FATAL EXCEPTION` lines;
  screenshots and the full logcat are in the `android-smoke-evidence` artifact.
- The app wrote `files/profileInstalled` in its private data directory, so the
  APK is debuggable and the Android data sandbox is in use.

The first attempt failed inside `android-actions/setup-android@v3` before any
build. The second failed after 28s because the generated wrapper is executed
line by line by `android-emulator-runner`; the checks now live in
`scripts/android-smoke.sh`. The `google_apis` image then killed the app through
the GMS fonts provider. The AOSP image passed with the same APK.

## Consequences

- The laptop and Cloud Shell stay free of Android Studio and the SDK. The
  public-repo Actions allowance covers the build; if the repository becomes
  private, the 2,000-minute monthly allowance and 10 GB cache limit apply.
- Two Android builds run per push/PR. They are independent and can be cached,
  but the 14 GB runner disk means the emulator job must not also build Rust.
  The debug APKs are 111-114 MiB because they keep debug symbols; a release
  build must be stripped and measured separately.
- A debug APK from CI is not a release artifact. Installing a later release
  APK over it requires the usual signature migration: export, uninstall,
  install release, restore.
- The workflow does not touch the Worker, D1 or Cloudflare limits.
