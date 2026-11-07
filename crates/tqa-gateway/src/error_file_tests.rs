//! GW-U-139: a refused handed-back file names it: each refusal's code, status and retry, and which errors are a
//! file's (the rest pass through unchanged).
use super::{FileRefusal, GatewayError};
use axum::{http::StatusCode, response::IntoResponse};

const ALL: [(FileRefusal, &str, StatusCode); 6] = [
    (
        FileRefusal::PathInvalid,
        "path_invalid",
        StatusCode::BAD_REQUEST,
    ),
    (FileRefusal::NotFound, "not_found", StatusCode::NOT_FOUND),
    (
        FileRefusal::UploadTooLarge,
        "upload_too_large",
        StatusCode::PAYLOAD_TOO_LARGE,
    ),
    (
        FileRefusal::UploadsFull,
        "uploads_full",
        StatusCode::CONFLICT,
    ),
    (
        FileRefusal::FilesUnavailable,
        "files_unavailable",
        StatusCode::CONFLICT,
    ),
    (
        FileRefusal::FileChanged,
        "file_changed",
        StatusCode::CONFLICT,
    ),
];

// TEST: GW-U-139
#[test]
fn gw_u_139_each_refusal_keeps_its_errors_code_status_and_retry() {
    for (refusal, code, status) in ALL {
        let error = refusal.error();
        assert_eq!(
            (error.code(), error.status()),
            (code, status),
            "{refusal:?}"
        );
        let named = GatewayError::ForFile {
            path: "src/report.md".to_owned(),
            refusal,
        };
        assert_eq!(
            (named.code(), named.status()),
            (code, status),
            "{refusal:?}"
        );
        assert_eq!(
            named.retryable(),
            refusal == FileRefusal::FileChanged,
            "only a changed file is retried"
        );
        assert_eq!(named.to_string(), format!("src/report.md: {refusal}"));
    }
}

// TEST: GW-U-139
#[test]
fn gw_u_139_a_files_outcome_names_it_and_anything_else_passes_through() {
    for (error, refusal) in [
        (GatewayError::PathInvalid, FileRefusal::PathInvalid),
        (GatewayError::NotFound, FileRefusal::NotFound),
        (GatewayError::UploadTooLarge, FileRefusal::UploadTooLarge),
        (
            GatewayError::Conflict("uploads_full"),
            FileRefusal::UploadsFull,
        ),
        (
            GatewayError::FilesUnavailable,
            FileRefusal::FilesUnavailable,
        ),
        (
            GatewayError::Conflict("sandbox_not_ready"),
            FileRefusal::FilesUnavailable,
        ),
        (GatewayError::FileChanged, FileRefusal::FileChanged),
    ] {
        assert_eq!(
            error.clone().for_file("data.csv"),
            GatewayError::ForFile {
                path: "data.csv".to_owned(),
                refusal,
            },
            "{error:?}"
        );
    }
    for other in [
        GatewayError::Io,
        GatewayError::Conflict("team_stopped"),
        GatewayError::StoreUnavailable,
    ] {
        assert_eq!(
            other.clone().for_file("data.csv"),
            other,
            "not a file's outcome"
        );
    }
}

// TEST: GW-U-139
#[tokio::test]
async fn gw_u_139_the_answer_names_the_file_in_details() -> Result<(), Box<dyn std::error::Error>> {
    let response = GatewayError::NotFound.for_file("notes.txt").into_response();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = axum::body::to_bytes(response.into_body(), 1 << 16).await?;
    let body: serde_json::Value = serde_json::from_slice(&body)?;
    assert_eq!(body["error"]["code"], "not_found");
    assert_eq!(
        body["error"]["details"],
        serde_json::json!({"path": "notes.txt"})
    );
    Ok(())
}
