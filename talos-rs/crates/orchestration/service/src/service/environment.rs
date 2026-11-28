//! Actor-authorized routing to the selected Worker's environment owner.
use super::TalosService;
use crate::Actor;
use talos_error::TalosResult;
use talos_types::*;
impl TalosService {
    pub async fn list_memory_files(
        &self,
        actor: &Actor,
        mut params: MemoryFileListRequest,
    ) -> TalosResult<MemoryFileListResponse> {
        let node_id = params.node_id.take();
        self.worker_environment(actor, node_id.as_deref())
            .await?
            .list_memory_files(crate::access::worker_environment_context(actor), params)
            .await
    }
    pub async fn read_memory_file(
        &self,
        actor: &Actor,
        mut params: MemoryFileReadRequest,
    ) -> TalosResult<MemoryFileReadResponse> {
        let node_id = params.node_id.take();
        self.worker_environment(actor, node_id.as_deref())
            .await?
            .read_memory_file(crate::access::worker_environment_context(actor), params)
            .await
    }
    pub async fn write_memory_file(
        &self,
        actor: &Actor,
        mut params: MemoryFileWriteRequest,
    ) -> TalosResult<MemoryFileWriteResponse> {
        let node_id = params.node_id.take();
        let (_admission, environment) = self
            .active_worker_environment(actor, node_id.as_deref())
            .await?;
        environment
            .write_memory_file(crate::access::worker_environment_context(actor), params)
            .await
    }
    pub async fn search_memory(
        &self,
        actor: &Actor,
        mut params: MemorySearchRequest,
    ) -> TalosResult<MemorySearchResponse> {
        let node_id = params.node_id.take();
        self.worker_environment(actor, node_id.as_deref())
            .await?
            .search_memory(crate::access::worker_environment_context(actor), params)
            .await
    }
    pub async fn get_memory(
        &self,
        actor: &Actor,
        mut params: MemoryGetRequest,
    ) -> TalosResult<MemoryGetResponse> {
        let node_id = params.node_id.take();
        self.worker_environment(actor, node_id.as_deref())
            .await?
            .get_memory(crate::access::worker_environment_context(actor), params)
            .await
    }
    pub async fn list_skills(
        &self,
        actor: &Actor,
        mut params: SkillsListRequest,
    ) -> TalosResult<SkillsListResponse> {
        let node_id = params.node_id.take();
        self.worker_environment(actor, node_id.as_deref())
            .await?
            .list_skills(crate::access::worker_environment_context(actor), params)
            .await
    }
    pub async fn refresh_skills(
        &self,
        actor: &Actor,
        mut params: SkillsRefreshRequest,
    ) -> TalosResult<SkillsListResponse> {
        let node_id = params.node_id.take();
        self.worker_environment(actor, node_id.as_deref())
            .await?
            .refresh_skills(crate::access::worker_environment_context(actor), params)
            .await
    }
    pub async fn suggest_skills(
        &self,
        actor: &Actor,
        mut params: SkillsSuggestRequest,
    ) -> TalosResult<SkillsSuggestResponse> {
        let node_id = params.node_id.take();
        self.worker_environment(actor, node_id.as_deref())
            .await?
            .suggest_skills(crate::access::worker_environment_context(actor), params)
            .await
    }
    pub async fn read_skill(
        &self,
        actor: &Actor,
        mut params: SkillsReadRequest,
    ) -> TalosResult<SkillsReadResponse> {
        let node_id = params.node_id.take();
        self.worker_environment(actor, node_id.as_deref())
            .await?
            .read_skill(crate::access::worker_environment_context(actor), params)
            .await
    }
    pub async fn resolve_skills(
        &self,
        actor: &Actor,
        mut params: SkillsResolveRequest,
    ) -> TalosResult<SkillsResolveResponse> {
        let node_id = params.node_id.take();
        self.worker_environment(actor, node_id.as_deref())
            .await?
            .resolve_skills(crate::access::worker_environment_context(actor), params)
            .await
    }

    pub async fn list_workspace_docs(
        &self,
        actor: &Actor,
        mut params: WorkspaceDocListRequest,
    ) -> TalosResult<WorkspaceDocListResponse> {
        let node_id = params.node_id.take();
        self.worker_environment(actor, node_id.as_deref())
            .await?
            .list_workspace_docs(crate::access::worker_environment_context(actor), params)
            .await
    }
    pub async fn read_workspace_doc(
        &self,
        actor: &Actor,
        mut params: WorkspaceDocReadRequest,
    ) -> TalosResult<WorkspaceDocReadResponse> {
        let node_id = params.node_id.take();
        self.worker_environment(actor, node_id.as_deref())
            .await?
            .read_workspace_doc(crate::access::worker_environment_context(actor), params)
            .await
    }
    pub async fn write_workspace_doc(
        &self,
        actor: &Actor,
        mut params: WorkspaceDocWriteRequest,
    ) -> TalosResult<WorkspaceDocWriteResponse> {
        let node_id = params.node_id.take();
        let (_admission, environment) = self
            .active_worker_environment(actor, node_id.as_deref())
            .await?;
        environment
            .write_workspace_doc(crate::access::worker_environment_context(actor), params)
            .await
    }
}
