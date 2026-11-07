//! The built-in environments' test host (GW-I-145, GW-I-154, GW-I-172, GW-I-174, GW-I-190, GW-I-213, GW-I-278): a fake
//! engine with the images `build-images.sh` leaves, and the administrator's and people's reads of the environments.
#![allow(dead_code)] // Each integration target compiles this shared harness on its own.

use std::{collections::BTreeMap, sync::Mutex};

use async_trait::async_trait;
use reqwest::Method;
use serde_json::{Value, json};
use tqa_gateway::{EngineUnavailable, EnvironmentImageSource};
use tqa_sandbox::docker::ImageRecord;
use tqa_types::{MIN_RUNTIME_REVISION, RUNTIME_REVISION_LABEL};

use crate::support::{Fixture, call};

pub(crate) type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub(crate) const WORKSTATION: &str =
    "sha256:3333333333333333333333333333333333333333333333333333333333333333";

pub(crate) const REBUILT: &str =
    "sha256:4444444444444444444444444444444444444444444444444444444444444444";

pub(crate) const DERIVED: &str =
    "sha256:5555555555555555555555555555555555555555555555555555555555555555";

pub(crate) const OLD_DATA: &str =
    "sha256:6666666666666666666666666666666666666666666666666666666666666666";

pub(crate) const ADMIN_PIN: &str =
    "sha256:7777777777777777777777777777777777777777777777777777777777777777";

/// 2026-09-01T00:00:00Z and a month later.
pub(crate) const BUILT: i64 = 1_788_220_800;

pub(crate) const LATER: i64 = 1_790_812_800;

/// The engine host's images; `None` is an engine that doesn't answer.
pub(crate) struct Engine(Mutex<Option<Vec<ImageRecord>>>);

impl Engine {
    pub(crate) fn new(images: Vec<ImageRecord>) -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self(Mutex::new(Some(images))))
    }

    pub(crate) fn set(&self, images: Option<Vec<ImageRecord>>) {
        if let Ok(mut held) = self.0.lock() {
            *held = images;
        }
    }
}

#[async_trait]
impl EnvironmentImageSource for Engine {
    async fn list(&self) -> std::result::Result<Vec<ImageRecord>, EngineUnavailable> {
        self.0
            .lock()
            .ok()
            .and_then(|held| held.clone())
            .ok_or(EngineUnavailable)
    }
}

pub(crate) fn image(
    id: &str,
    tags: &[&str],
    created: i64,
    environment: (&str, &str, &str),
    revision: impl std::fmt::Display,
) -> ImageRecord {
    let (name, display, capabilities) = environment;
    ImageRecord {
        id: id.to_owned(),
        tags: tags.iter().map(|tag| (*tag).to_owned()).collect(),
        created,
        size_bytes: 2 << 30,
        labels: BTreeMap::from([
            (RUNTIME_REVISION_LABEL.to_owned(), revision.to_string()),
            ("org.tensorquay.environment".to_owned(), name.to_owned()),
            (
                "org.tensorquay.environment.name".to_owned(),
                display.to_owned(),
            ),
            (
                "org.tensorquay.environment.capabilities".to_owned(),
                capabilities.to_owned(),
            ),
        ]),
    }
}

/// A file whose Minimal is the chosen default, so Workstation registers as a plain built-in (not the default: GW-I-154)
/// and can be edited or deleted like any environment that isn't the default.
pub(crate) fn chosen_minimal() -> String {
    format!(
        "[[templates]]\nname='minimal'\ndisplay_name='Minimal'\nimage='{ADMIN_PIN}'\ncapabilities=['shell']\ndefault=true\n"
    )
}

pub(crate) const WORKSTATION_LABELS: (&str, &str, &str) = (
    "workstation",
    "Workstation",
    "shell,python,node,build,browser",
);

/// Workstation as `build-images.sh --workstation` leaves it, an image a person built FROM it (so it inherits the
/// labels, F10), and a Data image too old for this version.
pub(crate) fn host(workstation: &str, created: i64) -> Vec<ImageRecord> {
    vec![
        image(
            workstation,
            &["tqa-runtime:workstation"],
            created,
            WORKSTATION_LABELS,
            MIN_RUNTIME_REVISION,
        ),
        image(
            DERIVED,
            &["team/data-science:1"],
            created + 3600,
            WORKSTATION_LABELS,
            MIN_RUNTIME_REVISION,
        ),
        image(
            OLD_DATA,
            &["tqa-runtime:data"],
            created,
            ("data", "Data", "shell,python"),
            MIN_RUNTIME_REVISION - 1,
        ),
    ]
}

pub(crate) async fn admin_items(f: &Fixture, cookie: &str) -> Result<Vec<Value>> {
    let (status, listed) = call(
        f,
        Method::GET,
        "/api/v1/admin/templates",
        cookie,
        (None, None),
    )
    .await?;
    assert_eq!(status, 200, "{listed}");
    Ok(listed["items"].as_array().ok_or("items")?.clone())
}

pub(crate) async fn admin_item(f: &Fixture, cookie: &str, name: &str) -> Result<Option<Value>> {
    Ok(admin_items(f, cookie)
        .await?
        .into_iter()
        .find(|item| item["name"] == name))
}

pub(crate) async fn offered(f: &Fixture, cookie: &str) -> Result<Vec<(String, Value, Value)>> {
    let listed: Value = f
        .get("/api/v1/templates", cookie)
        .await?
        .error_for_status()?
        .json()
        .await?;
    Ok(listed["items"]
        .as_array()
        .ok_or("items")?
        .iter()
        .map(|item| {
            (
                item["name"].as_str().unwrap_or_default().to_owned(),
                item["capabilities"].clone(),
                item["is_default"].clone(),
            )
        })
        .collect())
}

pub(crate) async fn reset(
    f: &Fixture,
    cookie: &str,
    name: &str,
    version: i64,
    key: &str,
) -> Result<(u16, Value)> {
    call(
        f,
        Method::POST,
        &format!("/api/v1/admin/templates/{name}/reset"),
        cookie,
        (Some(json!({"version": version})), Some(key)),
    )
    .await
}

/// The offered list once it shows `name`. The administrator's list reads the database and this one the settings
/// snapshot, which a recorded check refreshes just after its commit: a read in between sees the snapshot before it
/// (GW-I-145 under load, 4 Oct 2026).
pub(crate) async fn offered_once(
    f: &Fixture,
    cookie: &str,
    name: &str,
) -> Result<Vec<(String, Value, Value)>> {
    for _ in 0..100 {
        let list = offered(f, cookie).await?;
        if list.iter().any(|item| item.0 == name) {
            return Ok(list);
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    offered(f, cookie).await
}

/// Waits (bounded) until the administrators' list shows a check result for `name`'s current version, and returns its
/// state: the list is the installed settings, so the default it shows is the one that went with that result.
pub(crate) async fn checked(f: &Fixture, cookie: &str, name: &str) -> Result<String> {
    for _ in 0..100 {
        if let Some(item) = admin_item(f, cookie, name).await?
            && item["check"]["state"] != json!("unchecked")
        {
            return Ok(item["check"]["state"]
                .as_str()
                .unwrap_or_default()
                .to_owned());
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    Err(format!("{name} was never checked").into())
}
