//! The reader as `tqa-pdf-bench` measures an engine: `tqa-pdf-adapter <pdf> --out <json> [--max-pages <n>]` writes
//! one JSON object with the engine's name and version, an `error` (`password`, `limit`, `panic` or `other`), the page
//! count, each page's text and milliseconds, and the run's peak resident memory. The benchmark scores this against its
//! corpus, so the product's reader is held to the engine's own bars at every change.
#![allow(clippy::print_stderr, reason = "an adapter's failures go to its log")]

use std::fs;
use std::panic::{self, AssertUnwindSafe};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use serde::Serialize;
use tqa_pdf::{Document, MAX_PAGES, OpenError};

#[derive(Serialize)]
struct Failure {
    kind: &'static str,
    message: String,
}

#[derive(Serialize)]
struct PageRead {
    n: usize,
    ms: f64,
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<Failure>,
}

#[derive(Serialize)]
struct Reading {
    engine: &'static str,
    version: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<Failure>,
    page_count: Option<usize>,
    open_ms: f64,
    pages: Vec<PageRead>,
    total_ms: f64,
    peak_rss_kb: u64,
}

struct Args {
    pdf: PathBuf,
    out: PathBuf,
    max_pages: Option<u32>,
}

fn parse(args: &[String]) -> Option<Args> {
    let mut pdf = None;
    let mut out = None;
    let mut max_pages = None;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--out" => out = Some(PathBuf::from(rest.next()?)),
            "--max-pages" => max_pages = Some(rest.next()?.parse().ok()?),
            _ if pdf.is_none() => pdf = Some(PathBuf::from(arg)),
            _ => return None,
        }
    }
    Some(Args {
        pdf: pdf?,
        out: out?,
        max_pages,
    })
}

/// The process's peak resident memory in KiB, from the kernel's own account; 0 where it can't be read.
fn peak_rss_kb() -> u64 {
    fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|status| {
            status
                .lines()
                .find_map(|line| line.strip_prefix("VmHWM:"))
                .and_then(|value| value.split_whitespace().next()?.parse().ok())
        })
        .unwrap_or(0)
}

fn failure(error: &OpenError) -> Failure {
    let kind = match error {
        OpenError::Password => "password",
        OpenError::TooManyPages => "limit",
        OpenError::NotPdf | OpenError::Damaged(_) => "other",
    };
    Failure {
        kind,
        message: error.to_string(),
    }
}

fn read(args: &Args) -> Reading {
    let started = Instant::now();
    let mut reading = Reading {
        engine: "tqa-pdf",
        version: env!("CARGO_PKG_VERSION"),
        error: None,
        page_count: None,
        open_ms: 0.0,
        pages: Vec::new(),
        total_ms: 0.0,
        peak_rss_kb: 0,
    };
    let bytes = match fs::read(&args.pdf) {
        Ok(bytes) => bytes,
        Err(error) => {
            reading.error = Some(Failure {
                kind: "other",
                message: error.to_string(),
            });
            return reading;
        }
    };
    let opened = panic::catch_unwind(AssertUnwindSafe(|| Document::open(bytes)));
    reading.open_ms = started.elapsed().as_secs_f64() * 1000.0;
    let document = match opened {
        Ok(Ok(document)) => document,
        Ok(Err(error)) => {
            reading.error = Some(failure(&error));
            return reading;
        }
        Err(_) => {
            reading.error = Some(Failure {
                kind: "panic",
                message: "the engine panicked while opening".to_owned(),
            });
            return reading;
        }
    };
    let count = document.page_count();
    reading.page_count = Some(usize::try_from(count).unwrap_or(usize::MAX));
    let last = args
        .max_pages
        .map_or(count, |max| max.min(count))
        .min(MAX_PAGES);
    for page in 1..=last {
        let page_started = Instant::now();
        let text = panic::catch_unwind(AssertUnwindSafe(|| document.text(page)));
        let ms = page_started.elapsed().as_secs_f64() * 1000.0;
        reading.pages.push(match text {
            Ok(Ok(text)) => PageRead {
                n: usize::try_from(page).unwrap_or(usize::MAX),
                ms,
                text: text.text,
                error: None,
            },
            Ok(Err(error)) => PageRead {
                n: usize::try_from(page).unwrap_or(usize::MAX),
                ms,
                text: String::new(),
                error: Some(Failure {
                    kind: "other",
                    message: error.to_string(),
                }),
            },
            Err(_) => PageRead {
                n: usize::try_from(page).unwrap_or(usize::MAX),
                ms,
                text: String::new(),
                error: Some(Failure {
                    kind: "panic",
                    message: "the engine panicked on the page".to_owned(),
                }),
            },
        });
    }
    reading.total_ms = started.elapsed().as_secs_f64() * 1000.0;
    reading
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(args) = parse(&args) else {
        eprintln!("usage: tqa-pdf-adapter <pdf> --out <json> [--max-pages <n>]");
        return ExitCode::from(2);
    };
    let mut reading = read(&args);
    reading.peak_rss_kb = peak_rss_kb();
    match serde_json::to_vec(&reading)
        .map_err(|error| error.to_string())
        .and_then(|json| fs::write(&args.out, json).map_err(|error| error.to_string()))
    {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("the reading couldn't be written: {error}");
            ExitCode::FAILURE
        }
    }
}
