//! GW-I-077 (ADR-0032): the template checker, with a fake engine side. Every template version is checked once, with
//! the file-provided templates as its bases; results are stored per version with their reason codes; engine trouble
//! never marks a template verified; a result for a version the template has since left is discarded.
#[path = "support/log_capture.rs"]
mod log_capture;
#[path = "support/fixture.rs"]
mod support;

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use async_trait::async_trait;
use reqwest::Method;
use sqlx::PgPool;
use support::{Fixture, TestResult, call};
use tqa_gateway::{CheckOutcome, CheckReason, CheckRequest, TemplateCheck};
use tqa_sandbox::template::secret_files::{Bound, Bounds, Rule, SecretFile, SecretFiles};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
/// A stored check: state, reason and the version it was taken for.
type Stored = (String, Option<String>, Option<i64>);
/// What one check was asked: the image, the bases and the sandbox environment keys.
type Request = (String, Vec<(String, String)>, Vec<String>);

const PIN: &str = "sha256:1111111111111111111111111111111111111111111111111111111111111111";

fn templates() -> String {
    format!(
        "[[templates]]\nname='workstation'\ndisplay_name='W'\nimage='{PIN}'\ncapabilities=['shell']\ndefault=true\n\n\
         [[templates]]\nname='minimal'\ndisplay_name='M'\nimage='{PIN}'\ncapabilities=['shell']\n"
    )
}

/// Answers from a function of the request; records the bases each request carried. With `changes`, it first moves
/// that template to its next version, as an update landing during the check would.
struct Fake {
    answer: Box<dyn Fn(&CheckRequest<'_>) -> CheckOutcome + Send + Sync>,
    seen: Mutex<Vec<(String, Vec<String>)>>,
    /// Each request's image, bases (name and image) and sandbox environment keys.
    requests: Mutex<Vec<Request>>,
    changes: Option<(PgPool, String)>,
}

impl Fake {
    fn new(
        answer: impl Fn(&CheckRequest<'_>) -> CheckOutcome + Send + Sync + 'static,
    ) -> Arc<Self> {
        Arc::new(Self {
            answer: Box::new(answer),
            seen: Mutex::new(Vec::new()),
            requests: Mutex::new(Vec::new()),
            changes: None,
        })
    }
}

#[async_trait]
impl TemplateCheck for Fake {
    async fn check(&self, request: &CheckRequest<'_>) -> CheckOutcome {
        if let Ok(mut seen) = self.seen.lock() {
            let bases = request.bases.iter().map(|(name, _)| name.clone()).collect();
            seen.push((request.name.to_owned(), bases));
        }
        if let Ok(mut requests) = self.requests.lock() {
            requests.push((
                request.image.to_owned(),
                request.bases.to_vec(),
                request.sandbox_env.iter().cloned().collect(),
            ));
        }
        if let Some((pool, name)) = &self.changes
            && name == request.name
            && sqlx::query("UPDATE templates SET version=version+1 WHERE name=$1 AND version=1")
                .bind(name)
                .execute(pool)
                .await
                .is_err()
        {
            return CheckOutcome::Unavailable;
        }
        (self.answer)(request)
    }
}

async fn stored(pool: &PgPool, name: &str) -> Result<Stored> {
    Ok(sqlx::query_as(
        "SELECT check_state,check_reason,checked_version FROM templates WHERE name=$1",
    )
    .bind(name)
    .fetch_one(pool)
    .await?)
}

/// Waits (bounded) until `name`'s stored check is `expected`.
async fn until_stored(pool: &PgPool, name: &str, expected: &Stored) -> TestResult {
    for _ in 0..100 {
        if &stored(pool, name).await? == expected {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Err(format!(
        "{name}: {:?}, expected {expected:?}",
        stored(pool, name).await?
    )
    .into())
}

async fn pending(f: &Fixture, cookie: &str) -> Result<serde_json::Value> {
    let (_, detail) = call(f, Method::GET, "/readyz?detail=1", cookie, (None, None)).await?;
    Ok(detail["templates"]["checks_pending"].clone())
}

// TEST: GW-I-026
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_077_each_version_is_checked_against_the_file_bases_and_stored_with_its_reason(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool, None, &templates(), async |f| {
        let (_, cookie) = f.admin().await?;
        let fake = Fake::new(|request| match request.name {
            "minimal" => CheckOutcome::Refused(CheckReason::ImageUser),
            _ => CheckOutcome::Verified {
                warnings: SecretFiles::default(),
                base: request.name.to_owned(),
                image_id: PIN.into(),
            },
        });
        f.gateway.start_template_checks(fake.clone());
        until_stored(&f.pool, "workstation", &("verified".into(), None, Some(1))).await?;
        until_stored(
            &f.pool,
            "minimal",
            &("refused".into(), Some("image_user".into()), Some(1)),
        )
        .await?;
        let seen = fake.seen.lock().map_err(|_| "lock")?.clone();
        assert!(!seen.is_empty());
        for (_, bases) in &seen {
            assert_eq!(
                bases,
                &["minimal", "workstation"],
                "the file-provided templates are the bases"
            );
        }
        let checks_before = seen.len();
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert_eq!(
            fake.seen.lock().map_err(|_| "lock")?.len(),
            checks_before,
            "each version is checked once"
        );
        assert_eq!(pending(f, &cookie).await?, 0);
        let base: Option<String> =
            sqlx::query_scalar("SELECT base FROM templates WHERE name='workstation'")
                .fetch_one(&f.pool)
                .await?;
        assert_eq!(base.as_deref(), Some("workstation"));
        Ok(())
    })
    .await
}

// TEST: GW-I-026
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_077_engine_trouble_never_marks_a_template_verified(pool: PgPool) -> TestResult {
    support::run_configured(pool, None, &templates(), async |f| {
        let (_, cookie) = f.admin().await?;
        let fake = Fake::new(|_| CheckOutcome::Unavailable);
        f.gateway.start_template_checks(fake.clone());
        for _ in 0..100 {
            if fake.seen.lock().map_err(|_| "lock")?.len() >= 2 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(
            fake.seen.lock().map_err(|_| "lock")?.len() >= 2,
            "both templates were tried"
        );
        for name in ["workstation", "minimal"] {
            assert_eq!(
                stored(&f.pool, name).await?,
                ("unchecked".into(), None, None),
                "{name}"
            );
        }
        assert_eq!(
            pending(f, &cookie).await?,
            2,
            "the readiness detail reports the waiting versions"
        );
        Ok(())
    })
    .await
}

// TEST: GW-I-026
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_077_a_result_for_a_version_the_template_has_left_is_discarded(
    pool: PgPool,
) -> TestResult {
    support::run_configured(pool.clone(), None, &templates(), async |f| {
        // While `minimal` version 1 is being checked, it changes (as an update would): the result belongs to nothing.
        // Only that first check gets an answer; version 2's finds the engine unavailable. So what is stored shows
        // whether version 1's result was discarded: it must never appear as a verified version 1.
        let minimal_checks = std::sync::atomic::AtomicUsize::new(0);
        let fake = Arc::new(Fake {
            answer: Box::new(move |request| {
                let first = request.name != "minimal"
                    || minimal_checks.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0;
                if first {
                    CheckOutcome::Verified {
                        warnings: SecretFiles::default(),
                        base: "workstation".into(),
                        image_id: PIN.into(),
                    }
                } else {
                    CheckOutcome::Unavailable
                }
            }),
            seen: Mutex::new(Vec::new()),
            requests: Mutex::new(Vec::new()),
            changes: Some((pool.clone(), "minimal".into())),
        });
        f.gateway.start_template_checks(fake.clone());
        let minimal = |fake: &Fake| -> usize {
            fake.seen.lock().map_or(0, |seen| {
                seen.iter().filter(|(name, _)| name == "minimal").count()
            })
        };
        for _ in 0..100 {
            if minimal(&fake) >= 2 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert_eq!(minimal(&fake), 2, "checked once per version");
        assert_eq!(
            stored(&f.pool, "minimal").await?,
            ("unchecked".into(), None, None),
            "version 1's result was discarded"
        );
        let version: i64 = sqlx::query_scalar("SELECT version FROM templates WHERE name='minimal'")
            .fetch_one(&f.pool)
            .await?;
        assert_eq!(version, 2);
        Ok(())
    })
    .await
}

// TEST: GW-I-026
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_077_a_refused_default_is_logged_at_error_once(pool: PgPool) -> TestResult {
    let capture = log_capture::Capture::default();
    tracing::subscriber::set_global_default(
        tracing_subscriber::fmt()
            .with_writer(capture.clone())
            .with_ansi(false)
            .with_max_level(tracing::Level::WARN)
            .finish(),
    )?;
    support::run_configured(pool, None, &templates(), async |f| {
        let fake = Fake::new(|_| CheckOutcome::Refused(CheckReason::RuntimeTooOld));
        f.gateway.start_template_checks(fake);
        for name in ["workstation", "minimal"] {
            let refused = ("refused".into(), Some("runtime_too_old".into()), Some(1));
            until_stored(&f.pool, name, &refused).await?;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
        let text = capture.text()?;
        let errors: Vec<&str> = text.lines().filter(|line| line.contains("ERROR")).collect();
        assert_eq!(errors.len(), 1, "{text}");
        assert!(
            errors[0].contains("template=\"workstation\"")
                || errors[0].contains("template=workstation"),
            "{text}"
        );
        // Any other refused template stays a warning.
        assert!(
            text.lines()
                .any(|line| line.contains("WARN") && line.contains("minimal")),
            "{text}"
        );
        Ok(())
    })
    .await
}

const OTHER_PIN: &str = "sha256:2222222222222222222222222222222222222222222222222222222222222222";

// TEST: GW-I-026
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_077_ext_check_gets_the_outcome_the_checker_stores(pool: PgPool) -> TestResult {
    // Two file-provided templates on different images: one the engine side verifies, one it refuses.
    let config = format!(
        "[[templates]]\nname='workstation'\ndisplay_name='W'\nimage='{PIN}'\ncapabilities=['shell']\ndefault=true\n\n\
         [[templates]]\nname='minimal'\ndisplay_name='M'\nimage='{OTHER_PIN}'\ncapabilities=['shell']\n"
    );
    support::run_configured(pool, None, &config, async |f| {
        let fake = Fake::new(|request| {
            if request.image == OTHER_PIN {
                CheckOutcome::Refused(CheckReason::ImageUser)
            } else {
                CheckOutcome::Verified {
                    warnings: SecretFiles::default(),
                    base: "workstation".into(),
                    image_id: request.image.to_owned(),
                }
            }
        });
        f.gateway.start_template_checks(fake.clone());
        until_stored(&f.pool, "workstation", &("verified".into(), None, Some(1))).await?;
        until_stored(
            &f.pool,
            "minimal",
            &("refused".into(), Some("image_user".into()), Some(1)),
        )
        .await?;
        let registered: Vec<Request> = fake.requests.lock().map_err(|_| "lock")?.clone();
        // ext-check's path, with the same engine side and the same configuration.
        for (image, expected) in [
            (
                PIN,
                CheckOutcome::Verified {
                    warnings: SecretFiles::default(),
                    base: "workstation".into(),
                    image_id: PIN.into(),
                },
            ),
            (OTHER_PIN, CheckOutcome::Refused(CheckReason::ImageUser)),
        ] {
            let before = fake.requests.lock().map_err(|_| "lock")?.len();
            let outcome =
                tqa_gateway::check_template_image(f.config(), image, Some(fake.as_ref())).await;
            assert_eq!(outcome, expected, "{image}");
            let asked = fake.requests.lock().map_err(|_| "lock")?[before].clone();
            assert!(
                registered.contains(&asked),
                "ext-check asked {asked:?}; the checker asked {registered:?}"
            );
        }
        Ok(())
    })
    .await
}

// TEST: GW-I-100
#[sqlx::test]
#[ignore = "needs PostgreSQL; run with --run-ignored all and TQA_DATABASE_URL set"]
async fn gw_i_100_credential_files_and_scan_limits_are_stored_and_shown_with_the_check(
    pool: PgPool,
) -> TestResult {
    // The install file acknowledges one path for workstation; minimal and custom acknowledge none.
    let file = templates().replacen(
        "default=true\n",
        "default=true\nacknowledged_secret_files=['/srv/dev.key']\n",
        1,
    ) + &format!(
        "\n[[templates]]\nname='custom'\ndisplay_name='C'\nimage='{PIN}'\ncapabilities=['shell']\n"
    );
    support::run_configured(pool, None, &file, async |f| {
        let (_, cookie) = f.admin().await?;
        let item = |path: &str, rule, acknowledged| SecretFile { path: path.into(), rule, acknowledged };
        let fake = Fake::new(move |request| {
            let acknowledged: Vec<&str> = request.acknowledged_secret_files.iter().map(String::as_str).collect();
            let expected: &[&str] = if request.name == "workstation" { &["/srv/dev.key"] } else { &[] };
            if acknowledged != expected || request.scan_bounds != Bounds::default() {
                return CheckOutcome::Unavailable;
            }
            match request.name {
                "workstation" => CheckOutcome::Verified {
                    warnings: SecretFiles { items: vec![item("/srv/dev.key", Rule::PrivateKey, true)], total: 1, unacknowledged: 0 },
                    base: "workstation".into(),
                    image_id: PIN.into(),
                },
                "minimal" => CheckOutcome::SecretFiles(SecretFiles {
                    items: vec![
                        item("/root/.aws/credentials", Rule::AwsCredentials, false),
                        item("/srv/app/dev.key", Rule::PrivateKey, true),
                    ],
                    total: 18,
                    unacknowledged: 17,
                }),
                _ => CheckOutcome::ScanLimit { bound: Bound::Entries, limit: 160_000 },
            }
        });
        f.gateway.start_template_checks(fake);
        until_stored(&f.pool, "workstation", &("verified".into(), None, Some(1))).await?;
        until_stored(&f.pool, "minimal", &("refused".into(), Some("image_secret_file".into()), Some(1))).await?;
        until_stored(&f.pool, "custom", &("refused".into(), Some("image_scan_limit".into()), Some(1))).await?;
        let (status, listed) = call(f, Method::GET, "/api/v1/admin/templates", &cookie, (None, None)).await?;
        assert_eq!(status, 200);
        let check = |name: &str| {
            listed["items"]
                .as_array()
                .and_then(|items| items.iter().find(|item| item["name"] == name))
                .map(|item| (item["check"]["secret_files"].clone(), item["check"]["scan_limit"].clone()))
        };
        assert_eq!(
            check("workstation"),
            Some((
                serde_json::json!({"items": [{"path": "/srv/dev.key", "rule": "private_key", "acknowledged": true}], "more": 0}),
                serde_json::Value::Null
            ))
        );
        assert_eq!(
            check("minimal"),
            Some((
                serde_json::json!({"items": [
                    {"path": "/root/.aws/credentials", "rule": "aws_credentials", "acknowledged": false},
                    {"path": "/srv/app/dev.key", "rule": "private_key", "acknowledged": true}
                ], "more": 16}),
                serde_json::Value::Null
            ))
        );
        assert_eq!(
            check("custom"),
            Some((serde_json::Value::Null, serde_json::json!({"bound": "entries", "limit": 160_000})))
        );
        Ok(())
    })
    .await
}
