//! `POST /api/v1/admin/routes/{name}/probe` (GW-I-037): an administrator tests a configured route the way
//! a runtime will use it, through the relay's own shaping, credential, limiter and decoder. This module holds the
//! admission rules: one probe per route at a time, at most one per route version per [`PER_ROUTE`], and [`PER_ADMIN`] per
//! administrator per [`PER_ADMIN_WINDOW`], from tables bounded at [`MAX_TRACKED`] entries.
use super::{
    endpoint::credential,
    probe::{self, Probe},
};
use crate::{
    GatewayError, admin::require_admin, app, auth::Authz, conversations::idempotent::Keyed,
    settings,
};
use axum::{
    Json,
    extract::{Path, State as AppState},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse as _, Response},
};
use sha2::Digest as _;
use std::fmt::Write as _;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant},
};

/// The longest a probe may take; the admin request waits for it.
/// Beyond the check's own bound (GW-U-092): the check ends first and reports running out of time as its result.
const PROBE_MARGIN: Duration = Duration::from_secs(15);

/// The API's own limit on one check: always longer than the check's, so a slow model's check is a named result
/// ("did not answer in time"), never a 504.
pub(super) fn request_limit(route: &tqa_llm::Route, config: &super::RelayConfig) -> Duration {
    super::probe::deadline(route, config) + PROBE_MARGIN
}

/// The router's backstop for a check request (GW-I-153): past every route's [`request_limit`], so the general
/// `[limits] io_timeout` never cuts a check short with a 504; [`request_limit`] still bounds each check exactly.
pub(crate) fn request_ceiling(config: &super::RelayConfig) -> Duration {
    super::probe::longest_deadline(config) + PROBE_MARGIN + Duration::from_secs(1)
}
/// Mixed with the Idempotency-Key to derive the probe's nonce.
const NONCE_PREFIX: &str = "tqa-route-probe:";

/// How long after a probe of a route the next one of the same version may start. A changed route (a new version) is a
/// new thing to check, so its first probe may start at once (decided on 12 Oct 2026: a fix saved from a check's
/// suggestion is checked again at once).
pub(super) const PER_ROUTE: Duration = Duration::from_secs(30);
/// Probes one administrator may start in [`PER_ADMIN_WINDOW`].
pub(super) const PER_ADMIN: usize = 10;
/// The window of [`PER_ADMIN`].
pub(super) const PER_ADMIN_WINDOW: Duration = Duration::from_mins(1);
/// Routes and administrators tracked at once; expired entries are dropped first, and a full table refuses.
pub(super) const MAX_TRACKED: usize = 4096;

/// Why a probe may not start now.
#[derive(Debug, thiserror::Error)]
pub(super) enum Admission {
    /// A probe of this route is in flight (`409 probe_running`).
    #[error("a probe of this route is running")]
    Running,
    /// A limit applies; the wait until it may (`429 rate_limited` with `Retry-After`).
    #[error("route probes are rate limited for {0:?}")]
    RateLimited(Duration),
}

#[derive(Default)]
struct Tables {
    running: HashSet<String>,
    /// Each route's last probe: the version it tested, and when.
    last_by_route: HashMap<String, (i64, Instant)>,
    by_admin: HashMap<uuid::Uuid, VecDeque<Instant>>,
}

impl Tables {
    /// Whether admitting this route and administrator needs a table entry that a full table has no room for.
    fn crowded(&self, route: &str, admin: uuid::Uuid) -> bool {
        (!self.last_by_route.contains_key(route) && self.last_by_route.len() >= MAX_TRACKED)
            || (!self.by_admin.contains_key(&admin) && self.by_admin.len() >= MAX_TRACKED)
    }

    fn prune(&mut self, now: Instant) {
        self.last_by_route
            .retain(|_, (_, last)| now.saturating_duration_since(*last) < PER_ROUTE);
        self.by_admin.retain(|_, starts| {
            starts.retain(|start| now.saturating_duration_since(*start) < PER_ADMIN_WINDOW);
            !starts.is_empty()
        });
    }
}

/// Admission for route probes, shared by every request.
#[derive(Clone, Default)]
pub(crate) struct Guard(Arc<Mutex<Tables>>);

/// Held while a probe runs; dropping it lets the next probe of the route through its window.
pub(super) struct Ticket {
    route: String,
    tables: Arc<Mutex<Tables>>,
}

impl Drop for Ticket {
    fn drop(&mut self) {
        self.tables
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .running
            .remove(&self.route);
    }
}

impl Guard {
    /// Admits a probe of `route` at `version` by `admin` at `now`, or says why not and for how long.
    pub(super) fn admit(
        &self,
        route: &str,
        version: i64,
        admin: uuid::Uuid,
        now: Instant,
    ) -> Result<Ticket, Admission> {
        let mut tables = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if tables.running.contains(route) {
            return Err(Admission::Running);
        }
        if let Some((tested, last)) = tables.last_by_route.get(route)
            && *tested == version
        {
            let since = now.saturating_duration_since(*last);
            if since < PER_ROUTE {
                return Err(Admission::RateLimited(PER_ROUTE.saturating_sub(since)));
            }
        }
        if let Some(starts) = tables.by_admin.get_mut(&admin) {
            starts.retain(|start| now.saturating_duration_since(*start) < PER_ADMIN_WINDOW);
            if starts.len() >= PER_ADMIN
                && let Some(oldest) = starts.front()
            {
                return Err(Admission::RateLimited(
                    PER_ADMIN_WINDOW.saturating_sub(now.saturating_duration_since(*oldest)),
                ));
            }
        }
        if tables.crowded(route, admin) {
            tables.prune(now);
            if tables.crowded(route, admin) {
                return Err(Admission::RateLimited(PER_ROUTE));
            }
        }
        tables.running.insert(route.to_owned());
        tables
            .last_by_route
            .insert(route.to_owned(), (version, now));
        tables.by_admin.entry(admin).or_default().push_back(now);
        Ok(Ticket {
            route: route.to_owned(),
            tables: Arc::clone(&self.0),
        })
    }

    /// Entries held, for the bound's test.
    #[cfg(test)]
    pub(super) fn tracked(&self) -> usize {
        let tables = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        tables.last_by_route.len().max(tables.by_admin.len())
    }
}

/// The probe's nonce: the first eight hex digits of SHA-256 over [`NONCE_PREFIX`] and the Idempotency-Key. The model
/// never sees the key, and a replay of the key returns the stored result, so a nonce is never used twice.
pub(super) fn nonce(key: &str) -> String {
    sha2::Sha256::digest(format!("{NONCE_PREFIX}{key}").as_bytes())
        .iter()
        .take(4)
        .fold(String::with_capacity(8), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}

/// `POST /api/v1/admin/routes/{name}/probe` (administrator, session or token; an Idempotency-Key is required).
///
/// # Errors
/// `400` without a key, `403`, `404` for an unknown route, `409 key_missing`, `409 probe_running`, `429
/// rate_limited` with `Retry-After`, store failure.
pub(crate) async fn probe(
    AppState(state): AppState<Arc<app::State>>,
    Authz(user): Authz,
    Path(name): Path<String>,
    headers: HeaderMap,
) -> Result<Response, GatewayError> {
    require_admin(&user)?;
    let path = format!("POST /api/v1/admin/routes/{name}/probe");
    let keyed = Keyed::new(user.id, &headers, path, &serde_json::json!({}))?;
    let nonce = nonce(crate::actions::idempotency_key(&headers)?);
    if let Some(cached) = replay(&state, &keyed).await? {
        return Ok(cached);
    }
    let (stored, digest) = tested(&state, &name).await?;
    if settings::routes::key_missing(&stored.config, state.secret_store.as_ref()).await? {
        return Err(GatewayError::Conflict("key_missing"));
    }
    let route = state
        .settings
        .current()
        .registry
        .get(&name)
        .ok_or(GatewayError::NotFound)?
        .clone();
    let ticket = match state
        .relay
        .probes
        .admit(&name, stored.version, user.id, Instant::now())
    {
        Ok(ticket) => ticket,
        Err(Admission::Running) => return Err(GatewayError::Conflict("probe_running")),
        Err(Admission::RateLimited(wait)) => {
            return Err(GatewayError::RateLimited(wait.as_secs().max(1)));
        }
    };
    // An administrator's probe: no conversation or run.
    let reader = crate::secrets::reads::ReadContext {
        user: Some(user.id),
        ..crate::secrets::reads::ReadContext::default()
    };
    let secret = credential(&state, &route, &reader).await?;
    let run = probe::run(
        &route,
        Probe {
            client: &state.relay.client,
            config: &state.config.relay,
            limiter: state.relay.limiters.for_route(&route),
            secret,
            nonce,
        },
        &state.cancel,
    );
    let observed = tokio::time::timeout(request_limit(&route, &state.config.relay), run)
        .await
        .map_err(|_| GatewayError::Timeout)?;
    drop(ticket);
    let (status, body) = store(
        &state,
        &keyed,
        &Tested {
            name: &name,
            version: stored.version,
            digest: &digest,
            admin: user.id,
        },
        &observed,
    )
    .await?;
    Ok((status, Json(body)).into_response())
}

/// A cached reply for this key, if the probe already ran; nothing is held once it returns.
async fn replay(state: &app::State, keyed: &Keyed<'_>) -> Result<Option<Response>, GatewayError> {
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut transaction = state.pool.begin().await?;
        let cached = keyed.begin(state, &mut transaction).await?;
        transaction.rollback().await?;
        Ok(cached.map(|(status, body)| (status, Json(body)).into_response()))
    })
    .await
}

/// The stored route and the digest of its document, as the probe starts (an edit during the probe makes the result
/// stale on arrival; no lock).
async fn tested(
    state: &app::State,
    name: &str,
) -> Result<(settings::routes::StoredRoute, Vec<u8>), GatewayError> {
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut connection = state.pool.acquire().await?;
        let digest = settings::routes::document_digest(&mut connection, name)
            .await?
            .ok_or(GatewayError::NotFound)?;
        let stored = settings::routes::entries(&mut connection)
            .await?
            .into_iter()
            .find(|entry| entry.config.name == name)
            .ok_or(GatewayError::NotFound)?;
        Ok((stored, digest))
    })
    .await
}

/// What a probe tested, for its record.
struct Tested<'a> {
    name: &'a str,
    version: i64,
    digest: &'a [u8],
    admin: uuid::Uuid,
}

/// Stores the latest observation, audits the probe and records the reply, in one transaction.
async fn store(
    state: &app::State,
    keyed: &Keyed<'_>,
    tested: &Tested<'_>,
    observed: &probe::Observation,
) -> Result<(StatusCode, serde_json::Value), GatewayError> {
    let value = serde_json::to_value(observed)?;
    tracing::info!(
        route = tested.name,
        route_version = tested.version,
        ok = observed.ok,
        failure = observed.failure.as_ref().map(|failure| failure.code),
        prompt_tokens = observed.tokens.prompt,
        completion_tokens = observed.tokens.completion,
        "route probe finished"
    );
    crate::io::bounded(state.config.io_timeout(), &state.cancel, async {
        let mut transaction = state.pool.begin().await?;
        if let Some(cached) = keyed.begin(state, &mut transaction).await? {
            return Ok(cached);
        }
        let probed_at = sqlx::query_scalar!(
            "INSERT INTO route_probes (route,route_version,document_digest,probed_by,ok,observed) \
             VALUES ($1,$2,$3,$4,$5,$6) ON CONFLICT (route) DO UPDATE SET route_version=EXCLUDED.route_version, \
             document_digest=EXCLUDED.document_digest, probed_at=CURRENT_TIMESTAMP, probed_by=EXCLUDED.probed_by, \
             ok=EXCLUDED.ok, observed=EXCLUDED.observed RETURNING probed_at",
            tested.name,
            tested.version,
            tested.digest,
            tested.admin,
            observed.ok,
            value
        )
        .fetch_one(&mut *transaction)
        .await?;
        settings::audit(
            &mut transaction,
            &settings::Change {
                area: "routes",
                actor: Some(tested.admin),
                item: tested.name,
                action: "probe",
                before: Some(tested.version),
                after: Some(tested.version),
                value_changed: false,
            },
        )
        .await?;
        let body = serde_json::json!({"probe": {
            "state": if observed.ok { "verified" } else { "failed" },
            "probed_at": probed_at
                .format(&time::format_description::well_known::Rfc3339)
                .map_err(|_| GatewayError::Io)?,
            "route_version": tested.version,
            "observed": value,
        }});
        keyed.record(&mut transaction, StatusCode::OK, &body).await?;
        transaction.commit().await?;
        Ok((StatusCode::OK, body))
    })
    .await
}
