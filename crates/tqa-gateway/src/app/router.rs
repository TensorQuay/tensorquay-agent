use super::{State as AppState, endpoints};
use crate::{
    actions, auth, conversations, link,
    routes::{
        ChannelRoute, Handler, HostKeyRoute, ProfileRoute, ROUTES, ScreenRoute, TeamRoute,
        TemplateRoute,
    },
    stream,
};
use axum::{
    Router,
    extract::{DefaultBodyLimit, MatchedPath, Request, State},
    handler::Handler as _,
    http::Method,
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{MethodRouter, delete, get, patch, post, put},
};
use std::sync::Arc;
use tower_http::{limit::RequestBodyLimitLayer, trace::TraceLayer};

pub(super) fn build(state: Arc<AppState>) -> Router {
    let mut router = Router::new();
    for route in ROUTES {
        let handler = method_router(route.handler);
        let maximum = match route.handler {
            Handler::RelayChat | Handler::RelayResponses | Handler::RelayMessages => {
                state.config.relay.body_bytes
            }
            Handler::McpProxy => crate::mcp_proxy::jsonrpc::MAX_MESSAGE_BYTES,
            Handler::RelayDecision => tqa_types::decision_v1::MAX_REQUEST_BYTES,
            // The upload handler refuses over its own limit first; this outer bound only stops a runaway body.
            Handler::Upload => crate::files::upload_body_limit(&state.config.files),
            _ => state.config.limits.body_bytes,
        };
        router = router.route(
            route.path,
            handler
                .layer::<_, std::convert::Infallible>(DefaultBodyLimit::max(maximum))
                .layer(RequestBodyLimitLayer::new(maximum)),
        );
    }
    let router = router.fallback(
        crate::web::page.layer(RequestBodyLimitLayer::new(state.config.limits.body_bytes)),
    );
    // The tests' check of the team responses against openapi.json (GW-U-142); never in a release build.
    #[cfg(feature = "testing")]
    let router = router.layer(middleware::from_fn_with_state(
        state.clone(),
        super::contract_check::layer,
    ));
    router
        .layer(middleware::from_fn_with_state(state.clone(), boundary))
        .layer(
            TraceLayer::new_for_http().make_span_with(|request: &Request| {
                let route = request
                    .extensions()
                    .get::<MatchedPath>()
                    .map_or("unmatched", MatchedPath::as_str);
                tracing::info_span!(
                    "http",
                    method = method_label(request.method()),
                    route = crate::routes::label(route)
                )
            }),
        )
        .with_state(state)
}

/// The template list and the admin templates API (ADR-0032).
/// The handlers of the grouped routes (templates, profiles and the admin tool-server list).
fn grouped(handler: Handler) -> MethodRouter<Arc<AppState>> {
    match handler {
        Handler::Profiles(route) => profiles_router(route),
        Handler::Channels(route) => channels_router(route),
        Handler::AdminMcpServers => get(crate::admin::mcp_servers),
        Handler::Templates(route) => templates_router(route),
        Handler::Teams(route) => teams_router(route),
        // Only grouped handlers are passed here.
        _ => MethodRouter::new(),
    }
}

/// The agent teams' sub-router: a run's team tools, then the people's side.
fn teams_router(route: TeamRoute) -> MethodRouter<Arc<AppState>> {
    use crate::teams::{members_api, people, runtime};
    match route {
        TeamRoute::RuntimeRead => get(runtime::read),
        TeamRoute::RuntimePost => post(runtime::post),
        TeamRoute::RuntimeStart => post(runtime::start),
        TeamRoute::RuntimeTakeFiles => post(runtime::take_files),
        TeamRoute::RuntimeRole => get(runtime::role),
        TeamRoute::RuntimeDone => post(runtime::done),
        TeamRoute::RuntimeAcceptance => get(runtime::acceptance),
        TeamRoute::RuntimeStopMember => post(members_api::stop),
        TeamRoute::RuntimeReadHelper => get(members_api::transcript),
        TeamRoute::Get => get(people::team),
        TeamRoute::Messages => get(people::messages),
        TeamRoute::Answer => post(people::answer),
        TeamRoute::Stop => post(people::stop),
        TeamRoute::AdminSettings => get(crate::admin::agent_teams::get),
        TeamRoute::AdminSettingsPut => put(crate::admin::agent_teams::put),
    }
}

fn profiles_router(route: ProfileRoute) -> MethodRouter<Arc<AppState>> {
    match route {
        ProfileRoute::List => get(crate::profiles::list),
        ProfileRoute::Get => get(crate::profiles::get),
        ProfileRoute::Create => post(crate::profiles::create),
        ProfileRoute::Put => put(crate::profiles::put),
        ProfileRoute::Delete => delete(crate::profiles::remove),
    }
}

fn channels_router(route: ChannelRoute) -> MethodRouter<Arc<AppState>> {
    use crate::channels::api;
    match route {
        ChannelRoute::Create => post(api::create),
        ChannelRoute::List => get(api::list),
        ChannelRoute::Get => get(api::get),
        ChannelRoute::End => post(api::end),
        ChannelRoute::Delete => delete(api::delete),
        ChannelRoute::Members => get(api::members),
        ChannelRoute::PutMember => put(api::put_member),
        ChannelRoute::RemoveMember => delete(api::remove_member),
        ChannelRoute::Read => get(api::read),
        ChannelRoute::Post => post(api::post),
        ChannelRoute::RuntimeList => get(crate::channels::runtime::list),
        ChannelRoute::RuntimeRead => get(crate::channels::runtime::read),
        ChannelRoute::RuntimePost => post(crate::channels::runtime::post),
        ChannelRoute::Claim => post(api::claim),
        ChannelRoute::Tasks => get(api::tasks),
        ChannelRoute::Proposal => get(api::proposal),
        ChannelRoute::Events => get(crate::channels::stream::events),
        ChannelRoute::PutMemberByEmail => put(api::put_member_by_email),
        ChannelRoute::Candidates => get(crate::channels::candidates::candidates),
        ChannelRoute::RuntimeClaim => post(crate::channels::runtime::claim),
    }
}

fn host_keys_router(route: HostKeyRoute) -> MethodRouter<Arc<AppState>> {
    match route {
        HostKeyRoute::List => get(crate::host_keys::list),
        HostKeyRoute::Add => post(crate::host_keys::add),
        HostKeyRoute::Put => put(crate::host_keys::put),
        HostKeyRoute::Delete => delete(crate::host_keys::remove),
        HostKeyRoute::Shared => get(crate::host_keys::shared_list),
        HostKeyRoute::Reads => get(crate::host_keys::reads),
        HostKeyRoute::AdminReads => get(crate::host_keys::admin_reads),
        HostKeyRoute::TaskUses => get(crate::host_keys::task_uses),
        HostKeyRoute::AdminList => get(crate::host_keys::admin_list),
        HostKeyRoute::AdminAdd => post(crate::host_keys::admin_add),
        HostKeyRoute::AdminPut => put(crate::host_keys::admin_put),
        HostKeyRoute::AdminDelete => delete(crate::host_keys::admin_remove),
    }
}

fn screen_router(route: ScreenRoute) -> MethodRouter<Arc<AppState>> {
    match route {
        ScreenRoute::Notices => get(crate::screen::list),
        ScreenRoute::SignOut => post(crate::screen::sign_out),
        ScreenRoute::View => get(crate::screen::view),
        ScreenRoute::Bridge => get(crate::screen::dial),
        ScreenRoute::Screenshot => get(crate::screen::read),
    }
}

fn templates_router(route: TemplateRoute) -> MethodRouter<Arc<AppState>> {
    match route {
        TemplateRoute::List => get(crate::environments::templates),
        TemplateRoute::AdminList => get(crate::admin::templates),
        TemplateRoute::AdminPut => put(crate::admin::template_put),
        TemplateRoute::AdminDelete => delete(crate::admin::template_remove),
        TemplateRoute::AdminDefault => post(crate::admin::template_default),
        TemplateRoute::AdminCheck => post(crate::admin::template_check),
        TemplateRoute::AdminImages => get(crate::admin::environment_images),
        TemplateRoute::AdminReset => post(crate::admin::template_reset),
    }
}

/// The handler for each registered operation; grouped ones go to their own sub-routers.
fn method_router(handler: Handler) -> MethodRouter<Arc<AppState>> {
    match handler {
        Handler::Health => get(endpoints::health),
        Handler::FileList => get(crate::files::list),
        Handler::FileContent => get(crate::files::content),
        Handler::FileDownload => get(crate::files::download),
        Handler::FileView => get(crate::files::view),
        Handler::FileSheet => get(crate::files::sheet),
        Handler::Upload => post(crate::files::upload),
        Handler::RunChanges => get(crate::files::changes::run_changes),
        Handler::RunChangesDiff => get(crate::files::changes::diff),
        Handler::Ready => get(endpoints::ready),
        Handler::Bootstrap => post(auth::bootstrap),
        Handler::Login => post(auth::login),
        Handler::Logout => post(auth::logout),
        Handler::ChangePassword => post(auth::change_password),
        Handler::Me => get(auth::me),
        Handler::Setup => get(auth::setup),
        Handler::Models => get(crate::models::list),
        Handler::Create => post(conversations::create),
        Handler::List => get(conversations::list),
        Handler::Get => get(conversations::get),
        Handler::Patch => patch(conversations::patch),
        Handler::Templates(_)
        | Handler::Teams(_)
        | Handler::Profiles(_)
        | Handler::Channels(_)
        | Handler::AdminMcpServers => grouped(handler),
        Handler::Environment => put(conversations::environment),
        Handler::Delete => delete(conversations::delete),
        Handler::Events => get(stream::events),
        Handler::Message => post(actions::message),
        Handler::HeldMessages => get(actions::held::list),
        Handler::CancelHeldMessage => delete(actions::held::cancel),
        Handler::Cancel => post(actions::cancel),
        Handler::Runtime => get(link::upgrade),
        Handler::History => get(link::history),
        Handler::RelayChat => post(crate::relay::chat),
        Handler::RelayResponses => post(crate::relay::responses),
        Handler::RelayMessages => post(crate::relay::messages),
        Handler::RelayModels => get(crate::relay::models),
        Handler::RelayDecision => post(crate::relay::decision),
        Handler::McpProxy => post(crate::mcp_proxy::call),
        Handler::McpProxyListen => get(crate::mcp_proxy::listen),
        Handler::McpProxyClose => delete(crate::mcp_proxy::close),
        Handler::Users => get(auth::users::list),
        Handler::UserChange => patch(auth::users::change),
        Handler::Connectors => get(crate::connectors::credentials::list),
        Handler::ConnectorCredentialPut => put(crate::connectors::credentials::put),
        Handler::ConnectorCredentialDelete => delete(crate::connectors::credentials::remove),
        Handler::ConnectorCredentialReads => get(crate::connectors::credentials::reads),
        Handler::Invitations => get(auth::invitations::list),
        Handler::InvitationCreate => post(auth::invitations::create),
        Handler::InvitationRevoke => delete(auth::invitations::revoke),
        Handler::InvitationLookup => post(auth::invitations::lookup),
        Handler::InvitationAccept => post(auth::invitations::accept),
        Handler::Tokens => get(auth::api_tokens::list),
        Handler::TokenCreate => post(auth::api_tokens::create),
        Handler::TokenRevoke => delete(auth::api_tokens::revoke),
        Handler::WorkspaceList => get(crate::workspaces::list),
        Handler::WorkspaceCreate => post(crate::workspaces::create),
        Handler::WorkspaceGet => get(crate::workspaces::get),
        Handler::WorkspaceUpdate => patch(crate::workspaces::update),
        Handler::WorkspaceFork => post(crate::workspaces::fork),
        Handler::HostKeys(route) => host_keys_router(route),
        Handler::Screen(route) => screen_router(route),
        Handler::WorkspaceDelete => delete(crate::workspaces::delete),
        Handler::WorkspaceShares => get(crate::workspaces::shares),
        Handler::WorkspaceShareByEmail => put(crate::workspaces::share_by_email),
        Handler::WorkspaceShare => put(crate::workspaces::share),
        Handler::WorkspaceUnshare => delete(crate::workspaces::unshare),
        admin => admin_method_router(admin),
    }
}

/// The handler for each administration operation; `method_router` sends every other operation elsewhere, so the
/// last arm is never reached.
fn admin_method_router(handler: Handler) -> MethodRouter<Arc<AppState>> {
    match handler {
        Handler::AdminUsage => get(crate::admin::usage),
        Handler::AdminSecrets => get(crate::admin::secrets),
        Handler::AdminSecretPut => put(crate::admin::secret_put),
        Handler::AdminSecretReminder => put(crate::admin::secret_remind),
        Handler::AdminSecretReads => get(crate::admin::secret_reads),
        Handler::AdminSecretConsumers => put(crate::admin::secret_bind),
        Handler::AdminSecretDelete => delete(crate::admin::secret_remove),
        Handler::AdminEgress => get(crate::admin::egress),
        Handler::AdminConnectors => get(crate::admin::connectors),
        Handler::AdminConnectorPut => put(crate::admin::connector_put),
        Handler::AdminConnectorDelete => delete(crate::admin::connector_remove),
        Handler::AdminDecisionEngines => get(crate::admin::decision_engines),
        Handler::AdminDecisionEnginePut => put(crate::admin::decision_engine_put),
        Handler::AdminDecisionEngineDelete => delete(crate::admin::decision_engine_remove),
        Handler::AdminEgressPut => put(crate::admin::egress_put),
        Handler::AdminEgressPresets => get(crate::admin::egress_presets),
        Handler::AdminEgressPresetApply => post(crate::admin::egress_preset_apply),
        Handler::AdminEgressPresetRemove => delete(crate::admin::egress_preset_remove),
        Handler::AdminEgressDelete => delete(crate::admin::egress_remove),
        Handler::AdminLimits => get(crate::admin::limits),
        Handler::AdminLimitPut => put(crate::admin::limit_put),
        Handler::AdminLimitReset => delete(crate::admin::limit_reset),
        Handler::AdminRoutes => get(crate::admin::routes),
        Handler::AdminRoutesExport => get(crate::admin::routes_export),
        Handler::AdminRoutePut => put(crate::admin::route_put),
        Handler::AdminRouteDelete => delete(crate::admin::route_remove),
        Handler::AdminRouteProbe => post(crate::relay::route_probe),
        Handler::AdminRouteModels => get(crate::relay::route_models),
        _ => MethodRouter::new(),
    }
}

async fn boundary(State(state): State<Arc<AppState>>, request: Request, next: Next) -> Response {
    let method = method_label(request.method());
    let unmatched = request.extensions().get::<MatchedPath>().is_none();
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map_or("unmatched", MatchedPath::as_str);
    // A route check and the other long admin reads wait their own bound (GW-I-153, GW-U-100).
    let long_read = super::long_reads::limit(route, &state.config);
    let route = crate::routes::label(route);
    let start = std::time::Instant::now();
    let changing = !matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    );
    let page = crate::web::is_page(request.uri().path());
    // Runtimes call the model and decision engine relays with their token, never from a browser (ADR-0013).
    let relay = request.uri().path().starts_with("/relay/r/")
        || request.uri().path().starts_with("/relay/d/");
    // The MCP proxy is called by runtimes with their token, like the relay (ADR-0031).
    let mcp = request.uri().path().starts_with("/mcp/");
    // So are the channel routes of runtimes (ADR-0014 point 6); their reads may wait up to 300 s.
    // So are the agent-team routes (ADR-0034), with a runtime token too.
    let channels = request.uri().path().starts_with("/runtime/channels")
        || request.uri().path().starts_with("/runtime/teams/");
    // A personal API token is never an ambient credential, so a bearer API request needs no Origin (answers-063 Q4);
    // the extractor then authenticates it by the token alone, never by a cookie on the same request.
    let bearer = request.uri().path().starts_with("/api/v1/")
        && request
            .headers()
            .contains_key(axum::http::header::AUTHORIZATION);
    // An unmatched API route cannot change state and keeps its JSON not-found response.
    let origin = if relay || mcp || channels || page || unmatched || bearer {
        Ok(())
    } else {
        auth::check_origin(request.headers(), &state.config.public_origin, changing)
    };
    let timeout = if relay {
        // A backstop only: the model relay waits each route's first-response bound itself (up to the longest a route
        // may set, after the route's queue), and the other relays their own deadlines.
        std::time::Duration::from_secs(
            state
                .config
                .relay
                .header_timeout
                .max(u64::from(*tqa_llm::FIRST_RESPONSE_RANGE.end()))
                + state.config.relay.queue_wait
                + 5,
        )
    } else if let Some(limit) = long_read {
        limit
    } else if mcp {
        crate::mcp_proxy::DEADLINE
    } else if channels {
        std::time::Duration::from_secs(u64::from(tqa_types::MAX_CHANNEL_WAIT_SECONDS) + 30)
    } else if crate::files::is_upload(request.uri().path()) {
        std::time::Duration::from_secs(state.config.files.upload_seconds)
    } else if page {
        crate::web::HEAD_TIMEOUT + std::time::Duration::from_secs(1)
    } else {
        state.config.io_timeout()
    };
    let response = match origin {
        Err(error) => error.into_response(),
        Ok(()) => crate::io::bounded(timeout, &state.cancel, async {
            Ok(next.run(request).await)
        })
        .await
        .unwrap_or_else(IntoResponse::into_response),
    };
    state.telemetry.request(
        method,
        route,
        response.status().as_u16(),
        start.elapsed().as_secs_f64(),
    );
    if page {
        response
    } else {
        let mut response = security_headers(typed_limit(response)).await;
        if relay || mcp {
            crate::error::name_refusal(&mut response);
        }
        response
    }
}

/// The body-size layer refuses on `Content-Length` before any parsing, with a bare 413; API callers get the typed
/// error instead (SEC-012).
fn typed_limit(response: Response) -> Response {
    let typed = response
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .is_some_and(|value| value.as_bytes().starts_with(b"application/json"));
    if response.status() == axum::http::StatusCode::PAYLOAD_TOO_LARGE && !typed {
        crate::GatewayError::BodyTooLarge.into_response()
    } else {
        response
    }
}

pub(super) async fn security_headers(mut response: Response) -> Response {
    use axum::http::{HeaderValue, header};
    response.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

fn method_label(method: &Method) -> &'static str {
    match *method {
        Method::GET => "GET",
        Method::POST => "POST",
        Method::DELETE => "DELETE",
        Method::PATCH => "PATCH",
        Method::PUT => "PUT",
        Method::HEAD => "HEAD",
        Method::OPTIONS => "OPTIONS",
        _ => "OTHER",
    }
}
