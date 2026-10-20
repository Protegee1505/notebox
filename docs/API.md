# Sync server HTTP API

Version 1, wrapped in `Envelope { version, payload }` on every request and
response. Unknown versions are rejected with `400 invalid_request`.

The Worker serves this API and, from Phase 7, the web app from the same origin.
There is no CORS configuration because the web client is same-origin and native
clients are not browsers.

## Authentication

| Call | Header |
| --- | --- |
| `POST /v1/auth/setup` | `X-Setup-Token: <SETUP_TOKEN secret>` |
| Everything else | `Authorization: Bearer <device token>` or the `notebox_session` cookie |

`POST /v1/auth/setup` runs once, only while no account exists. It returns `409`
afterwards. `SETUP_TOKEN` is a Worker secret; when it is absent, setup answers
`503 not_configured`.

`POST /v1/auth/login` takes the same body as setup. The device has already run
Argon2id over the passphrase and sends the 32-byte result (`login_key`, base64).
The server stores `SHA-256(login_key)` and compares it in constant time. A
successful login creates a new device and returns a token; it never reuses an
existing token.

Failed logins are counted in the account row and back off exponentially: 1, 2,
4, 8, … seconds, capped at 300. Responses while blocked are `429 rate_limited`
with a `retry-after` header. Three consecutive failures return `429`
immediately. A successful login resets the counter.

The setup/login response also sets an `HttpOnly; Secure; SameSite=Strict`
cookie. The token remains in the JSON body for native clients.

## Routes

| Method | Path | Body / query | Response |
| --- | --- | --- | --- |
| `GET` | `/` | — | `notebox` (plain text health check) |
| `POST` | `/v1/auth/setup` | `AuthRequest` | `AuthResponse` |
| `POST` | `/v1/auth/login` | `AuthRequest` | `AuthResponse` |
| `POST` | `/v1/sync` | `SyncRequest` | `SyncResponse` |
| `PUT` | `/v1/blob/{hash}` | `BlobChunk` | `BlobStored` |
| `GET` | `/v1/blob/{hash}?index={n}` | — | `BlobChunk` |
| `GET` | `/v1/devices` | — | `DeviceList` |
| `DELETE` | `/v1/devices/{id}` | — | `204` |

## Sync

`SyncRequest { cursor, updates: [{ note_id, blob }] }`

- `cursor` is the last server sequence number the device stored; `0` starts
  from the beginning.
- A push carries at most **25** updates, each at most **1 MiB** before base64.
- The server appends updates in order through `db.batch()`, then reads the next
  page for the same device.
- A page holds **100** rows. `more: true` means pull again immediately.
- A device's own updates are filtered from the response, but the cursor still
  advances past them. A page of only its own rows therefore returns no updates
  and may still say `more: true`; the next page continues after it.
- Importing an update twice is harmless on the client, so a lost response
  cannot duplicate or lose text.
- The server never merges, parses or rewrites a note update.

`SyncResponse { cursor, updates: [{ seq, note_id, blob, device_id, created_at }], more }`

## Attachments

- `hash` is the lowercase hex SHA-256 of the whole attachment. The server
  validates the shape and uses the value as the content address; it does not
  re-hash chunks, because hashing 1 MiB on a free Worker would spend the CPU
  budget. Clients verify the hash after download.
- `total` is between 1 and 1024 chunks, `index` is zero-based and below
  `total`, and each decoded chunk is at most **1 MiB**.
- `size` and `mime` describe the whole attachment and must match on re-upload.
  A mismatch is `409 conflict`.
- Chunks are idempotent: re-uploading the same `{hash, index, data}` succeeds
  and costs one row replace.
- `GET /v1/blob/{hash}?index=0` is the usual way to learn `total`, `mime` and
  `size`; a missing chunk is `404 not_found`.

## Limits and costs

| Limit | Value | Where |
| --- | --- | --- |
| Updates per push | 25 | rejected with `413 too_large` |
| Sync page | 100 rows | `more` flag |
| Update | 1 MiB | rejected with `413 too_large` |
| Attachment chunk | 1 MiB | rejected with `413 too_large` |
| Attachment chunks | 1024 | rejected with `400 invalid_request` |
| Queries in a 25-update push | 28 | under the free limit of 50 |
| Bound parameters per query | 5 | under 100 |

A push of 25 updates costs 1 auth lookup, 25 batched inserts, 1 pull and at
most 1 `last_seen` write. `last_seen` is written at most every five minutes per
device, so polling does not multiply D1 writes.

## Errors

Non-success responses carry `Envelope<ErrorBody>`:

```json
{ "version": 1, "payload": { "code": "invalid_credentials", "message": "invalid credentials", "retry_after": null } }
```

Codes: `not_configured`, `setup_closed`, `bad_setup_token`,
`invalid_credentials`, `rate_limited`, `unauthorized`, `invalid_request`,
`too_large`, `not_found`, `conflict`, `storage_failed`.

All API responses set `Cache-Control: no-store`.
