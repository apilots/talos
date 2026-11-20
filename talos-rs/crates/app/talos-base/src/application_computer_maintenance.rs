//! The Server coordinates an independent maintenance schedule for each owned Computer.
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use talos_automation::{Automation, AutomationJob};
use talos_config::{TalosTimezone, YamlConfigStore};
use talos_error::{TalosError, TalosResult};
use talos_store::automation::native_jobs::NativeJobControlStore;
use talos_store::node::{AgentNodeStore, ListNodesParams, NodeListScope, NodeRecord};
use talos_store::session::{ComputerDataScope, UnassignedComputerData};
use talos_types::BuiltinJobId;

use super::application_automation::{MaintenanceJobs, MaintenanceTarget};

pub(super) struct ComputerMaintenance {
    nodes: Arc<dyn AgentNodeStore>,
    config_path: PathBuf,
    peer_id: String,
    controls: Arc<dyn NativeJobControlStore>,
    definitions: MaintenanceJobs,
    automation: OnceLock<Automation>,
}

impl ComputerMaintenance {
    pub(super) fn new(
        nodes: Arc<dyn AgentNodeStore>,
        config_path: PathBuf,
        peer_id: String,
        controls: Arc<dyn NativeJobControlStore>,
        definitions: MaintenanceJobs,
    ) -> Self {
        Self {
            nodes,
            config_path,
            peer_id,
            controls,
            definitions,
            automation: OnceLock::new(),
        }
    }

    pub(super) async fn attach(&self, automation: Automation) -> TalosResult<()> {
        self.automation.set(automation).map_err(|_| {
            TalosError::internal("Computer maintenance is already attached to a scheduler.")
        })?;
        self.refresh().await
    }

    pub(super) fn refresh_job(self: &Arc<Self>, timezone: TalosTimezone) -> AutomationJob {
        let owner = Arc::downgrade(self);
        AutomationJob::cron(
            "computer-maintenance-refresh",
            "*/30 * * * * *",
            timezone,
            move || {
                let owner = owner.clone();
                async move {
                    if let Some(owner) = owner.upgrade() {
                        owner.refresh().await?;
                    }
                    Ok(())
                }
            },
        )
    }

    pub(super) async fn owned_target(
        &self,
        computer_id: &str,
        owner_id: &str,
    ) -> TalosResult<Option<ComputerJobTarget>> {
        Ok(self
            .nodes
            .get(computer_id)
            .await?
            .and_then(|record| self.target(record))
            .filter(|target| target.owner_id == owner_id))
    }

    pub(super) async fn ensure(&self, target: &ComputerJobTarget) -> TalosResult<()> {
        let automation = self
            .automation
            .get()
            .expect("application scheduler attached");
        if automation.has_managed_job(&target.job_name(BuiltinJobId::ProfileUsageRetention)) {
            return Ok(());
        }
        let paused = self
            .controls
            .load_paused(&target.execution_instance_id)
            .await?;
        automation
            .ensure_jobs(
                self.definitions
                    .definitions(&paused, MaintenanceTarget::Computer(target.clone())),
            )
            .await
    }

    pub(super) async fn unassigned_data(&self) -> TalosResult<UnassignedComputerData> {
        let embedded_owner_id = match embedded_computer(&self.config_path)? {
            Some(id) => self
                .nodes
                .get_authentication(&id)
                .await?
                .filter(|record| record.credential_hash.is_some())
                .and_then(|record| record.owner_user_id),
            None => None,
        };
        Ok(UnassignedComputerData { embedded_owner_id })
    }

    fn target(&self, record: NodeRecord) -> Option<ComputerJobTarget> {
        // A synchronized directory entry alone does not belong to this control plane.
        record.credential_hash?;
        let owner_id = record.owner_user_id.filter(|owner| !owner.is_empty())?;
        Some(ComputerJobTarget {
            nodes: self.nodes.clone(),
            config_path: self.config_path.clone(),
            execution_instance_id: format!("{}/{}", self.peer_id, record.node_id),
            computer_id: record.node_id,
            owner_id,
        })
    }

    async fn refresh(&self) -> TalosResult<()> {
        let Some(automation) = self.automation.get() else {
            return Ok(());
        };
        let mut removed: HashSet<String> = automation
            .jobs()
            .await?
            .into_iter()
            .map(|job| job.name)
            .collect();
        let mut cursor = None;
        loop {
            let page = self
                .nodes
                .list(ListNodesParams {
                    exclude_node_id: None,
                    include_managed_workers: true,
                    scope: NodeListScope::All,
                    query: None,
                    cursor,
                    limit: 200,
                })
                .await?;
            for record in page.data {
                if let Some(target) = self.target(record) {
                    self.ensure(&target).await?;
                    removed.remove(&target.job_name(BuiltinJobId::ProfileUsageRetention));
                    removed.remove(&target.job_name(BuiltinJobId::DreamMemoryMaintenance));
                }
            }
            cursor = page.next_cursor;
            if cursor.is_none() {
                return automation.retire_jobs(&removed).await;
            }
        }
    }
}

#[derive(Clone)]
pub(super) struct ComputerJobTarget {
    nodes: Arc<dyn AgentNodeStore>,
    config_path: PathBuf,
    pub execution_instance_id: String,
    computer_id: String,
    owner_id: String,
}

impl ComputerJobTarget {
    pub fn job_name(&self, id: BuiltinJobId) -> String {
        format!("{}{}", self.job_prefix(), id.as_str())
    }

    pub fn job_prefix(&self) -> String {
        format!("{}/", self.execution_instance_id)
    }

    pub async fn current_scope(&self) -> TalosResult<Option<ComputerDataScope>> {
        let valid = self
            .nodes
            .get_authentication(&self.computer_id)
            .await?
            .is_some_and(|record| {
                record.credential_hash.is_some()
                    && record.owner_user_id.as_deref() == Some(self.owner_id.as_str())
            });
        if !valid {
            return Ok(None);
        }
        let is_embedded =
            embedded_computer(&self.config_path)?.as_deref() == Some(&self.computer_id);
        Ok(Some(ComputerDataScope {
            computer_id: self.computer_id.clone(),
            owner_user_id: self.owner_id.clone(),
            include_embedded_execution: is_embedded,
        }))
    }
}

fn embedded_computer(config_path: &Path) -> TalosResult<Option<String>> {
    let store = YamlConfigStore::new(config_path.to_path_buf());
    if store.removed_registration_owner()?.is_some() {
        return Ok(None);
    }
    Ok(store.load_agent_node()?.runner.map(|runner| runner.node_id))
}
