use super::{checked, document, expected, mismatches};
use serde_json::{Value, json};

/// A small API document: one route with a referenced schema, a no-body status, and a shared response.
fn spec() -> Value {
    json!({
        "openapi": "3.1.0",
        "paths": {
            "/api/v1/conversations/{id}/team/messages": {
                "get": {"responses": {
                    "200": {"content": {"application/json": {"schema": {"$ref": "#/components/schemas/Page"}}}},
                    "204": {"description": "nothing"},
                    "4XX": {"$ref": "#/components/responses/Failed"}
                }}
            }
        },
        "components": {
            "schemas": {
                "Page": {"type": "object", "required": ["messages"], "additionalProperties": false,
                         "properties": {"messages": {"type": "array", "items": {"$ref": "#/components/schemas/To"}}}},
                "To": {"type": "object", "required": ["kind"],
                       "properties": {"kind": {"enum": ["lead", "member", "members", "team"]}}},
                "Error": {"type": "object", "required": ["error"]}
            },
            "responses": {"Failed": {"content": {"application/json": {"schema": {"$ref": "#/components/schemas/Error"}}}}}
        }
    })
}

const ROUTE: &str = "/api/v1/conversations/{id}/team/messages";

// TEST: GW-U-142
#[test]
fn gw_u_142_a_response_is_checked_against_its_routes_schema() {
    let spec = spec();
    let page = expected(&spec, ROUTE, "get", 200);
    assert_eq!(
        mismatches(&page, br#"{"messages":[{"kind":"lead"}]}"#),
        Vec::<String>::new()
    );
    // A value the schema doesn't allow (the 'person' case of 4 Oct 2026), named with where it is.
    let found = mismatches(
        &page,
        br#"{"messages":[{"kind":"lead"},{"kind":"person"}]}"#,
    );
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].contains("/messages/1/kind"), "{found:?}");
    // A field the schema doesn't have, a body that isn't JSON.
    assert_eq!(mismatches(&page, br#"{"messages":[],"more":1}"#).len(), 1);
    assert_eq!(
        mismatches(&page, b"<html>"),
        vec!["the body is not JSON".to_owned()]
    );
    // A status class and a shared response resolve; no body where none is listed.
    let failed = expected(&spec, ROUTE, "get", 409);
    assert_eq!(
        mismatches(&failed, br#"{"error":{}}"#),
        Vec::<String>::new()
    );
    assert_eq!(mismatches(&failed, b"{}").len(), 1);
    let empty = expected(&spec, ROUTE, "get", 204);
    assert_eq!(mismatches(&empty, b""), Vec::<String>::new());
    assert_eq!(
        mismatches(&empty, b"{}"),
        vec!["a body where openapi.json lists none".to_owned()]
    );
}

// TEST: GW-U-142
#[test]
fn gw_u_142_a_route_or_status_the_document_lacks_is_a_mismatch() {
    let spec = spec();
    assert_eq!(
        mismatches(
            &expected(&spec, "/api/v1/conversations/{id}/team/other", "get", 200),
            b"{}"
        ),
        vec!["the route is not in openapi.json".to_owned()]
    );
    assert_eq!(
        mismatches(&expected(&spec, ROUTE, "post", 200), b"{}"),
        vec!["the route is not in openapi.json".to_owned()]
    );
    assert_eq!(
        mismatches(&expected(&spec, ROUTE, "get", 500), b"{}"),
        vec!["openapi.json lists no 500 response".to_owned()]
    );
}

// TEST: GW-U-142
#[test]
fn gw_u_142_the_team_routes_are_checked_and_the_published_document_compiles() {
    for route in [
        "/api/v1/conversations/{id}/team",
        "/api/v1/conversations/{id}/team/messages",
        "/runtime/teams/messages",
        "/api/v1/admin/agent-teams",
    ] {
        assert!(checked(route), "{route}");
    }
    for route in [
        "/api/v1/conversations/{id}",
        "/api/v1/conversations",
        "/runtime/channels/{id}",
    ] {
        assert!(!checked(route), "{route}");
    }
    // The real document: each team route's every listed response compiles, and an error body that lacks its fields
    // is caught.
    let document = document();
    let paths = document["paths"]
        .as_object()
        .map(serde_json::Map::len)
        .unwrap_or_default();
    assert!(paths > 100, "the published document parsed");
    // Every (route, method, status) the document lists for a checked route.
    let listed: Vec<(&String, &String, u16)> = document["paths"]
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(route, _)| checked(route))
        .flat_map(|(route, operations)| {
            operations
                .as_object()
                .into_iter()
                .flatten()
                .flat_map(move |(method, operation)| {
                    operation["responses"]
                        .as_object()
                        .into_iter()
                        .flatten()
                        .filter_map(move |(status, _)| {
                            Some((route, method, status.parse::<u16>().ok()?))
                        })
                })
        })
        .collect();
    for (route, method, status) in &listed {
        assert!(
            expected(document, route, method, *status).is_ok(),
            "{method} {route} {status}"
        );
    }
    let compiled = listed.len();
    assert!(compiled > 50, "{compiled} responses");
    let refused = expected(document, "/api/v1/conversations/{id}/team", "get", 404);
    assert_eq!(
        mismatches(&refused, b"{}").len(),
        1,
        "an error body without its fields"
    );
}
