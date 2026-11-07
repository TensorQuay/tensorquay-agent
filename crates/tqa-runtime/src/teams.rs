//! The gateway's runtime agent-team routes (ADR-0034 slice 3), for the team tools: `<gateway origin>/runtime/teams/…`
//! with the runtime token, the call's idempotency key and the run's id (`X-TQA-Run-ID`). As for the channel routes: no
//! environment proxy, no redirects, a bounded body, every call ends promptly when cancelled, and failures keep the
//! gateway's stable codes.
use std::time::Duration;

use async_trait::async_trait;
use reqwest::{Method, header::HeaderValue};
use secrecy::{ExposeSecret, SecretString};
use serde_json::Value;
use tokio_util::sync::CancellationToken;
use tqa_tools::builtin::team::{ReadAnswer, TeamClient, TeamFailure, TeamRole};
use url::Url;

use crate::RuntimeError;
use crate::channels::{parse, send};

/// A plain call's deadline; a read gets its wait on top.
const CALL_TIMEOUT: Duration = Duration::from_secs(30);
/// Beyond a read's own wait, the time the gateway has to answer it.
const WAIT_GRACE: Duration = Duration::from_secs(10);

/// The agent-team routes of one gateway, as one run's conversation.
pub struct GatewayTeams {
    client: reqwest::Client,
    origin: Url,
    authorization: HeaderValue,
}

impl GatewayTeams {
    /// A client for `origin` with the run's `token`.
    ///
    /// # Errors
    /// `Configuration` when the token can't be a header or the client can't be built.
    pub fn new(origin: &Url, token: &SecretString) -> Result<Self, RuntimeError> {
        let mut authorization = HeaderValue::from_str(&format!("Bearer {}", token.expose_secret()))
            .map_err(|_| RuntimeError::Configuration("team client credential"))?;
        authorization.set_sensitive(true);
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .build()
            .map_err(|_| RuntimeError::Configuration("team client"))?;
        Ok(Self {
            client,
            origin: origin.clone(),
            authorization,
        })
    }

    fn request(
        &self,
        method: Method,
        path: &str,
        (key, run): (&str, &str),
        timeout: Duration,
    ) -> reqwest::RequestBuilder {
        let mut url = self.origin.clone();
        url.set_path(path);
        url.set_query(None);
        let request = self
            .client
            .request(method, url)
            .header(reqwest::header::AUTHORIZATION, self.authorization.clone())
            .timeout(timeout);
        let request = if key.is_empty() {
            request
        } else {
            request.header("idempotency-key", key)
        };
        if run.is_empty() {
            request
        } else {
            request.header("x-tqa-run-id", run)
        }
    }

    /// The run's role, which decides its team tools; `None` when the gateway has no such route (an older one) or
    /// can't say.
    pub async fn role(&self, cancel: &CancellationToken) -> TeamRole {
        let request = self.request(Method::GET, "/runtime/teams/role", ("", ""), CALL_TIMEOUT);
        match send(request, cancel)
            .await
            .and_then(|(_, body)| parse::<Value>(&body))
        {
            Ok(answer) => TeamRole::parse(answer["role"].as_str().unwrap_or_default()),
            Err(failure) => {
                if failure.code != "not_found" {
                    tracing::warn!(code = %failure.code, "agent-team role unavailable; the team tools are left out of this run");
                }
                TeamRole::None
            }
        }
    }
}

#[async_trait]
impl TeamClient for GatewayTeams {
    async fn start(
        &self,
        body: &Value,
        call: (&str, &str),
        cancel: &CancellationToken,
    ) -> Result<Value, TeamFailure> {
        let request = self
            .request(Method::POST, "/runtime/teams/members", call, CALL_TIMEOUT)
            .json(body);
        let (_, answer) = send(request, cancel).await?;
        parse(&answer)
    }

    async fn post(
        &self,
        body: &Value,
        call: (&str, &str),
        cancel: &CancellationToken,
    ) -> Result<Value, TeamFailure> {
        let request = self
            .request(Method::POST, "/runtime/teams/messages", call, CALL_TIMEOUT)
            .json(body);
        let (_, answer) = send(request, cancel).await?;
        parse(&answer)
    }

    async fn take_files(
        &self,
        body: &Value,
        call: (&str, &str),
        cancel: &CancellationToken,
    ) -> Result<Value, TeamFailure> {
        let request = self
            .request(Method::POST, "/runtime/teams/files", call, CALL_TIMEOUT)
            .json(body);
        let (_, answer) = send(request, cancel).await?;
        parse(&answer)
    }

    async fn done(
        &self,
        body: &Value,
        call: (&str, &str),
        cancel: &CancellationToken,
    ) -> Result<Value, TeamFailure> {
        let request = self
            .request(Method::POST, "/runtime/teams/done", call, CALL_TIMEOUT)
            .json(body);
        let (_, answer) = send(request, cancel).await?;
        parse(&answer)
    }

    async fn stop_member(
        &self,
        name: &str,
        body: &Value,
        call: (&str, &str),
        cancel: &CancellationToken,
    ) -> Result<Value, TeamFailure> {
        let path = format!("/runtime/teams/members/{}/stop", member_segment(name)?);
        let request = self
            .request(Method::POST, &path, call, CALL_TIMEOUT)
            .json(body);
        let (_, answer) = send(request, cancel).await?;
        parse(&answer)
    }

    async fn transcript(
        &self,
        name: &str,
        (since, limit): (Option<u64>, Option<u32>),
        run: &str,
        cancel: &CancellationToken,
    ) -> Result<Value, TeamFailure> {
        let path = format!(
            "/runtime/teams/members/{}/transcript",
            member_segment(name)?
        );
        let request = self.request(Method::GET, &path, ("", run), CALL_TIMEOUT);
        let mut query: Vec<(&str, String)> = Vec::new();
        if let Some(since) = since {
            query.push(("since", since.to_string()));
        }
        if let Some(limit) = limit {
            query.push(("limit", limit.to_string()));
        }
        let (_, answer) = send(request.query(&query), cancel).await?;
        parse(&answer)
    }

    async fn read(
        &self,
        (limit, wait): (Option<u32>, Option<u32>),
        (key, run): (&str, &str),
        cancel: &CancellationToken,
    ) -> Result<ReadAnswer, TeamFailure> {
        let waiting = Duration::from_secs(u64::from(wait.unwrap_or(0)));
        let request = self.request(
            Method::GET,
            "/runtime/teams/messages",
            (key, run),
            CALL_TIMEOUT + waiting + WAIT_GRACE,
        );
        let mut query: Vec<(&str, String)> = Vec::new();
        if let Some(limit) = limit {
            query.push(("limit", limit.to_string()));
        }
        if let Some(wait) = wait {
            query.push(("wait", wait.to_string()));
        }
        let (_, answer) = send(request.query(&query), cancel).await?;
        parse(&answer)
    }

    async fn acceptance(
        &self,
        run: &str,
        cancel: &CancellationToken,
    ) -> Result<Option<String>, TeamFailure> {
        let request = self.request(
            Method::GET,
            "/runtime/teams/acceptance",
            ("", run),
            CALL_TIMEOUT,
        );
        let (_, answer) = send(request, cancel).await?;
        let answer: Value = parse(&answer)?;
        Ok(answer["check"].as_str().map(str::to_owned))
    }
}

/// A helper's name as one path segment: percent-encoded, so a name can only ever name a member, never another route.
fn member_segment(name: &str) -> Result<String, TeamFailure> {
    if name.is_empty() || name.len() > 68 {
        return Err(TeamFailure::new(
            "invalid_request",
            "name: the helper's name is 1 to 68 bytes",
        ));
    }
    Ok(url::form_urlencoded::byte_serialize(name.as_bytes())
        .collect::<String>()
        .replace('+', "%20"))
}
