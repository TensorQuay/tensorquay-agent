//! TK-U-029: the fake typed-question service the typed-question engine adapter is tested against (ADR-0013, the adapter
//! amendment). It answers each call with the script's next answer, keyed by the request's question, and records every
//! request as sent; past the script it answers 503.
use serde_json::{Value, json};
use std::time::Duration;
use tqa_testkit::{FakeQuestionServer, QuestionAnswer};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn ask(route_options: &[&str]) -> Value {
    let criteria: serde_json::Map<String, Value> = route_options
        .iter()
        .map(|name| ((*name).to_owned(), json!(format!("the {name} route"))))
        .collect();
    json!({"model": "pinned-model", "state": {"step": 1},
        "questions": {"route": {"type": "choice", "instructions": "Pick the route", "criteria": criteria}}})
}

// TEST: TK-U-029
#[tokio::test]
async fn tk_u_029_answers_by_the_question_records_requests_and_fails_as_scripted() -> TestResult {
    let server = FakeQuestionServer::start(vec![
        QuestionAnswer::Choose {
            choice: "local-text".into(),
            confidence: 0.82,
            tokens: (640, 12),
        },
        QuestionAnswer::Raw {
            status: 529,
            body: br#"{"error":"overloaded"}"#.to_vec(),
        },
        QuestionAnswer::Delay(
            Duration::from_millis(50),
            Box::new(QuestionAnswer::Choose {
                choice: "local-32k".into(),
                confidence: 0.4,
                tokens: (700, 10),
            }),
        ),
    ])
    .await?;
    let client = reqwest::Client::new();
    let endpoint = format!("{}/v1/evaluate", server.base_url());
    let post = || {
        client
            .post(&endpoint)
            .bearer_auth("synthetic-engine-key")
            .json(&ask(&["local-32k", "local-text"]))
            .send()
    };
    let first = post().await?;
    assert_eq!(first.status(), 200);
    let answer: Value = first.json().await?;
    assert_eq!(
        answer,
        json!({"model": "pinned-model",
            "answers": {"route": {"type": "choice", "choice": "local-text",
                "probabilities": {"local-text": 0.82}, "confidence": 0.82}},
            "usage": {"input_tokens": 640, "output_tokens": 12}})
    );
    assert_eq!(post().await?.status(), 529);
    let started = std::time::Instant::now();
    let late: Value = post().await?.json().await?;
    assert!(started.elapsed() >= Duration::from_millis(50));
    assert_eq!(late["answers"]["route"]["choice"], "local-32k");
    assert_eq!(post().await?.status(), 503, "past the script");
    let seen = server.requests();
    assert_eq!(seen.len(), 4);
    assert_eq!(seen[0].path, "/v1/evaluate");
    assert_eq!(
        seen[0].authorization.as_deref(),
        Some("Bearer synthetic-engine-key")
    );
    assert_eq!(seen[0].body["questions"]["route"]["type"], "choice");
    server.stop().await;
    Ok(())
}
