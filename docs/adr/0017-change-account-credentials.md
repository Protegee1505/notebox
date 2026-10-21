# ADR-0017: Change the account username and password in Settings

- **Status:** accepted
- **Date:** 2026-10-21
- **Phase:** P8
- **Owner:** app and server

## Question

How can the owner change the username or password of the single account
without an email reset, a second account, server-side Argon2id or a schema
migration, while keeping the current device signed in and not breaking the
10 ms Worker CPU budget?

## Context

- The account is created once behind `SETUP_TOKEN`. After setup the secret is
  deleted and the server refuses another account.
- Argon2id runs on the device. The Worker stores only `SHA-256(login_key)` and
  has a 10 ms CPU budget per request, so it cannot stretch a password.
- The Argon2id salt is `SHA-256(SALT_LABEL || username)`. A username change
  therefore changes the derived key even when the password stays the same.
- The current device proves possession of its bearer token. A credential change
  must also prove knowledge of the current password; otherwise a stolen device
  token could lock the owner out with one request.
- Other devices hold tokens that would remain valid after a password change
  unless they are revoked.
- The existing `account` table already has `username`, `auth_hash`,
  `failed_logins` and `blocked_until`. The `device` table has the device id.
  No new column or table is needed.

## Decision

1. **One authenticated endpoint.** `POST /v1/auth/credentials` accepts
   `CredentialUpdateRequest { current_login_key, new_username,
   new_login_key }`. Both keys are 32-byte, base64-encoded Argon2id outputs.
2. **The current device authorizes the change.** The bearer token or web cookie
   identifies the device; `current_login_key` must match the account's stored
   hash in constant time. A mismatch returns `403 wrong_password`, deliberately
   distinct from an expired-session `401`.
3. **One atomic D1 batch.** The server updates `account.username` and
   `account.auth_hash`, resets `failed_logins` and `blocked_until`, and deletes
   every device except the caller's. The current device keeps its token; every
   other device must sign in again.
4. **The client derives both keys.** The app derives the current key from the
   old username and current password, and the new key from the trimmed new
   username and the new password. A blank new password keeps the current one,
   but the key is still re-derived when the username changes. A new password
   must be at least 12 characters.
5. **The app updates its saved session.** After success it saves the new
   username in `SavedAccount` and in the native session file, rebuilds the
   client with the same device token, updates the Settings/login display and
   refreshes the device list. The browser keeps its `HttpOnly` cookie.
6. **No reset path.** There is no email reset, no recovery code and no
   server-side password hash. Losing both the password and every signed-in
   device means the account cannot normally be changed. The one dangerous
   escape hatch remains `SETUP_TOKEN`, which must stay deleted after setup.

## Evidence

- `cargo test -p notes-server --test server` passes 17 handler tests over the
  in-memory store, including:
  - a valid change keeps the caller's token, revokes the other device and
    rejects the old username/key pair;
  - a wrong current key returns `WrongPassword` and changes neither the account
    nor the device list;
  - blank/control usernames and a 31-byte key are rejected, and the stored
    username is trimmed.
- `cargo test -p notes-sync --test engine changing_` passes the two engine
  tests: a username-only change proves the salt re-derivation and old-token
  revocation; a password change rejects the old password, reports
  `403 wrong_password` for a bad current password, then accepts the new one.
- `cargo check -p notes-app -p notes-server -p notes-sync --all-targets` and
  `cargo check --target wasm32-unknown-unknown -p notes-app -p notes-server
  -p notes-sync` pass.
- `cargo nextest run --workspace`, `cargo clippy --workspace --all-targets --
  -D warnings`, `cargo clippy -p notes-server --target wasm32-unknown-unknown
  -- -D warnings`, `cargo fmt --all --check`, `cargo deny check` and
  `scripts/budget-check` pass.
- `node scripts/server-e2e.mjs` passes against `wrangler dev --local` and D1,
  including `403 wrong_password`, the update, current-device retention,
  other-device revocation, and login with the new pair but not the old one.
- `node scripts/web-wipe-login.mjs` passes in Chromium: it opens the labelled
  form, changes both the username and password, sees the new username in
  Settings, signs out, signs in with the new pair and finds the note again,
  with no console errors.
- The rebuilt release passes `scripts/web-budget.mjs` (1.71 MiB brotli against
  a 3.00 MiB limit), the 30-check axe audit and the existing persistence,
  restore and PWA checks.

## Consequences

- No D1 migration is needed. The existing account and device rows carry the
  whole change.
- The Worker still never runs Argon2id and gains one authenticated request, not
  a per-sync cost.
- The current device stays signed in; other devices are revoked by design. A
  user who changes credentials on one device must sign in again on the others.
- An attacker who already holds a valid device token can guess the current
  password against this endpoint. That attacker already has full sync access;
  the endpoint does not add a login-backoff write on a wrong current password.
  A future change can apply the existing backoff here if that trade-off changes.
- The old username and key stop working immediately. The account row still
  contains the same notes and updates; only the credentials change.
- The app and the account-setup example share `MIN_PASSWORD_CHARS = 12`.
  Changing this policy is a client change, not a server migration.
