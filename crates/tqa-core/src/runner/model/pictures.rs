//! Pictures the agent read from its workspace in the model's request (CORE-U-090): `read_file` names a picture as one
//! (TOOL-U-143), and on a route that reads pictures the latest [`SENT_PICTURES`] go as images on their own tool results,
//! each counted as [`IMAGE_TOKENS`], within the window and the route's request budget, as screenshots do. Any other
//! picture result gets one fixed line, so the model never says it can't see a picture it was shown, or thinks it was
//! shown one it wasn't. The owner's case (13 Nov 2026): the agent converted a photo to JPEG and still couldn't see it.
use std::collections::BTreeMap;

use serde_json::Value;
use tqa_llm::{CompletionRequest, ContentPart, Role};
use tqa_tools::{ImageData, ImageError, PICTURE_SUMMARY, read_workspace_image};
use tqa_types::IMAGE_TOKENS;

use crate::runner::session::{Execution, Stop};

/// How many of the run's latest pictures go to the model as images.
pub(crate) const SENT_PICTURES: usize = 3;

/// What the model reads for a picture it isn't sent, on a route that reads pictures.
pub(super) const NOT_SHOWN: &str =
    "\n[This picture is no longer shown. Read it again with read_file to see it.]";
/// What the model reads on a route that doesn't read pictures.
pub(super) const NO_VISION: &str = "\n[This model doesn't read pictures, so the picture isn't shown; describe what you need from it in words.]";

/// The built-in tool whose results these are.
const READ_FILE: &str = "read_file";

impl Execution<'_> {
    /// `request` with the latest pictures on their results and the fixed line on the others, and the image tokens of
    /// the pictures sent. `used` is the window's estimated input tokens so far and `capacity` its input room.
    pub(super) async fn attach_pictures(
        &self,
        mut request: CompletionRequest,
        used: u64,
        capacity: u32,
    ) -> Result<(CompletionRequest, u64), Stop> {
        let name = self.runner.names.to_model(READ_FILE);
        let pictures = picture_results(&request, name);
        if pictures.is_empty() {
            return Ok((request, 0));
        }
        let vision = self.route().vision;
        let room = u64::from(capacity).saturating_sub(used) / u64::from(IMAGE_TOKENS);
        let allowed = if vision {
            usize::try_from(room)
                .unwrap_or(usize::MAX)
                .min(SENT_PICTURES)
        } else {
            0
        };
        // Newest first, so the latest are the ones sent.
        let mut sent: Vec<(usize, ImageData)> = Vec::new();
        for (index, path) in pictures.iter().rev() {
            if sent.len() == allowed {
                break;
            }
            if let Ok(image) = self.read_picture(path).await? {
                sent.push((*index, image));
            }
        }
        let budget = usize::try_from(self.route().request_budget()).unwrap_or(usize::MAX);
        let mut route = self.provider().route().clone();
        route.vision = true;
        let missing = if vision { NOT_SHOWN } else { NO_VISION };
        loop {
            let rendered = render(&request, &pictures, &sent, missing);
            let fits =
                tqa_llm::request_body_len(&route, &rendered).is_ok_and(|length| length <= budget);
            if fits || sent.pop().is_none() {
                request = rendered;
                break;
            }
        }
        let tokens = u64::try_from(sent.len())
            .unwrap_or(u64::MAX)
            .saturating_mul(u64::from(IMAGE_TOKENS));
        Ok((request, tokens))
    }

    /// One confined read of a workspace picture off the async runtime, under the run's cancel and deadline.
    async fn read_picture(&self, path: &str) -> Result<Result<ImageData, ImageError>, Stop> {
        let (workspace, path) = (self.runner.workspace.clone(), relative(path));
        let task = tokio::task::spawn_blocking(move || read_workspace_image(&workspace, &path));
        Ok(self
            .watch()
            .wait(task)
            .await?
            .unwrap_or(Err(ImageError::Unreadable)))
    }

    /// The window tokens the next request's pictures take, for the compaction trigger.
    pub(in crate::runner) fn picture_tokens(&self) -> Result<u64, Stop> {
        if !self.route().vision {
            return Ok(0);
        }
        let projected = tqa_context::project(&self.frames)?;
        let pictures = projected
            .iter()
            .filter(|message| message.role == tqa_context::Role::Tool)
            .filter(|message| message.text.starts_with(PICTURE_SUMMARY))
            .count()
            .min(SENT_PICTURES);
        Ok(u64::try_from(pictures)
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

/// The tool results that answer a `read_file` of a picture, in order: (message index, the path read).
fn picture_results(request: &CompletionRequest, name: &str) -> Vec<(usize, String)> {
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
        .filter(|(_, message)| {
            message.content.iter().any(
                |part| matches!(part, ContentPart::Text(text) if text.starts_with(PICTURE_SUMMARY)),
            )
        })
        .filter_map(|(index, message)| {
            let path = paths.get(message.tool_call_id.as_deref()?)?;
            Some((index, path.clone()))
        })
        .collect()
}

/// The request with `sent`'s images on their results and `missing` on every other picture result.
fn render(
    request: &CompletionRequest,
    pictures: &[(usize, String)],
    sent: &[(usize, ImageData)],
    missing: &str,
) -> CompletionRequest {
    let mut rendered = request.clone();
    for (index, _) in pictures {
        let Some(message) = rendered.messages.get_mut(*index) else {
            continue;
        };
        match sent.iter().find(|(shown, _)| shown == index) {
            Some((_, image)) => message.content.push(ContentPart::Image {
                media_type: image.media_type,
                data: image.bytes.clone(),
            }),
            None => {
                if let Some(ContentPart::Text(text)) = message
                    .content
                    .iter_mut()
                    .rev()
                    .find(|part| matches!(part, ContentPart::Text(_)))
                {
                    text.push_str(missing);
                }
            }
        }
    }
    rendered
}
