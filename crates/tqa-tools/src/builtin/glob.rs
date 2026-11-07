//! Ignore-aware file discovery under the shared search budgets.
use super::{
    arguments, file_io,
    search::{
        budget::Budget,
        page::Page,
        walk::{self, Options},
    },
    specification,
};
use crate::{Tool, ToolContext, ToolError, ToolOutput, ToolSpec};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};

/// Finds sorted workspace files with stateless continuation and scan progress.
pub struct Glob {
    spec: ToolSpec,
}
impl Glob {
    /// Constructs the ignore-aware glob schema.
    #[must_use]
    pub fn new() -> Self {
        Self {
            spec: specification(
                "glob",
                "Find workspace files by path pattern, respecting .gitignore, .ignore and .tqaignore. Hidden paths require a named dot component; symlinks are not followed. Results are sorted and paged within 8 KiB. Repeat the same pattern and options with the returned cursor. If a scan budget stops the search, narrow the pattern. Use grep to find text, then read_file for a relevant line range.",
                json!({"type":"object","required":["pattern","max_results","cursor"],"additionalProperties":false,"properties":{"pattern":{"type":"string"},"max_results":{"type":["integer","null"],"minimum":1,"maximum":4_294_967_295_u64},"cursor":{"type":["string","null"],"description":super::CURSOR}}}),
                true,
            ),
        }
    }
}
impl Default for Glob {
    fn default() -> Self {
        Self::new()
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Args {
    pattern: String,
    max_results: Option<u32>,
    cursor: Option<String>,
}
#[async_trait]
impl Tool for Glob {
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
        args.cursor = super::absent(args.cursor);
        if args.max_results == Some(0) {
            return Err(ToolError::invalid("/max_results", "limit must be positive"));
        }
        let rows = cx
            .limits
            .glob_results
            .min(args.max_results.map_or(usize::MAX, |value| value as usize));
        let mut page = Page::new(
            &("glob", &args.pattern, rows, cx.limits.search_page_bytes),
            args.cursor.as_deref(),
            cx.limits.search_page_bytes,
            rows,
        )?;
        // Glob only lists paths, so it gets its own, shorter time budget; the shared walker and budget read it here.
        let mut resources = cx.clone();
        resources.limits.search_max_elapsed = cx.limits.glob_max_elapsed;
        file_io::run(cx, move |cancel| {
            let started = resources.clock.now();
            let walked = walk::collect(
                &resources,
                &Options {
                    path: ".",
                    glob: Some(&args.pattern),
                    file_type: None,
                },
                cancel,
                started,
            )?;
            let mut budget = Budget::new(&resources, cancel, started);
            for path in walked.paths {
                if !budget.file()? {
                    break;
                }
                if !budget.account(path.metadata()?.len()) {
                    break;
                }
                let relative = path
                    .strip_prefix(resources.workspace.path())
                    .ok()
                    .and_then(|path| path.to_str())
                    .ok_or_else(|| {
                        ToolError::PathInvalid("expected a workspace UTF-8 path".into())
                    })?;
                if !page.push(relative)? {
                    break;
                }
            }
            page.finish(&budget, walked.reason, 0)
        })
        .await
    }
}
