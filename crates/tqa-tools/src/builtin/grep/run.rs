//! Collect sorted paths, run the owned worker pool, then render one bounded page.
use super::{
    Args,
    pool::{self, Query},
};
use crate::{
    ToolContext, ToolError, ToolOutput,
    builtin::search::{
        budget::Budget,
        page::Page,
        walk::{self, Options},
    },
};
use grep_regex::RegexMatcher;
use tokio_util::sync::CancellationToken;

pub(super) fn run(
    args: &Args,
    matcher: &RegexMatcher,
    mut page: Page,
    cx: &ToolContext,
    cancel: &CancellationToken,
) -> Result<ToolOutput, ToolError> {
    let started = cx.clock.now();
    let walked = walk::collect(
        cx,
        &Options {
            path: &args.path,
            glob: args.glob.as_deref(),
            file_type: args.file_type.as_deref(),
        },
        cancel,
        started,
    )?;
    let mut budget = Budget::new(cx, cancel, started);
    // A stopped parallel traversal has no certified path prefix. Return an empty prefix.
    let binaries = if walked.reason.is_some() {
        0
    } else {
        pool::search(
            &Query {
                args,
                matcher,
                cx,
                cancel,
                started,
                #[cfg(test)]
                probe: None,
            },
            &walked.paths,
            &mut page,
            &mut budget,
        )?
    };
    page.finish(&budget, walked.reason, binaries)
}
