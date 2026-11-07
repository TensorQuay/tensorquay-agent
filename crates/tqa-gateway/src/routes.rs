mod handlers;

pub(crate) use handlers::{
    ChannelRoute, Handler, HostKeyRoute, ProfileRoute, Route, ScreenRoute, TeamRoute, TemplateRoute,
};

pub(crate) const ROUTES: &[Route] = &[
    Route {
        method: "GET",
        path: "/api/v1/models",
        handler: Handler::Models,
    },
    Route {
        method: "GET",
        path: "/runtime/history",
        handler: Handler::History,
    },
    Route {
        method: "GET",
        path: "/runtime/channels",
        handler: Handler::Channels(ChannelRoute::RuntimeList),
    },
    Route {
        method: "POST",
        path: "/runtime/channels/{id}/tasks/{seq}/claim",
        handler: Handler::Channels(ChannelRoute::RuntimeClaim),
    },
    Route {
        method: "GET",
        path: "/runtime/channels/{id}/messages",
        handler: Handler::Channels(ChannelRoute::RuntimeRead),
    },
    Route {
        method: "POST",
        path: "/runtime/channels/{id}/messages",
        handler: Handler::Channels(ChannelRoute::RuntimePost),
    },
    Route {
        method: "GET",
        path: "/runtime/teams/messages",
        handler: Handler::Teams(TeamRoute::RuntimeRead),
    },
    Route {
        method: "POST",
        path: "/runtime/teams/messages",
        handler: Handler::Teams(TeamRoute::RuntimePost),
    },
    Route {
        method: "POST",
        path: "/runtime/teams/members",
        handler: Handler::Teams(TeamRoute::RuntimeStart),
    },
    Route {
        method: "GET",
        path: "/runtime/teams/role",
        handler: Handler::Teams(TeamRoute::RuntimeRole),
    },
    Route {
        method: "POST",
        path: "/runtime/teams/done",
        handler: Handler::Teams(TeamRoute::RuntimeDone),
    },
    Route {
        method: "GET",
        path: "/runtime/teams/acceptance",
        handler: Handler::Teams(TeamRoute::RuntimeAcceptance),
    },
    Route {
        method: "POST",
        path: "/runtime/teams/members/{name}/stop",
        handler: Handler::Teams(TeamRoute::RuntimeStopMember),
    },
    Route {
        method: "GET",
        path: "/runtime/teams/members/{name}/transcript",
        handler: Handler::Teams(TeamRoute::RuntimeReadHelper),
    },
    Route {
        method: "POST",
        path: "/runtime/teams/files",
        handler: Handler::Teams(TeamRoute::RuntimeTakeFiles),
    },
    Route {
        method: "POST",
        path: "/relay/r/{route}/chat/completions",
        handler: Handler::RelayChat,
    },
    Route {
        method: "POST",
        path: "/relay/r/{route}/responses",
        handler: Handler::RelayResponses,
    },
    Route {
        method: "POST",
        path: "/relay/r/{route}/messages",
        handler: Handler::RelayMessages,
    },
    Route {
        method: "GET",
        path: "/relay/r/{route}/models",
        handler: Handler::RelayModels,
    },
    Route {
        method: "POST",
        path: "/relay/d/{engine}/v1/{point}",
        handler: Handler::RelayDecision,
    },
    Route {
        method: "POST",
        path: "/mcp/{connector}",
        handler: Handler::McpProxy,
    },
    Route {
        method: "GET",
        path: "/mcp/{connector}",
        handler: Handler::McpProxyListen,
    },
    Route {
        method: "DELETE",
        path: "/mcp/{connector}",
        handler: Handler::McpProxyClose,
    },
    Route {
        method: "GET",
        path: "/healthz",
        handler: Handler::Health,
    },
    Route {
        method: "GET",
        path: "/readyz",
        handler: Handler::Ready,
    },
    Route {
        method: "POST",
        path: "/api/v1/auth/bootstrap",
        handler: Handler::Bootstrap,
    },
    Route {
        method: "GET",
        path: "/api/v1/setup",
        handler: Handler::Setup,
    },
    Route {
        method: "POST",
        path: "/api/v1/auth/login",
        handler: Handler::Login,
    },
    Route {
        method: "POST",
        path: "/api/v1/auth/logout",
        handler: Handler::Logout,
    },
    Route {
        method: "POST",
        path: "/api/v1/auth/password",
        handler: Handler::ChangePassword,
    },
    Route {
        method: "GET",
        path: "/api/v1/auth/me",
        handler: Handler::Me,
    },
    Route {
        method: "POST",
        path: "/api/v1/channels",
        handler: Handler::Channels(ChannelRoute::Create),
    },
    Route {
        method: "GET",
        path: "/api/v1/channels",
        handler: Handler::Channels(ChannelRoute::List),
    },
    Route {
        method: "GET",
        path: "/api/v1/channels/candidates",
        handler: Handler::Channels(ChannelRoute::Candidates),
    },
    Route {
        method: "GET",
        path: "/api/v1/channels/{id}",
        handler: Handler::Channels(ChannelRoute::Get),
    },
    Route {
        method: "DELETE",
        path: "/api/v1/channels/{id}",
        handler: Handler::Channels(ChannelRoute::Delete),
    },
    Route {
        method: "POST",
        path: "/api/v1/channels/{id}/end",
        handler: Handler::Channels(ChannelRoute::End),
    },
    Route {
        method: "GET",
        path: "/api/v1/channels/{id}/members",
        handler: Handler::Channels(ChannelRoute::Members),
    },
    Route {
        method: "PUT",
        path: "/api/v1/channels/{id}/members",
        handler: Handler::Channels(ChannelRoute::PutMemberByEmail),
    },
    Route {
        method: "PUT",
        path: "/api/v1/channels/{id}/members/{kind}/{member}",
        handler: Handler::Channels(ChannelRoute::PutMember),
    },
    Route {
        method: "DELETE",
        path: "/api/v1/channels/{id}/members/{kind}/{member}",
        handler: Handler::Channels(ChannelRoute::RemoveMember),
    },
    Route {
        method: "GET",
        path: "/api/v1/channels/{id}/messages",
        handler: Handler::Channels(ChannelRoute::Read),
    },
    Route {
        method: "POST",
        path: "/api/v1/channels/{id}/messages",
        handler: Handler::Channels(ChannelRoute::Post),
    },
    Route {
        method: "POST",
        path: "/api/v1/channels/{id}/tasks/{seq}/claim",
        handler: Handler::Channels(ChannelRoute::Claim),
    },
    Route {
        method: "GET",
        path: "/api/v1/channels/{id}/tasks",
        handler: Handler::Channels(ChannelRoute::Tasks),
    },
    Route {
        method: "GET",
        path: "/api/v1/channels/{id}/proposals/{seq}",
        handler: Handler::Channels(ChannelRoute::Proposal),
    },
    Route {
        method: "GET",
        path: "/api/v1/channels/{id}/events",
        handler: Handler::Channels(ChannelRoute::Events),
    },
    Route {
        method: "GET",
        path: "/api/v1/workspaces",
        handler: Handler::WorkspaceList,
    },
    Route {
        method: "POST",
        path: "/api/v1/workspaces",
        handler: Handler::WorkspaceCreate,
    },
    Route {
        method: "GET",
        path: "/api/v1/workspaces/{id}",
        handler: Handler::WorkspaceGet,
    },
    Route {
        method: "PATCH",
        path: "/api/v1/workspaces/{id}",
        handler: Handler::WorkspaceUpdate,
    },
    Route {
        method: "GET",
        path: "/api/v1/host-keys",
        handler: Handler::HostKeys(HostKeyRoute::List),
    },
    Route {
        method: "POST",
        path: "/api/v1/host-keys",
        handler: Handler::HostKeys(HostKeyRoute::Add),
    },
    Route {
        method: "GET",
        path: "/api/v1/host-keys/shared",
        handler: Handler::HostKeys(HostKeyRoute::Shared),
    },
    Route {
        method: "GET",
        path: "/api/v1/host-keys/{id}/reads",
        handler: Handler::HostKeys(HostKeyRoute::Reads),
    },
    Route {
        method: "GET",
        path: "/api/v1/admin/host-keys/{id}/reads",
        handler: Handler::HostKeys(HostKeyRoute::AdminReads),
    },
    Route {
        method: "GET",
        path: "/api/v1/conversations/{id}/key-uses",
        handler: Handler::HostKeys(HostKeyRoute::TaskUses),
    },
    Route {
        method: "GET",
        path: "/api/v1/conversations/{id}/screen/notices",
        handler: Handler::Screen(ScreenRoute::Notices),
    },
    Route {
        method: "POST",
        path: "/api/v1/conversations/{id}/screen/sign-out",
        handler: Handler::Screen(ScreenRoute::SignOut),
    },
    Route {
        method: "GET",
        path: "/api/v1/conversations/{id}/screen",
        handler: Handler::Screen(ScreenRoute::View),
    },
    Route {
        method: "GET",
        path: "/runtime/screen",
        handler: Handler::Screen(ScreenRoute::Bridge),
    },
    Route {
        method: "GET",
        path: "/api/v1/conversations/{id}/screenshots/{screenshot}",
        handler: Handler::Screen(ScreenRoute::Screenshot),
    },
    Route {
        method: "GET",
        path: "/api/v1/admin/host-keys",
        handler: Handler::HostKeys(HostKeyRoute::AdminList),
    },
    Route {
        method: "POST",
        path: "/api/v1/admin/host-keys",
        handler: Handler::HostKeys(HostKeyRoute::AdminAdd),
    },
    Route {
        method: "PUT",
        path: "/api/v1/admin/host-keys/{id}",
        handler: Handler::HostKeys(HostKeyRoute::AdminPut),
    },
    Route {
        method: "DELETE",
        path: "/api/v1/admin/host-keys/{id}",
        handler: Handler::HostKeys(HostKeyRoute::AdminDelete),
    },
    Route {
        method: "PUT",
        path: "/api/v1/host-keys/{id}",
        handler: Handler::HostKeys(HostKeyRoute::Put),
    },
    Route {
        method: "DELETE",
        path: "/api/v1/host-keys/{id}",
        handler: Handler::HostKeys(HostKeyRoute::Delete),
    },
    Route {
        method: "DELETE",
        path: "/api/v1/workspaces/{id}",
        handler: Handler::WorkspaceDelete,
    },
    Route {
        method: "POST",
        path: "/api/v1/workspaces/{id}/fork",
        handler: Handler::WorkspaceFork,
    },
    Route {
        method: "GET",
        path: "/api/v1/workspaces/{id}/shares",
        handler: Handler::WorkspaceShares,
    },
    Route {
        method: "PUT",
        path: "/api/v1/workspaces/{id}/shares",
        handler: Handler::WorkspaceShareByEmail,
    },
    Route {
        method: "PUT",
        path: "/api/v1/workspaces/{id}/shares/{user_id}",
        handler: Handler::WorkspaceShare,
    },
    Route {
        method: "DELETE",
        path: "/api/v1/workspaces/{id}/shares/{user_id}",
        handler: Handler::WorkspaceUnshare,
    },
    Route {
        method: "GET",
        path: "/api/v1/users",
        handler: Handler::Users,
    },
    Route {
        method: "PATCH",
        path: "/api/v1/users/{id}",
        handler: Handler::UserChange,
    },
    Route {
        method: "GET",
        path: "/api/v1/admin/usage",
        handler: Handler::AdminUsage,
    },
    Route {
        method: "GET",
        path: "/api/v1/admin/secrets",
        handler: Handler::AdminSecrets,
    },
    Route {
        method: "PUT",
        path: "/api/v1/admin/secrets/{name}",
        handler: Handler::AdminSecretPut,
    },
    Route {
        method: "DELETE",
        path: "/api/v1/admin/secrets/{name}",
        handler: Handler::AdminSecretDelete,
    },
    Route {
        method: "PUT",
        path: "/api/v1/admin/secrets/{name}/reminder",
        handler: Handler::AdminSecretReminder,
    },
    Route {
        method: "GET",
        path: "/api/v1/admin/secrets/{name}/reads",
        handler: Handler::AdminSecretReads,
    },
    Route {
        method: "PUT",
        path: "/api/v1/admin/secrets/{name}/consumers",
        handler: Handler::AdminSecretConsumers,
    },
    Route {
        method: "GET",
        path: "/api/v1/admin/egress",
        handler: Handler::AdminEgress,
    },
    Route {
        method: "GET",
        path: "/api/v1/admin/egress/presets",
        handler: Handler::AdminEgressPresets,
    },
    Route {
        method: "POST",
        path: "/api/v1/admin/egress/presets/{id}",
        handler: Handler::AdminEgressPresetApply,
    },
    Route {
        method: "DELETE",
        path: "/api/v1/admin/egress/presets/{id}",
        handler: Handler::AdminEgressPresetRemove,
    },
    Route {
        method: "PUT",
        path: "/api/v1/admin/egress/{host}",
        handler: Handler::AdminEgressPut,
    },
    Route {
        method: "DELETE",
        path: "/api/v1/admin/egress/{host}",
        handler: Handler::AdminEgressDelete,
    },
    Route {
        method: "GET",
        path: "/api/v1/admin/limits",
        handler: Handler::AdminLimits,
    },
    Route {
        method: "PUT",
        path: "/api/v1/admin/limits/{key}",
        handler: Handler::AdminLimitPut,
    },
    Route {
        method: "DELETE",
        path: "/api/v1/admin/limits/{key}",
        handler: Handler::AdminLimitReset,
    },
    Route {
        method: "GET",
        path: "/api/v1/connectors",
        handler: Handler::Connectors,
    },
    Route {
        method: "PUT",
        path: "/api/v1/connectors/{name}/credential",
        handler: Handler::ConnectorCredentialPut,
    },
    Route {
        method: "DELETE",
        path: "/api/v1/connectors/{name}/credential",
        handler: Handler::ConnectorCredentialDelete,
    },
    Route {
        method: "GET",
        path: "/api/v1/connectors/{name}/credential/reads",
        handler: Handler::ConnectorCredentialReads,
    },
    Route {
        method: "GET",
        path: "/api/v1/admin/connectors",
        handler: Handler::AdminConnectors,
    },
    Route {
        method: "GET",
        path: "/api/v1/admin/mcp/servers",
        handler: Handler::AdminMcpServers,
    },
    Route {
        method: "PUT",
        path: "/api/v1/admin/connectors/{name}",
        handler: Handler::AdminConnectorPut,
    },
    Route {
        method: "DELETE",
        path: "/api/v1/admin/connectors/{name}",
        handler: Handler::AdminConnectorDelete,
    },
    Route {
        method: "GET",
        path: "/api/v1/admin/decision/engines",
        handler: Handler::AdminDecisionEngines,
    },
    Route {
        method: "PUT",
        path: "/api/v1/admin/decision/engines/{name}",
        handler: Handler::AdminDecisionEnginePut,
    },
    Route {
        method: "DELETE",
        path: "/api/v1/admin/decision/engines/{name}",
        handler: Handler::AdminDecisionEngineDelete,
    },
    Route {
        method: "GET",
        path: "/api/v1/admin/templates",
        handler: Handler::Templates(TemplateRoute::AdminList),
    },
    Route {
        method: "PUT",
        path: "/api/v1/admin/templates/{name}",
        handler: Handler::Templates(TemplateRoute::AdminPut),
    },
    Route {
        method: "DELETE",
        path: "/api/v1/admin/templates/{name}",
        handler: Handler::Templates(TemplateRoute::AdminDelete),
    },
    Route {
        method: "POST",
        path: "/api/v1/admin/templates/{name}/default",
        handler: Handler::Templates(TemplateRoute::AdminDefault),
    },
    Route {
        method: "POST",
        path: "/api/v1/admin/templates/{name}/check",
        handler: Handler::Templates(TemplateRoute::AdminCheck),
    },
    Route {
        method: "POST",
        path: "/api/v1/admin/templates/{name}/reset",
        handler: Handler::Templates(TemplateRoute::AdminReset),
    },
    Route {
        method: "GET",
        path: "/api/v1/admin/environment-images",
        handler: Handler::Templates(TemplateRoute::AdminImages),
    },
    Route {
        method: "GET",
        path: "/api/v1/admin/routes",
        handler: Handler::AdminRoutes,
    },
    Route {
        method: "GET",
        path: "/api/v1/admin/routes/export",
        handler: Handler::AdminRoutesExport,
    },
    Route {
        method: "PUT",
        path: "/api/v1/admin/routes/{name}",
        handler: Handler::AdminRoutePut,
    },
    Route {
        method: "DELETE",
        path: "/api/v1/admin/routes/{name}",
        handler: Handler::AdminRouteDelete,
    },
    Route {
        method: "POST",
        path: "/api/v1/admin/routes/{name}/probe",
        handler: Handler::AdminRouteProbe,
    },
    Route {
        method: "GET",
        path: "/api/v1/admin/routes/{name}/models",
        handler: Handler::AdminRouteModels,
    },
    Route {
        method: "GET",
        path: "/api/v1/invitations",
        handler: Handler::Invitations,
    },
    Route {
        method: "POST",
        path: "/api/v1/invitations",
        handler: Handler::InvitationCreate,
    },
    Route {
        method: "DELETE",
        path: "/api/v1/invitations/{id}",
        handler: Handler::InvitationRevoke,
    },
    Route {
        method: "POST",
        path: "/api/v1/invitations/lookup",
        handler: Handler::InvitationLookup,
    },
    Route {
        method: "POST",
        path: "/api/v1/invitations/accept",
        handler: Handler::InvitationAccept,
    },
    Route {
        method: "GET",
        path: "/api/v1/tokens",
        handler: Handler::Tokens,
    },
    Route {
        method: "POST",
        path: "/api/v1/tokens",
        handler: Handler::TokenCreate,
    },
    Route {
        method: "DELETE",
        path: "/api/v1/tokens/{id}",
        handler: Handler::TokenRevoke,
    },
    Route {
        method: "POST",
        path: "/api/v1/conversations",
        handler: Handler::Create,
    },
    Route {
        method: "GET",
        path: "/api/v1/conversations",
        handler: Handler::List,
    },
    Route {
        method: "GET",
        path: "/api/v1/conversations/{id}",
        handler: Handler::Get,
    },
    Route {
        method: "PATCH",
        path: "/api/v1/conversations/{id}",
        handler: Handler::Patch,
    },
    Route {
        method: "DELETE",
        path: "/api/v1/conversations/{id}",
        handler: Handler::Delete,
    },
    Route {
        method: "PUT",
        path: "/api/v1/conversations/{id}/environment",
        handler: Handler::Environment,
    },
    Route {
        method: "GET",
        path: "/api/v1/conversations/{id}/team",
        handler: Handler::Teams(TeamRoute::Get),
    },
    Route {
        method: "GET",
        path: "/api/v1/conversations/{id}/team/messages",
        handler: Handler::Teams(TeamRoute::Messages),
    },
    Route {
        method: "POST",
        path: "/api/v1/conversations/{id}/team/answers",
        handler: Handler::Teams(TeamRoute::Answer),
    },
    Route {
        method: "POST",
        path: "/api/v1/conversations/{id}/team/stop",
        handler: Handler::Teams(TeamRoute::Stop),
    },
    Route {
        method: "GET",
        path: "/api/v1/admin/agent-teams",
        handler: Handler::Teams(TeamRoute::AdminSettings),
    },
    Route {
        method: "PUT",
        path: "/api/v1/admin/agent-teams",
        handler: Handler::Teams(TeamRoute::AdminSettingsPut),
    },
    Route {
        method: "GET",
        path: "/api/v1/templates",
        handler: Handler::Templates(TemplateRoute::List),
    },
    Route {
        method: "GET",
        path: "/api/v1/profiles",
        handler: Handler::Profiles(ProfileRoute::List),
    },
    Route {
        method: "POST",
        path: "/api/v1/profiles",
        handler: Handler::Profiles(ProfileRoute::Create),
    },
    Route {
        method: "GET",
        path: "/api/v1/profiles/{id}",
        handler: Handler::Profiles(ProfileRoute::Get),
    },
    Route {
        method: "PUT",
        path: "/api/v1/profiles/{id}",
        handler: Handler::Profiles(ProfileRoute::Put),
    },
    Route {
        method: "DELETE",
        path: "/api/v1/profiles/{id}",
        handler: Handler::Profiles(ProfileRoute::Delete),
    },
    Route {
        method: "GET",
        path: "/api/v1/conversations/{id}/events",
        handler: Handler::Events,
    },
    Route {
        method: "POST",
        path: "/api/v1/conversations/{id}/messages",
        handler: Handler::Message,
    },
    Route {
        method: "GET",
        path: "/api/v1/conversations/{id}/messages/queued",
        handler: Handler::HeldMessages,
    },
    Route {
        method: "DELETE",
        path: "/api/v1/conversations/{id}/messages/queued/{message_id}",
        handler: Handler::CancelHeldMessage,
    },
    Route {
        method: "POST",
        path: "/api/v1/conversations/{id}/runs/{run_id}/cancel",
        handler: Handler::Cancel,
    },
    Route {
        method: "GET",
        path: "/api/v1/conversations/{id}/files",
        handler: Handler::FileList,
    },
    Route {
        method: "GET",
        path: "/api/v1/conversations/{id}/files/content",
        handler: Handler::FileContent,
    },
    Route {
        method: "GET",
        path: "/api/v1/conversations/{id}/files/download",
        handler: Handler::FileDownload,
    },
    Route {
        method: "GET",
        path: "/api/v1/conversations/{id}/files/view",
        handler: Handler::FileView,
    },
    Route {
        method: "GET",
        path: "/api/v1/conversations/{id}/files/sheet",
        handler: Handler::FileSheet,
    },
    Route {
        method: "POST",
        path: "/api/v1/conversations/{id}/uploads",
        handler: Handler::Upload,
    },
    Route {
        method: "GET",
        path: "/api/v1/conversations/{id}/runs/{run_id}/changes",
        handler: Handler::RunChanges,
    },
    Route {
        method: "GET",
        path: "/api/v1/conversations/{id}/runs/{run_id}/changes/diff",
        handler: Handler::RunChangesDiff,
    },
    Route {
        method: "GET",
        path: "/runtime/link",
        handler: Handler::Runtime,
    },
];

/// Iterates the method and path of every operation installed by the gateway router.
pub fn registered_routes() -> impl Iterator<Item = (&'static str, &'static str)> {
    ROUTES.iter().map(|route| (route.method, route.path))
}

pub(crate) fn label(path: &str) -> &'static str {
    ROUTES
        .iter()
        .find(|route| route.path == path)
        .map_or("unmatched", |route| route.path)
}
