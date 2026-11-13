use std::fmt;
use std::sync::Arc;

/// The clipboard operation a [`ClipboardError`] belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardOperation {
    /// Reading text from the system clipboard.
    ReadClipboard,
    /// Writing text to the system clipboard.
    WriteClipboard,
    /// Reading text from the primary selection.
    ReadPrimarySelection,
    /// Writing text to the primary selection.
    WritePrimarySelection,
    /// Reading an image from the clipboard.
    ReadImageClipboard,
    /// Writing an image to the clipboard.
    WriteImageClipboard,
    /// Reading file references from the clipboard.
    ReadFileClipboard,
    /// Writing file references to the clipboard.
    WriteFileClipboard,
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
    /// Caller supplied arguments the operation cannot represent.
    ///
    /// Distinct from [`Self::Provider`]: the platform clipboard was never asked
    /// to do anything, so retrying without changing the input cannot succeed.
    InvalidInput {
        /// Operation that was rejected.
        operation: ClipboardOperation,
        /// What was wrong with the input.
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

    /// Create an invalid input error with a message.
    pub fn invalid_input(operation: ClipboardOperation, message: impl Into<Arc<str>>) -> Self {
        Self::InvalidInput {
            operation,
            message: message.into(),
        }
    }

    /// Return the clipboard operation associated with the error.
    pub fn operation(&self) -> ClipboardOperation {
        match self {
            Self::Unsupported { operation } => *operation,
            Self::Provider { operation, .. } => *operation,
            Self::InvalidInput { operation, .. } => *operation,
        }
    }

    /// A sentence for the person at the terminal, such as an error toast.
    ///
    /// [`fmt::Display`] stays diagnostic (it names the operation variant); this names what
    /// the user tried, for example "Reading the clipboard is not supported on this system".
    pub fn user_message(&self) -> String {
        let target = match self.operation() {
            ClipboardOperation::ReadPrimarySelection
            | ClipboardOperation::WritePrimarySelection => "the primary selection",
            _ => "the clipboard",
        };
        let reading = self.operation().is_read();
        match self {
            Self::Unsupported { .. } if reading => {
                format!("Reading {target} is not supported on this system")
            }
            Self::Unsupported { .. } => {
                format!("Copying to {target} is not supported on this system")
            }
            Self::Provider { message, .. } | Self::InvalidInput { message, .. } if reading => {
                format!("Could not read {target}: {message}")
            }
            Self::Provider { message, .. } | Self::InvalidInput { message, .. } => {
                format!("Could not copy to {target}: {message}")
            }
        }
    }
}

impl ClipboardOperation {
    /// Whether the operation reads (pastes) rather than writes (copies).
    pub fn is_read(self) -> bool {
        matches!(
            self,
            Self::ReadClipboard
                | Self::ReadPrimarySelection
                | Self::ReadImageClipboard
                | Self::ReadFileClipboard
        )
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
            Self::InvalidInput { operation, message } => {
                write!(
                    f,
                    "clipboard operation {:?} rejected: {}",
                    operation, message
                )
            }
        }
    }
}

impl std::error::Error for ClipboardError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_messages_name_what_the_user_tried() {
        let read = ClipboardOperation::ReadClipboard;
        let write = ClipboardOperation::WriteClipboard;
        assert_eq!(
            ClipboardError::unsupported(read).user_message(),
            "Reading the clipboard is not supported on this system"
        );
        assert_eq!(
            ClipboardError::unsupported(write).user_message(),
            "Copying to the clipboard is not supported on this system"
        );
        assert_eq!(
            ClipboardError::provider(ClipboardOperation::ReadImageClipboard, "no display")
                .user_message(),
            "Could not read the clipboard: no display"
        );
        assert_eq!(
            ClipboardError::invalid_input(ClipboardOperation::WritePrimarySelection, "too large")
                .user_message(),
            "Could not copy to the primary selection: too large"
        );
    }
}
