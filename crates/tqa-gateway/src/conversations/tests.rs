use super::{Conversation, Record, uuid};
use crate::GatewayError;

#[test]
fn gw_i_004_record_boundaries_reject_unknown_backends_and_malformed_identifiers()
-> Result<(), Box<dyn std::error::Error>> {
    for backend in ["docker", "firecracker", "unknown"] {
        let record = Record {
            id: uuid::Uuid::now_v7(),
            owner_id: uuid::Uuid::now_v7(),
            title: "Fixture".into(),
            title_source: "message".into(),
            model_route: "local".into(),
            backend: backend.into(),
            state: "active".into(),
            template: "minimal".into(),
            pinned: false,
            created_at: time::OffsetDateTime::UNIX_EPOCH,
            updated_at: time::OffsetDateTime::UNIX_EPOCH,
            last_activity_at: time::OffsetDateTime::UNIX_EPOCH,
            workspace_id: uuid::Uuid::now_v7(),
            mcp_servers: Vec::new(),
            connectors: Vec::new(),
            profile_id: None,
            profile_version: None,
            profile_name: None,
            profile_display_name: None,
            profile_sets_route: None,
            profile_sets_template: None,
        };
        let result = Conversation::try_from(record);
        if backend == "unknown" {
            assert!(matches!(result, Err(GatewayError::StoreUnavailable)));
        } else {
            assert!(result.is_ok_and(|conversation| conversation.profile.is_none()));
        }
    }
    assert!(uuid(&"invalid".parse()?).is_err());
    let id = uuid::Uuid::parse_str("018f4ec2-7f71-7fd2-9ba0-31f8bc6f702d")?;
    assert!(uuid(&id.to_string().parse()?).is_ok());
    assert!(uuid(&id.simple().to_string().parse()?).is_err());
    assert!(uuid(&id.to_string().to_uppercase().parse()?).is_err());
    Ok(())
}

#[test]
fn gw_i_025_a_conversation_shows_the_profile_it_was_made_from()
-> Result<(), Box<dyn std::error::Error>> {
    let profile = uuid::Uuid::now_v7();
    let record = |version: Option<i64>| Record {
        id: uuid::Uuid::now_v7(),
        owner_id: uuid::Uuid::now_v7(),
        title: "Fixture".into(),
        title_source: "message".into(),
        model_route: "local".into(),
        backend: "docker".into(),
        state: "active".into(),
        template: "minimal".into(),
        pinned: false,
        created_at: time::OffsetDateTime::UNIX_EPOCH,
        updated_at: time::OffsetDateTime::UNIX_EPOCH,
        last_activity_at: time::OffsetDateTime::UNIX_EPOCH,
        workspace_id: uuid::Uuid::now_v7(),
        mcp_servers: Vec::new(),
        connectors: Vec::new(),
        profile_id: Some(profile),
        profile_version: version,
        profile_name: Some("team".into()),
        profile_display_name: Some("Team".into()),
        profile_sets_route: Some(true),
        profile_sets_template: Some(false),
    };
    let origin = Conversation::try_from(record(Some(3)))?
        .profile
        .ok_or("origin")?;
    assert_eq!(
        (origin.id, origin.version, origin.name.as_str()),
        (profile, 3, "team")
    );
    assert_eq!(origin.sets, ["model_route", "mcp_servers", "connectors"]);
    assert_eq!(origin.display_name, "Team");
    // A row the store's constraint would refuse shows no origin rather than half of one.
    assert!(Conversation::try_from(record(None))?.profile.is_none());
    Ok(())
}
