//! Bounded page and hot-reload upstream owned by a proxy test.

use axum::{
    Router,
    body::Body,
    extract::{Request, WebSocketUpgrade, ws::Message},
    http::{HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
};
use futures::StreamExt;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

pub struct Web {
    pub origin: String,
    pub requests: Arc<AtomicUsize>,
    server: tokio::task::JoinHandle<()>,
}

impl Web {
    pub async fn start() -> Result<Self, Box<dyn std::error::Error>> {
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
        let origin = format!("http://{}", listener.local_addr()?);
        let requests = Arc::new(AtomicUsize::new(0));
        let captured = requests.clone();
        let app =
            Router::new()
                .route("/_next/hmr", get(socket))
                .fallback(move |request: Request| {
                    captured.fetch_add(1, Ordering::SeqCst);
                    page(&request)
                });
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        Ok(Self {
            origin,
            requests,
            server,
        })
    }

    pub fn config(&self, dev: bool) -> String {
        format!("[web]\nupstream='{}'\ndev={dev}", self.origin)
    }
}

impl Drop for Web {
    fn drop(&mut self) {
        self.server.abort();
    }
}

fn page(request: &Request) -> std::future::Ready<Response> {
    let body = if request.uri().path() == "/stream" {
        let first = futures::stream::once(async { Ok::<_, std::io::Error>("first chunk") });
        Body::from_stream(first.chain(futures::stream::pending()))
    } else {
        let headers = request
            .headers()
            .iter()
            .map(|(key, value)| (key.as_str(), value.to_str().unwrap_or("invalid")))
            .collect::<std::collections::BTreeMap<_, _>>();
        Body::from(serde_json::json!({"method": request.method().as_str(), "uri": request.uri().to_string(), "headers": headers}).to_string())
    };
    let mut response = (StatusCode::NON_AUTHORITATIVE_INFORMATION, body).into_response();
    for (name, value) in [
        ("set-cookie", "synthetic=upstream; HttpOnly"),
        ("connection", "x-private-hop"),
        ("x-private-hop", "must not pass"),
        ("cache-control", "public, max-age=60"),
        ("content-type", "application/json"),
        ("x-upstream", "preserved"),
    ] {
        response
            .headers_mut()
            .insert(name, HeaderValue::from_static(value));
    }
    std::future::ready(response)
}

async fn socket(upgrade: WebSocketUpgrade) -> Response {
    upgrade.on_upgrade(async |mut socket| {
        while let Ok(Some(Ok(message))) =
            tokio::time::timeout(super::fixture::DEADLINE, socket.recv()).await
        {
            if matches!(message, Message::Close(_)) || socket.send(message).await.is_err() {
                break;
            }
        }
    })
}
