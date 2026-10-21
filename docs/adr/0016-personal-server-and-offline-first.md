# ADR-0016: A built-in personal server, offline-first onboarding and opt-in Android

- **Status:** accepted
- **Date:** 2026-10-21
- **Phase:** P8 release polish
- **Owner:** app and release

## Question

How can the owner install Notebox on a new machine or open the web app and sign
in with almost no setup, while everyone else can try the app offline first or
use their own Worker, without adding a second account, an in-app sign-up or a
paid service?

## Context

- The server is deliberately single-account. `POST /v1/auth/setup` succeeds once
  behind `SETUP_TOKEN`; deleting the secret closes setup with `503`.
- The web transport is same-origin with no CORS headers. A web page served by
  one Worker cannot log in to another Worker.
- The public release artifacts are downloadable by anyone. A URL compiled into
  them is visible, but it is an address, not a credential.
- Login backoff is account-wide. Before this change, any unknown username
  incremented the counter, so a stranger trying random names on a public
  personal URL could lock the owner out.
- A fresh native database seeds three welcome notes and two notebooks. Fixed
  random ids on each device would add another set to the account after sign-in.
- The owner only needs Windows, Linux and web for this release. Android runtime
  testing and signing are not ready, and the release workflow had made them a
  hard prerequisite for publishing anything.
- The free-tier rule excludes R2, paid plans and a server the owner must host.
  No change here adds a request per sync run or a new Cloudflare resource.

## Decision

1. **Bake the personal server into native builds at compile time.**
   `NBX_SERVER_URL` is read with `option_env!` in `notes-app`; trailing slashes
   are trimmed. The login screen pre-fills it, keeps the field editable on
   desktop, and offers "Use the built-in server" after an edit. A build without
   the variable starts empty. The web build ignores the variable and uses
   `window.origin`, because it can only talk to the Worker that served it.
2. **Keep offline mode on the same screen and make it explicit.** The secondary
   action is "Try it offline — no account". Local notes, history, images and
   export need no Worker. There is no in-app sign-up and no second account.
3. **Create the one account with a native CLI.** `scripts/create-account` runs
   the `notes-sync` example. It reads the setup token and passphrase without
   echo, requires a passphrase of at least 12 characters, removes the temporary
   setup device after creating the account, and tells the owner to delete
   `SETUP_TOKEN`. If the device removal fails, it prints the device id so the
   owner can revoke it in Settings.
4. **Only a wrong key for the real username counts toward backoff.** An unknown
   username returns the same `invalid_credentials` error without a D1 write or
   counter increment. A targeted attacker who knows the owner's username can
   still trigger the existing 1–300 s backoff; a random public visitor cannot.
5. **Give the welcome content fixed ids.** `NewNote` accepts an optional
   `NoteId` and `NoteStore::create_notebook_with_id` accepts a `NotebookId`.
   The seed uses five fixed UUIDs, so a fresh second device merges the welcome
   notes and notebooks with the account's copies instead of adding duplicates.
   User-created notes and notebooks still get random ids.
6. **Make Android opt-in.** `RELEASE_ANDROID=true` enables the Android job.
   `publish` waits for all jobs with `always()` plus a failure guard, so a
   skipped Android job does not block Linux, Windows and web. Release notes
   mention Android only when an APK is present.
7. **Pass the URL through a repository variable.** The release workflow gives
   `NBX_SERVER_URL` to the Linux, Windows and opt-in Android bundles. It is not
   a secret; it must not contain a passphrase, token or setup secret.

## Evidence

- `cargo check --workspace`, `cargo check -p notes-app --features desktop`,
  `cargo check -p notes-app --no-default-features --features web --target
  wasm32-unknown-unknown`, and `cargo check -p notes-sync --examples` pass on
  2026-10-21.
- A server test starts with an unknown username, checks that
  `failed_logins` and `blocked_until` stay at zero, and logs the real account in
  immediately at the same timestamp.
- The seed now uses `NoteId` `1111…`, `2222…`, `3333…` and notebook ids
  `aaaa…`, `bbbb…`; the store creates from the supplied ids.
- The workflow changes are static: a skipped `android-bundle` job is followed by
  a `publish` job guarded by `always() && !failure() && !cancelled()`.
- No deployed Worker, real passphrase or GitHub tag was used in this
  workstation session. Cloudflare login and the first release tag are still
  owner actions.

## Consequences

- The personal URL is visible in public native artifacts. Anyone can see it and
  attempt a login. They cannot create an account, and unknown usernames no
  longer consume the owner's backoff budget. The owner should choose a username
  that is not published, and can rebuild without the variable for a completely
  blank public build.
- Web users cannot point one deployment at another server. Self-hosting means
  deploying the web bundle on their own Worker; desktop users can edit the URL.
- `SETUP_TOKEN` remains the one dangerous secret. It must be deleted after the
  account exists; leaving it in place allows anyone to race the owner for the
  only account.
- The welcome notes still seed on a fresh device before sign-in. Fixed ids stop
  duplication, but a device can re-push welcome metadata if the account trashed
  those notes. A later change can defer seeding until offline mode is chosen.
- Android remains buildable but is no longer a release gate. Enabling it still
  requires the keystore secrets and a tag.
- No paid service, R2 bucket, CORS surface or new per-sync request was added.
