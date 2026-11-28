//! The handlers a route names, and the route itself; the table is `routes::ROUTES`.
/// Host keys (ADR-0037): a member's own, the shared ones they may use, and the administrators'.
#[derive(Clone, Copy, Debug)]
pub(crate) enum HostKeyRoute {
    List,
    Add,
    Put,
    Delete,
    Shared,
    Reads,
    AdminList,
    AdminAdd,
    AdminPut,
    AdminDelete,
    AdminReads,
    TaskUses,
}

/// The template operations, grouped so the dispatch stays one line per area.
#[derive(Clone, Copy)]
pub(crate) enum TemplateRoute {
    List,
    AdminList,
    AdminPut,
    AdminDelete,
    AdminDefault,
    AdminCheck,
    AdminImages,
    AdminReset,
}

/// The agent teams' routes, one sub-router (ADR-0034): a run's team tools (the S1 contract and its slices) and the
/// people's side (S6): a team's state, its messages, answering the lead, Stop, and the administrator's two settings.
#[derive(Clone, Copy)]
pub(crate) enum TeamRoute {
    RuntimeRead,
    RuntimePost,
    RuntimeStart,
    RuntimeTakeFiles,
    RuntimeRole,
    RuntimeDone,
    RuntimeAcceptance,
    RuntimeStopMember,
    RuntimeReadHelper,
    Get,
    Messages,
    Answer,
    Stop,
    AdminSettings,
    AdminSettingsPut,
}

/// The agent profile operations (ADR-0012).
#[derive(Clone, Copy)]
pub(crate) enum ProfileRoute {
    List,
    Get,
    Create,
    Put,
    Delete,
}

/// The channel operations (ADR-0014).
#[derive(Clone, Copy)]
pub(crate) enum ChannelRoute {
    Create,
    List,
    Get,
    /// `PATCH /api/v1/channels/{id}`: its wake budget (ADR-0014's amendment, A5; decided on 20 Nov 2026, C-1).
    SetBudget,
    End,
    Delete,
    Members,
    PutMember,
    RemoveMember,
    Read,
    Post,
    RuntimeList,
    RuntimeRead,
    RuntimePost,
    Claim,
    GiveBack,
    Tasks,
    RuntimeClaim,
    Proposal,
    Events,
    PutMemberByEmail,
    Candidates,
    Upload,
    FileDownload,
    DeleteUpload,
}

#[derive(Clone, Copy)]
pub(crate) enum Handler {
    Health,
    FileList,
    FileContent,
    FileDownload,
    FileView,
    FileSheet,
    /// A pass that serves a workspace folder as a site, and the files it serves (18.0 previews for real sites).
    SiteMint,
    SiteRoot,
    SiteFile,
    Upload,
    /// A person's changes to a workspace's memory notes (ADR-0041).
    MemoryNotePut,
    MemoryNoteDelete,
    MemoryNoteUndo,
    RunChanges,
    RunChangesDiff,
    Ready,
    Bootstrap,
    Setup,
    Login,
    Logout,
    ChangePassword,
    Me,
    Models,
    Create,
    List,
    Get,
    Patch,
    Delete,
    /// The template list and the admin templates API (ADR-0032).
    Templates(TemplateRoute),
    Teams(TeamRoute),
    Profiles(ProfileRoute),
    Channels(ChannelRoute),
    Environment,
    Events,
    Message,
    HeldMessages,
    CancelHeldMessage,
    Cancel,
    Runtime,
    History,
    RelayChat,
    RelayResponses,
    RelayMessages,
    RelayModels,
    /// `decision/v1` to a decision engine (ADR-0013 point 6).
    RelayDecision,
    Users,
    UserChange,
    AdminUsage,
    /// The Sandboxes page: the listing and an administrator's stop (the Sandboxes page design note).
    AdminSandboxes,
    AdminSandboxStop,
    AdminSecrets,
    AdminSecretPut,
    AdminSecretReminder,
    AdminSecretReads,
    AdminSecretConsumers,
    AdminSecretDelete,
    AdminConnectors,
    AdminMcpServers,
    McpProxy,
    McpProxyListen,
    McpProxyClose,
    Connectors,
    ConnectorCredentialPut,
    ConnectorCredentialDelete,
    ConnectorCredentialReads,
    AdminConnectorPut,
    AdminConnectorDelete,
    AdminDecisionEngines,
    AdminDecisionEnginePut,
    AdminDecisionEngineDelete,
    AdminEgress,
    AdminEgressRefusals,
    AdminEgressPut,
    AdminEgressDelete,
    AdminEgressPresets,
    AdminEgressPresetApply,
    AdminEgressPresetRemove,
    AdminLimits,
    AdminLimitPut,
    AdminLimitReset,
    AdminRoutes,
    AdminRoutesExport,
    AdminRoutePut,
    AdminRouteDelete,
    AdminRouteProbe,
    AdminRouteModels,
    Invitations,
    InvitationCreate,
    InvitationRevoke,
    InvitationLookup,
    InvitationAccept,
    Tokens,
    TokenCreate,
    TokenRevoke,
    WorkspaceList,
    WorkspaceCreate,
    WorkspaceGet,
    WorkspaceUpdate,
    WorkspaceFork,
    HostKeys(HostKeyRoute),
    /// The desktop's screen (ADR-0039 slice 5).
    Screen(ScreenRoute),
    WorkspaceDelete,
    WorkspaceShares,
    WorkspaceShareByEmail,
    WorkspaceShare,
    WorkspaceUnshare,
}

pub(crate) struct Route {
    pub method: &'static str,
    pub path: &'static str,
    pub handler: Handler,
}

/// The desktop's screen (ADR-0039 slice 5): the viewer and the sandbox's bridge, the notices, Sign out everywhere and a
/// stored screenshot's read.
#[derive(Clone, Copy, Debug)]
pub(crate) enum ScreenRoute {
    Notices,
    SignOut,
    View,
    Bridge,
    Screenshot,
}
