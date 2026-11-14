//! GW-I-320: a model that can't be reached says so in the model list, and the next calls fail at once.
//! A server down only briefly (a restart) is still tried on every call, so a task's retries can reach it again.
//! The owner's case (13 Nov 2026): a local model server was stopped; New task still offered it as if it worked, and
//! each of the task's three tries waited for the connection before failing, so the message sat on "Sending…".

#[path = "support/fixture.rs"]
mod support;

use secrecy::ExposeSecret;
use serde_json::{Value, json};
use sqlx::PgPool;
use std::net::Ipv4Addr;
use support::TestResult;

/// The fixture's task route `local`, to a port nothing listens on, beside one that is never called.
fn registry(port: u16) -> Result<tqa_llm::Registry, Box<dyn std::error::Error>> {
    Ok(tqa_llm::Registry::from_toml(&format!(
        "[[routes]]\nname='local'\nprotocol='openai_chat'\nbase_url='http://127.0.0.1:{port}/v1'\nmodel='m'\nauth={{kind='none'}}\n\
         [[routes]]\nname='other'\nprotocol='openai_chat'\nbase_url='http://127.0.0.1:{port}/v1'\nmodel='n'\nauth={{kind='none'}}\n"
    ))?)
}

/// A port that was free a moment ago: binding then dropping the listener leaves nothing on it.
fn closed_port() -> Result<u16, std::io::Error> {
    let listener = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    let port = listener.local_addr()?.port();
    drop(listener);
    Ok(port)
}

fn answering(listed: &Value) -> Vec<(String, Value)> {
    listed["items"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .map(|item| {
                    (
                        item["name"].as_str().unwrap_or_default().to_owned(),
                        item["answering"].clone(),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

// TEST: GW-I-320
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_320_a_model_that_cant_be_reached_says_so_and_stays_marked_while_down(
    pool: PgPool,
) -> TestResult {
    let port = closed_port()?;
    support::run_configured(pool, Some(registry(port)?), "", async |f| {
        let (owner, cookie) = f.admin().await?;
        let conversation = f.conversation(owner.id).await?;
        let token = f
            .gateway
            .runtime_tokens()
            .mint(
                &conversation.id,
                &"unreachable-sandbox".parse()?,
                Ipv4Addr::LOCALHOST.into(),
            )
            .await?;
        let call = || {
            f.client
                .post(f.url("/relay/r/local/chat/completions"))
                .header("x-tqa-run-id", "r-1")
                .bearer_auth(token.expose_secret())
                .json(&json!({"model":"m","messages":[{"role":"user","content":"hello"}],"stream":true}))
                .send()
        };

        // Before any call, nothing is known: every route reads null, as an unchecked route does.
        let listed = f.get("/api/v1/models", &cookie).await?.json::<Value>().await?;
        assert_eq!(
            answering(&listed),
            [("local".to_owned(), Value::Null), ("other".to_owned(), Value::Null)]
        );

        // The first call tries and fails: the model can't be reached.
        assert_eq!(call().await?.status(), 502);
        let listed = f.get("/api/v1/models", &cookie).await?.json::<Value>().await?;
        assert_eq!(
            answering(&listed),
            [("local".to_owned(), json!(false)), ("other".to_owned(), Value::Null)],
            "only the route that failed is marked"
        );

        // Within the grace a server may be restarting: the task's retries still try it (and still fail here), and the
        // route stays marked. Answering at once after the grace is the reach module's unit test (an injected clock).
        for _ in 0..2 {
            assert_eq!(call().await?.status(), 502);
        }
        let listed = f.get("/api/v1/models", &cookie).await?.json::<Value>().await?;
        assert_eq!(
            answering(&listed),
            [("local".to_owned(), json!(false)), ("other".to_owned(), Value::Null)]
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-320
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_320_a_model_that_answers_again_is_no_longer_marked(pool: PgPool) -> TestResult {
    // The port is closed for the first call, then a server listens on it: the next call reaches it at once, and the
    // mark goes.
    let port = closed_port()?;
    support::run_configured(pool, Some(registry(port)?), "", async |f| {
        let (owner, cookie) = f.admin().await?;
        let conversation = f.conversation(owner.id).await?;
        let token = f
            .gateway
            .runtime_tokens()
            .mint(
                &conversation.id,
                &"unreachable-sandbox".parse()?,
                Ipv4Addr::LOCALHOST.into(),
            )
            .await?;
        let call = || {
            f.client
                .post(f.url("/relay/r/local/chat/completions"))
                .header("x-tqa-run-id", "r-1")
                .bearer_auth(token.expose_secret())
                .json(&json!({"model":"m","messages":[{"role":"user","content":"hello"}],"stream":true}))
                .send()
        };
        assert_eq!(call().await?.status(), 502);

        // The server comes back on the same port and answers anything with 200.
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, port)).await?;
        let server = tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                tokio::spawn(async move {
                    use tokio::io::{AsyncReadExt, AsyncWriteExt};
                    let mut buffer = [0_u8; 4096];
                    let _ = socket.read(&mut buffer).await;
                    let body = "{\"choices\":[]}";
                    let answer = format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = socket.write_all(answer.as_bytes()).await;
                });
            }
        });

        // A server back within the grace (a restart) is reached by the very next call.
        assert_eq!(call().await?.status(), 200);
        let listed = f.get("/api/v1/models", &cookie).await?.json::<Value>().await?;
        assert_eq!(
            answering(&listed),
            [("local".to_owned(), json!(true)), ("other".to_owned(), Value::Null)]
        );
        server.abort();
        Ok(())
    })
    .await
}
