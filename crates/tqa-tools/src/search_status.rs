//! Structured search completeness for the loop and tool cards.

/// The resource ceiling that stopped a search before exhaustion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SearchStopReason {
    /// The file or directory-entry ceiling was reached.
    Files,
    /// The scanned-byte ceiling was reached.
    Bytes,
    /// The elapsed-time ceiling was reached.
    Time,
}
impl SearchStopReason {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Files => "files",
            Self::Bytes => "bytes",
            Self::Time => "time",
        }
    }
}

/// Progress for one search call; notices carry the same facts to the model.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchStatus {
    /// True only when exhausted, without a budget stop or an additional page.
    pub complete: bool,
    /// Regular files examined by this call, excluding ignored paths.
    pub scanned_files: u64,
    /// File bytes examined by grep, or accounted from metadata by glob.
    pub scanned_bytes: u64,
    /// The budget that stopped the search, if any.
    pub reason: Option<SearchStopReason>,
    /// Stateless continuation with the same search options.
    pub next_cursor: Option<String>,
}
