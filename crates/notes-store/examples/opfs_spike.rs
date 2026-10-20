//! S4 probe: sqlite-wasm-rs over the OPFS SAH-pool VFS in a dedicated worker.
//!
//! The example is built for `wasm32-unknown-unknown`, processed with
//! `wasm-bindgen`, and loaded from `crates/notes-store/web/worker.js`.
//! `scripts/opfs-spike.mjs` drives a page reload in each browser and checks
//! that the same 1,000 notes come back.
//!
//! Run natively only to keep `cargo test --workspace` happy: there is no browser
//! database outside wasm.

#[cfg(target_arch = "wasm32")]
mod browser {
    use rusqlite::{params, Connection};
    use sqlite_wasm_rs::WasmOsCallback;
    use sqlite_wasm_vfs::sahpool::{install, OpfsSAHPoolCfgBuilder};
    use wasm_bindgen::prelude::*;

    const DB_NAME: &str = "notebox-s4.db";
    const ROWS: usize = 1_000;

    /// Open the OPFS database, seed it on first run, and report what is stored.
    ///
    /// `reset` removes the rows so a test run starts from an empty database.
    #[wasm_bindgen]
    pub async fn run(reset: bool) -> Result<String, JsValue> {
        let cfg = OpfsSAHPoolCfgBuilder::new()
            .directory("notebox-s4")
            .initial_capacity(6)
            .build();
        install::<WasmOsCallback>(&cfg, true)
            .await
            .map_err(|error| JsValue::from_str(&format!("install VFS: {error}")))?;

        let mut conn = Connection::open(DB_NAME).map_err(|error| js(&format!("open: {error}")))?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS note (
                 id INTEGER PRIMARY KEY,
                 body TEXT NOT NULL
             );",
        )
        .map_err(|error| js(&format!("schema: {error}")))?;

        // Keep the database in rollback-journal mode: the SAH-pool VFS does not
        // support shared-memory WAL.
        conn.pragma_update(None, "journal_mode", "delete")
            .map_err(|error| js(&format!("journal mode: {error}")))?;

        if reset {
            conn.execute("DELETE FROM note", [])
                .map_err(|error| js(&format!("reset: {error}")))?;
        }

        let before: i64 = conn
            .query_row("SELECT count(*) FROM note", [], |row| row.get(0))
            .map_err(|error| js(&format!("count before: {error}")))?;

        let seeded = before == 0;
        if seeded {
            let tx = conn
                .transaction()
                .map_err(|error| js(&format!("begin: {error}")))?;
            {
                let mut insert = tx
                    .prepare("INSERT INTO note (body) VALUES (?1)")
                    .map_err(|error| js(&format!("prepare insert: {error}")))?;
                for i in 0..ROWS {
                    let body = format!(
                        "note {i:04}: the quick brown fox jumps over the lazy dog — \
                         Unicode check 😀 中文\nsecond line"
                    );
                    insert
                        .execute(params![body])
                        .map_err(|error| js(&format!("insert {i}: {error}")))?;
                }
            }
            tx.commit()
                .map_err(|error| js(&format!("commit: {error}")))?;
        }

        let count: i64 = conn
            .query_row("SELECT count(*) FROM note", [], |row| row.get(0))
            .map_err(|error| js(&format!("count after: {error}")))?;
        let total_chars: i64 = conn
            .query_row(
                "SELECT coalesce(sum(length(body)), 0) FROM note",
                [],
                |row| row.get(0),
            )
            .map_err(|error| js(&format!("sum: {error}")))?;
        let first: String = conn
            .query_row("SELECT body FROM note ORDER BY id LIMIT 1", [], |row| {
                row.get(0)
            })
            .map_err(|error| js(&format!("first: {error}")))?;
        let last: String = conn
            .query_row(
                "SELECT body FROM note ORDER BY id DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .map_err(|error| js(&format!("last: {error}")))?;
        let page_count: i64 = conn
            .query_row("PRAGMA page_count", [], |row| row.get(0))
            .map_err(|error| js(&format!("page count: {error}")))?;
        let page_size: i64 = conn
            .query_row("PRAGMA page_size", [], |row| row.get(0))
            .map_err(|error| js(&format!("page size: {error}")))?;
        let journal_mode: String = conn
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .map_err(|error| js(&format!("journal mode read: {error}")))?;

        Ok(format!(
            "{{\"count\":{count},\"totalChars\":{total_chars},\"seeded\":{seeded},\
             \"first\":{},\"last\":{},\"pageCount\":{page_count},\
             \"pageSize\":{page_size},\"journalMode\":{},\"bytes\":{}}}",
            json_string(&first),
            json_string(&last),
            json_string(&journal_mode),
            page_count.saturating_mul(page_size)
        ))
    }

    fn js(message: &str) -> JsValue {
        JsValue::from_str(message)
    }

    fn json_string(value: &str) -> String {
        let mut out = String::with_capacity(value.len() + 2);
        out.push('"');
        for ch in value.chars() {
            match ch {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                ch if (ch as u32) < 0x20 => {
                    out.push_str(&format!("\\u{:04x}", ch as u32));
                }
                ch => out.push(ch),
            }
        }
        out.push('"');
        out
    }
}

#[cfg(target_arch = "wasm32")]
pub use browser::run;

#[cfg(target_arch = "wasm32")]
fn main() {}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    eprintln!("opfs_spike only runs in a browser worker; build it for wasm32.");
}
