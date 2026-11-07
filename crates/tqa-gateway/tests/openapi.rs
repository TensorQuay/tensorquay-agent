//! GW-U-022: the route definitions used by the router and the `OpenAPI` operations stay in sync. An operation documented
//! ahead of its code carries `x-tqa-planned` naming the slice that builds it (contract first, ADR-0039): it is left out
//! of the comparison and must not be registered, so the marker comes off in the slice that serves it.

use std::collections::BTreeSet;

#[test]
fn gw_u_022_openapi_and_registered_routes_have_identical_operations()
-> Result<(), Box<dyn std::error::Error>> {
    // The public route table and this machine-checked API contract describe the same operations.
    let document: serde_json::Value = serde_json::from_str(include_str!("../openapi.json"))?;
    assert_eq!(document["openapi"], "3.1.0");
    let (documented, planned) = operations(&document)?;
    let actual = tqa_gateway::registered_routes()
        .map(|(method, path)| (method.to_owned(), path.to_owned()))
        .collect::<BTreeSet<_>>();
    // Checked before the comparison, so a served route still marked says so rather than showing as a mismatch.
    unserved(&planned, &actual);
    assert!(
        planned.is_empty(),
        "every planned operation is served now: {planned:?}"
    );
    assert_eq!(actual, documented);
    assert_eq!(actual.len(), 156);
    // The field-refusal reasons are one closed list: the documented enum is exactly the gateway's.
    let reasons: Vec<&str> = tqa_gateway::InvalidReason::ALL
        .iter()
        .map(|reason| reason.as_str())
        .collect();
    // `details` is one of several shapes (an agent team's runtime routes answer objects); the list is the array one.
    let fields =
        document["components"]["schemas"]["Error"]["properties"]["error"]["properties"]["details"]
            ["anyOf"]
            .as_array()
            .and_then(|shapes| shapes.iter().find(|shape| shape["type"] == "array"))
            .ok_or("Error.details has no field list")?;
    assert_eq!(
        fields["items"]["properties"]["reason"]["enum"],
        serde_json::json!(reasons)
    );
    for (path, method) in [
        ("/relay/r/{route}/chat/completions", "post"),
        ("/relay/r/{route}/responses", "post"),
        ("/relay/r/{route}/messages", "post"),
        ("/relay/r/{route}/models", "get"),
    ] {
        assert_eq!(
            document["paths"][path][method]["security"],
            serde_json::json!([{"runtimeBearer": []}])
        );
        assert!(
            document["paths"][path][method]["responses"]["429"]["headers"]["Retry-After"]
                .is_object()
        );
    }
    let protocol: serde_json::Value =
        serde_json::from_str(include_str!("../../tqa-types/schema/tqa-protocol-v1.json"))?;
    for name in ["ServerFrame", "ClientFrame"] {
        let reference = document["components"]["schemas"][name]["$ref"]
            .as_str()
            .ok_or("missing frame schema reference")?;
        let (_, pointer) = reference.split_once('#').ok_or("missing schema fragment")?;
        assert!(
            protocol.pointer(pointer).is_some(),
            "broken reference: {reference}"
        );
    }
    Ok(())
}

type Operation = (String, String);
/// A planned operation with its marker.
type Planned = (Operation, serde_json::Value);

/// The document's operations as (method, path): the served ones, and the planned ones with their markers.
fn operations(
    document: &serde_json::Value,
) -> Result<(BTreeSet<Operation>, Vec<Planned>), Box<dyn std::error::Error>> {
    let paths = document["paths"].as_object().ok_or("missing paths")?;
    let (planned, documented): (Vec<_>, Vec<_>) = paths
        .iter()
        .flat_map(|(path, value)| {
            value.as_object().into_iter().flat_map(move |operations| {
                operations
                    .iter()
                    .filter(|(method, _)| {
                        ["get", "post", "delete", "put", "patch"].contains(&method.as_str())
                    })
                    .map(move |(method, operation)| {
                        (
                            (method.to_uppercase(), path.clone()),
                            operation["x-tqa-planned"].clone(),
                        )
                    })
            })
        })
        .partition(|(_, marker)| !marker.is_null());
    Ok((
        documented
            .into_iter()
            .map(|(operation, _)| operation)
            .collect(),
        planned,
    ))
}

/// A planned operation names the slice that builds it, and isn't served yet: once it is, the marker comes off.
fn unserved(planned: &[Planned], actual: &BTreeSet<Operation>) {
    for ((method, path), marker) in planned {
        let slice = marker.as_str().unwrap_or_default();
        assert!(
            slice.starts_with("ADR-") && slice.contains(" slice "),
            "{method} {path}: x-tqa-planned must name an ADR's slice, not {marker}"
        );
        assert!(
            !actual.contains(&(method.clone(), path.clone())),
            "{method} {path} is served: take its x-tqa-planned marker off"
        );
    }
}
