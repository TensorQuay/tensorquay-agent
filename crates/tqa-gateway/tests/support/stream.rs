use super::{DEADLINE, Fixture};
use futures::{StreamExt, stream::BoxStream};
use tqa_types::{ConversationId, SandboxState, Seq, ServerFrame};

type ParsedFrame = (Option<Seq>, ServerFrame);

pub struct Stream {
    /// The sandbox state reported by the stream's opening `session.ready`.
    pub initial: SandboxState,
    /// The queue position (`ahead`) in the opening `session.ready`, when queued.
    pub initial_queue: Option<u32>,
    /// What the initial `queued` snapshot says the run waits for.
    pub initial_reason: Option<tqa_types::QueueReason>,
    chunks: BoxStream<'static, Result<Vec<u8>, reqwest::Error>>,
    buffer: Vec<u8>,
}

impl Fixture {
    pub async fn stream(
        &self,
        id: &ConversationId,
        cookie: &str,
        after: u64,
    ) -> Result<Stream, Box<dyn std::error::Error>> {
        let response = self
            .stream_client
            .get(self.url(&format!(
                "/api/v1/conversations/{id}/events?stream=1&protocol=1"
            )))
            .header("cookie", cookie)
            .header("last-event-id", after.to_string())
            .send()
            .await?;
        assert_eq!(response.status(), 200);
        assert_eq!(
            response
                .headers()
                .get("cache-control")
                .ok_or("cache header")?,
            "no-store"
        );
        assert_eq!(
            response
                .headers()
                .get("x-accel-buffering")
                .ok_or("buffer header")?,
            "no"
        );
        let mut stream = Stream {
            initial: SandboxState::Failed,
            initial_queue: None,
            initial_reason: None,
            chunks: response
                .bytes_stream()
                .map(|result| result.map(|bytes| bytes.to_vec()))
                .boxed(),
            buffer: Vec::new(),
        };
        let (_, ready) = stream.next().await?;
        let ServerFrame::SessionReady {
            conversation_id,
            sandbox,
            queue,
            ..
        } = ready
        else {
            return Err("stream did not open with session.ready".into());
        };
        assert_eq!(conversation_id, *id);
        stream.initial = sandbox.state;
        stream.initial_queue = queue.map(tqa_types::QueuePosition::ahead);
        stream.initial_reason = queue.and_then(tqa_types::QueuePosition::reason);
        Ok(stream)
    }
}

impl Stream {
    pub async fn next(&mut self) -> Result<(Option<Seq>, ServerFrame), Box<dyn std::error::Error>> {
        tokio::time::timeout(DEADLINE, self.next_record()).await?
    }

    async fn next_record(
        &mut self,
    ) -> Result<(Option<Seq>, ServerFrame), Box<dyn std::error::Error>> {
        loop {
            let Some(index) = self.buffer.windows(2).position(|pair| pair == b"\n\n") else {
                self.read_chunk().await?;
                continue;
            };
            let record = self.buffer.drain(..index + 2).collect::<Vec<_>>();
            if let Some(frame) = parse_record(&record)? {
                return Ok(frame);
            }
        }
    }

    async fn read_chunk(&mut self) -> super::TestResult {
        let chunk = self.chunks.next().await.ok_or("stream ended")??;
        self.buffer.extend_from_slice(&chunk);
        if self.buffer.len() > 4 * tqa_types::MAX_FRAME_BYTES {
            return Err("SSE fixture buffer exceeded bound".into());
        }
        Ok(())
    }

    /// Reads the next frame, which must be an unsequenced sandbox state notice.
    pub async fn state(&mut self) -> Result<SandboxState, Box<dyn std::error::Error>> {
        match self.next().await? {
            (None, ServerFrame::SessionReady { sandbox, .. }) => Ok(sandbox.state),
            (_, frame) => {
                Err(format!("expected a sandbox state notice, received {frame:?}").into())
            }
        }
    }

    pub async fn durable(&mut self) -> Result<(Seq, ServerFrame), Box<dyn std::error::Error>> {
        for _ in 0..32 {
            let (id, frame) = self.next().await?;
            if let Some(id) = id {
                return Ok((id, frame));
            }
            if let ServerFrame::Error { code, .. } = frame {
                return Err(format!("stream error: {code}").into());
            }
            if !matches!(frame, ServerFrame::SessionReady { .. }) {
                return Err("runtime-only control frame reached a client".into());
            }
        }
        Err("too many unsequenced frames".into())
    }
}

fn parse_record(bytes: &[u8]) -> Result<Option<ParsedFrame>, Box<dyn std::error::Error>> {
    let text = std::str::from_utf8(bytes)?;
    let id = text
        .lines()
        .find_map(|line| line.strip_prefix("id:"))
        .map(|value| value.trim().parse::<u64>().map(Seq::from))
        .transpose()?;
    text.lines()
        .find_map(|line| line.strip_prefix("data:"))
        .map(|data| {
            ServerFrame::parse(data.trim_start().as_bytes())
                .map(|frame| (id, frame))
                .map_err(Into::into)
        })
        .transpose()
}
