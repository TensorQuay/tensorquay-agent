use super::{EventQuery, encode};
use crate::GatewayError;
use axum::{
    http::{HeaderMap, HeaderValue},
    response::{IntoResponse, Sse, sse::KeepAlive},
};
use futures::StreamExt;
use std::{convert::Infallible, time::Duration};
use tqa_types::{ErrorCode, ServerFrame};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn proto_u_006_stream_version_and_replay_headers_are_exact() -> TestResult {
    let query = |protocol| EventQuery {
        after: None,
        limit: None,
        stream: Some(1),
        protocol,
    };
    let mut headers = HeaderMap::new();
    assert!(query(None).stream_start(&headers).is_err());
    assert_eq!(query(Some(1)).stream_start(&headers)?, 0.into());
    assert!(matches!(
        query(Some(2)).stream_start(&headers),
        Err(GatewayError::UnsupportedProtocol(_))
    ));
    for text in [
        "-1",
        "",
        "1,2",
        " 1",
        "18446744073709551615",
        "18446744073709551616",
    ] {
        headers.insert("last-event-id", HeaderValue::from_str(text)?);
        assert!(query(Some(1)).stream_start(&headers).is_err(), "{text:?}");
    }
    headers.insert("last-event-id", HeaderValue::from_bytes(&[0xff])?);
    assert!(query(Some(1)).stream_start(&headers).is_err());
    headers.insert("last-event-id", HeaderValue::from_static("42"));
    assert_eq!(query(Some(1)).stream_start(&headers)?, 42.into());
    headers.append("last-event-id", HeaderValue::from_static("42"));
    assert!(query(Some(1)).stream_start(&headers).is_err());
    let invalid = EventQuery {
        after: Some(1),
        limit: Some(1),
        stream: Some(9),
        protocol: Some(1),
    };
    assert!(invalid.is_stream().is_err());
    assert!(invalid.stream_start(&HeaderMap::new()).is_err());
    Ok(())
}

#[test]
fn gw_i_005_pagination_defaults_and_limits_match_the_store() -> TestResult {
    let page = EventQuery {
        after: None,
        limit: None,
        stream: None,
        protocol: None,
    };
    assert_eq!(page.page(1000)?, (0.into(), 100));
    assert!(!page.is_stream()?);
    assert!(
        !EventQuery {
            stream: Some(0),
            ..page
        }
        .is_stream()?
    );
    assert!(
        EventQuery {
            stream: Some(1),
            ..page
        }
        .is_stream()?
    );
    for limit in [0, 1001] {
        assert!(
            EventQuery {
                limit: Some(limit),
                ..page
            }
            .page(1000)
            .is_err()
        );
    }
    let outside_store = EventQuery {
        after: Some(u64::MAX),
        limit: Some(1),
        stream: None,
        protocol: None,
    };
    assert!(outside_store.page(1000).is_err());
    Ok(())
}

#[tokio::test(start_paused = true)]
async fn gw_i_007_sse_emits_sequence_ids_json_data_and_keepalive_comments() -> TestResult {
    let durable = ServerFrame::Error {
        seq: Some(7.into()),
        code: ErrorCode::Io,
        message: "line one\nline two".into(),
        retryable: false,
        target: None,
    };
    let control = ServerFrame::Error {
        seq: None,
        code: ErrorCode::Busy,
        message: String::new(),
        retryable: true,
        target: None,
    };
    let items = futures::stream::iter(vec![
        Ok::<_, Infallible>(encode(&control)?),
        Ok(encode(&durable)?),
    ])
    .chain(futures::stream::pending());
    let response = Sse::new(items)
        .keep_alive(
            KeepAlive::new()
                .interval(Duration::from_secs(15))
                .text("keep-alive"),
        )
        .into_response();
    let mut stream = response.into_body().into_data_stream();
    let first = stream.next().await.ok_or("missing control")??;
    let first = std::str::from_utf8(&first)?;
    assert!(first.starts_with("data:"));
    assert!(!first.contains("id:"));
    let second = stream.next().await.ok_or("missing event")??;
    let second = std::str::from_utf8(&second)?;
    assert!(second.contains("id: 7\n"));
    assert!(second.contains("\\nline two"));
    tokio::time::advance(Duration::from_secs(15)).await;
    let comment = stream.next().await.ok_or("missing comment")??;
    assert_eq!(std::str::from_utf8(&comment)?, ": keep-alive\n\n");
    Ok(())
}

#[test]
fn web_u_002_stream_cursor_fallback_and_native_header_precedence() -> TestResult {
    let mut query = EventQuery {
        after: Some(12),
        limit: None,
        stream: Some(1),
        protocol: Some(1),
    };
    let mut headers = HeaderMap::new();
    assert_eq!(query.stream_start(&headers)?, 12.into());
    headers.insert("last-event-id", HeaderValue::from_static("15"));
    assert_eq!(query.stream_start(&headers)?, 15.into());
    query.limit = Some(1);
    assert!(query.stream_start(&headers).is_err());
    query.limit = None;
    query.after = Some(u64::MAX);
    assert!(query.stream_start(&headers).is_err());
    for invalid in [
        "after=-1",
        "after=abc",
        "after=1&after=2",
        "after=18446744073709551616",
        "after=%2B1",
    ] {
        let uri = format!("/events?stream=1&protocol=1&{invalid}").parse::<axum::http::Uri>()?;
        assert!(
            axum::extract::Query::<EventQuery>::try_from_uri(&uri).is_err(),
            "{invalid}"
        );
    }
    Ok(())
}
