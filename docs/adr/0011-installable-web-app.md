# ADR-0011: Installable web app with deliberate updates

- **Status:** accepted
- **Date:** 2026-10-21
- **Phase:** P7 installable web app
- **Owner:** app + release

## Question

How can the web build keep working after the first visit, survive a deploy
without swapping code under an open editor, and tell the user when a new
version is ready?

## Context

- `dx build` hashes every generated JS, CSS and WASM file. A static service
  worker cannot know those names ahead of time.
- The Worker already serves the web build from the sync origin, so a service
  worker can be root-scoped and browser sync stays same-origin.
- The app keeps its database in OPFS, not in the service-worker cache. The
  service worker only has to serve the app shell and static assets.
- The browser may install the app before the user ever signs in; the PWA path
  cannot depend on an account.
- A deployment may happen while a long note is open. Replacing the app or
  reloading automatically risks losing an edit that has not reached OPFS.
- Cloudflare Workers static assets support a `_headers` file, but not dynamic
  cache headers from a service worker.

## Decision

1. **Generate the worker after the build.** `scripts/pwa-build.mjs` reads the
   Dioxus asset manifest, copies the manifest and icons, lists the current
   hashed assets, hashes their contents into a build id (CI sets
   `NBX_BUILD_ID` to the run number instead), and writes `sw.js` from
   `crates/notes-app/web/sw.js`. Stale assets left by an earlier `dx` build are
   not precached.
2. **Precache the shell, not the notes.** The install step caches `/`,
   `/index.html`, the manifest, icons and every current hashed asset. Cache
   names are versioned by build id. API paths (`/v1/*`), `/sw.js` and
   `/version.json` are never intercepted.
3. **Serve navigations network-first, hashed assets cache-first.** An offline
   navigation falls back to the cached index. Hashed assets are immutable.
   The manifest and icons use stale-while-revalidate. Non-GET requests and
   files outside the app origin pass through untouched.
4. **Update only on request.** `index.html` registers the worker with
   `updateViaCache: "none"`. When a new worker is installed and an old worker
   still controls the page, the script publishes `noteboxPwa.ready` and fires
   `notebox-update-ready`. Rust shows the banner. **Reload** saves and flushes
   the note, asks the waiting worker to skip waiting, waits for
   `controllerchange`, then reloads. **Later** dismisses the banner for this
   session.
5. **Stamp the build.** CI sets `NBX_BUILD_ID` to the run number and
   `NBX_GIT_SHA` to the commit; the About card shows the package version and
   those identifiers. Local builds show `dev`.
6. **Use Cloudflare's cache headers.** `_headers` marks `/`, `sw.js` and
   `index.html` as revalidate-on-use, `version.json` as no-store, and
   `/assets/*` as immutable.

## Evidence

- `scripts/pwa-check.mjs` passes 12 functional checks in a clean Chromium
  context: manifest, icons, precache contents, service-worker control, empty
  `Page.getInstallabilityErrors`, offline reload with a local-only note, an
  offline edit that survives another offline reload, a simulated second build
  showing the banner, cache replacement on Reload, the note still open after
  the update, no `/v1/*` requests and no page or console errors. With
  `NBX_PWA_SCREENSHOTS=1` it also writes the six layout captures below.
- Web release after the change: 1,818,081 bytes brotli (1.73 MiB), under the
  3 MiB budget. The PWA adds 32,623 bytes of manifest, icons, HTML and worker
  before brotli.
- Service worker precaches 10 files (index, manifest, three icons and four
  hashed assets) for this build.
- Screenshots at 360, 900 and 1400 px in both themes:
  `docs/screenshots/p7-pwa-*`.
- Existing browser persistence checks still pass (13/13) with the service
  worker installed.

## Consequences

- The web app installs and opens offline after the first successful load.
  Notes remain in OPFS; the cache never contains note text or attachment bytes
  beyond the app's own assets.
- Update checks use one static `sw.js` request on load or when the tab becomes
  visible. They do not touch the Worker API or D1, and are free static-asset
  requests.
- A waiting worker can stay waiting indefinitely if the user keeps the tab
  open and clicks Later. Closing all tabs activates it on the next launch.
- The cache holds the app shell, including the WASM binary, in browser
  storage. `navigator.storage.persist()` is still requested by the OPFS path.
- This covers the web PWA only. Desktop update notices and installer artifacts
  belong to the remaining P7 packaging work; they are not implied by this
  record.
- No PLAN.md budget or server limit changed. The service worker adds no D1
  writes and no Worker CPU work beyond serving static files.
