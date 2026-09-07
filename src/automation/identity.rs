use std::sync::Arc;

/// App-authored identity for stable automation.
///
/// Unlike [`crate::Key`], an automation ID is global to one realized semantic
/// tree. The session rejects duplicate IDs in every build.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AutomationId(Arc<str>);

impl AutomationId {
    /// Maximum encoded ID length accepted by scripts and transports.
    pub const MAX_LEN: usize = 128;

    /// Create an automation ID.
    ///
    /// # Panics
    ///
    /// Panics when `value` is empty, too long, or contains characters that
    /// cannot round-trip through action scripts and line-oriented transports.
    pub fn new(value: impl Into<Arc<str>>) -> Self {
        Self::try_new(value).unwrap_or_else(|error| panic!("invalid automation ID: {error}"))
    }

    /// Validate and create an automation ID without panicking.
    pub fn try_new(value: impl Into<Arc<str>>) -> Result<Self, AutomationIdError> {
        let value = value.into();
        validate(value.as_ref())?;
        Ok(Self(value))
    }
}

/// Why an app-authored automation ID was rejected.
#[derive(Clone, Copy, Debug, thiserror::Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum AutomationIdError {
    /// IDs cannot be empty.
    #[error("the ID is empty")]
    Empty,
    /// IDs are bounded for control-protocol and diagnostic safety.
    #[error("the ID exceeds {max} bytes")]
    TooLong {
        /// Maximum accepted byte length.
        max: usize,
    },
    /// A character cannot be represented by the stable script grammar.
    #[error("unsupported character {character:?}")]
    InvalidCharacter {
        /// Rejected character.
        character: char,
    },
}

fn validate(value: &str) -> Result<(), AutomationIdError> {
    if value.is_empty() {
        return Err(AutomationIdError::Empty);
    }
    if value.len() > AutomationId::MAX_LEN {
        return Err(AutomationIdError::TooLong {
            max: AutomationId::MAX_LEN,
        });
    }
    if let Some(character) = value
        .chars()
        .find(|character| !is_valid_character(*character))
    {
        return Err(AutomationIdError::InvalidCharacter { character });
    }
    Ok(())
}

fn is_valid_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.' | '/' | ':')
}

impl From<&'static str> for AutomationId {
    fn from(value: &'static str) -> Self {
        Self::new(value)
    }
}

impl From<String> for AutomationId {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl From<Arc<str>> for AutomationId {
    fn from(value: Arc<str>) -> Self {
        Self::new(value)
    }
}

impl AsRef<str> for AutomationId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for AutomationId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip_through_the_script_grammar() {
        assert!(AutomationId::try_new("dialog/save:primary").is_ok());
        for invalid in ["", "two words", "line\nbreak", "comma,id", "drag>target"] {
            assert!(AutomationId::try_new(invalid).is_err(), "{invalid:?}");
        }
    }
}
