//! Images of the active run's user message on vision routes (FR-36).
//! Earlier runs' images stay paths; image bytes are never journaled or logged.
use super::request::{Built, window_exceeded};
use crate::runner::session::{Execution, Stop};
use tqa_llm::{CompletionRequest, ContentPart};
use tqa_tools::{ImageData, ImageError, WorkspaceRoot, probe_image, read_image};
use tqa_types::{IMAGE_TOKENS, MAX_IMAGES_PER_MESSAGE};

/// Why an attached image was not sent; the fixed words shown to the model.
#[derive(Clone, Copy)]
enum Reason {
    TooLarge,
    Unreadable,
    LimitReached,
    RequestTooLarge,
    NoImageInput,
}
impl Reason {
    const fn as_str(self) -> &'static str {
        match self {
            Self::TooLarge => "too large",
            Self::Unreadable => "unreadable",
            Self::LimitReached => "limit reached",
            Self::RequestTooLarge => "request too large",
            Self::NoImageInput => "no image input on this route",
        }
    }
}

/// Readable images in attachment order, and notices keyed by attachment position.
#[derive(Default)]
struct Selection {
    images: Vec<(usize, String, ImageData)>,
    notices: Vec<(usize, String, Reason)>,
}

impl Execution<'_> {
    /// Extends the text request with the current turn's images: at most
    /// [`MAX_IMAGES_PER_MESSAGE`], within the route's request budget, each counted as
    /// [`IMAGE_TOKENS`] in the window. Images that are not sent get one fixed notice line.
    /// The request with its images, and the image tokens it carries (for its usage, CORE-U-084).
    pub(super) async fn attach_images(
        &self,
        built: Built,
    ) -> Result<(CompletionRequest, u64), Stop> {
        let Built {
            request,
            current,
            needed,
            capacity,
        } = built;
        let Some((index, attachments)) = current else {
            return Ok((request, 0));
        };
        if attachments.is_empty() {
            return Ok((request, 0));
        }
        if !self.route().vision {
            // After a switch away from a vision route, say so instead of dropping images silently.
            if !self.own_route().vision {
                return Ok((request, 0));
            }
            let selection = self.unsupported(&attachments).await?;
            return Ok((render(&request, index, &selection), 0));
        }
        let mut selection = self.select(&attachments).await?;
        let request = self.fit(&request, index, &mut selection);
        let images = u64::try_from(selection.images.len())
            .unwrap_or(u64::MAX)
            .saturating_mul(u64::from(IMAGE_TOKENS));
        if needed.saturating_add(images) > u64::from(capacity) {
            return Err(window_exceeded());
        }
        Ok((request, images))
    }

    /// Estimated window tokens of the images the next request would send, for the compaction
    /// trigger. Reads only file headers.
    pub(in crate::runner) async fn image_tokens(&self) -> Result<u64, Stop> {
        self.image_tokens_in(&self.frames).await
    }

    /// [`Self::image_tokens`] for the journal as it stood at `frames`: the images a request built then sent, so the
    /// real count of that request can be told apart from its text (CORE-U-084).
    pub(in crate::runner) async fn image_tokens_in(
        &self,
        frames: &[tqa_types::ServerFrame],
    ) -> Result<u64, Stop> {
        if !self.route().vision {
            return Ok(0);
        }
        let turn = tqa_context::project_turn(frames)?;
        if turn.current_user.is_none() {
            return Ok(0);
        }
        let mut count = 0_u64;
        for path in &turn.attachments {
            if count == MAX_IMAGES_PER_MESSAGE as u64 {
                break;
            }
            if self.blocking(path, probe_image).await?.is_ok() {
                count += 1;
            }
        }
        Ok(count.saturating_mul(u64::from(IMAGE_TOKENS)))
    }

    async fn select(&self, attachments: &[String]) -> Result<Selection, Stop> {
        let mut selection = Selection::default();
        for (position, path) in attachments.iter().enumerate() {
            if selection.images.len() == MAX_IMAGES_PER_MESSAGE {
                let probe = self.blocking(path, probe_image).await?;
                let reason = probe.map_or_else(not_sent, |_| Some(Reason::LimitReached));
                selection.note(position, path, reason);
                continue;
            }
            match self.blocking(path, read_image).await? {
                Ok(image) => selection.images.push((position, path.clone(), image)),
                Err(error) => selection.note(position, path, not_sent(error)),
            }
        }
        Ok(selection)
    }

    /// Notices for the images of a turn on a route without image input.
    async fn unsupported(&self, attachments: &[String]) -> Result<Selection, Stop> {
        let mut selection = Selection::default();
        for (position, path) in attachments.iter().enumerate() {
            let reason = match self.blocking(path, probe_image).await? {
                Ok(_) => Some(Reason::NoImageInput),
                Err(error) => not_sent(error),
            };
            selection.note(position, path, reason);
        }
        Ok(selection)
    }

    /// Runs one confined file read off the async runtime, under the run's cancel and deadline.
    async fn blocking<T: Send + 'static>(
        &self,
        path: &str,
        read: fn(&WorkspaceRoot, &str) -> Result<T, ImageError>,
    ) -> Result<Result<T, ImageError>, Stop> {
        let (workspace, path) = (self.runner.workspace.clone(), path.to_owned());
        let task = tokio::task::spawn_blocking(move || read(&workspace, &path));
        Ok(self
            .watch()
            .wait(task)
            .await?
            .unwrap_or(Err(ImageError::Unreadable)))
    }

    /// Adds images in attachment order while the exact request body fits the route's budget;
    /// the rest become `request too large` notices.
    fn fit(
        &self,
        request: &CompletionRequest,
        index: usize,
        selection: &mut Selection,
    ) -> CompletionRequest {
        // Vision was already decided from the route snapshot; the length does not depend on it.
        let mut route = self.provider().route().clone();
        route.vision = true;
        let budget = usize::try_from(self.route().request_budget()).unwrap_or(usize::MAX);
        let fits = |candidate: &CompletionRequest| {
            tqa_llm::request_body_len(&route, candidate).is_ok_and(|length| length <= budget)
        };
        let pending = std::mem::take(&mut selection.images);
        for image in pending {
            selection.images.push(image);
            if !fits(&render(request, index, selection)) {
                selection.reject_last();
            }
        }
        // A later notice can lengthen the text; drop the last image until everything fits.
        let mut rendered = render(request, index, selection);
        while !fits(&rendered) && selection.reject_last() {
            rendered = render(request, index, selection);
        }
        rendered
    }
}

impl Selection {
    fn note(&mut self, position: usize, path: &str, reason: Option<Reason>) {
        if let Some(reason) = reason {
            self.notices.push((position, path.to_owned(), reason));
        }
    }
    /// Moves the last selected image to the notices as `request too large`; false if none.
    fn reject_last(&mut self) -> bool {
        let Some((position, path, _)) = self.images.pop() else {
            return false;
        };
        self.notices.push((position, path, Reason::RequestTooLarge));
        true
    }
}

/// Non-images are ordinary attachments and get no notice.
fn not_sent(error: ImageError) -> Option<Reason> {
    match error {
        ImageError::NotImage => None,
        ImageError::TooLarge => Some(Reason::TooLarge),
        ImageError::Unreadable => Some(Reason::Unreadable),
    }
}

/// The request with the notice line appended to the current user text and the images after it.
fn render(request: &CompletionRequest, index: usize, selection: &Selection) -> CompletionRequest {
    let mut rendered = request.clone();
    let Some(message) = rendered.messages.get_mut(index) else {
        return rendered;
    };
    if let Some(line) = notice_line(&selection.notices)
        && let Some(ContentPart::Text(text)) = message.content.first_mut()
    {
        text.push_str(&line);
    }
    message.content.extend(
        selection
            .images
            .iter()
            .map(|(_, _, image)| ContentPart::Image {
                media_type: image.media_type,
                data: image.bytes.clone(),
            }),
    );
    rendered
}

/// `\n[Images not sent, data only: "/workspace/uploads/a.png": too large; ...]`, in attachment
/// order, with each path JSON-quoted as data.
fn notice_line(notices: &[(usize, String, Reason)]) -> Option<String> {
    if notices.is_empty() {
        return None;
    }
    let mut ordered: Vec<_> = notices.iter().collect();
    ordered.sort_by_key(|(position, _, _)| *position);
    let items: Vec<String> = ordered
        .into_iter()
        .map(|(_, path, reason)| {
            let quoted = serde_json::to_string(&format!("/workspace/{path}"))
                .unwrap_or_else(|_| "\"\"".into());
            format!("{quoted}: {}", reason.as_str())
        })
        .collect();
    Some(format!(
        "\n[Images not sent, data only: {}]",
        items.join("; ")
    ))
}
