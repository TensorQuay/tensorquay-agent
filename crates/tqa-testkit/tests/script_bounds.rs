//! LLM-C-002/E-016: invalid synthetic scripts fail before a listener is exposed.

use std::{error::Error, sync::Arc};
use tqa_testkit::{FakeClock, FakeModelServer, FakeSleeper, MalformedSse, Script, Scripted};

#[tokio::test]
async fn llm_e_016_fake_server_rejects_unbounded_script_expansion() -> Result<(), Box<dyn Error>> {
    let sleeper = Arc::new(FakeSleeper::new(FakeClock::new()));
    for scripts in [
        vec![Script::new(vec![]); 129],
        vec![Script::new(vec![Scripted::Text(String::new()); 16_385])],
        vec![Script::new(vec![Scripted::ToolCall {
            name: "read".into(),
            arguments_json: "{}".into(),
            split_into: 0,
        }])],
        vec![Script::new(vec![Scripted::ToolCall {
            name: "read".into(),
            arguments_json: "{}".into(),
            split_into: 16_385,
        }])],
        vec![Script::new(vec![
            Scripted::Text("x".repeat(16_384)),
            Scripted::MalformedSse(MalformedSse::SplitUtf8),
        ])],
    ] {
        let error = FakeModelServer::start(scripts, sleeper.clone())
            .await
            .err()
            .ok_or("accepted oversized script")?;
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    }
    Ok(())
}
