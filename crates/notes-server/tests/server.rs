//! Handler tests over the in-memory store.
//!
//! These cover the contract the Worker adapter and P3 sync engine rely on:
//! one account, one token per device, ordered updates, and idempotent chunks.

use notes_proto::{
    AuthRequest, Base64Bytes, BlobChunk, CredentialUpdateRequest, NoteId, PushUpdate, SyncRequest,
    MAX_PUSH_UPDATES, MAX_UPDATE_BYTES,
};
use notes_server::{MemoryStorage, Server, ServerConfig, ServerError, Storage};

fn server() -> Server<MemoryStorage> {
    Server::new(
        MemoryStorage::new(),
        ServerConfig::with_setup_token("setup-secret"),
    )
}

fn request(username: &str, device: &str, key: u8) -> AuthRequest {
    AuthRequest {
        username: username.to_string(),
        login_key: Base64Bytes::new(vec![key; notes_proto::LOGIN_KEY_BYTES]),
        device_name: device.to_string(),
    }
}

fn change(current: u8, username: &str, new: u8) -> CredentialUpdateRequest {
    CredentialUpdateRequest {
        current_login_key: Base64Bytes::new(vec![current; notes_proto::LOGIN_KEY_BYTES]),
        new_username: username.to_string(),
        new_login_key: Base64Bytes::new(vec![new; notes_proto::LOGIN_KEY_BYTES]),
    }
}

fn update(byte: u8) -> PushUpdate {
    PushUpdate {
        note_id: NoteId::new(),
        blob: Base64Bytes::new(vec![byte; 8]),
    }
}

#[test]
fn setup_needs_the_deployment_token_and_runs_once() {
    let server = server();
    let auth = request("alice", "laptop", 7);

    assert_eq!(
        pollster::block_on(server.setup(None, &auth, 1_000)).unwrap_err(),
        ServerError::BadSetupToken
    );
    assert_eq!(
        pollster::block_on(server.setup(Some("wrong"), &auth, 1_000)).unwrap_err(),
        ServerError::BadSetupToken
    );
    assert_eq!(server.storage().device_count(), 0);

    let created =
        pollster::block_on(server.setup(Some("setup-secret"), &auth, 1_000)).expect("setup");
    assert_eq!(created.token.len(), 64);
    assert_eq!(
        pollster::block_on(server.setup(Some("setup-secret"), &auth, 1_001)).unwrap_err(),
        ServerError::SetupClosed
    );
    assert_eq!(server.storage().device_count(), 1);
}

#[test]
fn setup_is_closed_when_no_token_is_configured() {
    let server = Server::new(MemoryStorage::new(), ServerConfig::without_setup());
    let auth = request("alice", "laptop", 7);
    assert_eq!(
        pollster::block_on(server.setup(Some("anything"), &auth, 1_000)).unwrap_err(),
        ServerError::NotConfigured
    );
}

#[test]
fn login_issues_a_second_device_and_keeps_the_first() {
    let server = server();
    let first = pollster::block_on(server.setup(
        Some("setup-secret"),
        &request("alice", "laptop", 7),
        1_000,
    ))
    .expect("setup");
    let second =
        pollster::block_on(server.login(&request("alice", "phone", 7), 1_100)).expect("login");

    assert_ne!(first.token, second.token);
    assert_ne!(first.device_id, second.device_id);

    let device = pollster::block_on(server.authenticate(&second.token, 1_101)).expect("auth");
    assert_eq!(device.name, "phone");
    // `last_seen` is written at most every five minutes.
    assert_eq!(device.last_seen, 1_100);
    let refreshed = pollster::block_on(server.authenticate(&second.token, 1_400)).expect("auth");
    assert_eq!(refreshed.last_seen, 1_400);

    let listed = pollster::block_on(server.list_devices(&second.device_id)).expect("devices");
    assert_eq!(listed.devices.len(), 2);
    assert!(listed
        .devices
        .iter()
        .any(|device| device.id == second.device_id && device.current));
}

#[test]
fn a_bad_login_key_waits_before_the_next_attempt() {
    let server = server();
    pollster::block_on(server.setup(Some("setup-secret"), &request("alice", "laptop", 7), 1_000))
        .expect("setup");

    let wrong = request("alice", "tablet", 8);
    assert_eq!(
        pollster::block_on(server.login(&wrong, 2_000)).unwrap_err(),
        ServerError::InvalidCredentials
    );
    assert_eq!(
        pollster::block_on(server.storage().account())
            .expect("account")
            .expect("some")
            .failed_logins,
        1
    );

    let right = request("alice", "tablet", 7);
    assert!(matches!(
        pollster::block_on(server.login(&right, 2_000)).unwrap_err(),
        ServerError::RateLimited { retry_after: 1 }
    ));
    pollster::block_on(server.login(&right, 2_002)).expect("retry after the delay");
}

#[test]
fn an_unknown_username_does_not_count_as_a_failed_login() {
    let server = server();
    pollster::block_on(server.setup(Some("setup-secret"), &request("alice", "laptop", 7), 1_000))
        .expect("setup");

    assert_eq!(
        pollster::block_on(server.login(&request("bob", "tablet", 8), 2_000)).unwrap_err(),
        ServerError::InvalidCredentials
    );
    let account = pollster::block_on(server.storage().account())
        .expect("account")
        .expect("some");
    assert_eq!(account.failed_logins, 0);
    assert_eq!(account.blocked_until, 0);

    // The real account is not blocked by a stranger's username.
    pollster::block_on(server.login(&request("alice", "tablet", 7), 2_000)).expect("login");
}

#[test]
fn repeated_failures_back_off_exponentially() {
    let server = server();
    pollster::block_on(server.setup(Some("setup-secret"), &request("alice", "laptop", 7), 1_000))
        .expect("setup");
    let wrong = request("alice", "tablet", 9);

    assert_eq!(
        pollster::block_on(server.login(&wrong, 2_000)).unwrap_err(),
        ServerError::InvalidCredentials
    );
    assert_eq!(
        pollster::block_on(server.login(&wrong, 2_001)).unwrap_err(),
        ServerError::InvalidCredentials
    );
    assert_eq!(
        pollster::block_on(server.login(&wrong, 2_003)).unwrap_err(),
        ServerError::RateLimited { retry_after: 4 }
    );
    assert_eq!(
        pollster::block_on(server.login(&request("alice", "tablet", 7), 2_003)).unwrap_err(),
        ServerError::RateLimited { retry_after: 4 }
    );
    pollster::block_on(server.login(&request("alice", "tablet", 7), 2_007))
        .expect("unblocked after four seconds");
    assert_eq!(
        pollster::block_on(server.storage().account())
            .expect("account")
            .expect("some")
            .failed_logins,
        0
    );
}

#[test]
fn a_credential_change_keeps_this_device_and_revokes_the_others() {
    let server = server();
    let laptop = pollster::block_on(server.setup(
        Some("setup-secret"),
        &request("alice", "laptop", 7),
        1_000,
    ))
    .expect("setup");
    let phone =
        pollster::block_on(server.login(&request("alice", "phone", 7), 1_100)).expect("login");
    let laptop_device = pollster::block_on(server.authenticate(&laptop.token, 1_100)).expect("a");

    let response =
        pollster::block_on(server.update_credentials(&laptop_device, &change(7, "bob", 9)))
            .expect("update");
    assert_eq!(response.username, "bob");

    let account = pollster::block_on(server.storage().account())
        .expect("account")
        .expect("some");
    assert_eq!(account.username, "bob");
    assert_eq!(
        account.auth_hash,
        notes_server::sha256_hex(&[9; notes_proto::LOGIN_KEY_BYTES])
    );
    assert_eq!(account.failed_logins, 0);
    assert_eq!(account.blocked_until, 0);
    assert_eq!(server.storage().device_count(), 1);

    // The device that made the change keeps its token; the other one is gone.
    pollster::block_on(server.authenticate(&laptop.token, 1_200)).expect("laptop stays signed in");
    assert_eq!(
        pollster::block_on(server.authenticate(&phone.token, 1_200)).unwrap_err(),
        ServerError::Unauthorized
    );

    // The old username/key pair no longer logs in; the new one does.
    assert_eq!(
        pollster::block_on(server.login(&request("alice", "tablet", 7), 1_300)).unwrap_err(),
        ServerError::InvalidCredentials
    );
    pollster::block_on(server.login(&request("bob", "tablet", 9), 1_300)).expect("new login");
}

#[test]
fn a_wrong_current_password_changes_nothing() {
    let server = server();
    let laptop = pollster::block_on(server.setup(
        Some("setup-secret"),
        &request("alice", "laptop", 7),
        1_000,
    ))
    .expect("setup");
    let phone =
        pollster::block_on(server.login(&request("alice", "phone", 7), 1_100)).expect("login");
    let laptop_device = pollster::block_on(server.authenticate(&laptop.token, 1_100)).expect("a");

    assert_eq!(
        pollster::block_on(server.update_credentials(&laptop_device, &change(8, "bob", 9)))
            .unwrap_err(),
        ServerError::WrongPassword
    );

    let account = pollster::block_on(server.storage().account())
        .expect("account")
        .expect("some");
    assert_eq!(account.username, "alice");
    assert_eq!(
        account.auth_hash,
        notes_server::sha256_hex(&[7; notes_proto::LOGIN_KEY_BYTES])
    );
    assert_eq!(server.storage().device_count(), 2);
    pollster::block_on(server.authenticate(&laptop.token, 1_200)).expect("laptop");
    pollster::block_on(server.authenticate(&phone.token, 1_200)).expect("phone");
}

#[test]
fn a_credential_change_validates_the_new_fields() {
    let server = server();
    let laptop = pollster::block_on(server.setup(
        Some("setup-secret"),
        &request("alice", "laptop", 7),
        1_000,
    ))
    .expect("setup");
    let device = pollster::block_on(server.authenticate(&laptop.token, 1_000)).expect("auth");

    let blank = change(7, "   ", 9);
    assert!(matches!(
        pollster::block_on(server.update_credentials(&device, &blank)).unwrap_err(),
        ServerError::Invalid(_)
    ));
    let mut bad_key = change(7, "bob", 9);
    bad_key.new_login_key = Base64Bytes::new(vec![9; 31]);
    assert!(matches!(
        pollster::block_on(server.update_credentials(&device, &bad_key)).unwrap_err(),
        ServerError::Invalid(_)
    ));
    let control = change(7, "bob\nadmin", 9);
    assert!(matches!(
        pollster::block_on(server.update_credentials(&device, &control)).unwrap_err(),
        ServerError::Invalid(_)
    ));
    let valid = change(7, " bob ", 9);
    let response =
        pollster::block_on(server.update_credentials(&device, &valid)).expect("trimmed username");
    assert_eq!(response.username, "bob");
    let account = pollster::block_on(server.storage().account())
        .expect("account")
        .expect("some");
    assert_eq!(account.username, "bob");
}

#[test]
fn sync_pushes_then_pulls_only_other_devices() {
    let server = server();
    let laptop = pollster::block_on(server.setup(
        Some("setup-secret"),
        &request("alice", "laptop", 7),
        1_000,
    ))
    .expect("setup");
    let phone =
        pollster::block_on(server.login(&request("alice", "phone", 7), 1_100)).expect("login");
    let laptop_device = pollster::block_on(server.authenticate(&laptop.token, 1_100)).expect("a");
    let phone_device = pollster::block_on(server.authenticate(&phone.token, 1_100)).expect("b");

    let pushed = SyncRequest {
        cursor: 0,
        updates: vec![update(1), update(2)],
    };
    let laptop_after =
        pollster::block_on(server.sync(&laptop_device, &pushed, 1_200)).expect("sync");
    // Own updates are filtered out, but the cursor still advances past them.
    assert!(laptop_after.updates.is_empty());
    assert_eq!(laptop_after.cursor, 2);
    assert!(!laptop_after.more);

    let phone_after = pollster::block_on(server.sync(
        &phone_device,
        &SyncRequest {
            cursor: 0,
            updates: Vec::new(),
        },
        1_200,
    ))
    .expect("pull");
    assert_eq!(phone_after.updates.len(), 2);
    assert_eq!(phone_after.updates[0].seq, 1);
    assert_eq!(phone_after.updates[0].device_id, laptop_device.id);
    assert_eq!(phone_after.updates[0].blob.as_bytes(), vec![1; 8]);
    assert_eq!(phone_after.cursor, 2);

    let from_phone = pollster::block_on(server.sync(
        &phone_device,
        &SyncRequest {
            cursor: phone_after.cursor,
            updates: vec![update(3)],
        },
        1_300,
    ))
    .expect("push");
    assert!(from_phone.updates.is_empty());

    let laptop_pull = pollster::block_on(server.sync(
        &laptop_device,
        &SyncRequest {
            cursor: laptop_after.cursor,
            updates: Vec::new(),
        },
        1_300,
    ))
    .expect("pull");
    assert_eq!(laptop_pull.updates.len(), 1);
    assert_eq!(laptop_pull.updates[0].seq, 3);
    assert_eq!(laptop_pull.cursor, 3);
}

#[test]
fn a_pull_pages_at_one_hundred_rows() {
    let server = server();
    let laptop = pollster::block_on(server.setup(
        Some("setup-secret"),
        &request("alice", "laptop", 7),
        1_000,
    ))
    .expect("setup");
    let phone =
        pollster::block_on(server.login(&request("alice", "phone", 7), 1_100)).expect("login");
    let laptop_device = pollster::block_on(server.authenticate(&laptop.token, 1_100)).expect("a");
    let phone_device = pollster::block_on(server.authenticate(&phone.token, 1_100)).expect("b");

    let mut pushed = 0;
    while pushed < 120 {
        let count = (120 - pushed).min(MAX_PUSH_UPDATES);
        let batch: Vec<PushUpdate> = (0..count)
            .map(|index| update((pushed + index) as u8))
            .collect();
        pollster::block_on(server.sync(
            &phone_device,
            &SyncRequest {
                cursor: 0,
                updates: batch,
            },
            1_200,
        ))
        .expect("push");
        pushed += count;
    }
    assert_eq!(server.storage().update_count(), 120);

    let first = pollster::block_on(server.sync(
        &laptop_device,
        &SyncRequest {
            cursor: 0,
            updates: Vec::new(),
        },
        1_300,
    ))
    .expect("page one");
    assert_eq!(first.updates.len(), 100);
    assert!(first.more);
    assert_eq!(first.cursor, 100);

    let second = pollster::block_on(server.sync(
        &laptop_device,
        &SyncRequest {
            cursor: first.cursor,
            updates: Vec::new(),
        },
        1_300,
    ))
    .expect("page two");
    assert_eq!(second.updates.len(), 20);
    assert!(!second.more);
    assert_eq!(second.cursor, 120);
    assert_eq!(second.updates[19].seq, 120);
}

#[test]
fn a_page_of_only_own_updates_still_advances_the_cursor() {
    let server = server();
    let laptop = pollster::block_on(server.setup(
        Some("setup-secret"),
        &request("alice", "laptop", 7),
        1_000,
    ))
    .expect("setup");
    let phone =
        pollster::block_on(server.login(&request("alice", "phone", 7), 1_100)).expect("login");
    let laptop_device = pollster::block_on(server.authenticate(&laptop.token, 1_100)).expect("a");
    let phone_device = pollster::block_on(server.authenticate(&phone.token, 1_100)).expect("b");

    for _ in 0..4 {
        pollster::block_on(server.sync(
            &laptop_device,
            &SyncRequest {
                cursor: 0,
                updates: (0..MAX_PUSH_UPDATES).map(|_| update(1)).collect(),
            },
            1_200,
        ))
        .expect("own push");
    }
    pollster::block_on(server.sync(
        &phone_device,
        &SyncRequest {
            cursor: 0,
            updates: vec![update(2)],
        },
        1_300,
    ))
    .expect("phone push");

    let first = pollster::block_on(server.sync(
        &laptop_device,
        &SyncRequest {
            cursor: 0,
            updates: Vec::new(),
        },
        1_400,
    ))
    .expect("first pull");
    assert!(first.updates.is_empty());
    assert!(first.more);
    assert_eq!(first.cursor, 100);

    let second = pollster::block_on(server.sync(
        &laptop_device,
        &SyncRequest {
            cursor: first.cursor,
            updates: Vec::new(),
        },
        1_400,
    ))
    .expect("second pull");
    assert_eq!(second.updates.len(), 1);
    assert_eq!(second.cursor, 101);
    assert!(!second.more);
}

#[test]
fn sync_rejects_bad_sizes_and_cursors() {
    let server = server();
    let laptop = pollster::block_on(server.setup(
        Some("setup-secret"),
        &request("alice", "laptop", 7),
        1_000,
    ))
    .expect("setup");
    let device = pollster::block_on(server.authenticate(&laptop.token, 1_000)).expect("auth");

    let too_many = SyncRequest {
        cursor: 0,
        updates: (0..=MAX_PUSH_UPDATES).map(|_| update(1)).collect(),
    };
    assert!(matches!(
        pollster::block_on(server.sync(&device, &too_many, 1_100)).unwrap_err(),
        ServerError::TooLarge(_)
    ));

    let too_big = SyncRequest {
        cursor: 0,
        updates: vec![PushUpdate {
            note_id: NoteId::new(),
            blob: Base64Bytes::new(vec![0; MAX_UPDATE_BYTES + 1]),
        }],
    };
    assert!(matches!(
        pollster::block_on(server.sync(&device, &too_big, 1_100)).unwrap_err(),
        ServerError::TooLarge(_)
    ));

    let negative = SyncRequest {
        cursor: -1,
        updates: Vec::new(),
    };
    assert!(matches!(
        pollster::block_on(server.sync(&device, &negative, 1_100)).unwrap_err(),
        ServerError::Invalid(_)
    ));
}

#[test]
fn tokens_are_hashed_and_revocation_is_immediate() {
    let server = server();
    let laptop = pollster::block_on(server.setup(
        Some("setup-secret"),
        &request("alice", "laptop", 7),
        1_000,
    ))
    .expect("setup");
    let phone =
        pollster::block_on(server.login(&request("alice", "phone", 7), 1_100)).expect("login");

    assert!(pollster::block_on(server.authenticate("not-a-token", 1_200)).is_err());
    assert!(pollster::block_on(server.authenticate(&"a".repeat(64), 1_200)).is_err());

    assert!(pollster::block_on(server.revoke_device(&phone.device_id)).expect("revoke"));
    assert_eq!(
        pollster::block_on(server.authenticate(&phone.token, 1_300)).unwrap_err(),
        ServerError::Unauthorized
    );
    assert!(!pollster::block_on(server.revoke_device(&phone.device_id)).expect("revoke again"));
    pollster::block_on(server.authenticate(&laptop.token, 1_300)).expect("laptop survives");
}

#[test]
fn a_blob_chunk_round_trips_and_is_idempotent() {
    let server = server();
    let laptop = pollster::block_on(server.setup(
        Some("setup-secret"),
        &request("alice", "laptop", 7),
        1_000,
    ))
    .expect("setup");
    let chunk = BlobChunk {
        hash: "ab".repeat(32),
        index: 0,
        total: 1,
        size: 3,
        mime: "image/webp".into(),
        data: Base64Bytes::new(vec![1, 2, 3]),
    };
    pollster::block_on(server.put_blob(&chunk, 1_100)).expect("upload");
    pollster::block_on(server.put_blob(&chunk, 1_101)).expect("idempotent upload");
    assert_eq!(server.storage().chunk_count(), 1);

    let downloaded = pollster::block_on(server.get_blob(&chunk.hash, 0)).expect("download");
    assert_eq!(downloaded, chunk);

    let larger = BlobChunk {
        size: 4,
        ..chunk.clone()
    };
    assert!(matches!(
        pollster::block_on(server.put_blob(&larger, 1_200)).unwrap_err(),
        ServerError::Conflict(_)
    ));
    assert!(matches!(
        pollster::block_on(server.get_blob(&chunk.hash, 1)).unwrap_err(),
        ServerError::NotFound(_)
    ));
    assert!(matches!(
        pollster::block_on(server.get_blob(&"AB".repeat(32), 0)).unwrap_err(),
        ServerError::Invalid(_)
    ));
    let _ = laptop;
}

#[test]
fn malformed_fields_are_refused() {
    let server = server();
    let mut auth = request("alice", "laptop", 7);
    auth.login_key = Base64Bytes::new(vec![0; 31]);
    assert!(matches!(
        pollster::block_on(server.setup(Some("setup-secret"), &auth, 1_000)).unwrap_err(),
        ServerError::Invalid(_)
    ));

    let mut auth = request(&"a".repeat(65), "laptop", 7);
    assert!(matches!(
        pollster::block_on(server.setup(Some("setup-secret"), &auth, 1_000)).unwrap_err(),
        ServerError::Invalid(_)
    ));

    auth = request("alice", "  ", 7);
    assert!(matches!(
        pollster::block_on(server.setup(Some("setup-secret"), &auth, 1_000)).unwrap_err(),
        ServerError::Invalid(_)
    ));
}

#[test]
fn blob_limits_are_enforced() {
    let server = server();
    let laptop = pollster::block_on(server.setup(
        Some("setup-secret"),
        &request("alice", "laptop", 7),
        1_000,
    ))
    .expect("setup");
    let _ = laptop;

    let empty = BlobChunk {
        hash: "ab".repeat(32),
        index: 0,
        total: 1,
        size: 3,
        mime: "image/webp".into(),
        data: Base64Bytes::new(Vec::new()),
    };
    assert!(matches!(
        pollster::block_on(server.put_blob(&empty, 1_100)).unwrap_err(),
        ServerError::TooLarge(_)
    ));

    let bad_index = BlobChunk {
        index: 1,
        ..empty.clone()
    };
    assert!(matches!(
        pollster::block_on(server.put_blob(&bad_index, 1_100)).unwrap_err(),
        ServerError::Invalid(_)
    ));

    let bad_mime = BlobChunk {
        mime: String::new(),
        data: Base64Bytes::new(vec![1]),
        ..empty
    };
    assert!(matches!(
        pollster::block_on(server.put_blob(&bad_mime, 1_100)).unwrap_err(),
        ServerError::Invalid(_)
    ));
}
