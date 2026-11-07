//! CORE-U-039 / LLM-C-023: image limits, image media types and the route's request budget.
use serde_json::{Value, json};
use std::error::Error;
use tqa_types::{
    DEFAULT_MAX_REQUEST_BYTES, IMAGE_TOKENS, ImageMediaType, MAX_IMAGE_BYTES,
    MAX_IMAGES_PER_MESSAGE, MAX_REQUEST_BYTES, MIN_REQUEST_BYTES, RouteSnapshot, protocol_schema,
};

type TestResult = Result<(), Box<dyn Error>>;

fn route(extra: Option<Value>) -> Value {
    let mut route = json!({
        "name":"local","protocol":"openai_chat","model":"example-model",
        "relay_path":"/relay/r/local","context_window":32_768,"max_output":4096,
        "native_tools":true,"parallel_tools":true,"vision":true,"reasoning":null
    });
    if let Some(value) = extra {
        route["max_request_bytes"] = value;
    }
    route
}
fn schema_accepts(route: &Value) -> Result<bool, Box<dyn Error>> {
    let mut schema = protocol_schema();
    let frame = json!({
        "type":"session.update","route":route
    });
    schema["$ref"] = json!("#/definitions/ClientFrame");
    Ok(jsonschema::validator_for(&schema)?.is_valid(&frame))
}

#[test]
fn core_u_039_image_limits_are_the_approved_constants() {
    assert_eq!(MAX_IMAGES_PER_MESSAGE, 4);
    assert_eq!(MAX_IMAGE_BYTES, 5 * 1024 * 1024);
    assert_eq!(IMAGE_TOKENS, 1600);
    assert_eq!(DEFAULT_MAX_REQUEST_BYTES, 1024 * 1024);
    assert_eq!(MIN_REQUEST_BYTES, 1024);
    assert_eq!(MAX_REQUEST_BYTES, 16 * 1024 * 1024);
}

#[test]
fn llm_c_023_media_types_have_stable_wire_names() {
    for (media_type, name) in [
        (ImageMediaType::Png, "image/png"),
        (ImageMediaType::Jpeg, "image/jpeg"),
        (ImageMediaType::Gif, "image/gif"),
        (ImageMediaType::Webp, "image/webp"),
    ] {
        assert_eq!(media_type.as_str(), name);
    }
}

#[test]
fn core_u_039_request_budget_is_optional_and_bounded_inclusively() -> TestResult {
    let absent: RouteSnapshot = serde_json::from_value(route(None))?;
    assert_eq!(absent.max_request_bytes, None);
    assert_eq!(absent.request_budget(), DEFAULT_MAX_REQUEST_BYTES);
    assert!(
        serde_json::to_value(&absent)?
            .get("max_request_bytes")
            .is_none()
    );
    assert!(schema_accepts(&route(None))?);
    for bytes in [MIN_REQUEST_BYTES, MAX_REQUEST_BYTES] {
        let wire = route(Some(json!(bytes)));
        let parsed: RouteSnapshot = serde_json::from_value(wire.clone())?;
        assert_eq!(parsed.max_request_bytes, Some(bytes));
        assert_eq!(parsed.request_budget(), bytes);
        assert_eq!(serde_json::to_value(&parsed)?, wire);
        assert!(schema_accepts(&wire)?);
    }
    for bytes in [0, MIN_REQUEST_BYTES - 1, MAX_REQUEST_BYTES + 1] {
        let wire = route(Some(json!(bytes)));
        assert!(serde_json::from_value::<RouteSnapshot>(wire.clone()).is_err());
        assert!(!schema_accepts(&wire)?);
    }
    let mut constructed: RouteSnapshot = serde_json::from_value(route(None))?;
    constructed.max_request_bytes = Some(MIN_REQUEST_BYTES - 1);
    assert!(constructed.validate().is_err());
    Ok(())
}
