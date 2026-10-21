//! Version and build identity shown in Settings.
//!
//! CI sets `NBX_BUILD_ID` and `NBX_GIT_SHA` before the build. A local build
//! deliberately says `dev` rather than borrowing the checkout's commit, so a
//! screenshot of a development build cannot be mistaken for a release.

/// Package version from `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Build identifier set by the release workflow.
pub const BUILD: &str = match option_env!("NBX_BUILD_ID") {
    Some(value) => value,
    None => "dev",
};

/// Full commit hash set by the release workflow.
pub const COMMIT: &str = match option_env!("NBX_GIT_SHA") {
    Some(value) => value,
    None => "unknown",
};

/// The first eight characters of the commit, or `unknown`.
pub fn short_commit() -> &'static str {
    if COMMIT == "unknown" {
        return COMMIT;
    }
    match COMMIT.char_indices().nth(8) {
        Some((end, _)) if COMMIT.is_char_boundary(end) => &COMMIT[..end],
        _ => COMMIT,
    }
}

/// Version label for the About card, without a placeholder commit locally.
pub fn label() -> String {
    if COMMIT == "unknown" {
        format!("Notebox {VERSION} \u{00b7} build {BUILD}")
    } else {
        format!(
            "Notebox {VERSION} \u{00b7} build {BUILD} \u{00b7} {}",
            short_commit()
        )
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn short_commit_is_eight_characters_or_the_whole_value() {
        let short = super::short_commit();
        assert!(
            short == "unknown" || short.len() <= 8,
            "unexpected short commit {short:?}"
        );
    }

    #[test]
    fn label_never_says_unknown_commit() {
        let label = super::label();
        assert!(!label.contains("unknown"), "unexpected label {label:?}");
        assert!(label.starts_with("Notebox "), "unexpected label {label:?}");
    }
}
