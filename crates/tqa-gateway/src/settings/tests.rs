//! GW-U-030 (ADR-0028): snapshot atomicity under concurrent readers and writers, and swaps that never go backwards.
use super::{Settings, Snapshot};
use std::sync::Arc;

/// A registry whose single route's model names its generation, so a reader can tell snapshots apart.
fn snapshot(generation: i64) -> Result<Snapshot, Box<dyn std::error::Error>> {
    let registry = tqa_llm::Registry::from_toml(&format!(
        "[[routes]]\nname='local'\nprotocol='openai_chat'\nbase_url='https://models.example/v1'\nmodel='model-{generation}'\nauth={{kind='none'}}"
    ))?;
    Ok(Snapshot {
        generation,
        registry,
        egress: crate::egress::AllowList::default(),
        display_names: std::collections::BTreeMap::new(),
        descriptions: std::collections::BTreeMap::new(),
        limits: super::limits::Limits {
            slots: 1,
            per_user: 1,
            idle_seconds: 600,
            max_lifetime_seconds: 3600,
            mcp_calls_per_minute: 60,
            team_max_members: 3,
            team_token_budget: 2_000_000,
            team_deadline_minutes: 120,
            member_idle_minutes: 10,
            member_stuck_minutes: 15,
            team_idle_stop_minutes: 30,
            desktop_max_sessions: 2,
            desktop_screenshot_retention_days: 30,
            computer_max_screenshots_per_run: 200,
            computer_max_actions_per_run: 1_000,
        },
        connectors: Vec::new(),
        decision_engines: Vec::new(),
        templates: Vec::new(),
        bindings: crate::secrets::bindings::Bindings::new(),
    })
}

fn model(snapshot: &Snapshot) -> String {
    snapshot
        .registry
        .get("local")
        .map(|route| route.model.clone())
        .unwrap_or_default()
}

#[test]
fn gw_u_030_a_swap_never_installs_an_older_generation() -> Result<(), Box<dyn std::error::Error>> {
    let settings = Settings::new(snapshot(0)?);
    // Two rebuilds that finish out of order: generation 3 is installed first, then the older 2 arrives.
    assert!(settings.install(snapshot(3)?));
    assert!(!settings.install(snapshot(2)?));
    assert!(
        !settings.install(snapshot(3)?),
        "the same generation is not reinstalled"
    );
    assert_eq!(settings.current().generation, 3);
    assert_eq!(model(&settings.current()), "model-3");
    Ok(())
}

/// Reads many snapshots while writers swap: each is internally consistent and generations only move forward.
async fn read_many(settings: Arc<Settings>) {
    let mut last = 0;
    for _ in 0..2_000 {
        let current = settings.current();
        assert_eq!(model(&current), format!("model-{}", current.generation));
        assert!(current.generation >= last);
        last = current.generation;
        tokio::task::yield_now().await;
    }
}

/// Installs 200 newer snapshots, one rebuild at a time.
async fn write_many(settings: Arc<Settings>) -> Result<(), String> {
    for generation in 1..=200 {
        let _rebuilding = settings.rebuilding().await;
        settings.install(snapshot(generation).map_err(|error| error.to_string())?);
        tokio::task::yield_now().await;
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn gw_u_030_readers_see_whole_snapshots_while_writers_swap()
-> Result<(), Box<dyn std::error::Error>> {
    let settings = Arc::new(Settings::new(snapshot(0)?));
    let held = settings.current();
    let readers: Vec<_> = (0..4)
        .map(|_| tokio::spawn(read_many(Arc::clone(&settings))))
        .collect();
    tokio::spawn(write_many(Arc::clone(&settings))).await??;
    for reader in readers {
        reader.await?;
    }
    // A reader that took a snapshot before the swaps keeps using it, whole, for its whole operation.
    assert_eq!((held.generation, model(&held)), (0, "model-0".into()));
    assert_eq!(settings.current().generation, 200);
    Ok(())
}
