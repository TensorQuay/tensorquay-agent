//! PDF-U-001: the reader's text by page. A small PDF written here (uncompressed content streams, a plain cross-reference
//! table) reads page by page; a page with no text reads empty; what isn't a PDF, and a page past the end, are refused by
//! name; the cleaning drops the engines' control marks and bounds a page. The engine's accuracy and speed bars are the
//! benchmark's (`tqa-pdf-bench run --engine tqa-pdf=...`), not this file's.
use tqa_pdf::{Document, MAX_PAGE_TEXT_BYTES, OpenError, clean};

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// A PDF of `pages`, each a content stream (an empty one for an empty page), with a correct cross-reference table.
fn pdf(pages: &[&str]) -> Vec<u8> {
    pdf_with(pages, &Extras::default())
}

/// What a hostile or locked file adds to the plain one: entries in the catalogue and the trailer, and more objects.
#[derive(Default)]
struct Extras {
    catalog: String,
    trailer: String,
    objects: Vec<String>,
}

fn pdf_with(pages: &[&str], extras: &Extras) -> Vec<u8> {
    let mut objects: Vec<String> = Vec::new();
    // 1: the catalogue, 2: the page tree, 3: the font; then each page and its content stream, then the extras.
    let kids: Vec<String> = (0..pages.len())
        .map(|i| format!("{} 0 R", 4 + 2 * i))
        .collect();
    objects.push(format!(
        "<< /Type /Catalog /Pages 2 0 R {} >>",
        extras.catalog
    ));
    objects.push(format!(
        "<< /Type /Pages /Kids [{}] /Count {} >>",
        kids.join(" "),
        pages.len()
    ));
    objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned());
    for (i, content) in pages.iter().enumerate() {
        let stream = if content.is_empty() {
            String::new()
        } else {
            format!("BT /F1 12 Tf 72 720 Td ({content}) Tj ET")
        };
        objects.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 3 0 R >> >> /Contents {} 0 R >>",
            5 + 2 * i
        ));
        objects.push(format!(
            "<< /Length {} >>\nstream\n{stream}\nendstream",
            stream.len()
        ));
    }
    objects.extend(extras.objects.iter().cloned());
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (i, object) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R {} >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1,
            extras.trailer
        )
        .as_bytes(),
    );
    out
}

// TEST: PDF-U-001
#[test]
fn pdf_u_001_reads_text_by_page_and_an_empty_page_as_empty() -> TestResult {
    let document = Document::open(pdf(&["First page words", "", "Third page words"]))?;
    assert_eq!(document.page_count(), 3);
    assert_eq!(document.text(1)?.text, "First page words");
    let empty = document.text(2)?;
    assert!(empty.is_empty() && !empty.cut, "{empty:?}");
    assert_eq!(document.text(3)?.text, "Third page words");
    let past = document.text(4).err().ok_or("page 4 read")?;
    assert_eq!(
        (past.page, past.message.as_str()),
        (4, "the document has 3 pages")
    );
    assert!(document.text(0).is_err());
    Ok(())
}

#[test]
fn pdf_u_001_refuses_what_is_not_a_pdf_by_name() {
    assert_eq!(
        Document::open(b"plain words\n".to_vec()).err(),
        Some(OpenError::NotPdf)
    );
    assert_eq!(
        Document::open(b"\x89PNG\r\n\x1a\n".to_vec()).err(),
        Some(OpenError::NotPdf)
    );
    // A header with nothing after it is a PDF that is damaged, not another kind of file.
    assert!(matches!(
        Document::open(b"%PDF-1.7\n".to_vec()),
        Err(OpenError::Damaged(_))
    ));
}

#[test]
fn pdf_u_001_a_damaged_cross_reference_table_is_rebuilt() -> TestResult {
    let mut bytes = pdf(&["Still readable"]);
    // The table's offsets point nowhere: the lenient engine scans for the objects instead.
    let xref = bytes
        .windows(6)
        .rposition(|window| window == b"\nxref\n")
        .ok_or("no xref")?;
    for byte in &mut bytes[xref + 20..xref + 60] {
        if byte.is_ascii_digit() {
            *byte = b'9';
        }
    }
    let document = Document::open(bytes)?;
    assert_eq!(document.text(1)?.text, "Still readable");
    Ok(())
}

#[test]
fn pdf_u_001_cleaning_drops_control_marks_and_bounds_a_page() {
    let text = clean("Hy\u{2}phen\r\nnext  \n\n\n\nlast\t tab\n\n");
    assert_eq!(text.text, "Hyphen\nnext\n\nlast\t tab");
    assert!(!text.cut);
    let long = "x".repeat(100) + "\n";
    let cut = clean(&long.repeat(MAX_PAGE_TEXT_BYTES / 50));
    assert!(cut.cut);
    assert!(cut.text.len() <= MAX_PAGE_TEXT_BYTES);
    assert!(cut.text.ends_with('x'), "cut on a line, never inside one");
}

// TEST: PDF-U-001
/// A hostile file nested 100,000 deep (the benchmark's `hostile-deep-nesting`) is refused before the engine parses it:
/// the engine's parser recurses once per level and would overflow the stack, which aborts the process the reader runs
/// in (the runtime's). A plainly nested file still opens.
#[test]
fn pdf_u_001_a_file_nested_too_deep_is_refused_before_parsing() -> TestResult {
    let deep = Extras {
        catalog: format!("/Deep {}{}", "[".repeat(100_000), "]".repeat(100_000)),
        ..Extras::default()
    };
    let Err(OpenError::Damaged(why)) = Document::open(pdf_with(&["Hostile"], &deep)) else {
        return Err("a deeply nested file must be refused as damaged".into());
    };
    assert!(why.contains("nested"), "{why}");
    let plain = Extras {
        catalog: format!("/Deep {}{}", "[".repeat(64), "]".repeat(64)),
        ..Extras::default()
    };
    let document = Document::open(pdf_with(&["Plain"], &plain))?;
    assert_eq!(document.text(1)?.text, "Plain");
    Ok(())
}

// TEST: PDF-U-001
/// A file the standard security handler can't open with the empty user password needs a password, and says so (the
/// benchmark's `password-rc4` and `password-aes256`): never "damaged", though its catalogue decrypts to nonsense.
#[test]
fn pdf_u_001_a_locked_file_needs_a_password() {
    let locked = Extras {
        trailer: "/Encrypt 6 0 R /ID [<00112233445566778899aabbccddeeff> <00112233445566778899aabbccddeeff>]"
            .to_owned(),
        objects: vec![format!(
            "<< /Filter /Standard /V 1 /R 2 /Length 40 /P -1 /O <{}> /U <{}> >>",
            "ab".repeat(32),
            "cd".repeat(32)
        )],
        ..Extras::default()
    };
    assert!(
        matches!(
            Document::open(pdf_with(&["Secret"], &locked)),
            Err(OpenError::Password)
        ),
        "a locked file must ask for a password"
    );
}
