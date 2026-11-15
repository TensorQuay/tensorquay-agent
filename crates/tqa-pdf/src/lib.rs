//! The PDF reader (PRD item 30; the engine chosen by `tqa-pdf-bench`): a document's text by page for every model, and
//! a page as a PNG for a model that reads pictures. The reader runs inside the runtime, which is the sandbox's own
//! process, and is capped: how many pages it looks at, how much text a page yields, and the bytes it is given.
//!
//! Text comes from oxidize-pdf (pinned exactly; parsing, text and decryption only), pictures from hayro (pure Rust).
//! Nothing here reads a path: the caller opens the file confined and hands over its bytes.
pub mod render;

use std::io::Cursor;

use oxidize_pdf::parser::{ParseError, ParseOptions, PdfDocument, PdfReader};
use oxidize_pdf::text::ExtractionOptions;

/// The bytes every PDF starts with.
pub const MAGIC: &[u8] = b"%PDF-";
/// The most pages the reader looks at in one document: enough for any book, short of a hostile page tree.
pub const MAX_PAGES: u32 = 2_000;
/// The most text one page yields, in bytes: a page of dense text is some 5 KiB; past this the page is cut and says so.
pub const MAX_PAGE_TEXT_BYTES: usize = 64 * 1024;

/// Why a document couldn't be opened.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum OpenError {
    /// The bytes don't start a PDF.
    #[error("not a PDF")]
    NotPdf,
    /// The file is encrypted with a password the reader doesn't have (an empty user password opens by itself).
    #[error("a password is needed")]
    Password,
    /// The file has more pages than [`MAX_PAGES`].
    #[error("more than {MAX_PAGES} pages")]
    TooManyPages,
    /// The file is damaged beyond what the engine recovers; the engine's own words.
    #[error("damaged: {0}")]
    Damaged(String),
}

/// Why one page's text couldn't be read; the other pages are unaffected.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("page {page}: {message}")]
pub struct PageError {
    /// The page, from 1.
    pub page: u32,
    /// The engine's own words.
    pub message: String,
}

/// One page's text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageText {
    /// The text, cleaned: control characters but line ends and tabs dropped, line ends normalised, no trailing blanks.
    pub text: String,
    /// Whether the text was cut at [`MAX_PAGE_TEXT_BYTES`].
    pub cut: bool,
}

impl PageText {
    /// Whether the page has no text at all (a picture of a page, or a blank one).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.text.trim().is_empty()
    }
}

/// An open document.
pub struct Document {
    bytes: Vec<u8>,
    inner: PdfDocument<Cursor<Vec<u8>>>,
    pages: u32,
}

impl Document {
    /// Opens `bytes` leniently (damaged cross-reference tables are rebuilt), refusing what isn't a PDF, needs a
    /// password, or has too many pages.
    ///
    /// # Errors
    /// [`OpenError`], as its variants say.
    pub fn open(bytes: Vec<u8>) -> Result<Self, OpenError> {
        if !bytes.starts_with(MAGIC) {
            return Err(OpenError::NotPdf);
        }
        if nested_too_deep(&bytes) {
            return Err(OpenError::Damaged(format!(
                "nested more than {MAX_NESTING} levels deep; not read"
            )));
        }
        let reader =
            PdfReader::new_with_options(Cursor::new(bytes.clone()), ParseOptions::lenient())
                .map_err(open_error)?;
        // The standard security handler opens most encrypted files with the empty user password; one it couldn't open
        // needs a password, and reading on would make nonsense of its objects.
        if reader
            .encryption_handler()
            .is_some_and(|handler| !handler.is_unlocked())
        {
            return Err(OpenError::Password);
        }
        let inner = PdfDocument::new(reader);
        let pages = inner.page_count().map_err(open_error)?;
        if pages > MAX_PAGES {
            return Err(OpenError::TooManyPages);
        }
        Ok(Self {
            bytes,
            inner,
            pages,
        })
    }

    /// How many pages the document has.
    #[must_use]
    pub fn page_count(&self) -> u32 {
        self.pages
    }

    /// The document's bytes, as given.
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// The text of `page` (from 1), cleaned and cut at [`MAX_PAGE_TEXT_BYTES`].
    ///
    /// # Errors
    /// A page past the end, or one the engine can't read; the other pages are unaffected.
    pub fn text(&self, page: u32) -> Result<PageText, PageError> {
        if page == 0 || page > self.pages {
            return Err(PageError {
                page,
                message: format!("the document has {} pages", self.pages),
            });
        }
        let extracted = self
            .inner
            .extract_text_from_page_with_options(page - 1, ExtractionOptions::default())
            .map_err(|error| PageError {
                page,
                message: error.to_string(),
            })?;
        Ok(clean(&extracted.text))
    }
}

/// How deep arrays and dictionaries may nest. Real files nest a few levels; the engine's parser recurses once per level
/// and a hostile file nested 100,000 deep would overflow the stack, which aborts the process the reader runs in.
pub const MAX_NESTING: usize = 256;

/// Whether `bytes` nest arrays or dictionaries deeper than [`MAX_NESTING`], counted over the file's objects with the
/// streams' contents (where binary data lives) left out. A file this deep is refused before the engine parses it.
fn nested_too_deep(bytes: &[u8]) -> bool {
    let mut depth = 0usize;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'[' => depth += 1,
            b']' => depth = depth.saturating_sub(1),
            b'<' if bytes.get(i + 1) == Some(&b'<') => {
                depth += 1;
                i += 1;
            }
            b'>' if bytes.get(i + 1) == Some(&b'>') => {
                depth = depth.saturating_sub(1);
                i += 1;
            }
            // `stream` followed by its line end: skip to `endstream`, or to the end when there is none.
            b's' if bytes[i..].starts_with(b"stream")
                && matches!(bytes.get(i + 6), Some(b'\n' | b'\r')) =>
            {
                i += 6;
                i = find(bytes, i, b"endstream").map_or(bytes.len(), |at| at + 9);
                continue;
            }
            _ => {}
        }
        if depth > MAX_NESTING {
            return true;
        }
        i += 1;
    }
    false
}

/// Where `needle` first occurs in `bytes` at or after `from`.
fn find(bytes: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    bytes
        .get(from..)?
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|at| from + at)
}

/// The engine's open-time failure as the reader's.
fn open_error(error: ParseError) -> OpenError {
    match error {
        ParseError::EncryptionNotSupported | ParseError::WrongPassword | ParseError::PdfLocked => {
            OpenError::Password
        }
        ParseError::InvalidHeader | ParseError::EmptyFile => OpenError::NotPdf,
        other => OpenError::Damaged(other.to_string()),
    }
}

/// `text` as a model reads it: `\r\n` and `\r` become `\n`, control characters other than line ends and tabs go
/// (the engines write odd marks for hyphens and ligature joins), trailing blanks on each line go, runs of blank lines
/// shrink to one, and the whole is cut at [`MAX_PAGE_TEXT_BYTES`] on a character boundary.
#[must_use]
pub fn clean(text: &str) -> PageText {
    let mut out = String::with_capacity(text.len().min(MAX_PAGE_TEXT_BYTES));
    let mut blank_run = 0;
    let mut cut = false;
    for raw in text.replace("\r\n", "\n").replace('\r', "\n").split('\n') {
        let line: String = raw
            .chars()
            .filter(|c| *c == '\t' || !c.is_control())
            .collect();
        let line = line.trim_end();
        if line.is_empty() {
            blank_run += 1;
            if blank_run > 1 || out.is_empty() {
                continue;
            }
        } else {
            blank_run = 0;
        }
        if out.len() + line.len() + 1 > MAX_PAGE_TEXT_BYTES {
            cut = true;
            break;
        }
        out.push_str(line);
        out.push('\n');
    }
    while out.ends_with('\n') {
        out.pop();
    }
    PageText { text: out, cut }
}
