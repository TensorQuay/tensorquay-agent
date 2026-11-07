use crate::{GatewayError, app};
use std::sync::Arc;
use tokio::sync::oneshot;
use tqa_context::estimate_bytes;
use tqa_llm::{Usage, relay_shaping::StreamedBytes};
use tqa_types::{ConversationId, RunId};

/// What a metered request ended with: the usage the server reported, or else the relay's estimate.
#[derive(Debug, Default)]
struct Ended {
    usage: Option<Usage>,
    estimate: Option<Usage>,
}

pub(super) struct Meter {
    sender: Option<oneshot::Sender<Ended>>,
    latest: Option<Usage>,
    /// The request's own estimate, set where the relay knows it (`with_prompt_estimate`).
    prompt: Option<u64>,
    streamed: StreamedBytes,
}
impl Meter {
    pub async fn start(
        state: &Arc<app::State>,
        conversation: &ConversationId,
        route: &str,
        run: Option<&RunId>,
    ) -> Result<Self, GatewayError> {
        let id = uuid::Uuid::now_v7();
        let conversation = crate::conversations::uuid(conversation)?;
        let run = run.map(RunId::as_str);
        crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
            sqlx::query!(
                "INSERT INTO usage (id,conversation_id,user_id,route,run_id) VALUES ($1,$2,(SELECT owner_id FROM conversations WHERE id=$2),$3,$4)",
                id,
                conversation,
                route,
                run
            )
            .execute(&state.pool)
            .await?;
            Ok(())
        })
        .await?;
        let (sender, receiver) = oneshot::channel();
        let owned = state.clone();
        state.tasks.spawn(async move {
            let ended=tokio::select! {biased; ()=owned.cancel.cancelled()=>Ended::default(), result=receiver=>result.unwrap_or_default()};
            // Shutdown joins this bounded final write before closing the pool.
            if persist(&owned,id,ended).await.is_err() {tracing::warn!("relay usage finalization failed");}
        });
        Ok(Self {
            sender: Some(sender),
            latest: None,
            prompt: None,
            streamed: StreamedBytes::default(),
        })
    }
    /// The request's estimated prompt tokens, used only if the server reports no usage (`prompt_estimate`).
    pub fn with_prompt_estimate(mut self, tokens: u64) -> Self {
        self.prompt = Some(tokens);
        self
    }
    pub fn observe(&mut self, usage: Option<Usage>) {
        if usage.is_some() {
            self.latest = usage;
        }
    }
    /// What the response has streamed so far (`UsageTap::streamed`).
    pub fn observe_streamed(&mut self, streamed: StreamedBytes) {
        self.streamed = streamed;
    }
    /// The estimate for a request whose server reported no usage: only a relayed request with a prompt estimate that
    /// streamed something (one refused before any reply stays uncounted, as before), its output by the runtime's own
    /// byte rule.
    fn estimate(&self) -> Option<Usage> {
        let prompt = self.prompt?;
        (self.streamed.output > 0).then(|| Usage {
            prompt_tokens: prompt,
            completion_tokens: u64::from(estimate_bytes(self.streamed.output).tokens),
            cached_tokens: None,
            reasoning_tokens: (self.streamed.reasoning > 0)
                .then(|| u64::from(estimate_bytes(self.streamed.reasoning).tokens)),
        })
    }
}
impl Drop for Meter {
    fn drop(&mut self) {
        let usage = self.latest.take();
        let estimate = if usage.is_none() {
            self.estimate()
        } else {
            None
        };
        if let Some(sender) = self.sender.take()
            && sender.send(Ended { usage, estimate }).is_err()
        {
            tracing::debug!("relay meter owner already stopped");
        }
    }
}
async fn persist(
    state: &Arc<app::State>,
    id: uuid::Uuid,
    ended: Ended,
) -> Result<(), GatewayError> {
    // Reported counts win; an estimate is stored only without them, and marked.
    let estimated = ended.usage.is_none() && ended.estimate.is_some();
    let counts = ended.usage.or(ended.estimate).as_ref().and_then(counts);
    let (prompt, completion, cached, reasoning) = counts.unwrap_or((None, None, None, None));
    let spent = crate::io::bounded(state.config.io_timeout(),&tokio_util::sync::CancellationToken::new(),async {
        let mut transaction = state.pool.begin().await?;
        let conversation = sqlx::query_scalar!("UPDATE usage SET prompt_tokens=$2,completion_tokens=$3,cached_tokens=$4,reasoning_tokens=$5,estimated=$6,updated_at=CURRENT_TIMESTAMP WHERE id=$1 RETURNING conversation_id",id,prompt,completion,cached,reasoning,estimated).fetch_one(&mut *transaction).await?;
        // An agent team's spend, in the transaction that records the usage (ADR-0034 point 8).
        let tokens = prompt.unwrap_or(0).saturating_add(completion.unwrap_or(0));
        let spent = match conversation {
            Some(conversation) if state.config.agent_teams.enabled => crate::teams::budget::spend(&mut transaction, conversation, tokens, state.config.agent_teams.helpers_stop_percent).await?,
            _ => crate::teams::budget::Spent::default(),
        };
        transaction.commit().await?;
        Ok(spent)
    }).await?;
    if let Some((channel, seq)) = spent.published {
        state.channels.published(channel, seq);
    }
    if spent.stopped {
        state.reconciler.wake();
    }
    // Past the helpers' share they stop and their work is handed back (GW-I-311), off the answer's path.
    if let Some(team) = spent.helpers_stopped {
        let owned = Arc::clone(state);
        state.tasks.spawn(async move {
            if let Err(error) = crate::teams::hand_back::at_share(&owned, team).await {
                tracing::warn!(team = %team, code = error.code(), "the helpers weren't stopped at their share");
            }
        });
    }
    Ok(())
}
type Counts = (Option<i64>, Option<i64>, Option<i64>, Option<i64>);
fn counts(usage: &Usage) -> Option<Counts> {
    Some((
        Some(usage.prompt_tokens.try_into().ok()?),
        Some(usage.completion_tokens.try_into().ok()?),
        usage.cached_tokens.map(i64::try_from).transpose().ok()?,
        usage.reasoning_tokens.map(i64::try_from).transpose().ok()?,
    ))
}

/// The relay's estimate of a request's prompt tokens, for a step whose server reports no usage: the bytes of every key
/// and string in the request the server renders into its prompt (messages, instructions, tool schemas), by the
/// runtime's byte rule. An image's inline data (a `data:` URL, or a base64 source's `data`) is left out: a server
/// counts images by size, not by their encoded bytes.
pub(super) fn prompt_estimate(body: &serde_json::Value) -> u64 {
    fn walk(value: &serde_json::Value, bytes: &mut usize) {
        match value {
            serde_json::Value::String(text) if !text.starts_with("data:") => *bytes += text.len(),
            serde_json::Value::Array(items) => items.iter().for_each(|item| walk(item, bytes)),
            serde_json::Value::Object(fields) => {
                let base64 =
                    fields.get("type").and_then(serde_json::Value::as_str) == Some("base64");
                let rendered = fields.iter().filter(|(key, _)| !(base64 && *key == "data"));
                for (key, field) in rendered {
                    *bytes += key.len();
                    walk(field, bytes);
                }
            }
            _ => {}
        }
    }
    let mut bytes = 0;
    walk(body, &mut bytes);
    u64::from(estimate_bytes(bytes).tokens)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn meter(prompt: Option<u64>, output: usize, reasoning: usize) -> Meter {
        Meter {
            sender: None,
            latest: None,
            prompt,
            streamed: StreamedBytes { output, reasoning },
        }
    }

    #[test]
    fn gw_u_186_a_cancelled_reply_is_estimated_from_what_streamed_and_the_request()
    -> Result<(), Box<dyn std::error::Error>> {
        // 13974 thinking tokens' worth by the runtime's byte rule (3.5 bytes a token)
        let estimate = meter(Some(15_928), 48_909, 48_909)
            .estimate()
            .ok_or("an estimate")?;
        assert_eq!(15_928, estimate.prompt_tokens);
        assert_eq!(13_974, estimate.completion_tokens);
        assert_eq!(Some(13_974), estimate.reasoning_tokens);
        assert_eq!(None, estimate.cached_tokens);
        // text only: no reasoning counter
        assert_eq!(
            None,
            meter(Some(10), 7, 0)
                .estimate()
                .and_then(|usage| usage.reasoning_tokens)
        );
        Ok(())
    }

    #[test]
    fn gw_u_186_no_estimate_without_a_reply_or_outside_the_relay() {
        assert!(
            meter(Some(500), 0, 0).estimate().is_none(),
            "refused before any reply: uncounted, as before"
        );
        assert!(
            meter(None, 1_000, 0).estimate().is_none(),
            "a typed engine's answer has no prompt estimate"
        );
    }

    #[test]
    fn gw_u_186_reported_usage_wins_over_the_estimate() -> Result<(), Box<dyn std::error::Error>> {
        let (sender, mut receiver) = oneshot::channel();
        let mut reported = meter(Some(100), 3_500, 0);
        reported.sender = Some(sender);
        reported.observe(Some(Usage {
            prompt_tokens: 120,
            completion_tokens: 900,
            cached_tokens: None,
            reasoning_tokens: None,
        }));
        drop(reported);
        let ended = receiver.try_recv()?;
        assert_eq!(Some(900), ended.usage.map(|usage| usage.completion_tokens));
        assert!(ended.estimate.is_none());

        let (sender, mut receiver) = oneshot::channel();
        let mut cut = meter(Some(100), 3_500, 0);
        cut.sender = Some(sender);
        drop(cut);
        let ended = receiver.try_recv()?;
        assert!(ended.usage.is_none());
        assert_eq!(
            Some(1_000),
            ended.estimate.map(|usage| usage.completion_tokens)
        );
        Ok(())
    }

    #[test]
    fn gw_u_186_the_prompt_estimate_counts_what_is_rendered_and_skips_inline_images() {
        let text = json!({"model":"m","messages":[{"role":"user","content":"abcdefg"}]});
        // keys model, messages, role, content (5+8+4+7) and strings m, user, abcdefg (1+4+7): 36 bytes, 11 tokens
        assert_eq!(11, prompt_estimate(&text));
        let with_images = json!({"model":"m","messages":[{"role":"user","content":[
            {"type":"text","text":"abcdefg"},
            {"type":"image_url","image_url":{"url":"data:image/png;base64,AAAA"}},
            {"type":"image","source":{"type":"base64","media_type":"image/png","data":"QUJD"}}]}]});
        let without = json!({"model":"m","messages":[{"role":"user","content":[
            {"type":"text","text":"abcdefg"},
            {"type":"image_url","image_url":{"url":""}},
            {"type":"image","source":{"type":"base64","media_type":"image/png"}}]}]});
        assert_eq!(prompt_estimate(&without), prompt_estimate(&with_images));
    }
}
