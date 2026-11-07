use super::{DEADLINE, Fixture, TestResult};
use futures::{SinkExt, StreamExt};
use secrecy::ExposeSecret;
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream,
    tungstenite::{Message, client::IntoClientRequest},
};
use tqa_gateway::Conversation;
use tqa_types::{ClientFrame, ErrorCode, ServerFrame};

pub struct Runtime {
    pub sandbox: tqa_gateway::SandboxId,
    pub config: serde_json::Value,
    socket: WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>,
}

impl Fixture {
    pub fn forget_runtime(&self, id: &tqa_types::ConversationId) -> TestResult {
        self.credentials
            .lock()
            .map_err(|_| "credentials lock")?
            .remove(id.as_str());
        Ok(())
    }

    pub async fn runtime(
        &self,
        created: &Conversation,
    ) -> Result<Runtime, Box<dyn std::error::Error>> {
        Box::pin(self.runtime_for(&created.id)).await
    }

    /// Connects the fake runtime of the sandbox the engine now holds for `id`, with that sandbox's own token: for a
    /// sandbox the gateway made by itself (a hand-over's), whose runtime the test must register before its start
    /// deadline. It never asks the gateway for a sandbox, as [`Self::runtime_for`] does when it has no token.
    pub async fn runtime_of_sandbox(
        &self,
        id: &tqa_types::ConversationId,
    ) -> Result<Runtime, Box<dyn std::error::Error>> {
        let token = self
            .backend
            .live_token(id)
            .await?
            .ok_or("no sandbox for the task")?;
        self.credentials
            .lock()
            .map_err(|_| "lock")?
            .insert(id.to_string(), token);
        Box::pin(self.runtime_at(id, tqa_types::PROTOCOL_REVISION)).await
    }

    /// Connects the fake runtime of a conversation known only by id (for example, one created over HTTP).
    pub async fn runtime_for(
        &self,
        id: &tqa_types::ConversationId,
    ) -> Result<Runtime, Box<dyn std::error::Error>> {
        Box::pin(self.runtime_at(id, tqa_types::PROTOCOL_REVISION)).await
    }

    /// Connects the fake runtime announcing protocol `revision`, as an older runtime image would.
    pub async fn runtime_at(
        &self,
        id: &tqa_types::ConversationId,
        revision: u16,
    ) -> Result<Runtime, Box<dyn std::error::Error>> {
        let url = self.url("/runtime/link").replacen("http:", "ws:", 1);
        let mut request = url.into_client_request()?;
        let cached = self
            .credentials
            .lock()
            .map_err(|_| "lock")?
            .get(id.as_str())
            .cloned();
        let token = if let Some(token) = cached {
            token
        } else {
            self.gateway.sandboxes().ensure(id).await?;
            let token = self
                .backend
                .token(id)
                .ok_or("manager did not create runtime token")?;
            self.credentials
                .lock()
                .map_err(|_| "lock")?
                .insert(id.to_string(), token.clone());
            token
        };
        // Like the real runtime, state the protocol revision; without it the link refuses the runtime (GW-I-050).
        request.headers_mut().insert(
            tqa_types::PROTOCOL_REVISION_HEADER,
            revision.to_string().parse()?,
        );
        request.headers_mut().insert(
            "authorization",
            format!("Bearer {}", token.expose_secret()).parse()?,
        );
        let (socket, _) =
            tokio::time::timeout(DEADLINE, tokio_tungstenite::connect_async(request)).await??;
        tokio::time::timeout(DEADLINE, async {
            while !self.gateway.readiness().is_ready(id).await {
                tokio::task::yield_now().await;
            }
        })
        .await?;
        let (_, sandbox) = self
            .gateway
            .runtime_tokens()
            .resolve(&token, std::net::Ipv4Addr::LOCALHOST.into())
            .await?
            .ok_or("missing runtime binding")?;
        let mut runtime = Runtime {
            sandbox,
            socket,
            config: serde_json::Value::Null,
        };
        runtime.config = serde_json::from_str(&runtime.text().await?)?;
        // An error, not a panic: a test that dials a sandbox being retired (GW-I-251) tries again.
        if runtime.config["type"] != "session.config" {
            return Err(format!(
                "configuration must precede every action, got {}",
                runtime.config["type"]
            )
            .into());
        }
        Ok(runtime)
    }
}

impl Runtime {
    pub async fn pong(&mut self) -> TestResult {
        self.socket.send(Message::Pong(Vec::new().into())).await?;
        self.roundtrip().await
    }

    pub async fn answer_ping(&mut self) -> TestResult {
        match tokio::time::timeout(DEADLINE, self.socket.next())
            .await?
            .ok_or("runtime closed")??
        {
            Message::Ping(bytes) => self.socket.send(Message::Pong(bytes)).await?,
            other => return Err(format!("expected ping, received {other:?}").into()),
        }
        self.roundtrip().await
    }

    pub async fn roundtrip(&mut self) -> TestResult {
        self.socket.send(Message::Ping(Vec::new().into())).await?;
        tokio::time::timeout(DEADLINE, async {
            loop {
                match self.socket.next().await.ok_or("runtime closed")?? {
                    Message::Pong(_) => return Ok(()),
                    Message::Ping(bytes) => self.socket.send(Message::Pong(bytes)).await?,
                    other => return Err(format!("expected pong, received {other:?}").into()),
                }
            }
        })
        .await?
    }

    /// Sends `frame` as a live runtime does, first answering every ping the gateway already sent: a sender that never
    /// reads would miss its pong deadline (90 s) and lose the link mid-run (PERF-002's long bursts).
    pub async fn send_live(&mut self, frame: &ServerFrame) -> TestResult {
        while let Some(message) = futures::FutureExt::now_or_never(self.socket.next()).flatten() {
            match message? {
                Message::Ping(bytes) => self.socket.send(Message::Pong(bytes)).await?,
                Message::Pong(_) => {}
                other => return Err(format!("unexpected frame while sending: {other:?}").into()),
            }
        }
        self.send(frame).await
    }

    pub async fn send(&mut self, frame: &ServerFrame) -> TestResult {
        tokio::time::timeout(
            DEADLINE,
            self.socket
                .send(Message::Text(serde_json::to_string(frame)?.into())),
        )
        .await??;
        Ok(())
    }

    async fn text(&mut self) -> Result<String, Box<dyn std::error::Error>> {
        tokio::time::timeout(DEADLINE, async {
            loop {
                match self.socket.next().await.ok_or("link ended")?? {
                    Message::Text(text) => return Ok(text.to_string()),
                    Message::Ping(bytes) => self.socket.send(Message::Pong(bytes)).await?,
                    Message::Pong(_) => (),
                    _ => return Err("unexpected runtime frame".into()),
                }
            }
        })
        .await?
    }

    /// The next action if one arrives within `wait`; `None` otherwise (the link stays usable).
    pub async fn try_action(
        &mut self,
        wait: std::time::Duration,
    ) -> Result<Option<ClientFrame>, Box<dyn std::error::Error>> {
        loop {
            let Ok(next) = tokio::time::timeout(wait, self.socket.next()).await else {
                return Ok(None);
            };
            match next.ok_or("link ended")?? {
                Message::Text(text) => return Ok(Some(ClientFrame::parse(text.as_bytes())?)),
                Message::Ping(bytes) => self.socket.send(Message::Pong(bytes)).await?,
                Message::Pong(_) => (),
                _ => return Err("unexpected runtime frame".into()),
            }
        }
    }

    pub async fn action(&mut self) -> Result<ClientFrame, Box<dyn std::error::Error>> {
        Ok(ClientFrame::parse(self.text().await?.as_bytes())?)
    }
    pub async fn error(&mut self) -> Result<ErrorCode, Box<dyn std::error::Error>> {
        match ServerFrame::parse(self.text().await?.as_bytes())? {
            ServerFrame::Error { code, .. } => Ok(code),
            _ => Err("expected classified runtime error".into()),
        }
    }
    /// Closes the link from the runtime's side, as a leaving runtime does.
    pub async fn close(&mut self) -> TestResult {
        tokio::time::timeout(DEADLINE, self.socket.close(None)).await??;
        Ok(())
    }
    pub async fn closed(&mut self) -> TestResult {
        let next = tokio::time::timeout(DEADLINE, self.socket.next()).await?;
        assert!(
            matches!(next, None | Some(Ok(Message::Close(_)))),
            "expected the gateway to close the link, received {next:?}"
        );
        Ok(())
    }
}
