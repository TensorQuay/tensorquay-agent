//! Ordered durable replay and ephemeral control frames share exactly one socket owner.
use super::{Link, Socket};
use crate::{RuntimeError, history::sequence};
use futures::{SinkExt as _, stream::SplitSink};
use tokio_tungstenite::tungstenite::Message;
use tqa_types::{Seq, ServerFrame};
type Writer = SplitSink<Socket, Message>;
impl Link {
    pub(super) async fn deliver(
        &mut self,
        write: &mut Writer,
        frame: &ServerFrame,
        cursor: &mut Seq,
    ) -> Result<(), RuntimeError> {
        let seq = sequence(frame).ok_or(RuntimeError::History)?;
        if seq <= *cursor {
            return Ok(());
        }
        if cursor.next() == Some(seq) {
            return self.write_frame(write, frame, cursor).await;
        }
        self.replay(write, cursor).await?;
        if *cursor < seq {
            return Err(RuntimeError::History);
        }
        Ok(())
    }
    pub(super) async fn replay(
        &mut self,
        write: &mut Writer,
        cursor: &mut Seq,
    ) -> Result<(), RuntimeError> {
        loop {
            let page = self.journal.records_after(*cursor).await?;
            if page.is_empty() {
                return Ok(());
            }
            for (_, frame) in page {
                self.write_frame(write, &frame, cursor).await?;
            }
        }
    }
    pub(super) async fn write_frame(
        &mut self,
        write: &mut Writer,
        frame: &ServerFrame,
        cursor: &mut Seq,
    ) -> Result<(), RuntimeError> {
        let seq = sequence(frame).ok_or(RuntimeError::History)?;
        if seq <= *cursor {
            return Ok(());
        }
        if cursor.next() != Some(seq) {
            return Err(RuntimeError::History);
        }
        #[cfg(feature = "testing")]
        if Seq::from(
            self.testing
                .fail_delivery_at
                .load(std::sync::atomic::Ordering::SeqCst),
        ) == seq
        {
            self.testing
                .fail_delivery_at
                .store(0, std::sync::atomic::Ordering::SeqCst);
            return Err(RuntimeError::Link);
        }
        #[cfg(feature = "testing")]
        self.testing.before_write.checkpoint(&self.shutdown).await;
        self.write_control(write, frame).await?;
        *cursor = seq;
        Ok(())
    }
    pub(super) async fn write_control(
        &mut self,
        write: &mut Writer,
        frame: &ServerFrame,
    ) -> Result<(), RuntimeError> {
        let text = serde_json::to_string(frame).map_err(|_| RuntimeError::Frame)?;
        if text.len() > tqa_types::MAX_FRAME_BYTES {
            return Err(RuntimeError::Frame);
        }
        self.write_message(write, Message::Text(text.into())).await
    }
    pub(super) async fn write_message(
        &mut self,
        write: &mut Writer,
        message: Message,
    ) -> Result<(), RuntimeError> {
        self.time
            .wait(self.limits.io_timeout, &self.shutdown, write.send(message))
            .await?
            .map_err(|_| RuntimeError::Link)
    }
}
