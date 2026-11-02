//! Authentication projection for the existing closed Worker request oneof.
use agent_pb as pb;

/// Transport ingress validates the supplied account credential, then replaces
/// only authentication with the target connection's private node lease.
pub fn auth(request: &pb::worker_rpc_call::Request) -> Option<&pb::NodeAuth> {
    use pb::worker_rpc_call::Request as Call;
    macro_rules! read { ($($variant:ident),+ $(,)?) => { match request { $(Call::$variant(value) => value.auth.as_ref(),)+ } }; }
    read!(
        SetRegistrationAdmission,
        ActiveSessionCommand,
        ReadSessionExecutionState,
        CommitSessionExecutionState,
        PeerFileRead,
        PeerFileUpdate,
        PeerFileDelete,
        AppendSessionHistorySupplement,
        ReadSessionHistory,
        ResolveAttachments,
        ReadMemoryProgress,
        AdvanceMemoryProgress,
        SpawnSubagent,
        SendSubagentMessage,
        WaitSubagent,
        InterruptSubagent,
        FollowUpSubagent,
        ListSubagents,
        SaveToolImage,
        CompleteMcpAuthentication,
        StartMcpAuthentication,
        ImportMcpServer,
        UpdateMcpCredentials,
        ReadMcpAuthentication,
        Browser,
        DesktopOpen,
        ListMcpServers,
        DisablePlugin,
        EnablePlugin,
        UninstallPlugin,
        UpdatePlugin,
        InstallPlugin,
        PreparePluginChange,
        CancelPluginChange,
        ReadPlugin,
        ListPlugins,
        SearchPlugins,
        RemovePluginSource,
        RefreshPluginSource,
        AddPluginSource,
        ListPluginSources,
        ReloadConfig,
        DeleteConfig,
        UpdateConfig,
        GetConfig,
        Start,
        SubmitInput,
        Interrupt,
        CompactSession,
        ClearSession,
        CloseSession,
        ReadWorkspaceMemory,
        UpdateWorkspaceMemory,
        FormDreamMemory,
        Shutdown,
        ListMemoryFiles,
        ReadMemoryFile,
        WriteMemoryFile,
        SearchMemory,
        GetMemory,
        ListSkills,
        RefreshSkills,
        SuggestSkills,
        ReadSkill,
        ResolveSkills,
        WorkspaceInfo,
        ListModels,
        PreviewResource,
        ReadDirectory,
        CreateDirectory,
        ProjectStatus,
        ValidateProjectDirectory,
        ListWorkspaceDocs,
        ReadWorkspaceDoc,
        WriteWorkspaceDoc
    )
}

pub fn replace_auth(request: &mut pb::worker_rpc_call::Request, auth: &pb::NodeAuth) {
    use pb::worker_rpc_call::Request as Call;
    macro_rules! replace { ($($variant:ident),+ $(,)?) => { match request { $(Call::$variant(value) => value.auth = Some(auth.clone()),)+ } }; }
    replace!(
        SetRegistrationAdmission,
        ActiveSessionCommand,
        ReadSessionExecutionState,
        CommitSessionExecutionState,
        PeerFileRead,
        PeerFileUpdate,
        PeerFileDelete,
        AppendSessionHistorySupplement,
        ReadSessionHistory,
        ResolveAttachments,
        ReadMemoryProgress,
        AdvanceMemoryProgress,
        SpawnSubagent,
        SendSubagentMessage,
        WaitSubagent,
        InterruptSubagent,
        FollowUpSubagent,
        ListSubagents,
        SaveToolImage,
        CompleteMcpAuthentication,
        StartMcpAuthentication,
        ImportMcpServer,
        UpdateMcpCredentials,
        ReadMcpAuthentication,
        Browser,
        DesktopOpen,
        ListMcpServers,
        DisablePlugin,
        EnablePlugin,
        UninstallPlugin,
        UpdatePlugin,
        InstallPlugin,
        PreparePluginChange,
        CancelPluginChange,
        ReadPlugin,
        ListPlugins,
        SearchPlugins,
        RemovePluginSource,
        RefreshPluginSource,
        AddPluginSource,
        ListPluginSources,
        ReloadConfig,
        DeleteConfig,
        UpdateConfig,
        GetConfig,
        Start,
        SubmitInput,
        Interrupt,
        CompactSession,
        ClearSession,
        CloseSession,
        ReadWorkspaceMemory,
        UpdateWorkspaceMemory,
        FormDreamMemory,
        Shutdown,
        ListMemoryFiles,
        ReadMemoryFile,
        WriteMemoryFile,
        SearchMemory,
        GetMemory,
        ListSkills,
        RefreshSkills,
        SuggestSkills,
        ReadSkill,
        ResolveSkills,
        WorkspaceInfo,
        ListModels,
        PreviewResource,
        ReadDirectory,
        CreateDirectory,
        ProjectStatus,
        ValidateProjectDirectory,
        ListWorkspaceDocs,
        ReadWorkspaceDoc,
        WriteWorkspaceDoc
    )
}

/// Extract only execution identity; no input, checkpoint or business history is retained.
pub fn execution_scope(
    request: &pb::worker_rpc_call::Request,
) -> Option<pb::CoordinatorCallContext> {
    use pb::worker_rpc_call::Request as Call;
    let (session, turn_id, project_id) = match request {
        Call::Interrupt(value) => return Some(value.caller.clone().unwrap_or_default()),
        Call::SubmitInput(value) => return Some(value.caller.clone().unwrap_or_default()),
        Call::ReadSessionExecutionState(value) => (value.params.clone()?, None, None),
        Call::ClearSession(value) | Call::CloseSession(value) => {
            (value.params.clone()?, None, None)
        }
        Call::CommitSessionExecutionState(value) => {
            let params = value.params.as_ref()?;
            (
                params.session.clone()?,
                params.identity.as_ref().map(|id| id.turn_id.clone()),
                None,
            )
        }
        Call::Start(value) => {
            let pb::start_params::Body::Run(run) = value.params.as_ref()?.body.as_ref()? else {
                return None;
            };
            let context = run.context.as_ref()?;
            (
                context.session.clone()?,
                Some(context.turn_id.clone()),
                context.project_id.clone(),
            )
        }
        Call::CompactSession(value) => {
            let pb::compact_session_params::Body::Context(context) =
                value.params.as_ref()?.body.as_ref()?
            else {
                return None;
            };
            (
                context.session.clone()?,
                Some(context.turn_id.clone()),
                context.project_id.clone(),
            )
        }
        Call::FormDreamMemory(value) => {
            let pb::form_dream_memory_request::Body::Context(session) = value.body.as_ref()? else {
                return None;
            };
            (session.clone(), None, None)
        }
        _ => return None,
    };
    Some(pb::CoordinatorCallContext {
        coordinator_id: String::new(),
        connection_id: String::new(),
        rpc_call_id: 0,
        session: Some(session),
        turn_id,
        project_id,
    })
}
/// Attach verified transport routing without changing the business params.
pub fn set_execution_caller(
    request: &mut pb::worker_rpc_call::Request,
    caller: &pb::CoordinatorCallContext,
) {
    use pb::worker_rpc_call::Request as Call;
    match request {
        Call::ReadSessionExecutionState(value) => value.caller = Some(caller.clone()),
        Call::Interrupt(value) => value.caller = Some(caller.clone()),
        Call::SubmitInput(value) => value.caller = Some(caller.clone()),
        Call::ClearSession(value) | Call::CloseSession(value) => {
            value.caller = Some(caller.clone())
        }
        Call::CommitSessionExecutionState(value) => value.caller = Some(caller.clone()),
        Call::Start(value) => value.caller = Some(caller.clone()),
        Call::CompactSession(value) => value.caller = Some(caller.clone()),
        Call::FormDreamMemory(value) => value.caller = Some(caller.clone()),
        _ => {}
    }
}

/// Original resource requests carry the same invocation context in either transport.
pub fn resource_caller(
    request: &pb::worker_rpc_call::Request,
) -> Option<&pb::CoordinatorCallContext> {
    use pb::worker_rpc_call::Request as Call;
    match request {
        Call::AppendSessionHistorySupplement(value) => value.caller.as_ref(),
        Call::ReadSessionHistory(value) => value.caller.as_ref(),
        Call::ResolveAttachments(value) => value.caller.as_ref(),
        Call::ReadMemoryProgress(value) => value.caller.as_ref(),
        Call::AdvanceMemoryProgress(value) => value.caller.as_ref(),
        Call::SpawnSubagent(value) => value.caller.as_ref(),
        Call::SendSubagentMessage(value) => value.caller.as_ref(),
        Call::WaitSubagent(value) => value.caller.as_ref(),
        Call::InterruptSubagent(value) => value.caller.as_ref(),
        Call::FollowUpSubagent(value) => value.caller.as_ref(),
        Call::ListSubagents(value) => value.caller.as_ref(),
        Call::SaveToolImage(value) => value.caller.as_ref(),
        _ => None,
    }
}

pub fn is_management_peer_call(request: &pb::worker_rpc_call::Request) -> bool {
    matches!(
        request,
        pb::worker_rpc_call::Request::ActiveSessionCommand(_)
            | pb::worker_rpc_call::Request::PeerFileRead(_)
            | pb::worker_rpc_call::Request::PeerFileUpdate(_)
            | pb::worker_rpc_call::Request::PeerFileDelete(_)
    )
}
