//! A channel's woken runs spending tokens, for the budget's tests: a fake model whose every answer reports 40 tokens,
//! its route, a model call through the relay, and the channel's spend.
#![allow(
    dead_code,
    reason = "each channel test file uses its own share of these"
)]
use crate::channel_agent::{Agent, Result};
use crate::support::Fixture;
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use tqa_testkit::{FakeModelServer, Script, Scripted};
use uuid::Uuid;

/// A model server answering `answers` requests, each reporting 40 tokens (30 in, 10 out).
pub async fn model(answers: usize) -> Result<FakeModelServer> {
    let script = Script::new(vec![
        Scripted::Text("done".into()),
        Scripted::Usage(tqa_llm::Usage {
            prompt_tokens: 30,
            completion_tokens: 10,
            cached_tokens: None,
            reasoning_tokens: None,
        }),
    ]);
    Ok(FakeModelServer::start(vec![script; answers], Arc::new(tqa_llm::TokioSleeper)).await?)
}

pub fn registry(server: &FakeModelServer) -> Result<tqa_llm::Registry> {
    Ok(tqa_llm::Registry::from_toml(&format!(
        "[[routes]]\nname='local'\nprotocol='openai_chat'\nbase_url='{}'\nmodel='pinned-model'\nauth={{kind='bearer'}}\nkey_ref='secrets/local'\nstream_usage=true",
        server.base_url()
    ))?)
}

/// A model call from the agent's run, through the relay: the status and the refusal's code.
pub async fn model_call(f: &Fixture, agent: &Agent) -> Result<(u16, Value)> {
    let response = f
        .client
        .post(f.url("/relay/r/local/chat/completions"))
        .bearer_auth(agent.token(f)?)
        .json(
            &json!({"model": "m", "messages": [{"role": "user", "content": "hi"}], "stream": true}),
        )
        .send()
        .await?;
    let status = response.status().as_u16();
    let text = response.text().await?;
    Ok((
        status,
        serde_json::from_str::<Value>(&text)
            .map_or(Value::Null, |body| body["error"]["code"].clone()),
    ))
}

/// The channel's spend once it reaches `tokens` (the meter writes after the answer ends).
pub async fn spent(f: &Fixture, channel: &str, tokens: i64) -> Result<i64> {
    let id = Uuid::parse_str(channel)?;
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let now: i64 = sqlx::query_scalar("SELECT spent_tokens FROM channels WHERE id = $1")
                .bind(id)
                .fetch_one(&f.pool)
                .await?;
            if now >= tokens {
                return Ok::<i64, Box<dyn std::error::Error>>(now);
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .map_err(|_| format!("the channel's spend never reached {tokens}"))?
}
