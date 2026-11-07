//! GW-I-143 (the file previews design): an agent's HTML page served sandboxed by its headers, and a spreadsheet's
//! sheet as a table of display text, over HTTP: each answer, each refusal, and access and paths as for the download.
#[path = "support/files_api.rs"]
mod files_api;
#[path = "support/fixture.rs"]
mod support;

use files_api::{Result, body, call, code, task_with};
use reqwest::Method;
use serde_json::json;
use sqlx::PgPool;
use support::{Fixture, TestResult};

fn view(f: &Fixture, cookie: &str, task: &str, path: &str) -> reqwest::RequestBuilder {
    call(
        f,
        cookie,
        Method::GET,
        &format!("/api/v1/conversations/{task}/files/view"),
    )
    .query(&[("path", path)])
}

// TEST: GW-I-143
// The file previews design: an agent's HTML page is served for the panel's frame, sandboxed by its headers; any other
// type has no view; a page over the preview limit is only downloaded; access and paths are as for the download.
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_143_an_html_page_is_served_sandboxed_and_nothing_else_is(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, "[files]\npreview_html_bytes=2048", async |f| {
        f.admin().await?;
        let (_, owner) = f.member("owner@example.invalid").await?;
        let (_, stranger) = f.member("stranger@example.invalid").await?;
        let page =
            b"<!doctype html><title>Report</title><script>document.title='ran'</script>".to_vec();
        let id = task_with(
            f,
            &owner,
            &[
                ("report.html", page.clone()),
                ("big.htm", vec![b'x'; 2049]),
                ("doc.pdf", b"%PDF-1.4".to_vec()),
            ],
        )
        .await?;
        let served = view(f, &owner, &id, "uploads/report.html").send().await?;
        assert_eq!(served.status(), 200);
        let header = |name: &str| {
            served
                .headers()
                .get(name)
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_owned()
        };
        assert_eq!(header("content-type"), "text/html; charset=utf-8");
        let policy = header("content-security-policy");
        assert!(policy.starts_with("sandbox allow-scripts;"), "{policy}");
        assert!(
            policy.contains("connect-src 'none'") && policy.contains("frame-ancestors 'self'"),
            "{policy}"
        );
        assert!(!policy.contains("allow-same-origin"), "{policy}");
        assert_eq!(header("x-content-type-options"), "nosniff");
        assert!(
            header("content-disposition").is_empty(),
            "shown, not downloaded"
        );
        assert_eq!(
            served.bytes().await?.to_vec(),
            page,
            "the page's own bytes, unchanged"
        );
        // Any other type has no view: it can still be downloaded.
        assert_eq!(
            code(view(f, &owner, &id, "uploads/doc.pdf").send().await?).await?,
            (415, "preview_unsupported".into())
        );
        // Over files.preview_html_bytes: only downloaded.
        assert_eq!(
            code(view(f, &owner, &id, "uploads/big.htm").send().await?).await?,
            (413, "body_too_large".into())
        );
        // Access and paths as for the download.
        assert_eq!(
            code(
                view(f, &stranger, &id, "uploads/report.html")
                    .send()
                    .await?
            )
            .await?
            .0,
            403
        );
        assert_eq!(
            code(view(f, &owner, &id, "../report.html").send().await?).await?,
            (400, "path_invalid".into())
        );
        assert_eq!(
            code(view(f, &owner, &id, "uploads/missing.html").send().await?)
                .await?
                .0,
            404
        );
        Ok(())
    })
    .await
}

fn sheet(f: &Fixture, cookie: &str, task: &str, query: &[(&str, &str)]) -> reqwest::RequestBuilder {
    call(
        f,
        cookie,
        Method::GET,
        &format!("/api/v1/conversations/{task}/files/sheet"),
    )
    .query(query)
}

/// A minimal `.xlsx` whose sheets hold inline text in their first row.
fn workbook(sheets: &[(&str, &[&str])]) -> Result<Vec<u8>> {
    use std::fmt::Write as _;
    use std::io::Write as _;
    const MAIN: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
    const RELATIONSHIPS: &str =
        "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
    const PACKAGE: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
    let mut parts = vec![(
        "_rels/.rels".to_owned(),
        format!(
            r#"<Relationships xmlns="{PACKAGE}"><Relationship Id="r" Type="{RELATIONSHIPS}/officeDocument" Target="xl/workbook.xml"/></Relationships>"#
        ),
    )];
    let (mut names, mut rels) = (String::new(), String::new());
    for (index, (name, cells)) in sheets.iter().enumerate() {
        let n = index + 1;
        write!(names, r#"<sheet name="{name}" sheetId="{n}" r:id="s{n}"/>"#)?;
        write!(
            rels,
            r#"<Relationship Id="s{n}" Type="{RELATIONSHIPS}/worksheet" Target="worksheets/{n}.xml"/>"#
        )?;
        let mut row = String::new();
        for text in *cells {
            write!(row, r#"<c t="inlineStr"><is><t>{text}</t></is></c>"#)?;
        }
        parts.push((
            format!("xl/worksheets/{n}.xml"),
            format!(
                r#"<worksheet xmlns="{MAIN}"><sheetData><row>{row}</row></sheetData></worksheet>"#
            ),
        ));
    }
    parts.push((
        "xl/workbook.xml".into(),
        format!(r#"<workbook xmlns="{MAIN}" xmlns:r="{RELATIONSHIPS}"><sheets>{names}</sheets></workbook>"#),
    ));
    parts.push((
        "xl/_rels/workbook.xml.rels".into(),
        format!(r#"<Relationships xmlns="{PACKAGE}">{rels}</Relationships>"#),
    ));
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, content) in parts {
        writer.start_file(name, zip::write::SimpleFileOptions::default())?;
        writer.write_all(content.as_bytes())?;
    }
    Ok(writer.finish()?.into_inner())
}

// TEST: GW-I-143
// The file previews design: a spreadsheet's sheet is served as a table of display text; any other type has no table;
// a file over the preview limit, a damaged file and a missing sheet each say so; access and paths are as for the
// download.
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_143_a_spreadsheet_is_served_as_a_table_and_nothing_else_is(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, "[files]\npreview_sheet_bytes=4096", async |f| {
        f.admin().await?;
        let (_, owner) = f.member("owner@example.invalid").await?;
        let (_, stranger) = f.member("stranger@example.invalid").await?;
        let report = workbook(&[("Totals", &["Region", "=HYPERLINK(\"x\")"]), ("Notes", &["n"])])?;
        let mut big = workbook(&[("S", &["x"])])?;
        big.resize(4097, 0);
        let id = task_with(
            f,
            &owner,
            &[
                ("report.xlsx", report),
                ("big.xlsx", big),
                ("broken.xlsx", b"not a spreadsheet".to_vec()),
                ("old.xls", b"legacy".to_vec()),
            ],
        )
        .await?;
        let (status, first) = body(sheet(f, &owner, &id, &[("path", "uploads/report.xlsx")]).send().await?).await?;
        assert_eq!(status, 200, "{first}");
        assert_eq!(
            first,
            json!({"sheets": ["Totals", "Notes"], "sheet": "Totals", "rows": [["Region", "=HYPERLINK(\"x\")"]],
                   "total_rows": 1, "more_rows": false, "more_columns": false, "truncated_cells": 0})
        );
        let (status, notes) = body(
            sheet(f, &owner, &id, &[("path", "uploads/report.xlsx"), ("sheet", "Notes")])
                .send()
                .await?,
        )
        .await?;
        assert_eq!((status, &notes["sheet"], &notes["rows"]), (200, &json!("Notes"), &json!([["n"]])));
        assert_eq!(
            code(sheet(f, &owner, &id, &[("path", "uploads/report.xlsx"), ("sheet", "Gone")]).send().await?).await?,
            (404, "not_found".into())
        );
        for (path, expected) in [
            ("uploads/old.xls", (415, "preview_unsupported")),
            ("uploads/big.xlsx", (413, "body_too_large")),
            ("uploads/broken.xlsx", (422, "preview_damaged")),
            ("../report.xlsx", (400, "path_invalid")),
        ] {
            assert_eq!(
                code(sheet(f, &owner, &id, &[("path", path)]).send().await?).await?,
                (expected.0, expected.1.into()),
                "{path}"
            );
        }
        assert_eq!(
            code(sheet(f, &stranger, &id, &[("path", "uploads/report.xlsx")]).send().await?).await?.0,
            403
        );
        assert_eq!(
            code(sheet(f, &owner, &id, &[("path", "uploads/missing.xlsx")]).send().await?).await?.0,
            404
        );
        Ok(())
    })
    .await
}
