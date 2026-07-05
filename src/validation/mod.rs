#![allow(clippy::type_complexity)]

//! Composable validation for form fields.
//!
//! [`Validator<T>`] collects ordered *rules* that each return
//! `Result<(), ValidationError>`.  Rules are added with the builder pattern
//! and evaluated left-to-right.
//!
//! ```
//! use tui_lipan::validation::{StringValidator, ValidationError};
//!
//! let v = StringValidator::new()
//!     .required("Name is required")
//!     .min_length(3, "At least 3 characters");
//!
//! assert!(v.validate("ab").is_err());
//! assert!(v.validate("abc").is_ok());
//! ```

use std::rc::Rc;
use std::sync::Arc;

// ---------------------------------------------------------------------------
// ValidationError
// ---------------------------------------------------------------------------

/// A single validation failure carrying a human-readable message.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ValidationError {
    /// Human-readable description of what went wrong.
    pub message: Arc<str>,
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ValidationError {}

// ---------------------------------------------------------------------------
// Rule / Validator
// ---------------------------------------------------------------------------

/// A single validation rule: a closure that inspects a value and returns
/// `Err(ValidationError)` on failure.
type Rule<T> = Rc<dyn Fn(&T) -> Result<(), ValidationError>>;

/// An ordered, cloneable collection of validation rules.
///
/// Add rules with the builder pattern (`rule`, `required`, `min_length`, …)
/// and evaluate with [`Validator::validate`] (first failure) or
/// [`Validator::validate_all`] (all failures).
pub struct Validator<T: ?Sized> {
    rules: Vec<Rule<T>>,
}

// `Rule<T>` is `Rc<dyn Fn>`, which is not `Debug`.  Provide a lightweight
// implementation that just reports the rule count.
impl<T: ?Sized> std::fmt::Debug for Validator<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Validator")
            .field("rules", &self.rules.len())
            .finish()
    }
}

impl<T: ?Sized> Clone for Validator<T> {
    fn clone(&self) -> Self {
        Self {
            rules: self.rules.clone(),
        }
    }
}

impl<T: ?Sized> Validator<T> {
    /// Create an empty validator (always validates successfully).
    pub fn new() -> Self {
        Self { rules: Vec::new() }
    }

    /// Add a custom validation rule (builder pattern).
    ///
    /// The closure receives a reference to the value and should return
    /// `Err(ValidationError)` on failure.
    pub fn rule(self, check: impl Fn(&T) -> Result<(), ValidationError> + 'static) -> Self {
        let mut this = self;
        this.rules.push(Rc::new(check));
        this
    }

    /// Validate the value, returning the **first** failure (if any).
    ///
    /// Rules are evaluated left-to-right; evaluation stops at the first
    /// error.
    pub fn validate(&self, value: &T) -> Result<(), ValidationError> {
        for rule in &self.rules {
            rule(value)?;
        }
        Ok(())
    }

    /// Validate the value, collecting **all** failures.
    ///
    /// Every rule is evaluated regardless of earlier failures.
    pub fn validate_all(&self, value: &T) -> Vec<ValidationError> {
        self.rules
            .iter()
            .filter_map(|rule| rule(value).err())
            .collect()
    }
}

impl<T: ?Sized> Default for Validator<T> {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// StringValidator helpers
// ---------------------------------------------------------------------------

/// Convenience alias for the most common validator kind.
pub type StringValidator = Validator<str>;

impl Validator<str> {
    /// Reject empty or whitespace-only strings.
    ///
    /// A string is considered *empty* when `trim()` yields an empty string.
    pub fn required(self, msg: impl Into<Arc<str>>) -> Self {
        let msg = msg.into();
        self.rule(move |s: &str| {
            if s.trim().is_empty() {
                Err(ValidationError {
                    message: msg.clone(),
                })
            } else {
                Ok(())
            }
        })
    }

    /// Reject strings shorter than `n` characters.
    pub fn min_length(self, n: usize, msg: impl Into<Arc<str>>) -> Self {
        let msg = msg.into();
        self.rule(move |s: &str| {
            if s.chars().count() < n {
                Err(ValidationError {
                    message: msg.clone(),
                })
            } else {
                Ok(())
            }
        })
    }

    /// Reject strings longer than `n` characters.
    pub fn max_length(self, n: usize, msg: impl Into<Arc<str>>) -> Self {
        let msg = msg.into();
        self.rule(move |s: &str| {
            if s.chars().count() > n {
                Err(ValidationError {
                    message: msg.clone(),
                })
            } else {
                Ok(())
            }
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn required_rejects_empty() {
        let v = StringValidator::new().required(Arc::from("required"));
        assert!(v.validate("").is_err());
        assert!(v.validate("   ").is_err());
    }

    #[test]
    fn required_accepts_non_empty() {
        let v = StringValidator::new().required(Arc::from("required"));
        assert!(v.validate("hello").is_ok());
    }

    #[test]
    fn min_length_rejects_short() {
        let v = StringValidator::new().min_length(3, Arc::from("too short"));
        assert!(v.validate("ab").is_err());
    }

    #[test]
    fn min_length_accepts_exact() {
        let v = StringValidator::new().min_length(3, Arc::from("too short"));
        assert!(v.validate("abc").is_ok());
    }

    #[test]
    fn max_length_rejects_long() {
        let v = StringValidator::new().max_length(5, Arc::from("too long"));
        assert!(v.validate("abcdef").is_err());
    }

    #[test]
    fn max_length_accepts_exact() {
        let v = StringValidator::new().max_length(5, Arc::from("too long"));
        assert!(v.validate("abcde").is_ok());
    }

    #[test]
    fn validate_returns_first_failure() {
        let v = StringValidator::new()
            .required(Arc::from("required"))
            .min_length(3, Arc::from("too short"));
        let err = v.validate("").unwrap_err();
        assert_eq!(&*err.message, "required");
    }

    #[test]
    fn validate_all_collects_all_failures() {
        let v = StringValidator::new()
            .min_length(5, Arc::from("too short"))
            .max_length(2, Arc::from("too long"));
        let errs = v.validate_all("abc");
        assert_eq!(errs.len(), 2);
        assert_eq!(&*errs[0].message, "too short");
        assert_eq!(&*errs[1].message, "too long");
    }

    #[test]
    fn chained_rules_compose_left_to_right() {
        let v = StringValidator::new()
            .required(Arc::from("required"))
            .min_length(3, Arc::from("min 3"))
            .max_length(10, Arc::from("max 10"));
        // "ab" passes required but fails min_length
        let err = v.validate("ab").unwrap_err();
        assert_eq!(&*err.message, "min 3");
        // "hello" passes all
        assert!(v.validate("hello").is_ok());
    }

    #[test]
    fn empty_validator_always_succeeds() {
        let v: StringValidator = Validator::new();
        assert!(v.validate("").is_ok());
        assert!(v.validate("anything").is_ok());
        assert!(v.validate_all("").is_empty());
    }
}
