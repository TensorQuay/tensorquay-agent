use super::*;
use bollard::{API_DEFAULT_VERSION, Docker};
use std::{io, net::Ipv4Addr, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use tqa_types::{MIN_RUNTIME_REVISION, RUNTIME_REVISION_LABEL};

const DIGEST: &str = "sha256:1111111111111111111111111111111111111111111111111111111111111111";

/// Serves one image inspection: `None` answers 404, otherwise the image with these labels.
async fn inspect_with(
    labels: Option<serde_json::Value>,
    absent: bool,
) -> Result<Result<u16, SandboxError>, Box<dyn std::error::Error>> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
    let origin = format!("http://{}", listener.local_addr()?);
    let storage = tempfile::tempdir()?;
    let backend = DockerBackend::unconnected(
        Docker::connect_with_http(&origin, 5, API_DEFAULT_VERSION)?,
        super::super::DockerBackendConfig::new("revision", storage.path())?,
    );
    let image = crate::ImageRef::local_id(DIGEST)?;
    let mut config = serde_json::json!({});
    if let Some(labels) = labels {
        config["Labels"] = labels;
    }
    let body = serde_json::json!({"Id": DIGEST, "Config": config}).to_string();
    let response = if absent {
        "HTTP/1.1 404 Not Found\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}".to_owned()
    } else {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    };
    let (served, result) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(
            async {
                let (mut stream, _) = listener.accept().await?;
                let mut bytes = [0; 4096];
                let count = stream.read(&mut bytes).await?;
                let request = String::from_utf8_lossy(&bytes[..count]).into_owned();
                stream.write_all(response.as_bytes()).await?;
                Ok::<_, io::Error>(request)
            },
            backend.check_runtime_image(&image)
        )
    })
    .await?;
    let request = served?;
    assert!(request.starts_with("GET "), "{request}");
    assert!(
        request.contains("/images/sha256"),
        "only inspection: {request}"
    );
    Ok(result)
}

fn labelled(revision: &str) -> serde_json::Value {
    serde_json::json!({ RUNTIME_REVISION_LABEL: revision, "other": "x" })
}

#[tokio::test]
async fn sbx_u_001_an_image_at_or_above_the_floor_passes() -> Result<(), Box<dyn std::error::Error>>
{
    let floor = MIN_RUNTIME_REVISION.to_string();
    assert_eq!(
        inspect_with(Some(labelled(&floor)), false).await?,
        Ok(MIN_RUNTIME_REVISION)
    );
    let newer = (MIN_RUNTIME_REVISION + 1).to_string();
    assert_eq!(
        inspect_with(Some(labelled(&newer)), false).await?,
        Ok(MIN_RUNTIME_REVISION + 1)
    );
    Ok(())
}

#[tokio::test]
async fn sbx_u_001_an_older_or_unlabelled_image_is_too_old()
-> Result<(), Box<dyn std::error::Error>> {
    let older = (MIN_RUNTIME_REVISION - 1).to_string();
    let too_old = |has| {
        Err(SandboxError::RuntimeTooOld {
            needs: MIN_RUNTIME_REVISION,
            has,
        })
    };
    assert_eq!(
        inspect_with(Some(labelled(&older)), false).await?,
        too_old(MIN_RUNTIME_REVISION - 1)
    );
    for unlabelled in [
        None,
        Some(serde_json::json!({"other": "x"})),
        Some(serde_json::Value::Null),
    ] {
        assert_eq!(inspect_with(unlabelled, false).await?, too_old(1));
    }
    let error = SandboxError::RuntimeTooOld { needs: 4, has: 1 };
    assert_eq!(error.code(), "sandbox.runtime_too_old");
    assert!(!error.retryable());
    assert_eq!(
        error.to_string(),
        "sandbox runtime image protocol revision 1 is older than the required 4"
    );
    Ok(())
}

#[tokio::test]
async fn sbx_u_001_malformed_labels_and_absent_images_fail_closed()
-> Result<(), Box<dyn std::error::Error>> {
    for malformed in ["", "four", "0", "04", "70000"] {
        assert_eq!(
            inspect_with(Some(labelled(malformed)), false).await?,
            Err(SandboxError::InvalidSpec {
                field: "image",
                reason: "the runtime protocol revision label is malformed".into(),
            }),
            "{malformed:?}"
        );
    }
    assert_eq!(
        inspect_with(Some(labelled("4")), true).await?,
        Err(SandboxError::ImageNotFound)
    );
    Ok(())
}
