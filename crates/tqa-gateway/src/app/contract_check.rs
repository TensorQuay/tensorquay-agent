//! The agent teams' responses checked against `openapi.json` while the gateway's own tests run (GW-U-142). Only with
//! the `testing` feature, never in a release build (REL-U-009). Each response of a checked route is read whole and
//! validated against the schema the API document gives its route, method and status; a mismatch is recorded, never
//! answered differently, and the test fixture fails the test with every recorded mismatch when the gateway stops. A
//! route the document doesn't have, or a status it doesn't list, is a mismatch too.
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock, PoisonError},
};

use axum::{
    body::Body,
    extract::{MatchedPath, Request, State},
    middleware::Next,
    response::Response,
};
use serde_json::{Value, json};

/// The API document, as the gateway publishes it.
const DOCUMENT: &str = include_str!("../../openapi.json");
/// The longest body read for the check; past it the response is recorded as unreadable.
const MAX_BODY: usize = 16 << 20;
/// The mismatches named per response.
const NAMED: usize = 5;

/// The routes checked so far: the agent teams' (the people's, the runtime's and the administrator's).
pub(crate) fn checked(route: &str) -> bool {
    route.starts_with("/api/v1/conversations/{id}/team")
        || route.starts_with("/runtime/teams")
        || route.starts_with("/api/v1/admin/agent-teams")
}

/// One operation's response: the JSON it must match, or `None` for a response without a JSON body.
type Expected = Result<Option<Arc<jsonschema::Validator>>, String>;

/// The check's state on one gateway: the compiled schemas and the mismatches found.
#[derive(Default)]
pub(crate) struct Check {
    expected: Mutex<HashMap<(String, String, u16), Expected>>,
    violations: Mutex<Vec<String>>,
}

impl Check {
    /// Checks one response's `body` against the document's schema for `route`, `method` and `status`.
    pub(crate) fn response(&self, (route, method, status): (&str, &str, u16), body: &[u8]) {
        let key = (route.to_owned(), method.to_owned(), status);
        let expected = self
            .expected
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(key)
            .or_insert_with(|| expected(document(), route, method, status))
            .clone();
        let found = mismatches(&expected, body);
        if !found.is_empty() {
            let named = format!("{} {route} {status}", method.to_ascii_uppercase());
            self.violations
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .extend(found.into_iter().map(|found| format!("{named}: {found}")));
        }
    }

    /// The mismatches found so far, taken: each is reported once.
    pub(crate) fn take(&self) -> Vec<String> {
        std::mem::take(
            &mut *self
                .violations
                .lock()
                .unwrap_or_else(PoisonError::into_inner),
        )
    }
}

/// The API document, parsed once.
fn document() -> &'static Value {
    static PARSED: OnceLock<Value> = OnceLock::new();
    PARSED.get_or_init(|| serde_json::from_str(DOCUMENT).unwrap_or(Value::Null))
}

/// What `document` says a `status` response of `route`'s `method` holds: its JSON schema compiled with the document's
/// components beside it, so `#/components/...` references resolve; `None` when it has no JSON body.
pub(crate) fn expected(document: &Value, route: &str, method: &str, status: u16) -> Expected {
    let escaped = route.replace('~', "~0").replace('/', "~1");
    let operation = document
        .pointer(&format!("/paths/{escaped}/{method}"))
        .ok_or_else(|| "the route is not in openapi.json".to_owned())?;
    let responses = &operation["responses"];
    let response = [
        status.to_string(),
        format!("{}XX", status / 100),
        "default".into(),
    ]
    .iter()
    .find_map(|code| responses.get(code))
    .ok_or_else(|| format!("openapi.json lists no {status} response"))?;
    // A response may itself be a reference to the document's shared responses.
    let response = match response.get("$ref").and_then(Value::as_str) {
        Some(reference) => document
            .pointer(reference.trim_start_matches('#'))
            .ok_or_else(|| format!("openapi.json has no {reference}"))?,
        None => response,
    };
    let Some(schema) = response.pointer("/content/application~1json/schema") else {
        return Ok(None);
    };
    let mut root = json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "components": document["components"],
    });
    if let (Some(root), Some(schema)) = (root.as_object_mut(), schema.as_object()) {
        root.extend(schema.clone());
    }
    jsonschema::validator_for(&root)
        .map(|validator| Some(Arc::new(validator)))
        .map_err(|error| format!("openapi.json's schema doesn't compile: {error}"))
}

/// How `body` misses what was `expected`: none when it matches.
pub(crate) fn mismatches(expected: &Expected, body: &[u8]) -> Vec<String> {
    match expected {
        Err(reason) => vec![reason.clone()],
        Ok(None) if body.is_empty() => Vec::new(),
        Ok(None) => vec!["a body where openapi.json lists none".into()],
        Ok(Some(validator)) => match serde_json::from_slice::<Value>(body) {
            Err(_) => vec!["the body is not JSON".into()],
            Ok(value) => validator
                .iter_errors(&value)
                .take(NAMED)
                .map(|error| format!("at '{}': {error}", error.instance_path()))
                .collect(),
        },
    }
}

/// The router's layer: a checked route's response is read whole, checked and passed on unchanged.
pub(crate) async fn layer(
    State(state): State<Arc<super::State>>,
    request: Request,
    next: Next,
) -> Response {
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map(|matched| matched.as_str().to_owned())
        .filter(|route| checked(route));
    let method = request.method().as_str().to_ascii_lowercase();
    let response = next.run(request).await;
    let Some(route) = route else {
        return response;
    };
    let (parts, body) = response.into_parts();
    let Ok(bytes) = axum::body::to_bytes(body, MAX_BODY).await else {
        state
            .contract
            .violations
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(format!(
                "{} {route}: the body couldn't be read",
                method.to_ascii_uppercase()
            ));
        return Response::from_parts(parts, Body::empty());
    };
    state
        .contract
        .response((&route, &method, parts.status.as_u16()), &bytes);
    Response::from_parts(parts, Body::from(bytes))
}

impl super::Gateway {
    /// The team responses that didn't match `openapi.json` so far, taken (GW-U-142): the test fixture fails a test
    /// with them. Only with the `testing` feature, never in a release build (REL-U-009).
    #[must_use]
    pub fn contract_violations(&self) -> Vec<String> {
        self.state.contract.take()
    }
}

#[cfg(test)]
#[path = "contract_check_tests.rs"]
mod tests;
