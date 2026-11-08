use super::{Authz, SessionAuthz, User, accounts, sessions};
use crate::{GatewayError, app::State, request::ApiJson};
use axum::{
    Json,
    extract::{ConnectInfo, State as AppState},
    http::StatusCode,
};
use axum_extra::extract::CookieJar;
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use std::{net::SocketAddr, sync::Arc};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Bootstrap {
    email: String,
    password: SecretString,
    token: SecretString,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PasswordChange {
    current_password: SecretString,
    new_password: SecretString,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Login {
    email: String,
    password: SecretString,
}

/// The signed-in account as sign-in, invitation acceptance and `me` answer it: the account, and the installation's
/// upload limits (`[files]`), so the web refuses a file over them before sending it (GW-I-142).
#[derive(Serialize)]
pub(crate) struct SignedIn {
    #[serde(flatten)]
    user: User,
    limits: UploadLimits,
    /// Whether this person's tasks reach anything outside (GW-I-156): `none` with no rule or egress off, else `some`.
    /// It never lists hosts; New task says when a task has no internet.
    internet: Internet,
    /// The install's switches the page reads at start (ADR-0034 S6).
    features: Features,
}

/// The install's switches, for the page.
#[derive(Serialize)]
struct Features {
    /// Agent teams are on (`[agent_teams] enabled`): the page shows team parts only while true.
    agent_teams: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum Internet {
    None,
    Some,
}

#[derive(Serialize)]
struct UploadLimits {
    #[serde(rename = "upload_bytes")]
    upload: u64,
    #[serde(rename = "uploads_bytes")]
    uploads: u64,
    /// The file previews: an HTML page, a PDF and a spreadsheet larger than these are only downloaded.
    #[serde(rename = "preview_html_bytes")]
    preview_html: u64,
    #[serde(rename = "preview_pdf_bytes")]
    preview_pdf: u64,
    #[serde(rename = "preview_sheet_bytes")]
    preview_sheet: u64,
}

pub(crate) fn signed_in(state: &State, user: User) -> Json<SignedIn> {
    let files = &state.config.files;
    Json(SignedIn {
        features: Features {
            agent_teams: state.config.agent_teams.enabled,
        },
        user,
        limits: UploadLimits {
            upload: files.upload_bytes,
            uploads: files.uploads_bytes,
            preview_html: files.preview(files.preview_html_bytes),
            preview_pdf: files.preview(files.preview_pdf_bytes),
            preview_sheet: files.preview(files.preview_sheet_bytes),
        },
        internet: if state.config.egress.disabled || state.settings.current().egress.is_empty() {
            Internet::None
        } else {
            Internet::Some
        },
    })
}

/// Creating the first administrator signs them in with the same session cookie as login.
pub(crate) async fn bootstrap(
    AppState(state): AppState<Arc<State>>,
    jar: CookieJar,
    ApiJson(body): ApiJson<Bootstrap>,
) -> Result<(StatusCode, CookieJar, Json<SignedIn>), GatewayError> {
    let user = accounts::bootstrap(&state, &body.email, body.password, &body.token).await?;
    let cookie = sessions::create(&state, &user).await?;
    Ok((
        StatusCode::CREATED,
        jar.add(cookie),
        signed_in(&state, user),
    ))
}

/// Unauthenticated: whether a fresh install still needs its first administrator. Nothing else is exposed.
pub(crate) async fn setup(
    AppState(state): AppState<Arc<State>>,
) -> Result<Json<serde_json::Value>, GatewayError> {
    let required = accounts::setup_required(&state).await?;
    Ok(Json(serde_json::json!({ "required": required })))
}

pub(crate) async fn login(
    AppState(state): AppState<Arc<State>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    jar: CookieJar,
    ApiJson(body): ApiJson<Login>,
) -> Result<(CookieJar, Json<SignedIn>), GatewayError> {
    state
        .login_rate
        .check(peer.ip(), tokio::time::Instant::now())?;
    let user = accounts::authenticate(&state, &body.email, body.password).await?;
    let cookie = sessions::create(&state, &user).await?;
    Ok((jar.add(cookie), signed_in(&state, user)))
}

/// `POST /api/v1/auth/password` (session only, GW-I-124): the caller changes their own password. Attempts count
/// against the same per-address limit as login, before the current password is checked.
pub(crate) async fn change_password(
    AppState(state): AppState<Arc<State>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    SessionAuthz(user): SessionAuthz,
    jar: CookieJar,
    ApiJson(body): ApiJson<PasswordChange>,
) -> Result<StatusCode, GatewayError> {
    state
        .login_rate
        .check(peer.ip(), tokio::time::Instant::now())?;
    // SessionAuthz found this cookie's session, so it is present and well formed.
    let keep = super::token::credential(
        jar.get("tqa_session")
            .ok_or(GatewayError::Unauthorized)?
            .value(),
    )?;
    accounts::change_password(
        &state,
        &user,
        body.current_password,
        body.new_password,
        &keep,
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn logout(
    AppState(state): AppState<Arc<State>>,
    Authz(_): Authz,
    jar: CookieJar,
) -> Result<(CookieJar, StatusCode), GatewayError> {
    let cookie = sessions::revoke(&state, &jar).await?;
    Ok((jar.add(cookie), StatusCode::NO_CONTENT))
}

pub(crate) async fn me(
    AppState(state): AppState<Arc<State>>,
    Authz(user): Authz,
) -> Json<SignedIn> {
    signed_in(&state, user)
}
