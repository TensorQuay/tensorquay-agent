//! Reject incompatible configuration explicitly before fetching history or starting work.
use super::{Link, Socket};
use crate::{RuntimeError, session::Session};
use futures::{SinkExt as _, StreamExt as _};
use tokio_tungstenite::tungstenite::Message;
use tqa_types::{ClientFrame, ErrorCode, FrameError, Seq, ServerFrame};

impl Link {
    pub(super) async fn configuration(
        &mut self,
        socket: &mut Socket,
    ) -> Result<(Session, Seq), RuntimeError> {
        let first = self
            .time
            .wait(self.limits.io_timeout, &self.shutdown, socket.next())
            .await?;
        let Some(Ok(Message::Text(first))) = first else {
            return Err(RuntimeError::Frame);
        };
        match ClientFrame::parse(first.as_bytes()) {
            Ok(frame) => Session::parse(frame),
            Err(error) => {
                self.reject_configuration(socket, &error).await;
                Err(RuntimeError::Configuration("unsupported session.config"))
            }
        }
    }

    async fn reject_configuration(&mut self, socket: &mut Socket, error: &FrameError) {
        // Report only a bounded field identifier; serde details can contain received values.
        let path = if error.path.len() <= 128
            && error
                .path
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"$._[]".contains(&byte))
        {
            &error.path
        } else {
            "$"
        };
        let frame = ServerFrame::Error {
            target: None,
            seq: None,
            code: ErrorCode::UnsupportedProtocol,
            message: format!(
                "unsupported session.config field or value at {path} (runtime protocol revision {})",
                tqa_types::PROTOCOL_REVISION
            ),
            retryable: false,
        };
        let Ok(text) = serde_json::to_string(&frame) else {
            tracing::debug!("configuration error could not be encoded");
            return;
        };
        // The refusal is final: the error, then a close, so the gateway need not wait for readiness.
        for message in [Message::Text(text.into()), Message::Close(None)] {
            if !matches!(
                self.time
                    .wait(self.limits.io_timeout, &self.shutdown, socket.send(message))
                    .await,
                Ok(Ok(()))
            ) {
                tracing::debug!("configuration error could not be delivered on the runtime link");
                return;
            }
        }
    }
}
