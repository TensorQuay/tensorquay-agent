//! GW-U-069: roles and routes as a pure table (ADR-0034, the slice S1 contract): which role may call which route,
//! whom each post is addressed to, and who reads what. A member's `start_member` is refused (depth 1).
use super::{Addressee, Role, Route, addressee, allowed, reads};
use crate::error::GatewayError;
use tqa_types::{TeamMessageType, TeamPost};

fn code(result: Result<(), GatewayError>) -> Option<&'static str> {
    result.err().map(|error| error.code())
}

const LEAD_ROUTES: [Route; 7] = [
    Route::StartMember,
    Route::Post(TeamMessageType::Assign),
    Route::Post(TeamMessageType::Question),
    Route::Post(TeamMessageType::Decision),
    Route::Post(TeamMessageType::PersonQuestion),
    Route::TakeFiles,
    Route::StopMember,
];
const MEMBER_POSTS: [Route; 3] = [
    Route::Post(TeamMessageType::Answer),
    Route::Post(TeamMessageType::Report),
    Route::Post(TeamMessageType::Ask),
];

#[test]
fn gw_u_069_the_lead_calls_the_lead_routes_and_reads() {
    for route in LEAD_ROUTES
        .into_iter()
        .chain([Route::Done, Route::ReadHelper, Route::Read])
    {
        assert_eq!(code(allowed(Some(Role::Lead), route)), None, "{route:?}");
    }
    for route in MEMBER_POSTS {
        assert_eq!(
            code(allowed(Some(Role::Lead), route)),
            Some("members_only"),
            "{route:?}"
        );
    }
}

#[test]
fn gw_u_069_a_member_posts_to_its_lead_and_reads_and_nothing_else() {
    for route in MEMBER_POSTS.into_iter().chain([Route::Read]) {
        assert_eq!(code(allowed(Some(Role::Member), route)), None, "{route:?}");
    }
    for route in LEAD_ROUTES
        .into_iter()
        .chain([Route::Done, Route::ReadHelper])
    {
        assert_eq!(
            code(allowed(Some(Role::Member), route)),
            Some("not_lead"),
            "{route:?}"
        );
    }
}

#[test]
fn gw_u_069_a_conversation_outside_a_team_may_only_start_one() {
    assert_eq!(code(allowed(None, Route::StartMember)), None);
    for route in LEAD_ROUTES.into_iter().skip(1).chain(MEMBER_POSTS).chain([
        Route::Done,
        Route::ReadHelper,
        Route::Read,
    ]) {
        assert_eq!(
            code(allowed(None, route)),
            Some("not_a_member"),
            "{route:?}"
        );
    }
}

#[test]
fn gw_u_069_the_gateway_and_people_write_the_other_types() {
    for kind in [
        TeamMessageType::PersonAnswer,
        TeamMessageType::PersonMessage,
        TeamMessageType::Notice,
        TeamMessageType::Done,
    ] {
        for role in [None, Some(Role::Lead), Some(Role::Member)] {
            assert!(
                allowed(role, Route::Post(kind)).is_err(),
                "{role:?} {kind:?}"
            );
        }
    }
}

#[test]
fn gw_u_069_each_post_is_addressed_by_its_type() {
    let text = || "x".to_owned();
    assert_eq!(
        addressee(&TeamPost::Assign {
            to: "researcher".into(),
            text: text()
        }),
        Addressee::Member("researcher".into())
    );
    assert_eq!(
        addressee(&TeamPost::Question { text: text() }),
        Addressee::Members
    );
    assert_eq!(
        addressee(&TeamPost::Decision {
            text: text(),
            question_seq: None
        }),
        Addressee::Members
    );
    assert_eq!(
        addressee(&TeamPost::PersonQuestion { text: text() }),
        Addressee::Person
    );
    for post in [
        TeamPost::Answer {
            question_seq: 1,
            text: text(),
        },
        TeamPost::Report {
            text: text(),
            files: vec![],
        },
        TeamPost::Ask { text: text() },
    ] {
        assert_eq!(addressee(&post), Addressee::Lead, "{post:?}");
    }
}

#[test]
fn gw_u_069_a_member_reads_only_what_is_for_it_and_the_lead_only_what_is_for_the_lead() {
    let me = "researcher";
    let other = Addressee::Member("reviewer".into());
    assert!(reads(Role::Member, me, &Addressee::Member(me.into())));
    assert!(reads(Role::Member, me, &Addressee::Members));
    assert!(reads(Role::Member, me, &Addressee::Team));
    assert!(!reads(Role::Member, me, &other));
    assert!(!reads(Role::Member, me, &Addressee::Lead));
    assert!(!reads(Role::Member, me, &Addressee::Person));
    assert!(reads(Role::Lead, "lead", &Addressee::Lead));
    assert!(reads(Role::Lead, "lead", &Addressee::Team));
    for addressee in [other, Addressee::Members, Addressee::Person] {
        assert!(!reads(Role::Lead, "lead", &addressee), "{addressee:?}");
    }
}
