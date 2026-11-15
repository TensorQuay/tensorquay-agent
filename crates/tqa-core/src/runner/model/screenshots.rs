//! The run's `computer` screenshots in the model's request (ADR-0039 section 3, CORE-U-081): the latest
//! [`SENT_SCREENSHOTS`] go as images on their own tool results, on a route with images, each counted as
//! [`IMAGE_TOKENS`]; any other screenshot result, or one whose file is gone, gets one fixed line instead. The images
//! never crowd the text out: fewer are sent when the window or the route's request budget has no room for them. Only
//! the latest screenshot carries its marks, and only while no later look replaced its snapshot: the older ones go as
//! their clean copies, so a mark's number the model reads always means the current snapshot's element.
use std::collections::BTreeSet;

use serde_json::Value;
use tqa_llm::{CompletionRequest, ContentPart, Role};
use tqa_types::{IMAGE_TOKENS, ImageMediaType, ToolCallId};

use crate::SENT_SCREENSHOTS;
use crate::runner::session::{Execution, Stop};

/// What the model reads for a screenshot it isn't sent.
pub(super) const NOT_SHOWN: &str =
    "\n[This screenshot is no longer shown. Take another to see the screen as it is now.]";

/// The built-in tool whose screenshots these are.
const COMPUTER: &str = "computer";

impl Execution<'_> {
    /// The request as it is sent: the turn's attached images first, then the run's latest screenshots in the room the
    /// text and those images leave. With it, the image tokens it carries, the screenshots' included: they're images in
    /// the request like any other, so the route's density reads them out of the prompt too (CORE-U-084).
    pub(super) async fn with_images(
        &self,
        built: super::request::Built,
    ) -> Result<(CompletionRequest, u64), Stop> {
        let (needed, capacity) = (built.needed, built.capacity);
        let (request, attached) = self.attach_images(built).await?;
        let (request, screenshots) = self
            .attach_screenshots(request, needed.saturating_add(attached), capacity)
            .await?;
        // Then the pictures the agent read from its workspace, in the room left (CORE-U-090).
        let (request, pictures) = self
            .attach_pictures(
                request,
                needed.saturating_add(attached).saturating_add(screenshots),
                capacity,
            )
            .await?;
        Ok((
            request,
            attached
                .saturating_add(screenshots)
                .saturating_add(pictures),
        ))
    }

    /// `request` with the run's latest screenshots on their tool results and the fixed line on the others, and the
    /// image tokens of the screenshots sent. `used` is the window's estimated input tokens so far (the text and any
    /// attached images) and `capacity` its input room.
    pub(super) async fn attach_screenshots(
        &self,
        mut request: CompletionRequest,
        used: u64,
        capacity: u32,
    ) -> Result<(CompletionRequest, u64), Stop> {
        let computer = self.runner.names.to_model(COMPUTER);
        let shots = screenshot_results(&request, computer);
        if shots.is_empty() {
            return Ok((request, 0));
        }
        // The latest screenshot keeps its marks unless an observe came after it (its snapshot is no longer the latest).
        let marked = shots
            .last()
            .map(|(index, _)| *index)
            .filter(|index| !looked_since(&request, *index, computer));
        let room = u64::from(capacity).saturating_sub(used) / u64::from(IMAGE_TOKENS);
        let mut allowed = usize::try_from(room)
            .unwrap_or(usize::MAX)
            .min(SENT_SCREENSHOTS);
        if !self.route().vision {
            allowed = 0;
        }
        // Newest first, so the latest are the ones sent.
        let mut sent: Vec<(usize, Vec<u8>)> = Vec::new();
        for (index, call) in shots.iter().rev() {
            if sent.len() == allowed {
                break;
            }
            if let Some(png) = self.screenshot(call, marked != Some(*index)).await {
                sent.push((*index, png));
            }
        }
        let budget = usize::try_from(self.route().request_budget()).unwrap_or(usize::MAX);
        // Vision was already decided from the session's route; the length doesn't depend on it (as for attachments).
        let mut route = self.provider().route().clone();
        route.vision = true;
        loop {
            let rendered = render(&request, &shots, &sent);
            let fits =
                tqa_llm::request_body_len(&route, &rendered).is_ok_and(|length| length <= budget);
            if fits || sent.pop().is_none() {
                request = rendered;
                break;
            }
        }
        // Only those still sent after the budget's cut count (CORE-U-084).
        let tokens = u64::try_from(sent.len())
            .unwrap_or(u64::MAX)
            .saturating_mul(u64::from(IMAGE_TOKENS));
        Ok((request, tokens))
    }

    /// A screenshot's bytes while the run's store still holds them: marked, or its clean copy (`plain`).
    async fn screenshot(&self, call: &str, plain: bool) -> Option<Vec<u8>> {
        let store = self.runner.screenshots.as_ref()?;
        let run = self.context.run_id.as_ref()?;
        let call: ToolCallId = call.parse().ok()?;
        store.read(run, &call, plain).await
    }

    /// The window tokens the next request's screenshots take, for the compaction trigger: its latest few, on a route
    /// with images.
    pub(in crate::runner) fn screenshot_tokens(&self) -> Result<u64, Stop> {
        if !self.route().vision || self.runner.screenshots.is_none() {
            return Ok(0);
        }
        // The journal names tools by their registry names; the request by their model-facing ones.
        let shots = tqa_context::project(&self.frames)?
            .iter()
            .flat_map(|message| &message.tool_calls)
            .filter(|call| call.name == COMPUTER && is_screenshot(&call.arguments))
            .count()
            .min(SENT_SCREENSHOTS);
        Ok(u64::try_from(shots)
            .unwrap_or(u64::MAX)
            .saturating_mul(u64::from(IMAGE_TOKENS)))
    }
}

/// The tool-result messages that answer a `computer` screenshot, in order: (message index, call id).
fn screenshot_results(request: &CompletionRequest, name: &str) -> Vec<(usize, String)> {
    let calls: BTreeSet<&str> = request
        .messages
        .iter()
        .flat_map(|message| &message.tool_calls)
        .filter(|call| call.name == name && is_screenshot(&call.arguments))
        .map(|call| call.id.as_str())
        .collect();
    request
        .messages
        .iter()
        .enumerate()
        .filter(|(_, message)| message.role == Role::Tool)
        .filter_map(|(index, message)| {
            let id = message.tool_call_id.as_deref()?;
            calls.contains(id).then(|| (index, id.to_owned()))
        })
        .collect()
}

/// Whether a `computer` observe came after the message at `index`: a newer snapshot than that screenshot's.
fn looked_since(request: &CompletionRequest, index: usize, name: &str) -> bool {
    request
        .messages
        .iter()
        .skip(index + 1)
        .flat_map(|message| &message.tool_calls)
        .any(|call| {
            call.name == name
                && serde_json::from_str::<Value>(&call.arguments)
                    .is_ok_and(|arguments| arguments["action"] == "observe")
        })
}

/// Whether a `computer` call's arguments ask for a screenshot.
fn is_screenshot(arguments: &str) -> bool {
    serde_json::from_str::<Value>(arguments)
        .is_ok_and(|arguments| arguments["action"] == "screenshot")
}

/// The request with `sent`'s images on their results and the fixed line on every other screenshot result.
fn render(
    request: &CompletionRequest,
    shots: &[(usize, String)],
    sent: &[(usize, Vec<u8>)],
) -> CompletionRequest {
    let mut rendered = request.clone();
    for (index, _) in shots {
        let Some(message) = rendered.messages.get_mut(*index) else {
            continue;
        };
        match sent.iter().find(|(shown, _)| shown == index) {
            Some((_, png)) => message.content.push(ContentPart::Image {
                media_type: ImageMediaType::Png,
                data: png.clone(),
            }),
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
