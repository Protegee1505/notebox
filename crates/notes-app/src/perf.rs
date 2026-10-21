//! Optional probes for the P8 performance budgets.
//!
//! They are inert unless the matching environment variable is set. The cold
//! start script writes a timestamp to `NBX_READY_FILE`; production runs never
//! set it.

/// Record the time the editor became usable.
pub fn editor_ready() {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let Some(path) = std::env::var_os("NBX_READY_FILE") else {
            return;
        };
        let millis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or_default();
        let _ = std::fs::write(path, millis.to_string());
    }
}
