# ADR-0023: Android runtime bridge

- **Status:** accepted
- **Date:** 2026-10-24
- **Phase:** P4/P8 Android completion
- **Owner:** app

## Question

How does the Dioxus WebView on Android get private storage, backup
export/restore, camera and gallery images, Back handling and a lifecycle save
without adding permissions, a server, or Android code inside the headless
crates? What can CI verify without a phone?

## Context

- The app runs as a `WryActivity` with a WebView; the UI is Rust/Dioxus and the
  only Android-specific code is the activity and its manifest. ADR-0022 builds
  the APKs on free GitHub runners, so the build path is already settled.
- `notes-proto`, `notes-core`, `notes-store` and `notes-sync` must keep
  compiling for native and wasm; nothing user-facing belongs in them. The shell
  talks to them through `NoteStore`/`Storage`/`SyncTransport`, and the bridge
  only produces files and events for `notes-app`.
- Android's auto-backup would copy the SQLite database without attachment rows,
  so it is disabled and the app owns export/restore through SAF.
- `ACTION_IMAGE_CAPTURE` needs a writable URI for full-size output.
  `FileProvider` needs a `res/xml` path resource, and `dx` does not copy
  resource files into the generated Gradle project. Opening the camera directly
  with `CAMERA` would need a runtime permission and a preview surface, which
  this WebView app does not have.
- `minSdk` is 24, `targetSdk` 34. The API 33 photo picker is not available on
  every supported release, and stripped AOSP images may not ship it at all.
- The owner does not attach a phone. CI boots an AOSP API 34 x86_64 emulator
  with `-camera-back none`, so camera interop cannot be exercised there.

## Decision

1. **Paths come from the activity, not from `$HOME`.** `MainActivity.onCreate`
   sets `NOTEBOX_DATA_DIR` and `NOTEBOX_CACHE_DIR` to `filesDir` and `cacheDir`
   before `super.onCreate` starts Rust. `storage.rs` already honours both, so
   the headless crates stay unchanged and the database and attachments live in
   app-private storage.
2. **SAF is the only file transport.** Backups use `ACTION_OPEN_DOCUMENT` and
   exports use `ACTION_CREATE_DOCUMENT`; the app copies in or out of its cache
   and never keeps a persisted URI permission. No
   `READ/WRITE_EXTERNAL_STORAGE`, no `MANAGE_EXTERNAL_STORAGE`.
3. **Images come from the system picker or a camera app.** The native chooser
   offers "Take a photo" and "Choose from gallery". Android 13+ uses
   `MediaStore.ACTION_PICK_IMAGES`; older releases and images without the photo
   picker use `ACTION_OPEN_DOCUMENT` with `image/*`. Camera output uses
   `ACTION_IMAGE_CAPTURE` plus a per-URI grant to a small
   `CameraOutputProvider` declared in the manifest. The provider resolves names
   only inside `cacheDir/camera`, and the app declares no `CAMERA` permission
   because another app takes the photo.
4. **The bridge is polled, not pushed.** `window.NoteboxAndroid` has `pick`,
   `save` and `takeResult`; Rust installs one interval that forwards every
   queued JSON message into Dioxus `eval`. `notebox-pause` and
   `__noteboxHandleBack` let Android ask the page to save or to close an overlay
   before the activity finishes.
5. **One size limit, enforced twice.** The Kotlin copy stops images above 25 MiB
   before they reach the cache, and `images::MAX_INPUT_BYTES` still rejects them
   at ingest. Backups are not capped. Cache files older than 24 hours are swept
   at startup.
6. **The custom activity must restore `BuildConfig`.** Dioxus generates
   `Logger.kt` with an unqualified `BuildConfig`, which comes from a typealias in
   its template activity. Any custom `MainActivity.kt` has to add
   `typealias BuildConfig = app.notebox.BuildConfig` or Kotlin compilation
   fails.
7. **CI asserts the permission surface.** The Android workflow checks the
   packaged manifest for the provider and the two `<queries>` intents, and fails
   if `CAMERA`, `READ_EXTERNAL_STORAGE`, `WRITE_EXTERNAL_STORAGE` or
   `MANAGE_EXTERNAL_STORAGE` appears. The emulator job checks the provider is
   registered at runtime.

## Evidence

- Run [40069082706](https://github.com/Protegee1505/notebox/actions/runs/40069082706)
  (2026-10-24) built both APKs and passed the emulator after the alias fix:
  arm64 `9d934bf`, x86_64 and arm64 `success`, emulator `success`.
- The earlier run 40062174145 built Rust successfully and failed only at
  `Logger.kt:86:14 Unresolved reference 'BuildConfig'`, which identified the
  alias as the single blocker.
- The camera, photo picker and manifest assertions are covered by the run for
  `01a2757` on this branch (see `docs/STATUS.md` for the recorded numbers).

Not verified, and deliberately not claimed:

- Real camera-app interoperability, including apps that rotate, crop or write
  EXIF themselves. The CI emulator has no camera.
- Gboard and other IME resize behavior on the owner's phone.
- OEM-specific SAF and photo-picker quirks.
- Live native login/sync, because there is one production account and no
  in-app signup.
- Argon2id timing on a phone.

## Consequences

- Notebox ships no camera or storage permission. The only permission is
  `INTERNET`.
- Camera output is limited to `cacheDir/camera`; a malicious page cannot ask the
  provider for a private database file, and `save` refuses paths outside the
  cache.
- The provider authority is `${applicationId}.cameraout`, so changing the
  identifier in `Dioxus.toml` changes the authority too; the CI assertion on
  `app.notebox.cameraout` catches a mismatch.
- Nothing in this decision touches the Worker, D1, the sync protocol or the
  release keystore, and the free-tier limits are unaffected.
- The Android runtime still needs a hardware pass on the owner's phone for the
  items listed above; the debug APK is the deliverable, not a release artifact.
