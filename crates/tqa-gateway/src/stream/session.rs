use crate::{GatewayError, app::State, delivery::ReplayCursor, events::Subscription};
use std::{collections::VecDeque, sync::Arc};
use tqa_types::{ConversationId, Seq, ServerFrame};

pub(super) struct Session {
    state: Arc<State>,
    conversation: ConversationId,
    subscription: Subscription,
    cursor: ReplayCursor,
    watermark: i64,
    replay: bool,
    pending: VecDeque<ServerFrame>,
    closed: bool,
    backend: tqa_types::SandboxBackend,
    replayed: bool,
}

impl Session {
    pub async fn open(
        state: Arc<State>,
        conversation: ConversationId,
        after: Seq,
        opening: Vec<ServerFrame>,
        backend: tqa_types::SandboxBackend,
    ) -> Result<Self, GatewayError> {
        // Subscribe before taking the durable watermark: the live queue overlaps replay safely.
        let subscription = state.events.subscribe(&conversation)?;
        let watermark = state.events.watermark(&conversation).await?;
        // The task again, now that the stream holds its node: a delete commits before it closes the node, so an open
        // that subscribed after the close finds it deleted here, and one that subscribed before has its node closed.
        if !active(&state, &conversation).await? {
            return Err(GatewayError::NotFound);
        }
        state
            .events
            .open_gate()
            .pass(crate::events::OpenPoint::Subscribed)
            .await;
        let cursor = ReplayCursor::new(after, watermark)?;
        Ok(Self {
            state,
            conversation,
            subscription,
            cursor,
            watermark: crate::wire::db_sequence(watermark)?,
            replay: true,
            pending: VecDeque::from(opening),
            closed: false,
            backend,
            replayed: false,
        })
    }

    pub async fn next(&mut self) -> Option<ServerFrame> {
        if self.closed {
            return None;
        }
        match self.next_checked().await {
            Ok(frame) => Some(frame),
            Err(error) => {
                self.closed = true;
                Some(error.frame())
            }
        }
    }

    async fn next_checked(&mut self) -> Result<ServerFrame, GatewayError> {
        loop {
            if let Some(frame) = self.pending.pop_front() {
                return Ok(frame);
            }
            if self.replay {
                self.advance_replay().await?;
                continue;
            }
            let closed = self.subscription.closed();
            let frame = tokio::select! {
                biased;
                () = self.state.cancel.cancelled() => return Err(GatewayError::Cancelled),
                // The task was deleted: it is gone for its readers at once, as for a new request.
                () = closed.cancelled() => return Err(GatewayError::NotFound),
                frame = self.subscription.receiver.recv() => frame.map_err(|_| crate::delivery::gap())?,
            };
            let deliver =
                crate::wire::sequence(&frame).map_or(Ok(true), |seq| self.cursor.live(seq))?;
            if deliver {
                return Ok((*frame).clone());
            }
        }
    }

    async fn advance_replay(&mut self) -> Result<(), GatewayError> {
        if crate::wire::db_sequence(self.cursor.last())? == self.watermark {
            self.cursor.finish_replay()?;
            self.replay = false;
            if self.replayed {
                // Replayed history may end in an old state; close it with the current snapshot (answers-050).
                let (ready, _) =
                    super::snapshot::ready(&self.state, &self.conversation, self.backend, true)
                        .await?;
                self.pending.push_back(ready);
            }
            Ok(())
        } else {
            self.load_page().await
        }
    }

    async fn load_page(&mut self) -> Result<(), GatewayError> {
        let page = self
            .state
            .events
            .read_through(
                &self.conversation,
                self.cursor.last(),
                self.watermark,
                self.state.config.limits.page_size,
            )
            .await?;
        if page.is_empty() {
            return Err(crate::delivery::gap());
        }
        for frame in page {
            self.cursor
                .replay(crate::wire::sequence(&frame).ok_or(GatewayError::StoreUnavailable)?)?;
            self.pending.push_back(frame);
            self.replayed = true;
        }
        Ok(())
    }
}

/// Whether the task exists and is not deleted.
async fn active(state: &State, conversation: &ConversationId) -> Result<bool, GatewayError> {
    let id = crate::conversations::uuid(conversation)?;
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        Ok(sqlx::query_scalar!(
            "SELECT EXISTS(SELECT 1 FROM conversations WHERE id=$1 AND state<>'deleted') AS \"active!\"",
            id
        )
        .fetch_one(&state.pool)
        .await?)
    })
    .await
}
