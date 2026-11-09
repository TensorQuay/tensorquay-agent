//! `GET /conversations/{id}/files/sheet?path=&sheet=`: a spreadsheet of the task's workspace as a bounded table of
//! display text, for the Files panel's preview. The file is untrusted: formulas show their cached
//! values and are never evaluated, macros and external links are ignored, and no cell is ever markup or a link.
//!
//! The bounds hold whatever the file claims. The file is at most `files.preview_sheet_bytes`; its zip directory is
//! checked and every entry inflated once against [`EXPANDED_BYTES`] before any parsing; parsing stops at
//! [`DEADLINE`]; and only the first [`ROWS`] rows and [`COLUMNS`] columns of one sheet are ever held. `.xlsx`,
//! `.xlsm` and `.xlsb` stream their cells through calamine and stop at the last shown row; `.ods` goes through our
//! own reader ([`super::sheet_ods`]), which counts repeated rows and cells instead of expanding them.
use super::endpoints::{Tree, directory_download, tree};
use super::linked;
use super::path::WorkspacePath;
use super::sheet_ods;
use crate::{GatewayError, app::State, auth::Authz};
use axum::{
    Json,
    body::Body,
    extract::{Path as UrlPath, Query, State as AppState},
};
use calamine::{DataRef, Reader, Xlsb, Xlsx, XlsxError};
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::{
    io::{Cursor, Read},
    sync::Arc,
    time::{Duration, Instant},
};
use tqa_types::ConversationId;

#[cfg(test)]
#[path = "sheet_tests.rs"]
mod tests;

/// The rows a preview shows.
pub(super) const ROWS: u64 = 500;
/// The columns a preview shows.
pub(super) const COLUMNS: u64 = 50;
/// The characters a cell shows; a longer cell ends in `…`.
pub(super) const CELL_CHARS: usize = 200;
/// The most the file's zip entries may inflate to, together.
pub(super) const EXPANDED_BYTES: u64 = 100 * 1024 * 1024;
/// The most entries the file's zip may hold.
pub(super) const ENTRIES: usize = 10_000;
/// How long parsing one file may take.
pub(super) const DEADLINE: Duration = Duration::from_secs(5);
/// Parses in flight at once: each may hold up to [`EXPANDED_BYTES`] of a file's parts.
static PARSES: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SheetQuery {
    path: String,
    /// The sheet to show; the first when absent.
    sheet: Option<String>,
}

/// The table the panel shows.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub(crate) struct SheetPreview {
    /// Every sheet's name, in the workbook's order.
    pub(super) sheets: Vec<String>,
    /// The shown sheet's name.
    pub(super) sheet: String,
    /// At most [`ROWS`] rows of at most [`COLUMNS`] cells, each display text; every row is as wide as the widest.
    pub(super) rows: Vec<Vec<String>>,
    /// The sheet's rows up to its last non-empty one, when known: counted when the whole sheet was read, else as the
    /// file declares it.
    pub(super) total_rows: Option<u64>,
    /// A non-empty cell lies below the shown rows.
    pub(super) more_rows: bool,
    /// A non-empty cell lies right of the shown columns.
    pub(super) more_columns: bool,
    /// Shown cells cut at [`CELL_CHARS`].
    pub(super) truncated_cells: u32,
}

/// The formats a preview reads, by extension.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Format {
    Xlsx,
    Xlsb,
    Ods,
}

impl Format {
    /// The file's format, or none for any other type (legacy `.xls` included).
    pub(super) fn of(path: &WorkspacePath) -> Option<Self> {
        let (_, name) = path.split_last()?;
        let (stem, extension) = name.rsplit_once('.')?;
        if stem.is_empty() {
            return None;
        }
        match extension.to_ascii_lowercase().as_str() {
            "xlsx" | "xlsm" => Some(Self::Xlsx),
            "xlsb" => Some(Self::Xlsb),
            "ods" => Some(Self::Ods),
            _ => None,
        }
    }
}

/// `GET /conversations/{id}/files/sheet`: one sheet of a spreadsheet of the task's workspace, as display text.
///
/// # Errors
/// `415 preview_unsupported` for any other file type; `413 body_too_large` over `files.preview_sheet_bytes`;
/// `413 preview_too_large` when the file inflates past the preview's bounds or takes too long; `422
/// preview_damaged` or `422 preview_protected`; `404 not_found` for a sheet the file doesn't have; and as the file
/// download otherwise (access, `path_invalid`, a missing file).
pub(crate) async fn sheet(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
    UrlPath(id): UrlPath<ConversationId>,
    Query(query): Query<SheetQuery>,
) -> Result<Json<SheetPreview>, GatewayError> {
    let target = WorkspacePath::parse(&query.path)?;
    let format = Format::of(&target).ok_or(GatewayError::PreviewUnsupported)?;
    let files = &state.config.files;
    let limit = files.preview(files.preview_sheet_bytes);
    let body = match tree(&state, &user, &id, (true, false)).await? {
        Tree::Directory(root) => directory_download(root, target, limit).await?.1,
        Tree::Runtime(link) => {
            let (_, stream) = linked::download(state.clone(), link, &target, limit).await?;
            Body::from_stream(stream.map(|chunk| chunk.map(axum::body::Bytes::from)))
        }
    };
    let bytes = axum::body::to_bytes(body, usize::try_from(limit).unwrap_or(usize::MAX))
        .await
        .map_err(|_| GatewayError::BodyTooLarge)?;
    let _permit = PARSES.acquire().await.map_err(|_| GatewayError::Io)?;
    let preview = super::endpoints::blocking(move || {
        read(
            format,
            &bytes,
            query.sheet.as_deref(),
            Instant::now() + DEADLINE,
        )
    })
    .await?;
    Ok(Json(preview))
}

/// One sheet of the file, within the preview's bounds.
pub(super) fn read(
    format: Format,
    bytes: &[u8],
    sheet: Option<&str>,
    until: Instant,
) -> Result<SheetPreview, GatewayError> {
    let mut budget = Budget::new(until);
    if bytes.starts_with(&CFB) {
        return Err(sealed(bytes));
    }
    check_zip(bytes, &mut budget)?;
    match format {
        Format::Xlsx => {
            let mut book = Xlsx::new(Cursor::new(bytes)).map_err(|error| xlsx_error(&error))?;
            let (sheets, name) = choose(book.sheet_names(), sheet)?;
            let mut table = Table::default();
            let declared = match book.worksheet_cells_reader(&name) {
                Ok(mut cells) => {
                    let declared = cells.dimensions().end.0;
                    fill(&mut table, &mut budget, || {
                        cells.next_cell().map_err(|error| xlsx_error(&error))
                    })?;
                    Some(declared.into())
                }
                // A chart sheet has no cells.
                Err(XlsxError::NotAWorksheet(_)) => None,
                Err(error) => return Err(xlsx_error(&error)),
            };
            Ok(table.finish(sheets, name, declared))
        }
        Format::Xlsb => {
            let mut book =
                Xlsb::new(Cursor::new(bytes)).map_err(|_| GatewayError::PreviewDamaged)?;
            let (sheets, name) = choose(book.sheet_names(), sheet)?;
            let mut table = Table::default();
            let mut cells = book
                .worksheet_cells_reader(&name)
                .map_err(|_| GatewayError::PreviewDamaged)?;
            let declared = cells.dimensions().end.0;
            fill(&mut table, &mut budget, || {
                cells.next_cell().map_err(|_| GatewayError::PreviewDamaged)
            })?;
            Ok(table.finish(sheets, name, Some(declared.into())))
        }
        Format::Ods => sheet_ods::read(bytes, sheet, &mut budget),
    }
}

/// Streams cells in row order into the table until they pass the shown rows.
fn fill<'a>(
    table: &mut Table,
    budget: &mut Budget,
    mut next: impl FnMut() -> Result<Option<calamine::Cell<DataRef<'a>>>, GatewayError>,
) -> Result<(), GatewayError> {
    while let Some(cell) = next()? {
        budget.tick()?;
        let (row, column) = cell.get_position();
        if !table.put(row.into(), column.into(), &display(cell.get_value())) {
            break;
        }
    }
    Ok(())
}

/// The sheet names and the chosen one: the named sheet, or the first.
pub(super) fn choose(
    sheets: Vec<String>,
    wanted: Option<&str>,
) -> Result<(Vec<String>, String), GatewayError> {
    let name = match wanted {
        Some(wanted) => sheets.iter().find(|name| *name == wanted),
        None => sheets.first(),
    }
    .cloned()
    .ok_or(GatewayError::NotFound)?;
    Ok((sheets, name))
}

/// The start of a compound file: how an encrypted Office file is stored (or a legacy `.xls` renamed).
const CFB: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];

/// A compound file named as a zip spreadsheet: protected when it holds an encrypted package, else damaged.
fn sealed(bytes: &[u8]) -> GatewayError {
    match Xlsx::new(Cursor::new(bytes)) {
        Err(XlsxError::Password) => GatewayError::PreviewProtected,
        _ => GatewayError::PreviewDamaged,
    }
}

fn xlsx_error(error: &XlsxError) -> GatewayError {
    match error {
        XlsxError::Password => GatewayError::PreviewProtected,
        _ => GatewayError::PreviewDamaged,
    }
}

/// Checks the zip's directory, then inflates every entry once, counting: an entry's declared size can lie, so the
/// real sizes are what's bounded. Afterwards every entry is known to inflate to its declared size, together at most
/// [`EXPANDED_BYTES`].
pub(super) fn check_zip(bytes: &[u8], budget: &mut Budget) -> Result<(), GatewayError> {
    let mut archive =
        zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| GatewayError::PreviewDamaged)?;
    if archive.len() > ENTRIES {
        return Err(GatewayError::PreviewTooLarge);
    }
    let mut total = 0_u64;
    let mut buffer = vec![0; 64 * 1024];
    for index in 0..archive.len() {
        let entry = archive.by_index(index).map_err(|error| match error {
            zip::result::ZipError::UnsupportedArchive(reason)
                if reason == zip::result::ZipError::PASSWORD_REQUIRED =>
            {
                GatewayError::PreviewProtected
            }
            _ => GatewayError::PreviewDamaged,
        })?;
        let declared = entry.size();
        if declared > EXPANDED_BYTES - total {
            return Err(GatewayError::PreviewTooLarge);
        }
        let mut inflated = entry.take(declared.saturating_add(1));
        let mut actual = 0_u64;
        loop {
            budget.check()?;
            match inflated.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => actual += read as u64,
                Err(_) => return Err(GatewayError::PreviewDamaged),
            }
        }
        if actual != declared {
            return Err(GatewayError::PreviewDamaged);
        }
        total += actual;
    }
    Ok(())
}

/// The parse deadline, checked every so often in the loops that could run long.
pub(super) struct Budget {
    until: Instant,
    ticks: u32,
}

impl Budget {
    pub(super) fn new(until: Instant) -> Self {
        Self { until, ticks: 0 }
    }

    /// Past the deadline is too large to preview.
    pub(super) fn check(&self) -> Result<(), GatewayError> {
        if Instant::now() > self.until {
            return Err(GatewayError::PreviewTooLarge);
        }
        Ok(())
    }

    /// As [`Self::check`], once every 1024 calls.
    pub(super) fn tick(&mut self) -> Result<(), GatewayError> {
        self.ticks = self.ticks.wrapping_add(1);
        if self.ticks.is_multiple_of(1024) {
            self.check()?;
        }
        Ok(())
    }
}

/// The shown part of one sheet, filled cell by cell (xlsx) or row by row (ods).
#[derive(Default)]
pub(super) struct Table {
    rows: Vec<Vec<String>>,
    width: usize,
    more_rows: bool,
    more_columns: bool,
    truncated: u32,
    /// The last row holding a non-empty cell, shown or not.
    last_row: Option<u64>,
}

impl Table {
    /// Records one cell's text; empty text records nothing. False once the cell lies below the shown rows, so a
    /// reader in row order can stop.
    pub(super) fn put(&mut self, row: u64, column: u64, text: &str) -> bool {
        if text.is_empty() {
            return row < ROWS;
        }
        self.last_row = Some(self.last_row.map_or(row, |last| last.max(row)));
        if row >= ROWS {
            self.more_rows = true;
            return false;
        }
        if column >= COLUMNS {
            self.more_columns = true;
            return true;
        }
        let text = self.clip(text);
        let (row, column) = (to_index(row), to_index(column));
        if self.rows.len() <= row {
            self.rows.resize_with(row + 1, Vec::new);
        }
        let cells = &mut self.rows[row];
        if cells.len() <= column {
            cells.resize_with(column + 1, String::new);
        }
        cells[column] = text;
        self.width = self.width.max(column + 1);
        true
    }

    /// Records a row `repeat` times from `row`: its shown cells (already clipped, at most [`COLUMNS`]) and whether a
    /// non-empty cell lay right of them.
    pub(super) fn put_row(&mut self, row: u64, repeat: u64, cells: &[String], wide: bool) {
        let width = cells
            .iter()
            .rposition(|cell| !cell.is_empty())
            .map(|last| last + 1);
        if width.is_none() && !wide {
            return;
        }
        let last = row.saturating_add(repeat.max(1) - 1);
        self.last_row = Some(self.last_row.map_or(last, |known| known.max(last)));
        self.more_columns |= wide;
        if last >= ROWS {
            self.more_rows = true;
        }
        let Some(width) = width else { return };
        let cells = &cells[..width];
        let end = to_index(last.min(ROWS - 1));
        let start = to_index(row.min(ROWS));
        if start > end {
            return;
        }
        if self.rows.len() <= end {
            self.rows.resize_with(end + 1, Vec::new);
        }
        // A repeated row lands on rows already filled only in a malformed file; the later row wins.
        for shown in &mut self.rows[start..=end] {
            shown.clear();
            shown.extend_from_slice(cells);
        }
        self.width = self.width.max(width);
    }

    /// The last row holding a non-empty cell, shown or not.
    pub(super) fn last_row(&self) -> Option<u64> {
        self.last_row
    }

    /// `text` cut at [`CELL_CHARS`], counted.
    pub(super) fn clip(&mut self, text: &str) -> String {
        match text.char_indices().nth(CELL_CHARS) {
            Some((end, _)) => {
                self.truncated += 1;
                format!("{}…", &text[..end])
            }
            None => text.to_owned(),
        }
    }

    /// The preview: every row padded to the widest, and the row count as read or, when the reading stopped early,
    /// as known otherwise (`known_last`, the last row's index: declared by the file, or counted without being
    /// shown), if larger than what was shown.
    pub(super) fn finish(
        mut self,
        sheets: Vec<String>,
        sheet: String,
        known_last: Option<u64>,
    ) -> SheetPreview {
        let total_rows = if self.more_rows {
            known_last.filter(|&last| last >= ROWS).map(|last| last + 1)
        } else {
            self.last_row.map_or(Some(0), |last| Some(last + 1))
        };
        self.rows.truncate(
            self.last_row
                .map_or(0, |last| to_index(last.min(ROWS - 1)) + 1),
        );
        for row in &mut self.rows {
            row.resize_with(self.width, String::new);
        }
        SheetPreview {
            sheets,
            sheet,
            rows: self.rows,
            total_rows,
            more_rows: self.more_rows,
            more_columns: self.more_columns,
            truncated_cells: self.truncated,
        }
    }
}

/// An index within the shown bounds (always below [`ROWS`] or [`COLUMNS`]).
pub(super) fn to_index(value: u64) -> usize {
    usize::try_from(value).unwrap_or(usize::MAX)
}

/// A cell's display text: the cached value, never a formula.
pub(super) fn display(value: &DataRef<'_>) -> String {
    match value {
        DataRef::Int(value) => value.to_string(),
        DataRef::Float(value) => number(*value),
        DataRef::String(text) | DataRef::DateTimeIso(text) | DataRef::DurationIso(text) => {
            text.clone()
        }
        DataRef::SharedString(text) => (*text).to_owned(),
        DataRef::Bool(value) => if *value { "TRUE" } else { "FALSE" }.to_owned(),
        DataRef::DateTime(value) if value.is_duration() => duration(value.as_f64()),
        DataRef::DateTime(value) => {
            let (year, month, day, hour, minute, second, _) = value.to_ymd_hms_milli();
            if (hour, minute, second) == (0, 0, 0) {
                format!("{year:04}-{month:02}-{day:02}")
            } else {
                format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}")
            }
        }
        DataRef::Error(error) => error.to_string(),
        DataRef::Empty => String::new(),
    }
}

/// A number as a spreadsheet shows it by default: at most 15 significant digits, so binary noise
/// (`0.30000000000000004`) is gone.
pub(super) fn number(value: f64) -> String {
    if !value.is_finite() {
        return value.to_string();
    }
    format!("{value:.14e}")
        .parse::<f64>()
        .map_or_else(|_| value.to_string(), |rounded| rounded.to_string())
}

/// A duration in days as `h:mm:ss`, hours unbounded.
fn duration(days: f64) -> String {
    let seconds = (days * 86_400.0).round();
    let sign = if seconds < 0.0 { "-" } else { "" };
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "bounded by the f64's range; a spreadsheet's durations are small"
    )]
    let seconds = seconds.abs() as u64;
    format!(
        "{sign}{}:{:02}:{:02}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    )
}
