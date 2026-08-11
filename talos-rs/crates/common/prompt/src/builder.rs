use std::path::{Path, PathBuf};
use std::sync::Arc;
use talos_error::TalosResult;
use talos_llm_provider::ChatMessage;
use talos_skills::{SkillPromptContext, SkillPromptRequest, SkillProvider};
use talos_tools::{ToolDefinitionProvider, ToolSelectionPhase, ToolSelectionRequest};
use talos_types::{talos::TalosSessionEnvironment, ModelInfo};
use tokio::fs;

use crate::types::*;
use crate::{assembler, bootstrap, renderer, PromptBlock};

/// Unified prompt builder for kernel turns.
pub struct PromptBuilder<'a> {
    pub(crate) agent_name: String,
    pub(crate) agent_description: String,
    pub(crate) mode: PromptMode,
    pub(crate) phase: PromptPhase,
    pub(crate) planner_mode: PlannerMode,
    pub(crate) planning_instruction: Option<String>,
    pub(crate) user_input: Option<String>,
    pub(crate) tool_registry: Option<&'a dyn ToolDefinitionProvider>,
    pub(crate) skill_provider: Option<Arc<dyn SkillProvider>>,
    pub(crate) skill_prompt_request: Option<SkillPromptRequest>,
    pub(crate) skill_prompt_context: Option<SkillPromptContext>,
    pub(crate) session_prompt_context: Option<SessionPromptContext>,
    pub(crate) session_id: Option<&'a str>,
    pub(crate) session_environment: Option<TalosSessionEnvironment>,
    pub(crate) project_root: Option<PathBuf>,
    pub(crate) session_continuity: Option<SessionContinuity>,
    pub(crate) model_info: Option<ModelInfo>,
    pub(crate) config: PromptBuildConfig,
    pub(crate) dynamic_trim_policy: DynamicTrimPolicy,
}

impl<'a> Default for PromptBuilder<'a> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> PromptBuilder<'a> {
    pub fn new() -> Self {
        Self {
            agent_name: "talos".to_string(),
            agent_description: "AI assistant".to_string(),
            mode: PromptMode::Full,
            phase: PromptPhase::Execution,
            planner_mode: PlannerMode::BuiltIn,
            planning_instruction: None,
            user_input: None,
            tool_registry: None,
            skill_provider: None,
            skill_prompt_request: None,
            skill_prompt_context: None,
            session_prompt_context: None,
            session_id: None,
            session_environment: None,
            project_root: None,
            session_continuity: None,
            model_info: None,
            config: PromptBuildConfig::default(),
            dynamic_trim_policy: DynamicTrimPolicy::TrimToBudget,
        }
    }

    pub fn agent(mut self, name: impl Into<String>, description: impl Into<String>) -> Self {
        self.agent_name = name.into();
        self.agent_description = description.into();
        self
    }

    pub fn phase(mut self, phase: PromptPhase) -> Self {
        self.phase = phase;
        self
    }

    pub fn mode(mut self, mode: PromptMode) -> Self {
        self.mode = mode;
        self
    }

    pub fn planner_mode(mut self, planner_mode: impl Into<String>) -> TalosResult<Self> {
        self.planner_mode = PlannerMode::parse(&planner_mode.into())?;
        Ok(self)
    }

    pub fn planning_instruction(mut self, planning_instruction: Option<String>) -> Self {
        self.planning_instruction = planning_instruction;
        self
    }

    pub fn tools(mut self, tool_registry: &'a dyn ToolDefinitionProvider) -> Self {
        self.tool_registry = Some(tool_registry);
        self
    }

    pub fn skills(
        mut self,
        skill_provider: Arc<dyn SkillProvider>,
        request: SkillPromptRequest,
    ) -> Self {
        self.skill_provider = Some(skill_provider);
        self.skill_prompt_request = Some(request);
        self
    }

    pub fn skill_prompt_context(mut self, context: SkillPromptContext) -> Self {
        self.skill_prompt_context = Some(context);
        self
    }

    pub fn session_prompt_context(
        mut self,
        session_id: &'a str,
        context: SessionPromptContext,
    ) -> Self {
        self.session_id = Some(session_id);
        self.session_prompt_context = Some(context);
        self
    }

    pub fn user_input(mut self, input: impl Into<String>) -> Self {
        self.user_input = Some(input.into());
        self
    }

    pub fn config(mut self, config: PromptBuildConfig) -> Self {
        self.config = config;
        self
    }

    pub fn dynamic_trim_policy(mut self, policy: DynamicTrimPolicy) -> Self {
        self.dynamic_trim_policy = policy;
        self
    }

    pub fn session_environment(mut self, environment: Option<TalosSessionEnvironment>) -> Self {
        self.session_environment = environment;
        self
    }

    pub fn project_root(mut self, project_root: Option<PathBuf>) -> Self {
        self.project_root = project_root;
        self
    }

    pub fn session_continuity(mut self, session_continuity: Option<SessionContinuity>) -> Self {
        self.session_continuity = session_continuity;
        self
    }

    pub fn model_info(mut self, model_info: ModelInfo) -> Self {
        self.model_info = Some(model_info);
        self
    }

    pub fn include_session_summary(mut self, include: bool) -> Self {
        self.config.load_policy.include_session_summary = include;
        self
    }

    pub fn include_recent_messages(mut self, include: bool) -> Self {
        self.config.load_policy.include_recent_messages = include;
        self
    }

    pub fn include_recent_tool_history(mut self, include: bool) -> Self {
        self.config.load_policy.include_recent_tool_history = include;
        self
    }

    pub fn include_recent_tool_outputs(mut self, include: bool) -> Self {
        self.config.load_policy.include_recent_tool_outputs = include;
        self
    }

    pub async fn build(self) -> TalosResult<Vec<ChatMessage>> {
        let assembly = self.assemble().await?;
        Ok(renderer::render_assembly(&assembly))
    }

    pub async fn assemble(self) -> TalosResult<PromptAssembly> {
        assembler::assemble(&self).await
    }

    pub(crate) fn tool_selection_request(&self) -> ToolSelectionRequest {
        let mut request = ToolSelectionRequest {
            phase: match self.phase {
                PromptPhase::Planning => ToolSelectionPhase::Planning,
                PromptPhase::Execution => ToolSelectionPhase::Execution,
            },
            planner_mode: Some(self.planner_mode.as_str().to_string()),
            permission_mode: talos_tools::PermissionMode::Default,
            user_input: self.user_input.clone(),
            session_id: self.session_id.map(str::to_string),
        };

        if matches!(self.phase, PromptPhase::Planning) {
            request.permission_mode = talos_tools::PermissionMode::Plan;
        }

        request
    }

    async fn read_bootstrap_file(
        path: &Path,
        kind: WorkspaceBootstrapFileKind,
        scope: BootstrapFileScope,
    ) -> TalosResult<WorkspaceBootstrapFile> {
        let content = match fs::read_to_string(path).await {
            Ok(content) => Some(content),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
            Err(err) => {
                return Err(talos_error::TalosError::internal(format!(
                    "read bootstrap file {}: {}",
                    path.display(),
                    err
                )));
            }
        };
        Ok(WorkspaceBootstrapFile {
            kind,
            scope,
            path: path.to_path_buf(),
            content,
        })
    }

    pub(crate) async fn load_workspace_bootstrap_files(
        &self,
    ) -> TalosResult<Vec<WorkspaceBootstrapFile>> {
        if !self.config.prompt.bootstrap.enabled {
            return Ok(Vec::new());
        }
        let workspace_dir_config = self.workspace_dir();
        if workspace_dir_config.is_empty() {
            return Ok(Vec::new());
        }
        let workspace_dir = bootstrap::expand_workspace_dir(workspace_dir_config);
        fs::create_dir_all(&workspace_dir).await.map_err(|err| {
            talos_error::TalosError::internal(format!(
                "create prompt workspace dir {}: {}",
                workspace_dir.display(),
                err
            ))
        })?;

        let mut files = Vec::new();
        for kind in WorkspaceBootstrapFileKind::workspace_files() {
            if !kind.should_load_in_mode(self.mode, &self.config.prompt) {
                continue;
            }
            let path = workspace_dir.join(kind.file_name());
            if self.config.prompt.bootstrap.auto_create_missing_files && kind.should_auto_create() {
                bootstrap::ensure_bootstrap_file(&path, kind.default_template()).await?;
            }
            files.push(
                Self::read_bootstrap_file(&path, *kind, BootstrapFileScope::Workspace).await?,
            );
        }
        Ok(files)
    }

    pub(crate) async fn load_project_bootstrap_files(
        &self,
    ) -> TalosResult<Vec<WorkspaceBootstrapFile>> {
        if !self.config.prompt.bootstrap.enabled {
            return Ok(Vec::new());
        }
        let Some(project_root) = self.project_root.as_ref() else {
            return Ok(Vec::new());
        };
        let workspace_dir_config = self.workspace_dir();
        let workspace_dir = if workspace_dir_config.is_empty() {
            None
        } else {
            Some(bootstrap::expand_workspace_dir(workspace_dir_config))
        };

        let mut files = Vec::new();
        for kind in WorkspaceBootstrapFileKind::project_files() {
            if !kind.should_load_in_mode(self.mode, &self.config.prompt) {
                continue;
            }
            let project_path = project_root.join(kind.file_name());
            let project_file =
                Self::read_bootstrap_file(&project_path, *kind, BootstrapFileScope::ProjectRoot)
                    .await?;
            if project_file.content.is_some() {
                files.push(project_file);
                continue;
            }

            if let Some(workspace_dir) = workspace_dir.as_ref() {
                let workspace_path = workspace_dir.join(kind.file_name());
                let workspace_file = Self::read_bootstrap_file(
                    &workspace_path,
                    *kind,
                    BootstrapFileScope::Workspace,
                )
                .await?;
                if workspace_file.content.is_some() {
                    files.push(workspace_file);
                    continue;
                }
            }
            files.push(project_file);
        }
        Ok(files)
    }

    fn workspace_dir(&self) -> &str {
        self.config.workspace_dir.trim()
    }
}

pub fn render_prompt_assembly(assembly: &PromptAssembly) -> Vec<ChatMessage> {
    renderer::render_assembly(assembly)
}

pub fn render_prompt_blocks(assembly: &PromptAssembly) -> Vec<PromptBlock> {
    renderer::render_prompt_blocks(assembly)
}

/// Render bounded retrieved MemoryInfo values as one dynamic prompt section.
pub fn render_retrieved_memories(
    memories: &[talos_types::MemoryInfo],
    max_items: usize,
    max_chars: usize,
) -> Option<ChatMessage> {
    assembler::build_retrieved_memories_section(memories, max_items, max_chars)
        .map(|section| ChatMessage::system(renderer::render_section_text(&section)))
}
