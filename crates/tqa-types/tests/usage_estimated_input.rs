//! CORE-U-084: a `usage` event carries the runner's estimate of the request it sent, so the route's density is read
//! from the journal, exactly and after a resume, instead of being recomputed.
use serde_json::json;
use tqa_types::{Event, protocol_schema};

type TestResult = Result<(), Box<dyn std::error::Error>>;

// TEST: CORE-U-084
/// Both estimates round-trip, are left out when absent, and an older journal's `usage` (without them) still reads.
#[test]
fn core_u_084_a_usage_event_carries_the_estimate_of_the_request_it_sent() -> TestResult {
    let usage = Event::Usage {
        prompt_tokens: 1501,
        completion_tokens: 20,
        cached_tokens: None,
        reasoning_tokens: None,
        estimated: false,
        estimated_input: Some(914),
        estimated_images: Some(1600),
        estimated_route: Some("local-32k".into()),
    };
    let value = serde_json::to_value(&usage)?;
    assert_eq!(value["estimated_input"], json!(914));
    assert_eq!(value["estimated_images"], json!(1600));
    assert_eq!(value["estimated_route"], json!("local-32k"));
    assert_eq!(serde_json::from_value::<Event>(value)?, usage);

    let older = json!({"type": "usage", "prompt_tokens": 7, "completion_tokens": 2, "cached_tokens": null,
                       "reasoning_tokens": null});
    let Event::Usage {
        estimated_input,
        estimated_images,
        ..
    } = serde_json::from_value::<Event>(older)?
    else {
        return Err("not a usage event".into());
    };
    assert_eq!((estimated_input, estimated_images), (None, None));

    let summary = Event::Usage {
        prompt_tokens: 7,
        completion_tokens: 2,
        cached_tokens: None,
        reasoning_tokens: None,
        estimated: false,
        estimated_input: None,
        estimated_images: None,
        estimated_route: None,
    };
    let text = serde_json::to_string(&summary)?;
    assert!(
        !text.contains("estimated_input")
            && !text.contains("estimated_images")
            && !text.contains("estimated_route"),
        "{text}"
    );

    let schema = protocol_schema().to_string();
    assert!(
        ["estimated_input", "estimated_images", "estimated_route"]
            .iter()
            .all(|field| schema.contains(field))
    );
    Ok(())
}
