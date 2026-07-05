use std::fmt;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardOperation {
    ReadClipboard,
    WriteClipboard,
    ReadPrimarySelection,
    WritePrimarySelection,
    ReadImageClipboard,
    WriteImageClipboard,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Clipboard error emitted by providers.
pub enum ClipboardError {
    /// Operation is not supported by the provider or platform.
    Unsupported {
        /// Operation that failed.
        operation: ClipboardOperation,
    },
    /// Provider returned an error while executing the operation.
    Provider {
        /// Operation that failed.
        operation: ClipboardOperation,
        /// Provider error message.
        message: Arc<str>,
    },
}

impl ClipboardError {
    /// Create an unsupported operation error.
    pub fn unsupported(operation: ClipboardOperation) -> Self {
        Self::Unsupported { operation }
    }

    /// Create a provider error with a message.
    pub fn provider(operation: ClipboardOperation, message: impl Into<Arc<str>>) -> Self {
        Self::Provider {
            operation,
            message: message.into(),
        }
    }

    /// Return the clipboard operation associated with the error.
    pub fn operation(&self) -> ClipboardOperation {
        match self {
            Self::Unsupported { operation } => *operation,
            Self::Provider { operation, .. } => *operation,
        }
    }
}

impl fmt::Display for ClipboardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported { operation } => {
                write!(f, "clipboard operation {:?} is unsupported", operation)
            }
            Self::Provider { operation, message } => {
                write!(f, "clipboard operation {:?} failed: {}", operation, message)
            }
        }
    }
}

impl std::error::Error for ClipboardError {}
