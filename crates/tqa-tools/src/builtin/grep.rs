//! Regex and literal search using the ripgrep matcher, walker and streaming searcher.
mod engine;
mod large_files;
mod pool;
mod run;
mod sink;
use super::{arguments, file_io, search::page::Page, specification};
use crate::{Tool, ToolContext, ToolError, ToolOutput, ToolSpec};
use async_trait::async_trait;
use grep_regex::{RegexMatcher, RegexMatcherBuilder};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

// Bound query compilation independently of the source and output sizes.
const MAX_PATTERN_BYTES: usize = 64 * 1024;
const REGEX_COMPILED_BYTES: usize = 10 * 1024 * 1024;
const REGEX_DFA_BYTES: usize = 1024 * 1024;

/// Searches workspace text with cheap file discovery, counts or bounded content pages.
pub struct Grep {
    spec: ToolSpec,
}
impl Grep {
    /// Constructs the bounded search schema.
    #[must_use]
    pub fn new() -> Self {
        Self {
            spec: specification(
                "grep",
                "Search workspace text. Start with files_with_matches to find relevant files cheaply, then use content on a narrowed path to locate lines, then read_file with start_line from a result. Supports regex or fixed_string, case_insensitive, glob and ripgrep file_type filters. Respects ignore files, skips unnamed hidden paths and symlinks. Sorted results fit 8 KiB; repeat identical options with cursor to continue. On an incomplete notice, narrow path or glob. Content lines are capped at 500 bytes; context_lines (0-10) applies only to content. Null takes the default: path the workspace root, output_mode files_with_matches, case_insensitive and fixed_string false.",
                json!({"type":"object","additionalProperties":false,
                "required":["pattern","path","glob","file_type","case_insensitive","fixed_string","output_mode","context_lines","max_results","cursor"],
                "properties":{
                "pattern":{"type":"string","maxLength":MAX_PATTERN_BYTES},"path":{"type":["string","null"]},"glob":{"type":["string","null"]},"file_type":{"type":["string","null"]},"case_insensitive":{"type":["boolean","null"]},"fixed_string":{"type":["boolean","null"]},
                "output_mode":{"type":["string","null"],"enum":["files_with_matches","count","content",null]},"context_lines":{"type":["integer","null"],"minimum":0,"maximum":10},"max_results":{"type":["integer","null"],"minimum":1,"maximum":4_294_967_295_u64},"cursor":{"type":["string","null"],"description":super::CURSOR}}}),
                true,
            ),
        }
    }
}
impl Default for Grep {
    fn default() -> Self {
        Self::new()
    }
}
#[derive(Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Mode {
    #[default]
    FilesWithMatches,
    Count,
    Content,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    pattern: String,
    #[serde(default = "root_path")]
    path: String,
    glob: Option<String>,
    file_type: Option<String>,
    #[serde(default)]
    case_insensitive: bool,
    #[serde(default)]
    fixed_string: bool,
    #[serde(default)]
    output_mode: Mode,
    context_lines: Option<u8>,
    max_results: Option<u32>,
    cursor: Option<String>,
}
fn root_path() -> String {
    ".".into()
}
#[async_trait]
impl Tool for Grep {
    // Its schema is strict-valid: optional properties are nullable, and null means absent (TOOL-U-034).
    fn null_means_absent(&self) -> bool {
        true
    }
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }
    async fn call(&self, args: Value, cx: &ToolContext) -> Result<ToolOutput, ToolError> {
        cx.check()?;
        let mut args: Args = arguments(args)?;
        // Closed or opaque fields take a blank or null-word value as absent (TOOL-U-034).
        args.cursor = super::absent(args.cursor);
        args.file_type = super::absent(args.file_type);
        validate(&args)?;
        let matcher = matcher(&args)?;
        let page = page(&args, cx)?;
        let resources = cx.clone();
        file_io::run(cx, move |cancel| {
            run::run(&args, &matcher, page, &resources, cancel)
        })
        .await
    }
}
fn matcher(args: &Args) -> Result<RegexMatcher, ToolError> {
    RegexMatcherBuilder::new()
        .case_insensitive(args.case_insensitive)
        .fixed_strings(args.fixed_string)
        .line_terminator(Some(b'\n'))
        .crlf(true)
        .multi_line(true)
        .size_limit(REGEX_COMPILED_BYTES)
        .dfa_size_limit(REGEX_DFA_BYTES)
        .build(&args.pattern)
        .map_err(|error| {
            let reason = error.to_string();
            ToolError::invalid(
                "/pattern",
                reason
                    .lines()
                    .last()
                    .unwrap_or("invalid regular expression"),
            )
        })
}
fn page(args: &Args, cx: &ToolContext) -> Result<Page, ToolError> {
    let rows = cx
        .limits
        .grep_results
        .min(args.max_results.map_or(usize::MAX, |value| value as usize));
    let resolved = cx.workspace.resolve(&args.path)?;
    let relative = resolved
        .strip_prefix(cx.workspace.path())
        .map_err(|_| ToolError::PathOutsideWorkspace)?;
    let options = (
        "grep",
        &args.pattern,
        relative,
        &args.glob,
        &args.file_type,
        args.case_insensitive,
        args.fixed_string,
        args.output_mode,
        args.context_lines.unwrap_or(0),
        rows,
        cx.limits.search_page_bytes,
    );
    Page::new(
        &options,
        args.cursor.as_deref(),
        cx.limits.search_page_bytes,
        rows,
    )
}

fn validate(args: &Args) -> Result<(), ToolError> {
    if args.pattern.len() > MAX_PATTERN_BYTES {
        return Err(ToolError::invalid("/pattern", "pattern exceeds 64 KiB"));
    }
    if args.max_results == Some(0) {
        return Err(ToolError::invalid("/max_results", "limit must be positive"));
    }
    if args.context_lines.is_some_and(|lines| lines > 10)
        || (args.context_lines.is_some() && args.output_mode != Mode::Content)
    {
        return Err(ToolError::invalid(
            "/context_lines",
            "context_lines must be 0 through 10 and requires content mode",
        ));
    }
    Ok(())
}
