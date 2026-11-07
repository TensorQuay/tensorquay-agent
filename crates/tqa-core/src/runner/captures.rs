//! A `computer` call's captures for the person's record (ADR-0039 section 9 and "The record"): each is uploaded to
//! the conversation's screenshot store and journaled as `screen.capture` only once the store holds it. One the store
//! refuses, or doesn't answer in time, is dropped without a word of ours (the gateway tells the owner when its quota
//! is full); its phase reads false in the result's `evidence`.
use super::session::{Execution, Stop};
use std::time::Duration;
use tqa_tools::ToolImage;
use tqa_types::{CapturePhase, Event, ScreenEvidence, ToolCallId};

/// What the model reads after an action whose evidence wasn't all kept; a client reads `evidence` instead.
const EVIDENCE_NOT_KEPT: &str = "(evidence not kept)";
/// What the model reads after a screenshot whose capture wasn't kept: it did get the image, only the record lacks it.
const SCREENSHOT_NOT_KEPT: &str = "(not kept for the record)";

impl Execution<'_> {
    /// Stores one capture and journals its `screen.capture` once the store holds it; whether it was kept. Without a
    /// store nothing is uploaded.
    pub(super) async fn capture(
        &mut self,
        call: &ToolCallId,
        phase: CapturePhase,
        image: &ToolImage,
        wait: Duration,
    ) -> Result<bool, Stop> {
        let Some(store) = self.runner.screen_store.clone() else {
            return Ok(false);
        };
        let id = crate::screenshot_id(&image.png);
        let stored = self
            .watch()
            .wait(store.store(&id, &image.png, wait))
            .await?;
        if let Err(why) = stored {
            tracing::debug!(?phase, ?why, "a screenshot wasn't kept for the record");
            return Ok(false);
        }
        self.emit(Event::ScreenCapture {
            call_id: call.clone(),
            phase,
            id: id.clone(),
            width: image.width,
            height: image.height,
            bytes: u64::try_from(image.png.len()).unwrap_or(u64::MAX),
            sha256: id,
            sent_to_model: phase == CapturePhase::Screenshot,
            marked: image.marked,
        })
        .await?;
        Ok(true)
    }
}

/// The line the model is told when a capture that was due wasn't kept, if any: an action's evidence, or the record's
/// copy of a screenshot the model itself was sent.
pub(super) fn not_kept_note(evidence: Option<&ScreenEvidence>) -> Option<&'static str> {
    match evidence? {
        ScreenEvidence {
            before: Some(before),
            after,
        } => (!before || !after).then_some(EVIDENCE_NOT_KEPT),
        ScreenEvidence {
            before: None,
            after,
        } => (!after).then_some(SCREENSHOT_NOT_KEPT),
    }
}
