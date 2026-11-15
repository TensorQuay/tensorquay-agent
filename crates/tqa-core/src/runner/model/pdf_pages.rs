//! The pages of a PDF the agent read, as pictures in the model's request (CORE-U-092; PRD item 30): `read_file`
//! answers a PDF as text by page, its result starting with [`PDF_SUMMARY`] and each page under a [`PAGE_MARK`] line
//! (TOOL-U-145). Every model gets that text. On a route that reads pictures, the latest [`SENT_PAGES`] pages of the
//! run's latest PDF results are drawn (`tqa-pdf`, 144 dpi PNG) and go as images on their results, each counted as
//! [`IMAGE_TOKENS`], within the window and the route's request budget, as workspace pictures do (CORE-U-090); an older
//! result says its pages are no longer shown. A route without vision gets the text and nothing more: the text is the
//! content, and a model that doesn't read pictures loses nothing it could use.
use std::collections::BTreeMap;

use serde_json::Value;
use tqa_llm::{CompletionRequest, ContentPart, Role};
use tqa_pdf::Document;
use tqa_tools::{FileStamp, ImageError, PAGE_MARK, PDF_SUMMARY, read_workspace_bytes_stamped};
use tqa_types::{IMAGE_TOKENS, ImageMediaType};

use crate::runner::session::{Execution, Stop};

/// How many pages, over the run's latest PDF results, go to the model as pictures.
pub(crate) const SENT_PAGES: usize = 3;
/// What the model reads on a result whose pages aren't drawn any more, on a route that reads pictures.
pub(super) const NOT_SHOWN: &str =
    "\n[These pages are no longer shown as pictures. Read them again with read_file to see them.]";
/// The built-in tool whose results these are.
const READ_FILE: &str = "read_file";
/// The most bytes of a PDF the pages are drawn from: `read_file`'s own bound on what it reads.
const MAX_PDF_BYTES: usize = 64 * 1024 * 1024;

/// The pages a run has drawn, kept so a page shown on every request of the run is read and drawn once (the early
/// review of 15 Nov 2026): keyed by the file's path and version and the page, bounded to [`SENT_PAGES`] (the most a
/// request carries), the oldest drawn page giving way.
#[derive(Clone, Debug, Default)]
pub(crate) struct DrawnPages {
    kept: Vec<((String, FileStamp, u32), Vec<u8>)>,
}

impl DrawnPages {
    /// The PNG of `page` of the file at `path` in version `stamp`, if drawn in this run.
    pub(crate) fn get(&self, path: &str, stamp: FileStamp, page: u32) -> Option<&Vec<u8>> {
        self.kept
            .iter()
            .find(|((kept_path, kept_stamp, kept_page), _)| {
                kept_path == path && *kept_stamp == stamp && *kept_page == page
            })
            .map(|(_, png)| png)
    }

    /// Keeps `png` for the page, the oldest giving way past the bound.
    pub(crate) fn insert(&mut self, path: &str, stamp: FileStamp, page: u32, png: Vec<u8>) {
        self.kept.retain(|((kept_path, kept_stamp, kept_page), _)| {
            !(kept_path == path && *kept_stamp == stamp && *kept_page == page)
        });
        if self.kept.len() >= SENT_PAGES {
            self.kept.remove(0);
        }
        self.kept.push(((path.to_owned(), stamp, page), png));
    }

    /// How many pages are kept.
    #[cfg(test)]
    fn len(&self) -> usize {
        self.kept.len()
    }
}

/// One `read_file` result holding PDF pages: the message's index, the file's path and the pages it shows.
struct PdfResult {
    index: usize,
    path: String,
    pages: Vec<u32>,
}

/// What one draw found: the file's path and version, the pages drawn now, and every asked page in order.
type Drawn = (String, FileStamp, Vec<(u32, Vec<u8>)>, Vec<Vec<u8>>);

/// Reads the PDF at `path` and draws the `pages` not in `known` (the run's store) in one open; a page that can't be
/// drawn is left out. Blocking: called off the async runtime.
fn draw(
    workspace: &tqa_tools::WorkspaceRoot,
    path: String,
    pages: &[u32],
    known: &DrawnPages,
) -> Result<Drawn, ImageError> {
    let (bytes, stamp) = read_workspace_bytes_stamped(workspace, &path, MAX_PDF_BYTES)?;
    let missing: Vec<u32> = pages
        .iter()
        .copied()
        .filter(|page| known.get(&path, stamp, *page).is_none())
        .collect();
    let mut fresh: Vec<(u32, Vec<u8>)> = Vec::new();
    if !missing.is_empty() {
        let document = Document::open(bytes).map_err(|_| ImageError::Unreadable)?;
        for (page, png) in missing.iter().zip(document.render_pngs(&missing)) {
            if let Ok(png) = png {
                fresh.push((*page, png));
            }
        }
    }
    let pngs: Vec<Vec<u8>> = pages
        .iter()
        .filter_map(|page| {
            known.get(&path, stamp, *page).cloned().or_else(|| {
                fresh
                    .iter()
                    .find(|(drawn, _)| drawn == page)
                    .map(|(_, png)| png.clone())
            })
        })
        .collect();
    Ok((path, stamp, fresh, pngs))
}

impl Execution<'_> {
    /// `request` with the latest pages drawn on their results and the fixed line on the other PDF results, and the
    /// image tokens of the pages sent. `used` is the window's estimated input tokens so far and `capacity` its room.
    pub(super) async fn attach_pdf_pages(
        &self,
        mut request: CompletionRequest,
        used: u64,
        capacity: u32,
    ) -> Result<(CompletionRequest, u64), Stop> {
        let name = self.runner.names.to_model(READ_FILE);
        let results = pdf_results(&request, name);
        if results.is_empty() {
            return Ok((request, 0));
        }
        if !self.route().vision {
            // The text is the content: nothing to add, nothing to say.
            return Ok((request, 0));
        }
        let room = u64::from(capacity).saturating_sub(used) / u64::from(IMAGE_TOKENS);
        let allowed = usize::try_from(room).unwrap_or(usize::MAX).min(SENT_PAGES);
        // Newest results first, their pages in reading order, up to the allowance.
        let mut sent: Vec<(usize, Vec<Vec<u8>>)> = Vec::new();
        let mut drawn = 0;
        for result in results.iter().rev() {
            if drawn >= allowed {
                break;
            }
            let wanted: Vec<u32> = result.pages.iter().copied().take(allowed - drawn).collect();
            let Ok(pngs) = self.draw_pages(&result.path, &wanted).await? else {
                continue;
            };
            drawn += pngs.len();
            if !pngs.is_empty() {
                sent.push((result.index, pngs));
            }
        }
        let budget = usize::try_from(self.route().request_budget()).unwrap_or(usize::MAX);
        let mut route = self.provider().route().clone();
        route.vision = true;
        loop {
            let rendered = render(&request, &results, &sent);
            let fits =
                tqa_llm::request_body_len(&route, &rendered).is_ok_and(|length| length <= budget);
            if fits {
                request = rendered;
                break;
            }
            // Too big: drop the oldest result's last page, then the result itself when it has none left.
            let Some(last) = sent.last_mut() else {
                request = rendered;
                break;
            };
            last.1.pop();
            if last.1.is_empty() {
                sent.pop();
            }
        }
        let count = sent.iter().map(|(_, pngs)| pngs.len()).sum::<usize>();
        let tokens = u64::try_from(count)
            .unwrap_or(u64::MAX)
            .saturating_mul(u64::from(IMAGE_TOKENS));
        Ok((request, tokens))
    }

    /// `pages` of the PDF at `path` as PNGs, off the async runtime, under the run's cancel and deadline: the pages this
    /// run drew of the same file version come from its store, the rest are drawn in one open and kept; a page that
    /// can't be drawn is left out, a file that can't be read gives none.
    async fn draw_pages(
        &self,
        path: &str,
        pages: &[u32],
    ) -> Result<Result<Vec<Vec<u8>>, ImageError>, Stop> {
        let (workspace, path, pages) = (
            self.runner.workspace.clone(),
            relative(path),
            pages.to_vec(),
        );
        let known = self
            .drawn_pages
            .lock()
            .map(|drawn| drawn.clone())
            .unwrap_or_default();
        let task = tokio::task::spawn_blocking(move || draw(&workspace, path, &pages, &known));
        let outcome = self
            .watch()
            .wait(task)
            .await?
            .unwrap_or(Err(ImageError::Unreadable));
        Ok(outcome.map(|(path, stamp, fresh, pngs)| {
            self.keep_drawn(&path, stamp, fresh);
            pngs
        }))
    }

    /// Keeps the pages drawn now for the run's later requests.
    fn keep_drawn(&self, path: &str, stamp: FileStamp, fresh: Vec<(u32, Vec<u8>)>) {
        if let Ok(mut drawn) = self.drawn_pages.lock() {
            for (page, png) in fresh {
                drawn.insert(path, stamp, page, png);
            }
        }
    }

    /// The window tokens the next request's PDF pages take, for the compaction trigger.
    pub(in crate::runner) fn pdf_page_tokens(&self) -> Result<u64, Stop> {
        if !self.route().vision {
            return Ok(0);
        }
        let projected = tqa_context::project(&self.frames)?;
        let pages = projected
            .iter()
            .filter(|message| message.role == tqa_context::Role::Tool)
            .filter(|message| message.text.starts_with(PDF_SUMMARY))
            .map(|message| pages_in(&message.text).len())
            .sum::<usize>()
            .min(SENT_PAGES);
        Ok(u64::try_from(pages)
            .unwrap_or(u64::MAX)
            .saturating_mul(u64::from(IMAGE_TOKENS)))
    }
}

/// A `read_file` path as the workspace reader takes it: relative to the root, the sandbox's `/workspace/` prefix off.
fn relative(path: &str) -> String {
    path.strip_prefix("/workspace/")
        .unwrap_or(path)
        .trim_start_matches("./")
        .to_owned()
}

/// The pages a PDF result shows, from its page marks, in order.
fn pages_in(text: &str) -> Vec<u32> {
    text.lines()
        .filter_map(|line| {
            line.strip_prefix(PAGE_MARK)?
                .strip_suffix(" ---")?
                .parse()
                .ok()
        })
        .collect()
}

/// The tool results that answer a `read_file` of a PDF, in order.
fn pdf_results(request: &CompletionRequest, name: &str) -> Vec<PdfResult> {
    let paths: BTreeMap<&str, String> = request
        .messages
        .iter()
        .flat_map(|message| &message.tool_calls)
        .filter(|call| call.name == name)
        .filter_map(|call| {
            let arguments: Value = serde_json::from_str(&call.arguments).ok()?;
            Some((call.id.as_str(), arguments["path"].as_str()?.to_owned()))
        })
        .collect();
    request
        .messages
        .iter()
        .enumerate()
        .filter(|(_, message)| message.role == Role::Tool)
        .filter_map(|(index, message)| {
            let text = message.content.iter().find_map(|part| match part {
                ContentPart::Text(text) if text.starts_with(PDF_SUMMARY) => Some(text.as_str()),
                _ => None,
            })?;
            let pages = pages_in(text);
            if pages.is_empty() {
                return None;
            }
            let path = paths.get(message.tool_call_id.as_deref()?)?;
            Some(PdfResult {
                index,
                path: path.clone(),
                pages,
            })
        })
        .collect()
}

/// The request with `sent`'s pages as images on their results and the fixed line on every other PDF result.
fn render(
    request: &CompletionRequest,
    results: &[PdfResult],
    sent: &[(usize, Vec<Vec<u8>>)],
) -> CompletionRequest {
    let mut rendered = request.clone();
    for result in results {
        let Some(message) = rendered.messages.get_mut(result.index) else {
            continue;
        };
        match sent.iter().find(|(shown, _)| *shown == result.index) {
            Some((_, pngs)) => {
                for png in pngs {
                    message.content.push(ContentPart::Image {
                        media_type: ImageMediaType::Png,
                        data: png.clone(),
                    });
                }
            }
            None => {
                if let Some(ContentPart::Text(text)) = message
                    .content
                    .iter_mut()
                    .rev()
                    .find(|part| matches!(part, ContentPart::Text(_)))
                {
                    text.push_str(NOT_SHOWN);
                }
            }
        }
    }
    rendered
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use super::*;

    // TEST: CORE-U-092
    #[test]
    fn core_u_092_the_drawn_pages_store_is_bounded_and_keyed_by_the_file_version() {
        let stamp = FileStamp {
            len: 10,
            modified: UNIX_EPOCH + Duration::from_secs(1),
        };
        let mut drawn = DrawnPages::default();
        for page in 1..=4 {
            drawn.insert(
                "docs/a.pdf",
                stamp,
                page,
                vec![u8::try_from(page).unwrap_or(0)],
            );
        }
        assert_eq!(drawn.len(), SENT_PAGES, "the oldest gave way");
        assert!(drawn.get("docs/a.pdf", stamp, 1).is_none());
        assert_eq!(drawn.get("docs/a.pdf", stamp, 4), Some(&vec![4]));
        // A changed file is another version: its pages are drawn again.
        let changed = FileStamp { len: 11, ..stamp };
        assert!(drawn.get("docs/a.pdf", changed, 4).is_none());
        // The same page again replaces its picture, taking no second place.
        drawn.insert("docs/a.pdf", stamp, 4, vec![44]);
        assert_eq!(drawn.len(), SENT_PAGES);
        assert_eq!(drawn.get("docs/a.pdf", stamp, 4), Some(&vec![44]));
    }
}
