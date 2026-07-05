//! URL opening helpers.

/// Error returned by [`open_url`].
#[derive(Debug, thiserror::Error)]
pub enum OpenUrlError {
    /// URL uses an unsupported scheme.
    #[error("unsupported URL scheme `{scheme}`")]
    UnsupportedScheme {
        /// Parsed scheme value.
        scheme: String,
    },
    /// URL has no parseable scheme.
    #[error("URL is missing a valid scheme")]
    MissingScheme,
    /// Failed to launch the platform opener command.
    #[error("failed to open URL: {0}")]
    Launch(#[from] std::io::Error),
    /// Host platform cannot spawn an external opener.
    #[error("opening URLs is not available on this target")]
    UnsupportedTarget,
}

/// Open a URL with the system's default handler.
///
/// Allowed schemes are: `http`, `https`, and `mailto`.
pub fn open_url(url: &str) -> Result<(), OpenUrlError> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = url;
        return Err(OpenUrlError::UnsupportedTarget);
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let scheme = parse_scheme(url).ok_or(OpenUrlError::MissingScheme)?;
        if !is_allowed_scheme(scheme) {
            return Err(OpenUrlError::UnsupportedScheme {
                scheme: scheme.to_string(),
            });
        }

        open::that_detached(url).map_err(OpenUrlError::from)
    }
}

fn parse_scheme(url: &str) -> Option<&str> {
    let (scheme, _rest) = url.split_once(':')?;
    if scheme.is_empty() {
        return None;
    }

    let mut chars = scheme.chars();
    let first = chars.next()?;
    if !first.is_ascii_alphabetic() {
        return None;
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')) {
        return None;
    }

    Some(scheme)
}

fn is_allowed_scheme(scheme: &str) -> bool {
    matches!(scheme, "http" | "https" | "mailto")
}

#[cfg(test)]
mod tests {
    use super::{OpenUrlError, parse_scheme};

    #[test]
    fn parse_scheme_accepts_rfc_like_names() {
        assert_eq!(parse_scheme("https://example.com"), Some("https"));
        assert_eq!(parse_scheme("mailto:hello@example.com"), Some("mailto"));
        assert_eq!(
            parse_scheme("foo+bar.baz-qux:value"),
            Some("foo+bar.baz-qux")
        );
    }

    #[test]
    fn parse_scheme_rejects_invalid_or_missing_scheme() {
        assert_eq!(parse_scheme(""), None);
        assert_eq!(parse_scheme("//example.com"), None);
        assert_eq!(parse_scheme("1http://example.com"), None);
        assert_eq!(parse_scheme("ht*tp://example.com"), None);
    }

    #[test]
    fn unsupported_scheme_error_message_contains_scheme() {
        let err = OpenUrlError::UnsupportedScheme {
            scheme: "file".to_string(),
        };
        assert!(err.to_string().contains("file"));
    }
}
