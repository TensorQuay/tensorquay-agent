//! GW-I-025 (create), GW-I-085, GW-I-086 (ADR-0012): a conversation made from a profile takes what the profile sets and
//! keeps a copy; a request can never widen it; and a profile whose route, template or connector goes away becomes
//! unusable while conversations already made keep their copy.
#[path = "support/fixture.rs"]
mod support;
use reqwest::Method;
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{Fixture, TestResult, call};
use tqa_types::ConversationId;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const EXTRA: &str = "sha256:1111111111111111111111111111111111111111111111111111111111111111";

fn config() -> String {
    format!(
        "[run_limits]\nmax_steps=17\nwall_clock_secs=300\ntool_result_bytes=4096\n\
         [[mcp.servers]]\nname='docs'\ncommand='/usr/local/bin/docs-mcp'\nenabled_by_default=true\n\
         [[connectors]]\nname='tracker'\nurl='https://mcp.tracker.example/mcp'\nenabled_by_default=true\n\
         [[connectors]]\nname='wiki'\nurl='https://mcp.wiki.example/mcp'\n\
         [[templates]]\nname='workstation'\ndisplay_name='W'\nimage='{EXTRA}'\ncapabilities=['shell']\ndefault=true\n\
         [[templates]]\nname='extra'\ndisplay_name='Extra'\nimage='{EXTRA}'\ncapabilities=['shell']\n"
    )
}

fn registry() -> Result<tqa_llm::Registry> {
    let route = |name: &str| {
        format!(
            "[[routes]]\nname='{name}'\nprotocol='openai_chat'\nbase_url='https://models.example/v1'\nmodel='{name}-model'\nauth={{kind='none'}}\n"
        )
    };
    Ok(tqa_llm::Registry::from_toml(&format!(
        "{}{}",
        route("local"),
        route("spare")
    ))?)
}

/// A profile naming the spare route, the extra template and the wiki connector, with lower limits than the install's.
fn restricted(name: &str) -> Value {
    json!({
        "name": name, "display_name": "Restricted", "model_route": "spare", "template": "extra",
        "mcp_servers": [], "connectors": ["wiki"], "limits": {"max_steps": 5},
        "instructions": "Review only.", "tools": ["read_file", "grep"], "skills": ["notes"],
    })
}

async fn profile(f: &Fixture, cookie: &str, body: Value) -> Result<String> {
    let (status, created) = call(
        f,
        Method::POST,
        "/api/v1/profiles",
        cookie,
        (Some(body), None),
    )
    .await?;
    assert_eq!(status, 201, "{created}");
    Ok(created["id"].as_str().ok_or("id")?.to_owned())
}

async fn create(f: &Fixture, cookie: &str, key: &str, body: Value) -> Result<(u16, Value)> {
    call(
        f,
        Method::POST,
        "/api/v1/conversations",
        cookie,
        (Some(body), Some(key)),
    )
    .await
}

fn refused(reply: &(u16, Value)) -> (u16, &Value, &Value) {
    (
        reply.0,
        &reply.1["error"]["code"],
        &reply.1["error"]["details"][0]["field"],
    )
}

fn id(created: &Value) -> Result<ConversationId> {
    Ok(created["id"].as_str().ok_or("id")?.parse()?)
}

/// A fresh session's configuration for the conversation.
async fn session(f: &Fixture, id: &ConversationId) -> Result<Value> {
    let runtime = f.runtime_for(id).await?;
    Ok(runtime.config.clone())
}

fn names(entries: &Value) -> Vec<&str> {
    entries
        .as_array()
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| entry["name"].as_str())
                .collect()
        })
        .unwrap_or_default()
}

// TEST: GW-I-025
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_025_a_conversation_from_a_profile_keeps_its_copy(pool: PgPool) -> TestResult {
    Box::pin(support::run_configured(pool, Some(registry()?), &config(), async |f| {
        f.admin().await?;
        let (_, cookie) = f.member("owner@example.invalid").await?;
        let profile_id = profile(f, &cookie, restricted("restricted")).await?;
        let (status, created) = create(f, &cookie, "one", json!({"title": "From a profile", "profile": profile_id})).await?;
        assert_eq!(status, 201, "{created}");
        assert_eq!(
            (&created["model_route"], &created["template"], &created["mcp_servers"], &created["connectors"]),
            (&json!("spare"), &json!("extra"), &json!([]), &json!(["wiki"]))
        );
        assert_eq!(
            created["profile"],
            json!({"id": profile_id, "version": 1, "name": "restricted", "display_name": "Restricted",
                "sets": ["model_route", "template", "mcp_servers", "connectors"]})
        );
        let conversation = id(&created)?;
        // The session narrows the install's limits.
        let config = session(f, &conversation).await?;
        assert_eq!(
            config["limits"],
            json!({"max_steps": 5, "wall_clock_secs": 300, "tool_result_bytes": 4096, "max_output_tokens": 131_072})
        );
        assert_eq!(names(&config["mcp"]), ["wiki"]);
        // What only the runtime applies travels as the revision-8 profile snapshot.
        assert_eq!(
            config["profile"],
            json!({"name": "restricted", "instructions": "Review only.", "tools": ["read_file", "grep"], "skills": ["notes"]})
        );
        // Editing, then deleting, the profile changes nothing the conversation keeps.
        let path = format!("/api/v1/profiles/{profile_id}");
        let mut edited = restricted("restricted");
        edited["connectors"] = json!(["tracker"]);
        edited["limits"] = json!({"max_steps": 2});
        edited["version"] = json!(1);
        assert_eq!(call(f, Method::PUT, &path, &cookie, (Some(edited), None)).await?.0, 200);
        assert_eq!(call(f, Method::DELETE, &format!("{path}?version=2"), &cookie, (None, None)).await?.0, 204);
        let (_, kept) = call(f, Method::GET, &format!("/api/v1/conversations/{conversation}"), &cookie, (None, None)).await?;
        assert_eq!((&kept["connectors"], &kept["profile"]["version"]), (&json!(["wiki"]), &json!(1)));
        let config = session(f, &conversation).await?;
        assert_eq!(config["limits"]["max_steps"], json!(5));
        // A conversation made without a profile is as before.
        let (_, plain) = create(f, &cookie, "plain", json!({"title": "Plain", "model_route": "local"})).await?;
        assert!(plain.get("profile").is_none());
        let config = session(f, &id(&plain)?).await?;
        assert!(config.get("profile").is_none());
        assert_eq!(config["limits"]["max_steps"], json!(17));
        Ok(())
    }))
    .await
}

// TEST: GW-I-085
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_085_a_request_never_widens_a_profile(pool: PgPool) -> TestResult {
    Box::pin(support::run_configured(
        pool,
        Some(registry()?),
        &config(),
        async |f| {
            f.admin().await?;
            let (_, cookie) = f.member("owner@example.invalid").await?;
            let (_, other) = f.member("other@example.invalid").await?;
            let profile_id = profile(f, &cookie, restricted("restricted")).await?;
            let from = |extra: Value| {
                let mut body = json!({"title": "T", "profile": profile_id});
                if let (Some(body), Some(extra)) = (body.as_object_mut(), extra.as_object()) {
                    body.extend(extra.clone());
                }
                body
            };
            // Another route or template than the profile sets is refused; the same one is accepted.
            let route = create(f, &cookie, "route", from(json!({"model_route": "local"}))).await?;
            assert_eq!(
                refused(&route),
                (422, &json!("set_by_profile"), &json!("model_route"))
            );
            let template = create(
                f,
                &cookie,
                "template",
                from(json!({"template": "workstation"})),
            )
            .await?;
            assert_eq!(
                refused(&template),
                (422, &json!("set_by_profile"), &json!("template"))
            );
            let (status, created) = create(
                f,
                &cookie,
                "same",
                from(json!({"model_route": "spare", "template": "extra"})),
            )
            .await?;
            assert_eq!(status, 201, "{created}");
            let conversation = id(&created)?;
            // Another user's personal profile is indistinguishable from none.
            let theirs = create(
                f,
                &other,
                "theirs",
                json!({"title": "T", "profile": profile_id}),
            )
            .await?;
            assert_eq!(
                refused(&theirs),
                (422, &json!("unknown_reference"), &json!("profile"))
            );
            // PATCH may narrow the profile's set, never add to it or change its route.
            let path = format!("/api/v1/conversations/{conversation}");
            for (key, body, field) in [
                (
                    "add",
                    json!({"connectors": ["wiki", "tracker"]}),
                    "connectors",
                ),
                ("server", json!({"mcp_servers": ["docs"]}), "mcp_servers"),
                ("route", json!({"route": "local"}), "route"),
            ] {
                let reply = call(f, Method::PATCH, &path, &cookie, (Some(body), Some(key))).await?;
                assert_eq!(
                    refused(&reply),
                    (422, &json!("set_by_profile"), &json!(field)),
                    "{key}"
                );
            }
            let narrowed = call(
                f,
                Method::PATCH,
                &path,
                &cookie,
                (Some(json!({"connectors": []})), Some("narrow")),
            )
            .await?;
            assert_eq!((narrowed.0, &narrowed.1["connectors"]), (200, &json!([])));
            let switch = call(
                f,
                Method::PUT,
                &format!("{path}/environment"),
                &cookie,
                (Some(json!({"template": "workstation"})), Some("switch")),
            )
            .await?;
            assert_eq!(
                refused(&switch),
                (422, &json!("set_by_profile"), &json!("template"))
            );
            // Higher limits than the install's give the install's.
            let loose = json!({"name": "loose", "display_name": "Loose", "model_route": "local",
            "limits": {"max_steps": 400}});
            let loose_id = profile(f, &cookie, loose).await?;
            let (_, created) = create(
                f,
                &cookie,
                "loose",
                json!({"title": "T", "profile": loose_id}),
            )
            .await?;
            // It names a route but no template: the template is the request's to choose.
            assert_eq!(
                created["profile"]["sets"],
                json!(["model_route", "mcp_servers", "connectors"])
            );
            let config = session(f, &id(&created)?).await?;
            assert_eq!(config["limits"]["max_steps"], json!(17));
            // ADR-0035 removed a profile's policy and rules: a body that still sends them is refused.
            for removed in [json!({"policy": "read_only"}), json!({"rules": []})] {
                let mut body = json!({"name": "old", "display_name": "Old"});
                body.as_object_mut()
                    .ok_or("body")?
                    .extend(removed.as_object().ok_or("field")?.clone());
                let written = call(
                    f,
                    Method::POST,
                    "/api/v1/profiles",
                    &cookie,
                    (Some(body), None),
                )
                .await?;
                assert_eq!(written.0, 400, "{}", written.1);
            }
            Ok(())
        },
    ))
    .await
}

/// The profile as its owner lists it.
async fn listed(f: &Fixture, cookie: &str, name: &str) -> Result<Value> {
    let (_, listed) = call(f, Method::GET, "/api/v1/profiles", cookie, (None, None)).await?;
    Ok(listed["items"]
        .as_array()
        .ok_or("items")?
        .iter()
        .find(|item| item["name"] == name)
        .cloned()
        .ok_or("listed")?)
}

async fn unusable(f: &Fixture, cookie: &str, name: &str, id: &str, field: &str) -> TestResult {
    let shown = listed(f, cookie, name).await?;
    assert_eq!(
        (&shown["usable"], &shown["unusable_field"]),
        (&json!(false), &json!(field)),
        "{name}"
    );
    let reply = create(
        f,
        cookie,
        &format!("unusable-{name}"),
        json!({"title": "T", "profile": id}),
    )
    .await?;
    assert_eq!(
        refused(&reply),
        (409, &json!("profile_unusable"), &json!(field)),
        "{name}"
    );
    Ok(())
}

// TEST: GW-I-086
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_086_a_profile_whose_references_go_is_unusable_and_its_conversations_keep_their_copy(
    pool: PgPool,
) -> TestResult {
    Box::pin(support::run_configured(pool, Some(registry()?), &config(), async |f| {
        let (_, admin) = f.admin().await?;
        let (_, cookie) = f.member("owner@example.invalid").await?;
        let one = |name: &str, field: &str, value: Value| {
            let mut body = json!({"name": name, "display_name": name, "model_route": "local"});
            body[field] = value;
            body
        };
        let by_template = profile(f, &cookie, one("by-template", "template", json!("extra"))).await?;
        let by_route = profile(f, &cookie, one("by-route", "model_route", json!("spare"))).await?;
        let by_connector = profile(f, &cookie, one("by-connector", "connectors", json!(["wiki"]))).await?;
        let mut shared = one("shared-extra", "template", json!("extra"));
        shared["shared"] = json!(true);
        profile(f, &admin, shared).await?;
        let (_, made) = create(f, &cookie, "made", json!({"title": "T", "profile": by_connector})).await?;
        let conversation = id(&made)?;
        assert_eq!(listed(f, &cookie, "by-connector").await?["usable"], json!(true));
        // Deleting a template a profile names is refused, naming the shared ones and counting the personal one.
        let deleted = call(f, Method::DELETE, "/api/v1/admin/templates/extra?version=1", &admin, (None, None)).await?;
        assert_eq!(
            (deleted.0, &deleted.1["error"]["in_use"]["profiles"], &deleted.1["error"]["in_use"]["personal_profiles"]),
            (409, &json!(["shared-extra"]), &json!(1))
        );
        // A template no longer offered, a route removed, a connector disabled.
        let off = json!({"display_name": "Extra", "image": EXTRA, "capabilities": ["shell"], "enabled": false, "version": 1});
        let (status, body) = call(f, Method::PUT, "/api/v1/admin/templates/extra", &admin, (Some(off), None)).await?;
        assert_eq!(status, 200, "{body}");
        assert_eq!(call(f, Method::DELETE, "/api/v1/admin/routes/spare?version=1", &admin, (None, None)).await?.0, 204);
        let disabled = json!({"url": "https://mcp.wiki.example/mcp", "enabled": false, "version": 1});
        let (status, body) = call(f, Method::PUT, "/api/v1/admin/connectors/wiki", &admin, (Some(disabled), None)).await?;
        assert_eq!(status, 200, "{body}");
        unusable(f, &cookie, "by-template", &by_template, "template").await?;
        unusable(f, &cookie, "by-route", &by_route, "model_route").await?;
        unusable(f, &cookie, "by-connector", &by_connector, "connectors").await?;
        // The conversation made earlier keeps its copy; the disabled connector is left out of its session.
        let (_, kept) = call(f, Method::GET, &format!("/api/v1/conversations/{conversation}"), &cookie, (None, None)).await?;
        assert_eq!(kept["connectors"], json!(["wiki"]));
        assert!(names(&session(f, &conversation).await?["mcp"]).is_empty());
        Ok(())
    }))
    .await
}

// TEST: GW-I-025
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_025_administrators_list_the_tool_servers_a_shared_profile_may_name(
    pool: PgPool,
) -> TestResult {
    Box::pin(support::run_configured(pool, Some(registry()?), &config(), async |f| {
        let (_, admin) = f.admin().await?;
        let (_, member) = f.member("owner@example.invalid").await?;
        let (status, listed) = call(f, Method::GET, "/api/v1/admin/mcp/servers", &admin, (None, None)).await?;
        assert_eq!(
            (status, listed),
            (200, json!({"items": [{"name": "docs", "templates": [], "enabled_by_default": true}]}))
        );
        // Names only: nothing of the command, its arguments or its environment.
        let (status, _) = call(f, Method::GET, "/api/v1/admin/mcp/servers", &member, (None, None)).await?;
        assert_eq!(status, 403);
        Ok(())
    }))
    .await
}
