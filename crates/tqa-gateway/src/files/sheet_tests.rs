//! GW-U-088: `/files/sheet` reads a bounded table of display text, whatever the file claims.
//! GW-U-207: an `.xlsx` boolean cell is read as Excel's schema allows, and any other value is a damaged sheet.
//! GW-U-208: an `.ods` in an ASCII-compatible encoding other than UTF-8 previews its text in that encoding.
use super::{EXPANDED_BYTES, Format, ROWS, SheetPreview, number, read};
use crate::GatewayError;
use crate::files::path::WorkspacePath;
use std::fmt::Write as _;
use std::io::{Cursor, Write};
use std::time::{Duration, Instant};

type Outcome<T = ()> = Result<T, Box<dyn std::error::Error>>;

fn soon() -> Instant {
    Instant::now() + Duration::from_mins(1)
}

/// A zip of `(name, content)` parts, deflated.
fn zip(parts: &[(&str, &[u8])]) -> Outcome<Vec<u8>> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, content) in parts {
        writer.start_file(*name, options)?;
        writer.write_all(content)?;
    }
    Ok(writer.finish()?.into_inner())
}

const MAIN: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const RELATIONSHIPS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

/// A minimal `.xlsx`: each sheet's `<worksheet>` inner XML, shared strings, and a style 1 that is a date.
fn xlsx(sheets: &[(&str, String)], shared: &[&str]) -> Outcome<Vec<u8>> {
    let mut workbook = format!(r#"<workbook xmlns="{MAIN}" xmlns:r="{RELATIONSHIPS}"><sheets>"#);
    let mut rels = String::from(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
    );
    let mut parts: Vec<(String, Vec<u8>)> = Vec::new();
    for (index, (name, body)) in sheets.iter().enumerate() {
        let number = index + 1;
        write!(
            workbook,
            r#"<sheet name="{name}" sheetId="{number}" r:id="rId{number}"/>"#
        )?;
        write!(
            rels,
            r#"<Relationship Id="rId{number}" Type="{RELATIONSHIPS}/worksheet" Target="worksheets/sheet{number}.xml"/>"#
        )?;
        parts.push((
            format!("xl/worksheets/sheet{number}.xml"),
            format!(r#"<worksheet xmlns="{MAIN}">{body}</worksheet>"#).into_bytes(),
        ));
    }
    workbook.push_str("</sheets></workbook>");
    write!(
        rels,
        r#"<Relationship Id="rIdS" Type="{RELATIONSHIPS}/sharedStrings" Target="sharedStrings.xml"/><Relationship Id="rIdY" Type="{RELATIONSHIPS}/styles" Target="styles.xml"/></Relationships>"#
    )?;
    let mut strings = String::new();
    for text in shared {
        write!(strings, "<si><t>{text}</t></si>")?;
    }
    parts.push(("xl/workbook.xml".into(), workbook.into_bytes()));
    parts.push(("xl/_rels/workbook.xml.rels".into(), rels.into_bytes()));
    parts.push((
        "xl/sharedStrings.xml".into(),
        format!(r#"<sst xmlns="{MAIN}">{strings}</sst>"#).into_bytes(),
    ));
    parts.push((
        "xl/styles.xml".into(),
        format!(r#"<styleSheet xmlns="{MAIN}"><cellXfs count="2"><xf numFmtId="0"/><xf numFmtId="14"/></cellXfs></styleSheet>"#)
            .into_bytes(),
    ));
    parts.push((
        "_rels/.rels".into(),
        format!(r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="{RELATIONSHIPS}/officeDocument" Target="xl/workbook.xml"/></Relationships>"#)
            .into_bytes(),
    ));
    let borrowed: Vec<(&str, &[u8])> = parts
        .iter()
        .map(|(name, content)| (name.as_str(), content.as_slice()))
        .collect();
    zip(&borrowed)
}

/// A minimal `.ods` of `content.xml`'s sheets, with a manifest (`extra` goes inside it).
fn ods(tables: &str, manifest_extra: &str) -> Outcome<Vec<u8>> {
    let content = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0"><office:body><office:spreadsheet>{tables}</office:spreadsheet></office:body></office:document-content>"#
    );
    let manifest = format!(
        r#"<manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0"><manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml">{manifest_extra}</manifest:file-entry></manifest:manifest>"#
    );
    zip(&[
        (
            "mimetype",
            b"application/vnd.oasis.opendocument.spreadsheet",
        ),
        ("META-INF/manifest.xml", manifest.as_bytes()),
        ("content.xml", content.as_bytes()),
    ])
}

fn table(rows: &[&[&str]]) -> Vec<Vec<String>> {
    rows.iter()
        .map(|row| row.iter().map(|&cell| cell.to_owned()).collect())
        .collect()
}

fn sheet_of(
    format: Format,
    bytes: &[u8],
    sheet: Option<&str>,
) -> Result<SheetPreview, GatewayError> {
    read(format, bytes, sheet, soon())
}

// TEST: GW-U-088
#[test]
fn gw_u_088_only_spreadsheet_types_have_a_sheet_view() -> Outcome {
    for (path, format) in [
        ("a.xlsx", Some(Format::Xlsx)),
        ("d/REPORT.XLSM", Some(Format::Xlsx)),
        ("a.xlsb", Some(Format::Xlsb)),
        ("a.ods", Some(Format::Ods)),
        ("a.xls", None),
        ("a.csv", None),
        ("a.xlsx.txt", None),
        (".xlsx", None),
        ("xlsx", None),
    ] {
        let parsed = WorkspacePath::parse(path)?;
        assert_eq!(Format::of(&parsed), format, "{path}");
    }
    Ok(())
}

// TEST: GW-U-088
#[test]
fn gw_u_088_xlsx_cells_are_cached_values_as_plain_text() -> Outcome {
    let body = concat!(
        r#"<sheetData><row r="1">"#,
        r#"<c r="A1" t="s"><v>0</v></c>"#,
        r#"<c r="B1" t="inlineStr"><is><t>&lt;b&gt;bold&lt;/b&gt;</t></is></c>"#,
        r#"<c r="C1"><v>0.30000000000000004</v></c>"#,
        r#"<c r="D1" t="b"><v>1</v></c>"#,
        r#"<c r="E1" t="e"><v>#DIV/0!</v></c>"#,
        r#"<c r="F1" s="1"><v>45000</v></c>"#,
        r#"</row><row r="3">"#,
        r#"<c r="A3"><f>1+2</f><v>3</v></c>"#,
        r#"<c r="B3"><f>SUM(A1:A2)</f></c>"#,
        r#"<c r="C3" t="str"><f>HYPERLINK("https://example.invalid","click")</f><v>click</v></c>"#,
        r#"</row></sheetData>"#,
    );
    let preview = sheet_of(
        Format::Xlsx,
        &xlsx(&[("Data", body.into())], &["shared"])?,
        None,
    )?;
    assert_eq!(
        preview,
        SheetPreview {
            sheets: vec!["Data".into()],
            sheet: "Data".into(),
            rows: table(&[
                &[
                    "shared",
                    "<b>bold</b>",
                    "0.3",
                    "TRUE",
                    "#DIV/0!",
                    "2023-03-15"
                ],
                &["", "", "", "", "", ""],
                &["3", "", "click", "", "", ""],
            ]),
            total_rows: Some(3),
            more_rows: false,
            more_columns: false,
            truncated_cells: 0,
        }
    );
    Ok(())
}

// TEST: GW-U-088
#[test]
fn gw_u_088_xlsx_sheets_by_name_and_a_missing_one() -> Outcome {
    let bytes = xlsx(
        &[
            (
                "One",
                r#"<sheetData><row r="1"><c r="A1"><v>1</v></c></row></sheetData>"#.into(),
            ),
            (
                "Two",
                r#"<sheetData><row r="1"><c r="B1"><v>2</v></c></row></sheetData>"#.into(),
            ),
        ],
        &[],
    )?;
    let first = sheet_of(Format::Xlsx, &bytes, None)?;
    assert_eq!(
        (first.sheet.as_str(), first.rows),
        ("One", table(&[&["1"]]))
    );
    let second = sheet_of(Format::Xlsx, &bytes, Some("Two"))?;
    assert_eq!(second.sheets, ["One", "Two"]);
    assert_eq!(
        (second.sheet.as_str(), second.rows),
        ("Two", table(&[&["", "2"]]))
    );
    assert!(matches!(
        sheet_of(Format::Xlsx, &bytes, Some("Three")),
        Err(GatewayError::NotFound)
    ));
    Ok(())
}

fn column_name(mut index: u32) -> Outcome<String> {
    let mut name = String::new();
    loop {
        name.insert(0, char::from(b'A' + u8::try_from(index % 26)?));
        if index < 26 {
            return Ok(name);
        }
        index = index / 26 - 1;
    }
}

/// A sheet of `rows` by `columns` numbers, with the dimension declared or not, and one long cell at A1.
fn grid(rows: u32, columns: u32, declared: bool) -> Outcome<String> {
    let mut body = String::new();
    if declared {
        write!(
            body,
            r#"<dimension ref="A1:{}{rows}"/>"#,
            column_name(columns - 1)?
        )?;
    }
    body.push_str("<sheetData>");
    for row in 1..=rows {
        write!(body, r#"<row r="{row}">"#)?;
        for column in 0..columns {
            let reference = format!("{}{row}", column_name(column)?);
            if (row, column) == (1, 0) {
                write!(
                    body,
                    r#"<c r="{reference}" t="inlineStr"><is><t>{}</t></is></c>"#,
                    "é".repeat(300)
                )?;
            } else {
                write!(body, r#"<c r="{reference}"><v>{row}</v></c>"#)?;
            }
        }
        body.push_str("</row>");
    }
    body.push_str("</sheetData>");
    Ok(body)
}

// TEST: GW-U-088
#[test]
fn gw_u_088_xlsx_shows_500_rows_50_columns_and_200_characters() -> Outcome {
    let preview = sheet_of(
        Format::Xlsx,
        &xlsx(&[("Big", grid(600, 60, true)?)], &[])?,
        None,
    )?;
    assert_eq!(preview.rows.len(), 500);
    assert!(preview.rows.iter().all(|row| row.len() == 50));
    assert_eq!(preview.rows[499][1], "500");
    assert_eq!(preview.rows[0][0], format!("{}…", "é".repeat(200)));
    assert_eq!(
        (
            preview.total_rows,
            preview.more_rows,
            preview.more_columns,
            preview.truncated_cells
        ),
        (Some(600), true, true, 1)
    );
    let undeclared = sheet_of(
        Format::Xlsx,
        &xlsx(&[("Big", grid(600, 2, false)?)], &[])?,
        None,
    )?;
    assert_eq!(
        (
            undeclared.total_rows,
            undeclared.more_rows,
            undeclared.more_columns
        ),
        (None, true, false)
    );
    Ok(())
}

// TEST: GW-U-088
#[test]
fn gw_u_088_a_tiny_xlsx_of_a_million_rows_shows_its_first_500_at_once() -> Outcome {
    let mut body = String::from("<sheetData>");
    // Rows and cells may leave out their references; a repeated row compresses to almost nothing.
    body.push_str(&"<row><c><v>7</v></c></row>".repeat(1_000_000));
    body.push_str("</sheetData>");
    let bytes = xlsx(&[("Rows", body)], &[])?;
    assert!(
        bytes.len() < 1024 * 1024,
        "the file is tiny: {}",
        bytes.len()
    );
    let started = Instant::now();
    let preview = sheet_of(Format::Xlsx, &bytes, None)?;
    assert_eq!(
        (preview.rows.len(), preview.more_rows, preview.total_rows),
        (500, true, None)
    );
    assert!(
        started.elapsed() < super::DEADLINE,
        "{:?}",
        started.elapsed()
    );
    Ok(())
}

// TEST: GW-U-088
#[test]
fn gw_u_088_ods_cells_are_their_display_text() -> Outcome {
    let tables = concat!(
        r#"<table:table table:name="Summary">"#,
        r#"<table:table-row>"#,
        r#"<table:table-cell office:value-type="float" office:value="0.5"><text:p>50%</text:p></table:table-cell>"#,
        r#"<table:table-cell office:value-type="string"><text:p>a<text:s text:c="3"/>b &amp; c</text:p><text:p>line two</text:p></table:table-cell>"#,
        r#"<table:table-cell table:formula="of:=1+2" office:value-type="float" office:value="3"><office:annotation><text:p>a note</text:p></office:annotation><text:p>3</text:p></table:table-cell>"#,
        r#"<table:covered-table-cell/>"#,
        r#"<table:table-cell table:number-columns-repeated="2"><text:p>x</text:p></table:table-cell>"#,
        r#"<table:table-cell table:number-columns-repeated="1000"/>"#,
        r#"</table:table-row>"#,
        r#"<table:table-row table:number-rows-repeated="2"><table:table-cell><text:p>&lt;i&gt;twice</text:p></table:table-cell></table:table-row>"#,
        r#"<table:table-row table:number-rows-repeated="1048570"><table:table-cell table:number-columns-repeated="1024"/></table:table-row>"#,
        r#"</table:table>"#,
        r#"<table:table table:name="Other"><table:table-row><table:table-cell><text:p>o</text:p></table:table-cell></table:table-row></table:table>"#,
    );
    let bytes = ods(tables, "")?;
    let preview = sheet_of(Format::Ods, &bytes, None)?;
    assert_eq!(
        preview,
        SheetPreview {
            sheets: vec!["Summary".into(), "Other".into()],
            sheet: "Summary".into(),
            rows: table(&[
                &["50%", "a   b & c\nline two", "3", "", "x", "x"],
                &["<i>twice", "", "", "", "", ""],
                &["<i>twice", "", "", "", "", ""],
            ]),
            total_rows: Some(3),
            more_rows: false,
            more_columns: false,
            truncated_cells: 0,
        }
    );
    let other = sheet_of(Format::Ods, &bytes, Some("Other"))?;
    assert_eq!(
        (other.sheet.as_str(), other.rows),
        ("Other", table(&[&["o"]]))
    );
    assert!(matches!(
        sheet_of(Format::Ods, &bytes, Some("Gone")),
        Err(GatewayError::NotFound)
    ));
    Ok(())
}

// TEST: GW-U-088
#[test]
fn gw_u_088_a_tiny_ods_of_billions_of_cells_is_counted_not_expanded() -> Outcome {
    let long = "z".repeat(300);
    let tables = format!(
        r#"<table:table table:name="Bomb"><table:table-row table:number-rows-repeated="1000000"><table:table-cell table:number-columns-repeated="16384"><text:p>{long}</text:p></table:table-cell></table:table-row></table:table>"#
    );
    let bytes = ods(&tables, "")?;
    assert!(bytes.len() < 4096, "the file is tiny: {}", bytes.len());
    let started = Instant::now();
    let preview = sheet_of(Format::Ods, &bytes, None)?;
    // Expanding the cells would run into the parse deadline (an error); counting them takes milliseconds. The deadline
    // is the bound, not a loaded host's second (1 s was).
    assert!(
        started.elapsed() < super::DEADLINE,
        "{:?}",
        started.elapsed()
    );
    assert_eq!(preview.rows.len(), usize::try_from(ROWS)?);
    assert!(preview.rows.iter().all(|row| row.len() == 50));
    assert_eq!(preview.rows[0][0], format!("{}…", "z".repeat(200)));
    assert_eq!(
        (
            preview.total_rows,
            preview.more_rows,
            preview.more_columns,
            preview.truncated_cells
        ),
        (Some(1_000_000), true, true, 1)
    );
    Ok(())
}

// TEST: GW-U-088
#[test]
fn gw_u_088_a_zip_that_inflates_past_the_bound_is_too_large() -> Outcome {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    writer.start_file(
        "xl/worksheets/sheet1.xml",
        zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated),
    )?;
    let chunk = vec![b' '; 1024 * 1024];
    for _ in 0..=EXPANDED_BYTES / (1024 * 1024) {
        writer.write_all(&chunk)?;
    }
    let bytes = writer.finish()?.into_inner();
    assert!(
        bytes.len() < 1024 * 1024,
        "the file is small: {}",
        bytes.len()
    );
    for format in [Format::Xlsx, Format::Xlsb, Format::Ods] {
        assert!(
            matches!(
                sheet_of(format, &bytes, None),
                Err(GatewayError::PreviewTooLarge)
            ),
            "{format:?}"
        );
    }
    let names: Vec<String> = (0..=super::ENTRIES)
        .map(|index| format!("p{index}"))
        .collect();
    let many: Vec<(&str, &[u8])> = names.iter().map(|name| (name.as_str(), &b""[..])).collect();
    assert!(matches!(
        sheet_of(Format::Xlsx, &zip(&many)?, None),
        Err(GatewayError::PreviewTooLarge)
    ));
    Ok(())
}

/// Offsets of every zip header with `signature` in `bytes`.
fn headers(bytes: &[u8], signature: [u8; 4]) -> Vec<usize> {
    bytes
        .windows(4)
        .enumerate()
        .filter(|(_, window)| *window == signature)
        .map(|(at, _)| at)
        .collect()
}

// TEST: GW-U-088
#[test]
fn gw_u_088_an_entry_that_lies_about_its_size_is_damaged() -> Outcome {
    let mut bytes = ods(r#"<table:table table:name="S"/>"#, "")?;
    // Every part claims 1 byte: the local headers (size at 22) and the central directory (size at 24).
    for at in headers(&bytes, [0x50, 0x4B, 0x03, 0x04]) {
        bytes[at + 22..at + 26].copy_from_slice(&1_u32.to_le_bytes());
    }
    for at in headers(&bytes, [0x50, 0x4B, 0x01, 0x02]) {
        bytes[at + 24..at + 28].copy_from_slice(&1_u32.to_le_bytes());
    }
    assert!(matches!(
        sheet_of(Format::Ods, &bytes, None),
        Err(GatewayError::PreviewDamaged)
    ));
    Ok(())
}

// TEST: GW-U-088
#[test]
fn gw_u_088_damaged_files_and_other_files_named_as_spreadsheets() -> Outcome {
    let good = xlsx(&[("S", "<sheetData/>".into())], &[])?;
    let cases: Vec<(&str, Format, Vec<u8>)> = vec![
        ("not a zip", Format::Xlsx, b"hello, I am text".to_vec()),
        ("cut in half", Format::Xlsx, good[..good.len() / 2].to_vec()),
        (
            "a zip with no workbook",
            Format::Xlsx,
            zip(&[("readme.txt", b"hi")])?,
        ),
        (
            "a zip with no workbook, as xlsb",
            Format::Xlsb,
            zip(&[("readme.txt", b"hi")])?,
        ),
        (
            "an ods with no content",
            Format::Ods,
            zip(&[("mimetype", b"x")])?,
        ),
        (
            "an ods whose XML doesn't close",
            Format::Ods,
            ods(r#"<table:table table:name="S"><table:table-row>"#, "")?,
        ),
        (
            "an ods with an unknown entity",
            Format::Ods,
            ods(
                r#"<table:table table:name="S"><table:table-row><table:table-cell><text:p>&boom;</text:p></table:table-cell></table:table-row></table:table>"#,
                "",
            )?,
        ),
        (
            "an ods with a bad repeat",
            Format::Ods,
            ods(
                r#"<table:table table:name="S"><table:table-row table:number-rows-repeated="-1"/></table:table>"#,
                "",
            )?,
        ),
        (
            "a compound file with no encrypted package",
            Format::Xlsx,
            compound("Workbook")?,
        ),
    ];
    for (case, format, bytes) in cases {
        assert!(
            matches!(
                sheet_of(format, &bytes, None),
                Err(GatewayError::PreviewDamaged)
            ),
            "{case}"
        );
    }
    Ok(())
}

// TEST: GW-U-207
/// An `.xlsx` boolean cell holds `0`, `1`, `false` or `true` (any case, spaces trimmed), and a blank or space-only one
/// is an empty cell; any other value makes the sheet damaged, never TRUE. calamine 0.37.0 reads booleans so; 0.36.1
/// showed every value but `0` as TRUE, `false` and a space included. (`.xlsb` stores booleans as bits: unchanged.)
#[test]
fn gw_u_207_an_xlsx_boolean_is_its_schema_value_and_any_other_is_damaged() -> Outcome {
    let standard = concat!(
        r#"<sheetData><row r="1">"#,
        r#"<c r="A1" t="b"><v>0</v></c>"#,
        r#"<c r="B1" t="b"><v>1</v></c>"#,
        r#"<c r="C1" t="b"><v> </v></c>"#,
        r#"<c r="D1" t="b"><v>false</v></c>"#,
        r#"<c r="E1" t="b"><v>TRUE</v></c>"#,
        r#"<c r="F1" t="b"><v> 1 </v></c>"#,
        r#"</row></sheetData>"#,
    );
    let preview = sheet_of(Format::Xlsx, &xlsx(&[("S", standard.into())], &[])?, None)?;
    assert_eq!(
        preview.rows,
        table(&[&["FALSE", "TRUE", "", "FALSE", "TRUE", "TRUE"]])
    );
    for value in ["2", "yes", "-1"] {
        let body = format!(
            r#"<sheetData><row r="1"><c r="A1" t="b"><v>{value}</v></c></row></sheetData>"#
        );
        assert!(
            matches!(
                sheet_of(Format::Xlsx, &xlsx(&[("S", body)], &[])?, None),
                Err(GatewayError::PreviewDamaged)
            ),
            "{value}"
        );
    }
    Ok(())
}

// TEST: GW-U-208
/// An `.ods` whose `content.xml` declares an ASCII-compatible encoding other than UTF-8 (ISO-8859-1 here) previews its
/// sheet names and text in that encoding. It needs quick-xml's `encoding` feature: calamine 0.36.1 switched it on for
/// the whole build, calamine 0.37.0 no longer does, so the gateway asks for it itself. (UTF-16 isn't something
/// quick-xml reads at all.)
#[test]
fn gw_u_208_an_ods_in_latin_1_previews_its_text() -> Outcome {
    let content = concat!(
        r#"<?xml version="1.0" encoding="ISO-8859-1"?>"#,
        r#"<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0">"#,
        r#"<office:body><office:spreadsheet><table:table table:name="Café">"#,
        r#"<table:table-row><table:table-cell office:value-type="string"><text:p>crème brûlée</text:p></table:table-cell></table:table-row>"#,
        r#"</table:table></office:spreadsheet></office:body></office:document-content>"#,
    );
    // The same text as ISO-8859-1 bytes: each character here is below U+0100, so it is one byte.
    let latin1 = content
        .chars()
        .map(|character| u8::try_from(u32::from(character)))
        .collect::<Result<Vec<u8>, _>>()?;
    let manifest = r#"<manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0"><manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"/></manifest:manifest>"#;
    let bytes = zip(&[
        (
            "mimetype",
            b"application/vnd.oasis.opendocument.spreadsheet",
        ),
        ("META-INF/manifest.xml", manifest.as_bytes()),
        ("content.xml", latin1.as_slice()),
    ])?;
    let preview = sheet_of(Format::Ods, &bytes, None)?;
    assert_eq!(
        (preview.sheets, preview.rows),
        (vec!["Café".to_owned()], table(&[&["crème brûlée"]]))
    );
    Ok(())
}

/// A minimal compound file (the container of encrypted Office files) holding one empty stream named `stream`.
fn compound(stream: &str) -> Outcome<Vec<u8>> {
    let mut header = vec![0_u8; 512];
    header[..8].copy_from_slice(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]);
    header[24..26].copy_from_slice(&0x3E_u16.to_le_bytes());
    header[26..28].copy_from_slice(&3_u16.to_le_bytes());
    header[28..30].copy_from_slice(&0xFFFE_u16.to_le_bytes());
    header[30..32].copy_from_slice(&9_u16.to_le_bytes());
    header[32..34].copy_from_slice(&6_u16.to_le_bytes());
    header[44..48].copy_from_slice(&1_u32.to_le_bytes()); // one FAT sector
    header[48..52].copy_from_slice(&1_u32.to_le_bytes()); // the directory at sector 1
    header[56..60].copy_from_slice(&4096_u32.to_le_bytes());
    header[60..64].copy_from_slice(&0xFFFF_FFFE_u32.to_le_bytes());
    header[68..72].copy_from_slice(&0xFFFF_FFFE_u32.to_le_bytes());
    for slot in header[76..512].chunks_mut(4) {
        slot.copy_from_slice(&0xFFFF_FFFF_u32.to_le_bytes());
    }
    header[76..80].copy_from_slice(&0_u32.to_le_bytes()); // the FAT is sector 0
    let mut fat = vec![0xFF_u8; 512];
    fat[..4].copy_from_slice(&0xFFFF_FFFD_u32.to_le_bytes());
    fat[4..8].copy_from_slice(&0xFFFF_FFFE_u32.to_le_bytes());
    let mut directory = vec![0_u8; 512];
    for (index, name) in ["Root Entry", stream].into_iter().enumerate() {
        let entry = &mut directory[index * 128..(index + 1) * 128];
        let utf16: Vec<u8> = name.encode_utf16().flat_map(u16::to_le_bytes).collect();
        entry[..utf16.len()].copy_from_slice(&utf16);
        let length = u16::try_from(utf16.len() + 2)?;
        entry[64..66].copy_from_slice(&length.to_le_bytes());
        entry[66] = if index == 0 { 5 } else { 2 };
        entry[68..72].copy_from_slice(&0xFFFF_FFFF_u32.to_le_bytes());
        entry[72..76].copy_from_slice(&0xFFFF_FFFF_u32.to_le_bytes());
        let child: u32 = if index == 0 { 1 } else { 0xFFFF_FFFF };
        entry[76..80].copy_from_slice(&child.to_le_bytes());
        entry[116..120].copy_from_slice(&0xFFFF_FFFE_u32.to_le_bytes());
    }
    Ok([header, fat, directory].concat())
}

// TEST: GW-U-088
#[test]
fn gw_u_088_password_protected_files_say_so() -> Outcome {
    let encrypted_office = compound("EncryptedPackage")?;
    for format in [Format::Xlsx, Format::Xlsb] {
        assert!(
            matches!(
                sheet_of(format, &encrypted_office, None),
                Err(GatewayError::PreviewProtected)
            ),
            "{format:?}"
        );
    }
    let encrypted_ods = ods(
        r#"<table:table table:name="S"/>"#,
        r#"<manifest:encryption-data manifest:checksum-type="SHA1/1K" manifest:checksum="x"/>"#,
    )?;
    assert!(matches!(
        sheet_of(Format::Ods, &encrypted_ods, None),
        Err(GatewayError::PreviewProtected)
    ));
    // A zip whose part is encrypted (general purpose bit 0, in the local and central headers).
    let mut zip_encrypted = xlsx(&[("S", "<sheetData/>".into())], &[])?;
    for at in headers(&zip_encrypted, [0x50, 0x4B, 0x03, 0x04]) {
        zip_encrypted[at + 6] |= 1;
    }
    for at in headers(&zip_encrypted, [0x50, 0x4B, 0x01, 0x02]) {
        zip_encrypted[at + 8] |= 1;
    }
    assert!(matches!(
        sheet_of(Format::Xlsx, &zip_encrypted, None),
        Err(GatewayError::PreviewProtected)
    ));
    Ok(())
}

// TEST: GW-U-088
#[test]
fn gw_u_088_past_the_deadline_is_too_large() -> Outcome {
    let bytes = xlsx(&[("S", "<sheetData/>".into())], &[])?;
    let past = Instant::now()
        .checked_sub(Duration::from_secs(1))
        .ok_or("no past instant")?;
    assert!(matches!(
        read(Format::Xlsx, &bytes, None, past),
        Err(GatewayError::PreviewTooLarge)
    ));
    Ok(())
}

// TEST: GW-U-088
#[test]
fn gw_u_088_numbers_show_at_most_15_significant_digits() {
    for (value, shown) in [
        (0.1 + 0.2, "0.3"),
        (123.0, "123"),
        (-0.5, "-0.5"),
        (1.0 / 3.0, "0.333333333333333"),
        (1e20, "100000000000000000000"),
        (f64::NAN, "NaN"),
    ] {
        assert_eq!(number(value), shown, "{value}");
    }
}
