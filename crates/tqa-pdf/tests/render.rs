//! PDF-U-002: a page as a picture. The one-page PDF of `reader.rs`'s shape draws to a PNG of A4-at-144-dpi size on a
//! white ground; a page past the end is refused by name.
use tqa_pdf::Document;
use tqa_pdf::render::RenderError;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn one_page() -> Vec<u8> {
    page_of(612.0, 792.0)
}

/// A one-page PDF whose page is `width` by `height` points.
fn page_of(width: f32, height: f32) -> Vec<u8> {
    let content = "BT /F1 24 Tf 72 700 Td (Picture me) Tj ET";
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [4 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width} {height}] /Resources << /Font << /F1 3 0 R >> >> /Contents 5 0 R >>"
        ),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ),
    ];
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (i, object) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(b"xref\n0 6\n0000000000 65535 f \n");
    for offset in offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    out
}

// TEST: PDF-U-002
#[test]
fn pdf_u_002_a_page_draws_to_a_png_of_its_size_and_no_page_past_the_end() -> TestResult {
    let document = Document::open(one_page())?;
    let png = document.render_png(1)?;
    assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"), "a PNG");
    // The IHDR's width and height: 612 by 792 points at 2 pixels a point.
    let width = u32::from_be_bytes(png[16..20].try_into()?);
    let height = u32::from_be_bytes(png[20..24].try_into()?);
    assert_eq!((width, height), (1224, 1584));
    assert_eq!(
        document.render_png(2).err(),
        Some(RenderError::NoSuchPage(2))
    );
    assert_eq!(
        document.render_png(0).err(),
        Some(RenderError::NoSuchPage(0))
    );
    Ok(())
}

/// The IHDR's width and height of a PNG.
fn size(png: &[u8]) -> Result<(u32, u32), Box<dyn std::error::Error>> {
    Ok((
        u32::from_be_bytes(png[16..20].try_into()?),
        u32::from_be_bytes(png[20..24].try_into()?),
    ))
}

// TEST: PDF-U-002
/// A page's picture is capped at 2,000 px on its longer side before it is drawn (the early review of 15 Nov 2026): a
/// poster, a map or a hostile `MediaBox` never becomes a pixmap of gigabytes; A4 keeps its 144 DPI size.
#[test]
fn pdf_u_002_a_huge_page_draws_within_the_side_cap_and_a4_keeps_its_size() -> TestResult {
    let huge = Document::open(page_of(14_400.0, 14_400.0))?;
    let (width, height) = size(&huge.render_png(1)?)?;
    assert!(width <= 2_000 && height <= 2_000, "{width} by {height}");
    assert!(width >= 1_990, "drawn as large as the cap allows: {width}");
    let wide = Document::open(page_of(14_400.0, 720.0))?;
    let (width, height) = size(&wide.render_png(1)?)?;
    assert!(width <= 2_000 && height <= 100, "{width} by {height}");
    let a4 = Document::open(page_of(595.0, 842.0))?;
    assert_eq!(size(&a4.render_png(1)?)?, (1190, 1684));
    // Several pages in one call: one answer each, in order.
    let pages = Document::open(one_page())?.render_pngs(&[1, 2]);
    assert!(pages[0].is_ok() && pages[1] == Err(RenderError::NoSuchPage(2)));
    Ok(())
}
