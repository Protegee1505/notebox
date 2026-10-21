//! Measure the client-side Argon2id login key on this machine.
//!
//! The same derivation runs on desktop, WASM and Android; this example gives
//! the native release number for the S3 record.
//!
//! ```text
//! cargo run -p notes-sync --example argon2_bench --release
//! ```

use std::error::Error;
use std::time::Instant;

use notes_sync::derive_login_key;

const RUNS: usize = 20;

fn main() -> Result<(), Box<dyn Error>> {
    // One warm-up so the first page faults and allocator growth are not
    // reported as the steady-state cost.
    derive_login_key("argon2-bench", "correct horse battery staple")?;

    let mut samples = Vec::with_capacity(RUNS);
    for _ in 0..RUNS {
        let started = Instant::now();
        let key = derive_login_key("argon2-bench", "correct horse battery staple")?;
        samples.push(started.elapsed().as_secs_f64() * 1_000.0);
        std::hint::black_box(key);
    }
    samples.sort_by(|a, b| a.total_cmp(b));

    let median = samples[samples.len() / 2];
    let mean = samples.iter().sum::<f64>() / samples.len() as f64;
    println!(
        "argon2id {} runs: median {median:.1} ms, mean {mean:.1} ms, min {:.1} ms, max {:.1} ms",
        samples.len(),
        samples[0],
        samples[samples.len() - 1],
    );
    Ok(())
}
