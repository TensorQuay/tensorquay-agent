//! GW-I-028 and GW-I-046 parity: every endpoint the web uses gives the same result and errors with a personal
//! API token as with the web's cookie session.
#[path = "support/fixture.rs"]
mod support;

use reqwest::Method;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{Fixture, TestResult};
use tqa_types::{ClientFrame, ConversationId, Event, ServerFrame};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

enum Auth {
    /// The web: session cookie and the exact Origin.
    Cookie(String),
    /// A program: Bearer only.
    Token(String),
}

impl Auth {
    fn request(&self, f: &Fixture, method: Method, path: &str) -> reqwest::RequestBuilder {
        let request = f.client.request(method, f.url(path));
        match self {
            Self::Cookie(cookie) => request
                .header("origin", f.origin())
                .header("cookie", cookie),
            Self::Token(token) => request.bearer_auth(token),
        }
    }
}

/// Ids and times differ between the two runs; everything else must match.
fn normalized(value: Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(
            map.into_iter()
                // A message's id is a server counter (GW-I-129), different in each run.
                .map(|(key, value)| match key.as_str() {
                    "message_id" => (key, Value::String("<id>".into())),
                    _ => (key, normalized(value)),
                })
                .collect(),
        ),
        Value::Array(items) => Value::Array(items.into_iter().map(normalized).collect()),
        Value::String(text) if uuid::Uuid::parse_str(&text).is_ok() => Value::String("<id>".into()),
        Value::String(text)
            if time::OffsetDateTime::parse(
                &text,
                &time::format_description::well_known::Rfc3339,
            )
            .is_ok() =>
        {
            Value::String("<time>".into())
        }
        other => other,
    }
}

struct Step {
    name: &'static str,
    status: u16,
    body: Value,
}

async fn step(name: &'static str, request: reqwest::RequestBuilder) -> Result<Step> {
    let response = request.send().await?;
    let status = response.status().as_u16();
    let bytes = response.bytes().await?;
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        normalized(serde_json::from_slice(&bytes)?)
    };
    Ok(Step { name, status, body })
}

fn event(seq: u64, event: Event) -> ServerFrame {
    ServerFrame::Event {
        seq: seq.into(),
        run_id: None,
        step: None,
        ts: "2026-01-01T00:00:00Z".into(),
        event,
    }
}

/// The first frame a stream sends (the current sandbox snapshot).
async fn first_stream_frame(f: &Fixture, auth: &Auth, id: &str) -> Result<Value> {
    let response = auth
        .request(
            f,
            Method::GET,
            &format!("/api/v1/conversations/{id}/events?stream=1&protocol=1"),
        )
        .send()
        .await?;
    assert_eq!(response.status(), 200);
    let mut response = response;
    let mut buffer = Vec::new();
    while let Some(chunk) = tokio::time::timeout(support::DEADLINE, response.chunk()).await?? {
        buffer.extend_from_slice(&chunk);
        let text = String::from_utf8_lossy(&buffer).to_string();
        if let Some(line) = text.lines().find_map(|line| line.strip_prefix("data:")) {
            return Ok(normalized(serde_json::from_str(line.trim())?));
        }
    }
    Err("stream ended before its first frame".into())
}

/// One user's view of one task, for either kind of credential.
struct Run<'a> {
    f: &'a Fixture,
    auth: &'a Auth,
    tag: &'a str,
    id: ConversationId,
    base: String,
}

impl Run<'_> {
    /// Calls a web endpoint; a changing call carries an idempotency key unique to this run and step.
    async fn call(
        &self,
        name: &'static str,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<Step> {
        let mut request = self.auth.request(self.f, method.clone(), path);
        if method != Method::GET {
            request = request.header("idempotency-key", format!("{name}-{}", self.tag));
        }
        if let Some(body) = body {
            request = request.json(&body);
        }
        step(name, request).await
    }
}

/// Reading the catalogue and creating, reading, renaming and streaming a task.
async fn open<'a>(f: &'a Fixture, auth: &'a Auth, tag: &'a str) -> Result<(Vec<Step>, Run<'a>)> {
    let mut steps = vec![
        step("models", auth.request(f, Method::GET, "/api/v1/models")).await?,
        step(
            "templates",
            auth.request(f, Method::GET, "/api/v1/templates"),
        )
        .await?,
        step("me", auth.request(f, Method::GET, "/api/v1/auth/me")).await?,
    ];
    let created = auth
        .request(f, Method::POST, "/api/v1/conversations")
        .header("idempotency-key", format!("create-{tag}"))
        .json(&json!({"title":"Parity","model_route":"local"}))
        .send()
        .await?;
    assert_eq!(created.status(), 201);
    let id: ConversationId = created.json::<Value>().await?["id"]
        .as_str()
        .ok_or("id")?
        .parse()?;
    let run = Run {
        f,
        auth,
        tag,
        base: format!("/api/v1/conversations/{id}"),
        id,
    };
    steps.push(
        run.call("list", Method::GET, "/api/v1/conversations", None)
            .await?,
    );
    steps.push(run.call("get", Method::GET, &run.base, None).await?);
    steps.push(
        run.call(
            "rename",
            Method::PATCH,
            &run.base,
            Some(json!({"title":"Parity renamed"})),
        )
        .await?,
    );
    steps.push(Step {
        name: "stream",
        status: 200,
        body: first_stream_frame(f, auth, run.id.as_str()).await?,
    });
    Ok((steps, run))
}

/// A message, a refused switch while the run works, a cancel, and the stored history's event types.
async fn busy_run(run: &Run<'_>) -> Result<Vec<Step>> {
    let base = &run.base;
    let mut steps = vec![
        run.call(
            "message",
            Method::POST,
            &format!("{base}/messages"),
            Some(json!({"text":"hello"})),
        )
        .await?,
    ];
    let mut runtime = run.f.runtime_for(&run.id).await?;
    assert!(matches!(
        runtime.action().await?,
        ClientFrame::UserMessage { .. }
    ));
    runtime
        .send(&event(
            1,
            Event::RunStarted {
                run_id: "r-1".parse()?,
            },
        ))
        .await?;
    runtime
        .send(&event(
            2,
            Event::TextDelta {
                text: "working".into(),
            },
        ))
        .await?;
    run.f.until_sequence(&run.id, 2).await?;
    steps.push(
        run.call(
            "switch while running",
            Method::PUT,
            &format!("{base}/environment"),
            Some(json!({"template":"minimal"})),
        )
        .await?,
    );
    steps.extend(finish(run, &mut runtime).await?);
    Ok(steps)
}

/// Cancelling the run, then the stored history's event types.
async fn finish(run: &Run<'_>, runtime: &mut support::runtime::Runtime) -> Result<Vec<Step>> {
    let base = &run.base;
    let mut steps = Vec::new();
    steps.push(
        run.call(
            "cancel",
            Method::POST,
            &format!("{base}/runs/r-1/cancel"),
            Some(json!({})),
        )
        .await?,
    );
    assert!(matches!(
        runtime.action().await?,
        ClientFrame::RunCancel { .. }
    ));
    runtime
        .send(&event(
            3,
            Event::RunCancelled {
                run_id: "r-1".parse()?,
            },
        ))
        .await?;
    run.f.until_sequence(&run.id, 3).await?;
    let history = run
        .call(
            "history",
            Method::GET,
            &format!("{base}/events?after=0&limit=100"),
            None,
        )
        .await?;
    let kinds: Vec<Value> = history.body["items"]
        .as_array()
        .ok_or("items")?
        .iter()
        .map(|frame| {
            frame["event"]["type"]
                .as_str()
                .map_or(frame["type"].clone(), |kind| Value::String(kind.into()))
        })
        .collect();
    steps.push(Step {
        name: "event types",
        status: history.status,
        body: Value::Array(kinds),
    });
    Ok(steps)
}

/// The same errors for both credentials, then deleting the task.
async fn errors(run: &Run<'_>, foreign: &str) -> Result<Vec<Step>> {
    let base = &run.base;
    Ok(vec![
        run.call(
            "another user's task",
            Method::GET,
            &format!("/api/v1/conversations/{foreign}"),
            None,
        )
        .await?,
        run.call(
            "unknown task",
            Method::GET,
            &format!("/api/v1/conversations/{}", uuid::Uuid::now_v7()),
            None,
        )
        .await?,
        run.call(
            "invalid message",
            Method::POST,
            &format!("{base}/messages"),
            Some(json!({"text":""})),
        )
        .await?,
        run.call("delete", Method::DELETE, base, None).await?,
    ])
}

/// The web's operations, in the order a user meets them.
async fn script(f: &Fixture, auth: &Auth, tag: &str, foreign: &str) -> Result<Vec<Step>> {
    let (mut steps, run) = open(f, auth, tag).await?;
    steps.extend(busy_run(&run).await?);
    steps.extend(errors(&run, foreign).await?);
    Ok(steps)
}

#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_028_every_web_endpoint_gives_the_same_result_with_a_personal_token(
    pool: PgPool,
) -> TestResult {
    support::run(pool, async |f| {
        let (_, cookie) = f.admin().await?;
        let (other, _) = f.member("other@example.invalid").await?;
        let foreign = f.conversation(other.id).await?.id.to_string();
        let issued = f
            .client
            .post(f.url("/api/v1/tokens"))
            .header("origin", f.origin())
            .header("cookie", &cookie)
            .json(&json!({"name":"parity","expires_in_days":7}))
            .send()
            .await?;
        assert_eq!(issued.status(), 201);
        let token = issued.json::<Value>().await?["token"]
            .as_str()
            .ok_or("token")?
            .to_owned();
        // The admin sees other users' task metadata; members do not. Use the same admin for both runs.
        let web = script(f, &Auth::Cookie(cookie.clone()), "web", &foreign).await?;
        let program = script(f, &Auth::Token(token), "program", &foreign).await?;
        assert_eq!(web.len(), program.len());
        for (web, program) in web.iter().zip(&program) {
            assert_eq!(
                (web.name, web.status, &web.body),
                (program.name, program.status, &program.body),
                "{}",
                web.name
            );
        }
        let statuses: Vec<(&str, u16)> = web.iter().map(|step| (step.name, step.status)).collect();
        assert!(
            statuses.contains(&("unknown task", 404))
                && statuses.contains(&("invalid message", 400)),
            "{statuses:?}"
        );
        Ok(())
    })
    .await
}
