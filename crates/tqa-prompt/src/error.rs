//! Instruction discovery fails closed on path and encoding errors.

/// An instruction file could not be loaded safely.
#[derive(Debug, thiserror::Error)]
pub enum PromptError {
    /// Workspace confinement or path syntax failed.
    #[error("instruction path rejected: {0}")]
    Path(#[from] tqa_tools::ToolError),
    /// The current directory or instruction file has the wrong kind or path encoding.
    #[error("invalid instruction path")]
    InvalidPath,
    /// Instructions must be UTF-8 text.
    #[error("instructions are not UTF-8")]
    InvalidEncoding,
    /// Reading the bounded instruction file failed.
    #[error("instruction I/O failed")]
    Io(#[from] std::io::Error),
}
