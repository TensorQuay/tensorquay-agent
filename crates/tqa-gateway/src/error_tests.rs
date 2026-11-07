use super::GatewayError;
use axum::response::IntoResponse;
use tqa_types::{ErrorCode, ServerFrame};

/// Every error with its stable code, HTTP status and retryability.
/// Outcomes of the workspace service and the profile references (ADR-0030, ADR-0012).
fn workspace_cases() -> Vec<(GatewayError, &'static str, u16, bool)> {
    vec![
        (
            GatewayError::UpstreamInvalid,
            "upstream_invalid",
            502,
            false,
        ),
        (GatewayError::FileChanged, "file_changed", 409, true),
        // The image list when the container engine didn't answer (the built-in environments design, F1): retry in a
        // moment.
        (
            GatewayError::EngineUnavailable,
            "engine_unavailable",
            503,
            true,
        ),
        (
            GatewayError::UnknownReference {
                field: "model_route",
            },
            "unknown_reference",
            422,
            false,
        ),
    ]
}

/// The secret store's failures, and a key not used because its use couldn't be recorded (D4).
fn secret_cases() -> Vec<(GatewayError, &'static str, u16, bool)> {
    vec![
        (
            GatewayError::SecretUnavailable,
            "secret_unavailable",
            503,
            true,
        ),
        (
            GatewayError::KeyUseUnrecorded,
            "key_use_unrecorded",
            503,
            true,
        ),
    ]
}

fn cases() -> Vec<(GatewayError, &'static str, u16, bool)> {
    let mut cases = workspace_cases();
    cases.extend(secret_cases());
    cases.extend(vec![
        (
            GatewayError::Configuration("invalid setting"),
            "configuration",
            500,
            false,
        ),
        (
            GatewayError::BadRequest("invalid value"),
            "invalid_request",
            400,
            false,
        ),
        (GatewayError::BodyTooLarge, "body_too_large", 413, false),
        (GatewayError::Unauthorized, "unauthorized", 401, false),
        (
            GatewayError::Forbidden("ownership"),
            "forbidden",
            403,
            false,
        ),
        (GatewayError::NotFound, "not_found", 404, false),
        (
            GatewayError::Conflict("sandbox_not_ready"),
            "sandbox_not_ready",
            409,
            false,
        ),
        (GatewayError::Busy, "busy", 429, true),
        (
            GatewayError::RuntimeTooOld(tqa_types::RuntimeTooOld { needs: 6, has: 5 }),
            "runtime_too_old",
            426,
            false,
        ),
        (GatewayError::RateLimited(7), "rate_limited", 429, true),
        (
            GatewayError::UpstreamUnavailable,
            "upstream_unavailable",
            502,
            true,
        ),
        (
            GatewayError::StoreUnavailable,
            "store_unavailable",
            503,
            true,
        ),
        (GatewayError::Io, "io", 500, false),
        (GatewayError::Timeout, "timeout", 504, true),
        (GatewayError::Cancelled, "restarting", 503, true),
        (
            GatewayError::Protocol(ErrorCode::SeqGap),
            "seq_gap",
            400,
            true,
        ),
        (
            GatewayError::UnsupportedProtocol(tqa_types::UnsupportedProtocol {
                requested: 2,
                supported: vec![1],
            }),
            "unsupported_protocol",
            400,
            false,
        ),
    ]);
    cases
}

#[tokio::test]
async fn gw_u_022_failures_have_stable_redacted_http_and_protocol_classifications()
-> Result<(), Box<dyn std::error::Error>> {
    for (error, code, status, retryable) in cases() {
        let protocol = error.frame();
        assert!(
            matches!(protocol, ServerFrame::Error { seq: None, retryable: actual, .. } if actual == retryable)
        );
        let response = error.into_response();
        assert_eq!(response.status(), status);
        let bytes = axum::body::to_bytes(response.into_body(), 1024).await?;
        let body: serde_json::Value = serde_json::from_slice(&bytes)?;
        assert_eq!(body["error"]["code"], code);
        assert_eq!(body["error"]["retryable"], retryable);
    }
    Ok(())
}

#[test]
fn gw_u_022_dependency_failures_never_expose_raw_diagnostics()
-> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(
        GatewayError::from(std::io::Error::other("sensitive diagnostic")),
        GatewayError::Io
    );
    assert_eq!(
        GatewayError::from(sqlx::Error::PoolClosed),
        GatewayError::StoreUnavailable
    );
    let malformed = serde_json::from_str::<serde_json::Value>("{")
        .err()
        .ok_or("accepted malformed JSON")?;
    assert_eq!(
        GatewayError::from(malformed),
        GatewayError::Protocol(ErrorCode::Internal)
    );
    assert_eq!(
        GatewayError::from(tqa_types::SeqError::Exhausted),
        GatewayError::Protocol(ErrorCode::Internal)
    );
    Ok(())
}

#[tokio::test]
async fn gw_i_046_account_refusals_have_stable_codes() -> Result<(), Box<dyn std::error::Error>> {
    for (error, code, status) in [
        (GatewayError::InvitationInvalid, "invitation_invalid", 400),
        (GatewayError::SessionRequired, "session_required", 403),
    ] {
        assert!(!error.retryable());
        let response = error.into_response();
        assert_eq!(response.status(), status);
        let bytes = axum::body::to_bytes(response.into_body(), 1024).await?;
        let body: serde_json::Value = serde_json::from_slice(&bytes)?;
        assert_eq!(body["error"]["code"], code);
    }
    Ok(())
}

// TEST: GW-I-013
#[tokio::test]
async fn gw_i_045_file_refusals_have_stable_codes() -> Result<(), Box<dyn std::error::Error>> {
    for (error, code, status) in [
        (GatewayError::PathInvalid, "path_invalid", 400),
        (GatewayError::UploadTooLarge, "upload_too_large", 413),
        (GatewayError::AttachmentMissing, "attachment_missing", 400),
        (GatewayError::ChangesUnavailable, "changes_unavailable", 404),
        (GatewayError::UserNotFound, "user_not_found", 404),
        (GatewayError::Conflict("uploads_full"), "uploads_full", 409),
        (GatewayError::FilesUnavailable, "files_unavailable", 409),
    ] {
        assert!(!error.retryable());
        let response = error.into_response();
        assert_eq!(response.status(), status);
        let bytes = axum::body::to_bytes(response.into_body(), 1024).await?;
        let body: serde_json::Value = serde_json::from_slice(&bytes)?;
        assert_eq!(body["error"]["code"], code);
    }
    Ok(())
}

/// GW-U-029 (090): a field refusal is `400 invalid_request` with one `details` entry whose reason comes from the closed
/// list, so a client can show its own text next to the field; nothing from the input is echoed.
#[tokio::test]
async fn gw_u_029_field_refusals_carry_a_closed_reason_in_details()
-> Result<(), Box<dyn std::error::Error>> {
    use super::InvalidReason;
    let error = GatewayError::Invalid {
        field: "value",
        reason: InvalidReason::ValueTooLong,
    };
    assert!(!error.retryable());
    let response = error.into_response();
    assert_eq!(response.status(), 400);
    let bytes = axum::body::to_bytes(response.into_body(), 1024).await?;
    let body: serde_json::Value = serde_json::from_slice(&bytes)?;
    assert_eq!(body["error"]["code"], "invalid_request");
    assert_eq!(
        body["error"]["details"],
        serde_json::json!([{"field": "value", "reason": "value_too_long"}])
    );
    let names: Vec<&str> = InvalidReason::ALL
        .iter()
        .map(|reason| reason.as_str())
        .collect();
    let mut unique = names.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), names.len(), "reason codes are distinct");
    assert!(names.iter().all(|name| {
        name.bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte == b'_')
    }));
    Ok(())
}

#[test]
fn gw_u_090_the_refusal_header_names_the_gateways_own_code_and_nothing_else() {
    for (error, code, _, _) in cases() {
        // The runtime reads only a bare code (tqa_types::gateway_error_code), so every code must be one.
        assert_eq!(tqa_types::gateway_error_code(code), Some(code), "{code}");
        let mut response = error.into_response();
        super::name_refusal(&mut response);
        assert_eq!(
            response
                .headers()
                .get(tqa_types::GATEWAY_ERROR_HEADER)
                .map(axum::http::HeaderValue::as_bytes),
            Some(code.as_bytes()),
            "{code}"
        );
    }
    // An answer the gateway didn't make, such as a model server's error, never keeps a header it sent.
    let mut forwarded = axum::http::Response::builder()
        .status(400)
        .header(tqa_types::GATEWAY_ERROR_HEADER, "key_use_unrecorded")
        .body(axum::body::Body::empty())
        .unwrap_or_default();
    super::name_refusal(&mut forwarded);
    assert_eq!(
        forwarded.headers().get(tqa_types::GATEWAY_ERROR_HEADER),
        None
    );
}
