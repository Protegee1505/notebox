//! Create the single account on a Notebox Worker.
//!
//! Run it once after deploying, while `SETUP_TOKEN` still exists:
//!
//! ```text
//! cargo run -p notes-sync --example create_account -- \
//!     --server https://notebox.example.workers.dev --username alice
//! ```
//!
//! The setup token is prompted without echo. It is the short-lived secret set
//! with `npx wrangler secret put SETUP_TOKEN`; delete that secret after the
//! account exists, and the server then refuses every later setup attempt.

use std::env;
use std::error::Error;
use std::io::{self, Write};

use notes_sync::{HttpTransport, MemoryTokenStore, SyncClient, SyncConfig};

const USAGE: &str = "\
Create the single account on a Notebox Worker.

Usage:
  create_account --server URL --username NAME [--device NAME]

Options:
  --server URL    Worker base URL. Defaults to NBX_SERVER_URL.
  --username NAME Account name. Prompted when omitted.
  --device NAME   Device name shown in Settings. Default: Account setup
  --help          Show this message.

Environment:
  NBX_SETUP_TOKEN The server's setup secret. Prompted when omitted.
  NBX_SERVER_URL  Default server URL.
";

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print!("{USAGE}");
        return Ok(());
    }

    let server = value(&args, "--server")
        .or_else(|| env::var("NBX_SERVER_URL").ok())
        .unwrap_or_default();
    let server = server.trim().trim_end_matches('/');
    if server.is_empty() {
        return Err("missing --server (or NBX_SERVER_URL)".into());
    }

    let username = match value(&args, "--username") {
        Some(name) => name,
        None => prompt("Username: ")?,
    };
    let username = username.trim().to_string();
    if username.is_empty() {
        return Err("username is empty".into());
    }

    let device = value(&args, "--device").unwrap_or_else(|| "Account setup".to_string());
    let setup_token = match env::var("NBX_SETUP_TOKEN") {
        Ok(token) if !token.trim().is_empty() => token,
        _ => rpassword::prompt_password("Setup token: ")?,
    };
    let setup_token = setup_token.trim();
    if setup_token.is_empty() {
        return Err("setup token is empty".into());
    }

    let passphrase = rpassword::prompt_password("Passphrase: ")?;
    let confirm = rpassword::prompt_password("Repeat passphrase: ")?;
    if passphrase != confirm {
        return Err("the two passphrases do not match".into());
    }
    if passphrase.chars().count() < 12 {
        return Err("use a passphrase of at least 12 characters".into());
    }

    let transport = HttpTransport::new(server)?;
    let client = SyncClient::new(transport, SyncConfig::new(&username, &device));
    let sessions = MemoryTokenStore::new();
    let session = pollster::block_on(client.setup(setup_token, &passphrase, &sessions))?;

    println!();
    println!("Account created for {username} on {server}.");
    match pollster::block_on(client.revoke_device(&session.device_id)) {
        Ok(()) => println!("Removed the temporary setup device."),
        Err(error) => println!(
            "Could not remove the temporary setup device ({error}). Revoke {} in Settings.",
            session.device_id
        ),
    }
    println!("Sign in on each device with the same username and passphrase.");
    println!();
    println!("Now close setup on the server:");
    println!("  npx wrangler secret delete SETUP_TOKEN");
    Ok(())
}

fn value(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|arg| arg == flag)
        .and_then(|index| args.get(index + 1))
        .cloned()
}

fn prompt(label: &str) -> Result<String, io::Error> {
    print!("{label}");
    io::stdout().flush()?;
    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    Ok(line.trim().to_string())
}
