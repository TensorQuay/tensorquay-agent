//! CORE-U-053: on a native-tool route the tool catalogue travels once, in the request's `tools`; a text-protocol route
//! keeps it in the tools slot, where it's the only copy.
mod support;
use support::*;
use tokio_util::sync::CancellationToken;
use tqa_core::Runner;
use tqa_llm::ContentPart;

/// Whether any request message carries the catalogue's JSON text.
fn catalogue_in_messages(request: &tqa_llm::CompletionRequest) -> bool {
    request.messages.iter().any(|message| {
        message.content.iter().any(|part| {
            matches!(part, ContentPart::Text(text) if text.contains("\"name\":\"read_file\"") && text.contains("\"parameters\""))
        })
    })
}

async fn first_request(
    native: bool,
) -> Result<tqa_llm::CompletionRequest, Box<dyn std::error::Error>> {
    let tool = fake("read_file", true, vec![output("ok", 0)])?;
    let mut h = Harness::new(vec![answer()], registry(&[tool])?)?;
    h.deps.route.native_tools = native;
    let provider = h.provider.clone();
    let runner = Runner::new(h.deps)?;
    drive(
        runner.run(
            &[],
            Some(turn()),
            CancellationToken::new(),
            CancellationToken::new(),
        ),
        &h.clock,
    )
    .await??;
    Ok(provider.requests().into_iter().next().ok_or("no request")?)
}

#[tokio::test]
async fn core_u_053_a_native_request_carries_the_catalogue_only_in_tools() -> TestResult {
    let request = first_request(true).await?;
    assert!(
        request.tools.iter().any(|tool| tool.name == "read_file"),
        "tools lists the tool: {:?}",
        request.tools
    );
    assert!(
        !catalogue_in_messages(&request),
        "no message repeats the catalogue"
    );
    Ok(())
}

#[tokio::test]
async fn core_u_053_a_text_protocol_route_keeps_the_catalogue_in_its_slot() -> TestResult {
    let request = first_request(false).await?;
    assert!(request.tools.is_empty(), "no native tools on this route");
    assert!(
        catalogue_in_messages(&request),
        "the slot carries the only copy"
    );
    Ok(())
}
