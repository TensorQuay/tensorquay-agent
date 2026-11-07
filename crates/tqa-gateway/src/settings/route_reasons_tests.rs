//! GW-U-031: the route validator's reasons and the API's.
use super::reason_of;
use tqa_llm::RouteInvalid;

#[test]
fn gw_u_031_every_validator_reason_is_the_api_reason_with_the_same_code() {
    let reasons = [
        (RouteInvalid::RouteNameInvalid, "name"),
        (RouteInvalid::RouteNameReserved, "name"),
        (RouteInvalid::ModelEmpty, "model"),
        (RouteInvalid::BaseUrlInvalid("x"), "base_url"),
        (RouteInvalid::KeyRefRequired, "key_ref"),
        (RouteInvalid::KeyRefNotReference, "key_ref"),
        (RouteInvalid::KeyRefReserved, "key_ref"),
        (RouteInvalid::AuthHeaderInvalid, "auth"),
        (RouteInvalid::AuthQueryInvalid, "auth"),
        (RouteInvalid::ReasoningInvalid, "reasoning"),
        (RouteInvalid::BudgetInvalid("x"), "reasoning"),
        (RouteInvalid::MaxOutputRequired("x"), "max_output"),
        (RouteInvalid::LimitOutOfRange, "limits"),
        (RouteInvalid::ExtraBodyReserved, "extra_body"),
        (RouteInvalid::ExtraHeaderInvalid("x"), "extra_headers"),
        (RouteInvalid::ExtraHeaderReserved, "extra_headers"),
        (RouteInvalid::ExtraHeaderCredential, "extra_headers"),
        (RouteInvalid::QuirkUnsupported, "quirks"),
        (RouteInvalid::StrictInvalid, "strict"),
        (RouteInvalid::SchemaKeywordsInvalid, "schema_keywords"),
        (
            RouteInvalid::InstructionsRoleInvalid("x"),
            "instructions_role",
        ),
        (RouteInvalid::FirstResponseInvalid, "first_response_secs"),
        (RouteInvalid::FallbackInvalid("x"), "fallbacks"),
    ];
    for (reason, field) in reasons {
        let (got, api) = reason_of(reason.code());
        assert_eq!((got, api.as_str()), (field, reason.code()), "{reason:?}");
    }
}

#[test]
fn gw_u_031_a_reason_this_gateway_does_not_name_yet_is_route_invalid() {
    assert_eq!(
        reason_of("a_future_reason"),
        ("route", crate::InvalidReason::RouteInvalid)
    );
}

#[test]
fn gw_u_031_a_refusal_without_a_route_rule_is_route_invalid()
-> Result<(), Box<dyn std::error::Error>> {
    let source = "[[routes]]\nname='a'\nprotocol='openai_chat'\nbase_url='https://m.example/v1'\nmodel='m'\n";
    let route = tqa_llm::Registry::from_toml(source)?
        .get("a")
        .cloned()
        .ok_or("route a")?;
    let config = tqa_llm::RouteConfig::from(route);
    let error = tqa_llm::Registry::from_configs(vec![config.clone(), config])
        .err()
        .ok_or("a duplicate name is refused")?;
    assert_eq!(error.reason(), None, "a duplicate name has no route rule");
    let refused = super::refusal(&error);
    assert!(
        matches!(
            refused,
            crate::GatewayError::Invalid {
                field: "route",
                reason: crate::InvalidReason::RouteInvalid
            }
        ),
        "{refused:?}"
    );
    Ok(())
}

#[test]
fn gw_u_031_a_route_rule_refusal_names_its_field_and_reason()
-> Result<(), Box<dyn std::error::Error>> {
    let source =
        "[[routes]]\nname='a'\nprotocol='openai_chat'\nbase_url='https://m.example/v1'\nmodel=''\n";
    let error = tqa_llm::Registry::from_toml(source)
        .err()
        .ok_or("an empty model is refused")?;
    let refused = super::refusal(&error);
    assert!(
        matches!(
            refused,
            crate::GatewayError::Invalid {
                field: "model",
                reason: crate::InvalidReason::ModelEmpty
            }
        ),
        "{refused:?}"
    );
    Ok(())
}
