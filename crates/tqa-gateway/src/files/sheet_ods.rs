//! The `.ods` reader for sheet previews. An `OpenDocument` sheet says "this row, a million times" or "this cell, sixteen
//! thousand times across" in one attribute, so a tiny file can stand for billions of cells; a reader that expands
//! them (calamine's does) holds them all. This one counts repeats instead: it builds one row of at most
//! [`COLUMNS`](super::sheet::COLUMNS) cells and copies it only onto the shown rows, so what it holds is bounded by the
//! preview, not the file. Cells show their display text (`text:p`), which the file stores as the spreadsheet
//! formatted it; formulas are never evaluated and annotations are not shown.
use super::sheet::{Budget, CELL_CHARS, COLUMNS, ROWS, SheetPreview, Table, choose, to_index};
use crate::GatewayError;
use quick_xml::{
    Decoder, Reader, XmlVersion,
    events::{BytesRef, BytesStart, Event},
};
use std::io::{BufReader, Cursor, Read};

/// The part holding every sheet.
const CONTENT: &str = "content.xml";
/// The part listing the other parts, which marks encrypted ones.
const MANIFEST: &str = "META-INF/manifest.xml";

/// One sheet of an `.ods` file whose zip [`check_zip`](super::sheet::check_zip) has passed.
pub(super) fn read(
    bytes: &[u8],
    wanted: Option<&str>,
    budget: &mut Budget,
) -> Result<SheetPreview, GatewayError> {
    let mut archive =
        zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| GatewayError::PreviewDamaged)?;
    if let Ok(manifest) = archive.by_name(MANIFEST)
        && encrypted(manifest, budget)?
    {
        return Err(GatewayError::PreviewProtected);
    }
    let content = archive
        .by_name(CONTENT)
        .map_err(|_| GatewayError::PreviewDamaged)?;
    let (sheets, table) = scan(content, wanted, budget)?;
    let (sheets, name) = choose(sheets, wanted)?;
    // The whole sheet was read, so its last row is counted, not declared.
    let last = table.last_row();
    Ok(table.finish(sheets, name, last))
}

/// Whether the manifest marks any part encrypted.
fn encrypted(manifest: impl Read, budget: &mut Budget) -> Result<bool, GatewayError> {
    let mut reader = Reader::from_reader(BufReader::new(manifest));
    let mut buffer = Vec::new();
    loop {
        budget.tick()?;
        buffer.clear();
        match reader.read_event_into(&mut buffer) {
            Ok(Event::Start(element) | Event::Empty(element))
                if element.local_name().as_ref() == b"encryption-data" =>
            {
                return Ok(true);
            }
            Ok(Event::Eof) => return Ok(false),
            Ok(_) => {}
            Err(_) => return Err(GatewayError::PreviewDamaged),
        }
    }
}

/// What an open element is to the reader.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// A sheet (`table:table`); `true` for the shown one.
    Sheet(bool),
    /// A row of the shown sheet.
    Row,
    /// A cell of the shown sheet.
    Cell,
    /// A cell's own paragraph, whose text is the cell's.
    Paragraph,
    /// Passed over with everything inside: an annotation, a frame, a nested table.
    Skip,
    /// Anything else.
    Other,
}

/// The row being read (of the shown sheet only).
struct Row {
    repeat: u64,
    /// The next cell's column.
    column: u64,
    /// The shown cells, clipped.
    cells: Vec<String>,
    /// A non-empty cell lay right of the shown columns.
    wide: bool,
}

/// The cell being read.
struct Cell {
    repeat: u64,
    /// Its display text so far, cut a little past what a cell shows so a long one stays cheap.
    text: String,
    chars: usize,
    paragraphs: u32,
}

/// Every sheet's name, and the chosen sheet's shown table.
fn scan(
    content: impl Read,
    wanted: Option<&str>,
    budget: &mut Budget,
) -> Result<(Vec<String>, Table), GatewayError> {
    let mut reader = Reader::from_reader(BufReader::new(content));
    let mut buffer = Vec::new();
    let mut scanner = Scanner {
        wanted,
        sheets: Vec::new(),
        table: Table::default(),
        stack: Vec::new(),
        shown_taken: false,
        row_index: 0,
        row: None,
        cell: None,
    };
    loop {
        budget.tick()?;
        buffer.clear();
        let event = reader
            .read_event_into(&mut buffer)
            .map_err(|_| GatewayError::PreviewDamaged)?;
        match event {
            Event::Start(ref element) => scanner.open(element, reader.decoder())?,
            Event::Empty(ref element) => {
                scanner.open(element, reader.decoder())?;
                scanner.close();
            }
            Event::End(_) => scanner.close(),
            Event::Text(ref text) if scanner.in_text() => {
                scanner.push(&text.decode().map_err(|_| GatewayError::PreviewDamaged)?);
            }
            Event::GeneralRef(ref reference) if scanner.in_text() => {
                scanner.push(&resolve(reference)?);
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if !scanner.stack.is_empty() {
        return Err(GatewayError::PreviewDamaged);
    }
    Ok((scanner.sheets, scanner.table))
}

/// A reference's text: a character reference or one of XML's predefined entities (a document may not define
/// others here).
fn resolve(reference: &BytesRef<'_>) -> Result<String, GatewayError> {
    if let Some(character) = reference
        .resolve_char_ref()
        .map_err(|_| GatewayError::PreviewDamaged)?
    {
        return Ok(character.to_string());
    }
    let name = reference
        .decode()
        .map_err(|_| GatewayError::PreviewDamaged)?;
    let text = match name.as_ref() {
        "lt" => "<",
        "gt" => ">",
        "amp" => "&",
        "apos" => "'",
        "quot" => "\"",
        _ => return Err(GatewayError::PreviewDamaged),
    };
    Ok(text.to_owned())
}

/// The reader's state through `content.xml`.
struct Scanner<'a> {
    wanted: Option<&'a str>,
    sheets: Vec<String>,
    table: Table,
    stack: Vec<Kind>,
    shown_taken: bool,
    /// The shown sheet's next row index, counting repeats.
    row_index: u64,
    row: Option<Row>,
    cell: Option<Cell>,
}

impl Scanner<'_> {
    /// Inside a cell's own paragraph, outside anything skipped.
    fn in_text(&self) -> bool {
        self.cell.is_some()
            && self.stack.contains(&Kind::Paragraph)
            && self.stack.last() != Some(&Kind::Skip)
    }

    fn open(&mut self, element: &BytesStart<'_>, decoder: Decoder) -> Result<(), GatewayError> {
        let parent = self.stack.last().copied();
        let in_sheet = self.stack.iter().any(|kind| matches!(kind, Kind::Sheet(_)));
        let shown = self.stack.contains(&Kind::Sheet(true));
        let kind = match (element.local_name().as_ref(), parent) {
            (_, Some(Kind::Skip)) => Kind::Skip,
            (b"table", _) if !in_sheet => self.sheet(element, decoder)?,
            (b"table-row", _) if shown && self.row.is_none() => {
                self.row = Some(Row {
                    repeat: repeat(element, b"number-rows-repeated", decoder)?,
                    column: 0,
                    cells: Vec::new(),
                    wide: false,
                });
                Kind::Row
            }
            (b"table-cell" | b"covered-table-cell", Some(Kind::Row)) => {
                self.cell = Some(Cell {
                    repeat: repeat(element, b"number-columns-repeated", decoder)?,
                    text: String::new(),
                    chars: 0,
                    paragraphs: 0,
                });
                Kind::Cell
            }
            (b"p" | b"h", Some(Kind::Cell)) => {
                if self.cell.as_ref().is_some_and(|cell| cell.paragraphs > 0) {
                    self.push("\n");
                }
                if let Some(cell) = self.cell.as_mut() {
                    cell.paragraphs += 1;
                }
                Kind::Paragraph
            }
            (_, Some(Kind::Cell)) | (b"annotation" | b"table", _) if self.cell.is_some() => {
                Kind::Skip
            }
            (b"s", _) if self.in_text() => {
                let count = attribute(element, b"c", decoder)?
                    .and_then(|count| count.parse::<usize>().ok())
                    .unwrap_or(1);
                self.push(&" ".repeat(count.min(CELL_CHARS + 1)));
                Kind::Other
            }
            (b"tab", _) if self.in_text() => {
                self.push("\t");
                Kind::Other
            }
            (b"line-break", _) if self.in_text() => {
                self.push("\n");
                Kind::Other
            }
            _ => Kind::Other,
        };
        self.stack.push(kind);
        Ok(())
    }

    /// A sheet opens: named, and shown when it's the wanted one (or the first).
    fn sheet(&mut self, element: &BytesStart<'_>, decoder: Decoder) -> Result<Kind, GatewayError> {
        let title = attribute(element, b"name", decoder)?.unwrap_or_default();
        let chosen = !self.shown_taken
            && match self.wanted {
                Some(wanted) => title == wanted,
                None => self.sheets.is_empty(),
            };
        self.shown_taken |= chosen;
        self.sheets.push(title);
        Ok(Kind::Sheet(chosen))
    }

    /// Closes the innermost element: a cell joins its row, a row joins the table (counted, not expanded).
    fn close(&mut self) {
        match self.stack.pop() {
            Some(Kind::Cell) => {
                if let Some(cell) = self.cell.take() {
                    self.end_cell(cell);
                }
            }
            Some(Kind::Row) => {
                if let Some(row) = self.row.take() {
                    self.table
                        .put_row(self.row_index, row.repeat, &row.cells, row.wide);
                    self.row_index = self.row_index.saturating_add(row.repeat);
                }
            }
            _ => {}
        }
    }

    /// A cell's text fills its columns of the row, only as far as the shown columns reach.
    fn end_cell(&mut self, cell: Cell) {
        let Some(row) = self.row.as_mut() else { return };
        let start = row.column;
        row.column = start.saturating_add(cell.repeat);
        if cell.text.is_empty() {
            return;
        }
        row.wide |= row.column > COLUMNS;
        let (first, last) = (
            to_index(start.min(COLUMNS)),
            to_index(row.column.min(COLUMNS)),
        );
        if first >= last {
            return;
        }
        // Only shown cells are clipped and counted; a row below them only needs to be known as non-empty.
        let text = if self.row_index < ROWS {
            self.table.clip(&cell.text)
        } else {
            cell.text
        };
        row.cells.resize_with(first, String::new);
        row.cells.resize(last, text);
    }

    /// Adds text to the open cell, keeping only a little more than a cell shows.
    fn push(&mut self, text: &str) {
        let Some(cell) = self.cell.as_mut() else {
            return;
        };
        let room = (CELL_CHARS + 1).saturating_sub(cell.chars);
        for character in text.chars().take(room) {
            cell.text.push(character);
            cell.chars += 1;
        }
    }
}

/// An attribute's value by its local name.
fn attribute(
    element: &BytesStart<'_>,
    name: &[u8],
    decoder: Decoder,
) -> Result<Option<String>, GatewayError> {
    for attribute in element.attributes() {
        let attribute = attribute.map_err(|_| GatewayError::PreviewDamaged)?;
        if attribute.key.local_name().as_ref() == name {
            let value = attribute
                .decoded_and_normalized_value(XmlVersion::Implicit1_0, decoder)
                .map_err(|_| GatewayError::PreviewDamaged)?;
            return Ok(Some(value.into_owned()));
        }
    }
    Ok(None)
}

/// A repeat count: 1 when absent; a value that isn't a positive number is damage.
fn repeat(element: &BytesStart<'_>, name: &[u8], decoder: Decoder) -> Result<u64, GatewayError> {
    match attribute(element, name, decoder)? {
        None => Ok(1),
        Some(value) => value
            .parse::<u64>()
            .ok()
            .filter(|&count| count > 0)
            .ok_or(GatewayError::PreviewDamaged),
    }
}
