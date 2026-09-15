use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use semver::Version;
use serde::{Deserialize, Serialize};
use talos_config::{Config, TalosPaths};
use talos_error::{ErrorCode, TalosError, TalosResult};
use talos_fs::FileSystem;

mod cleanup;
use cleanup::UpgradeCleanupState;

const STATE_SCHEMA_VERSION: u32 = 1;
const UPGRADE_DIR: &str = "upgrades";
const UPGRADE_STATE_FILE: &str = "state.json";
const UPGRADE_LOCK_FILE: &str = ".lock";

/// Immutable execution context shared by product upgrade tasks.
pub(crate) struct UpgradeContext<'a> {
    pub config: &'a Config,
    pub database_pool: &'a sqlx::SqlitePool,
    pub paths: &'a TalosPaths,
}

/// Boxed async execution returned by one registered Product Upgrade task.
pub(crate) type UpgradeFuture<'a> = Pin<Box<dyn Future<Output = TalosResult<()>> + Send + 'a>>;
/// Required task callback; implementations mutate only the state owned by that task.
pub(crate) type UpgradeExecuteFn = for<'a> fn(&'a UpgradeContext<'_>) -> UpgradeFuture<'a>;

/// Standard execution boundary implemented by every startup upgrade runner.
///
/// The startup coordinator calls runners serially and consumes only their typed
/// reports. A runner owns the lifecycle rules of the migration system it wraps.
pub(crate) trait UpgradeRunner {
    type Report;

    fn run(self) -> impl Future<Output = TalosResult<Self::Report>> + Send;
}

/// Immutable metadata required for every Product Upgrade Task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UpgradeTaskDefinition {
    pub id: &'static str,
    pub revision: u32,
    pub target_version: Version,
    /// Lower values execute first within the same target version.
    pub priority: u32,
}

/// Standard API implemented by every Product Upgrade Task.
///
/// Tasks expose immutable metadata, business execution, and artifact cleanup. Implementations
/// must not read or write the Product Upgrade ledger or manage retries.
pub(crate) trait UpgradeTask: Send + Sync {
    fn definition(&self) -> &UpgradeTaskDefinition;

    fn execute<'a>(&'a self, context: &'a UpgradeContext<'_>) -> UpgradeFuture<'a>;

    /// Called under the upgrade lock after all applicable tasks and the product
    /// version are durable. Remove only owned intermediate data, preserve unique
    /// originals, and tolerate missing paths and partial cleanup. Errors are
    /// reported and retried by the runner without repeating successful execution.
    fn cleanup<'a>(&'a self, context: &'a UpgradeContext<'_>) -> UpgradeFuture<'a>;
}

/// Function-backed task registration used by the current product migrations.
pub(crate) struct UpgradeTaskRegistration {
    definition: UpgradeTaskDefinition,
    execute: UpgradeExecuteFn,
    cleanup: UpgradeExecuteFn,
}

impl UpgradeTaskRegistration {
    pub(crate) const fn new(
        definition: UpgradeTaskDefinition,
        execute: UpgradeExecuteFn,
        cleanup: UpgradeExecuteFn,
    ) -> Self {
        Self {
            definition,
            execute,
            cleanup,
        }
    }
}

impl UpgradeTask for UpgradeTaskRegistration {
    fn definition(&self) -> &UpgradeTaskDefinition {
        &self.definition
    }

    fn execute<'a>(&'a self, context: &'a UpgradeContext<'_>) -> UpgradeFuture<'a> {
        (self.execute)(context)
    }

    fn cleanup<'a>(&'a self, context: &'a UpgradeContext<'_>) -> UpgradeFuture<'a> {
        (self.cleanup)(context)
    }
}

/// Startup result for the generic product upgrade runner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpgradeRunReport {
    pub state_path: PathBuf,
    pub previous_version: String,
    pub current_version: String,
    pub fresh_install: bool,
    pub applied: Vec<String>,
    pub skipped: Vec<String>,
    pub deferred: Vec<String>,
}

/// Strongly typed on-disk Product Upgrade ledger.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct UpgradeState {
    schema_version: u32,
    product_version: Version,
    tasks: BTreeMap<String, UpgradeTaskState>,
}

impl UpgradeState {
    fn new(product_version: Version) -> Self {
        Self {
            schema_version: STATE_SCHEMA_VERSION,
            product_version,
            tasks: BTreeMap::new(),
        }
    }
}

/// Persisted lifecycle state for one Upgrade Task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct UpgradeTaskState {
    revision: u32,
    target_version: Version,
    executed_version: Version,
    status: UpgradeTaskStatus,
    updated_at: i64,
    #[serde(default)]
    cleanup: UpgradeCleanupState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum UpgradeTaskStatus {
    Running,
    Failed,
    Success,
}

/// Executes registered Product Upgrade Tasks and owns their common lifecycle state.
pub(crate) struct ProductUpgradeRunner<'a> {
    database_pool: &'a sqlx::SqlitePool,
    config: &'a Config,
    tasks: &'a [Box<dyn UpgradeTask>],
    existing_installation: bool,
    current_version: Version,
}

impl<'a> ProductUpgradeRunner<'a> {
    pub(crate) fn for_version(
        config: &'a Config,
        database_pool: &'a sqlx::SqlitePool,
        tasks: &'a [Box<dyn UpgradeTask>],
        existing_installation: bool,
        current_version: &str,
    ) -> TalosResult<Self> {
        Ok(Self {
            database_pool,
            config,
            tasks,
            existing_installation,
            current_version: parse_version(current_version, "current Talos product version")?,
        })
    }
}

impl UpgradeRunner for ProductUpgradeRunner<'_> {
    type Report = UpgradeRunReport;

    async fn run(self) -> TalosResult<Self::Report> {
        let paths = self.config.talos_paths();
        let result = run_with_paths(
            self.config,
            self.database_pool,
            &paths,
            &self.current_version,
            self.tasks,
            self.existing_installation,
        )
        .await;
        if let Err(error) = &result {
            tracing::error!(error = %error, "product upgrade runner failed");
        }
        result
    }
}

async fn run_with_paths(
    config: &Config,
    database_pool: &sqlx::SqlitePool,
    paths: &TalosPaths,
    current_version: &Version,
    tasks: &[Box<dyn UpgradeTask>],
    existing_installation: bool,
) -> TalosResult<UpgradeRunReport> {
    let ordered_tasks = validate_and_order_tasks(tasks)?;
    let task_target_ceiling = task_target_ceiling(current_version);
    let upgrade_dir = paths.talos_home.join(UPGRADE_DIR);
    fs::create_dir_all(&upgrade_dir)
        .map_err(|error| io_error("create upgrade state directory", &upgrade_dir, error))?;
    let _lock = talos_fs::std_fs().with_file_lock(&upgrade_dir.join(UPGRADE_LOCK_FILE))?;
    let state_path = upgrade_dir.join(UPGRADE_STATE_FILE);
    let context = UpgradeContext {
        config,
        database_pool,
        paths,
    };
    let loaded_state = load_state(&state_path)?;
    let fresh_install = loaded_state.is_none() && !existing_installation;
    let mut state = match loaded_state {
        Some(state) => state,
        None if fresh_install => UpgradeState::new(current_version.clone()),
        None => UpgradeState::new(Version::new(0, 1, 6)),
    };
    // Baseline only tasks shipped with a fresh installation. A later patch may
    // add a task for the same product version and must still execute it once.
    if fresh_install {
        for task in &ordered_tasks {
            let definition = task.definition();
            if definition.target_version <= task_target_ceiling {
                state.tasks.insert(
                    definition.id.to_string(),
                    UpgradeTaskState {
                        revision: definition.revision,
                        target_version: definition.target_version.clone(),
                        executed_version: current_version.clone(),
                        cleanup: UpgradeCleanupState::completed(unix_seconds()?),
                        status: UpgradeTaskStatus::Success,
                        updated_at: unix_seconds()?,
                    },
                );
            }
        }
    }
    if !state_path.exists() {
        write_state(&state_path, &state)?;
    }
    let installed_version = state.product_version.clone();
    if &installed_version > current_version {
        return Err(TalosError::new(
            ErrorCode::InvalidRequest,
            format!(
                "Talos product downgrade is not supported: installed {}, current {}",
                installed_version, current_version
            ),
        ));
    }
    validate_task_states(tasks, &state)?;
    tracing::info!(
        state_path = %state_path.display(),
        previous_version = %installed_version,
        current_version = %current_version,
        fresh_install,
        registered_tasks = tasks.len(),
        tracked_tasks = state.tasks.len(),
        "product upgrade runner started"
    );
    let mut report = UpgradeRunReport {
        state_path: state_path.clone(),
        previous_version: installed_version.to_string(),
        current_version: current_version.to_string(),
        fresh_install,
        applied: Vec::new(),
        skipped: Vec::new(),
        deferred: Vec::new(),
    };

    for task in &ordered_tasks {
        let definition = task.definition();
        let target_version = &definition.target_version;
        let task_started_at = Instant::now();
        if target_version > &task_target_ceiling {
            tracing::info!(
                upgrade_id = definition.id,
                revision = definition.revision,
                target_version = %target_version,
                current_version = %current_version,
                elapsed_ms = task_started_at.elapsed().as_millis(),
                "upgrade task deferred"
            );
            report.deferred.push(definition.id.to_string());
            continue;
        }
        if state
            .tasks
            .get(definition.id)
            .is_some_and(|task_state| task_state.status == UpgradeTaskStatus::Success)
        {
            log_skipped(
                definition,
                target_version,
                "completed_retry",
                task_started_at,
            );
            report.skipped.push(definition.id.to_string());
            continue;
        }

        state.tasks.insert(
            definition.id.to_string(),
            UpgradeTaskState {
                revision: definition.revision,
                target_version: target_version.clone(),
                executed_version: current_version.clone(),
                cleanup: UpgradeCleanupState::pending(unix_seconds()?),
                status: UpgradeTaskStatus::Running,
                updated_at: unix_seconds()?,
            },
        );
        write_state(&state_path, &state)?;
        tracing::info!(
            upgrade_id = definition.id,
            revision = definition.revision,
            target_version = %target_version,
            "upgrade task started"
        );
        if let Err(error) = task.execute(&context).await {
            let task_state = state.tasks.get_mut(definition.id).ok_or_else(|| {
                TalosError::internal(
                    "missing running upgrade task state; technical details were withheld.",
                )
                .context(format!(
                    "missing running upgrade task state: {}",
                    definition.id
                ))
            })?;
            task_state.status = UpgradeTaskStatus::Failed;
            task_state.updated_at = unix_seconds()?;
            if let Err(state_error) = write_state(&state_path, &state) {
                tracing::error!(
                    upgrade_id = definition.id,
                    revision = definition.revision,
                    state_path = %state_path.display(),
                    error = %state_error,
                    "upgrade failure state write failed"
                );
            }
            tracing::error!(
                upgrade_id = definition.id,
                revision = definition.revision,
                target_version = %target_version,
                elapsed_ms = task_started_at.elapsed().as_millis(),
                error = %error,
                "upgrade task failed"
            );
            return Err(error);
        }
        let task_state = state.tasks.get_mut(definition.id).ok_or_else(|| {
            TalosError::internal(
                "missing running upgrade task state; technical details were withheld.",
            )
            .context(format!(
                "missing running upgrade task state: {}",
                definition.id
            ))
        })?;
        task_state.status = UpgradeTaskStatus::Success;
        task_state.updated_at = unix_seconds()?;
        write_state(&state_path, &state).map_err(|error| {
            tracing::error!(
                upgrade_id = definition.id,
                revision = definition.revision,
                state_path = %state_path.display(),
                elapsed_ms = task_started_at.elapsed().as_millis(),
                error = %error,
                "upgrade completion state write failed"
            );
            error
        })?;
        tracing::info!(
            upgrade_id = definition.id,
            revision = definition.revision,
            target_version = %target_version,
            elapsed_ms = task_started_at.elapsed().as_millis(),
            "upgrade task completed"
        );
        report.applied.push(definition.id.to_string());
    }

    if &installed_version != current_version {
        state.product_version = current_version.clone();
        write_state(&state_path, &state)?;
        tracing::info!(
            previous_version = %installed_version,
            current_version = %current_version,
            "Talos product upgrade version advanced"
        );
    }
    cleanup::run(&ordered_tasks, &context, &state_path, &mut state).await;
    tracing::info!(report = ?report, "product upgrade runner completed");
    Ok(report)
}

/// Allows a development SNAPSHOT to exercise Tasks owned by its pending stable release.
fn task_target_ceiling(current_version: &Version) -> Version {
    if current_version.pre.as_str() == "SNAPSHOT" {
        Version::new(
            current_version.major,
            current_version.minor,
            current_version.patch,
        )
    } else {
        current_version.clone()
    }
}

fn validate_and_order_tasks(tasks: &[Box<dyn UpgradeTask>]) -> TalosResult<Vec<&dyn UpgradeTask>> {
    let mut ids = BTreeSet::new();
    for task in tasks {
        let definition = task.definition();
        if definition.id.trim().is_empty() || definition.revision == 0 {
            return Err(TalosError::internal(
                "upgrade IDs must be non-empty and revisions must be positive",
            ));
        }
        if !ids.insert(definition.id) {
            return Err(TalosError::internal(
                "duplicate upgrade ID; technical details were withheld.",
            )
            .context(format!("duplicate upgrade ID: {}", definition.id)));
        }
    }
    let mut ordered = tasks.iter().map(Box::as_ref).collect::<Vec<_>>();
    ordered.sort_by(|left, right| {
        left.definition()
            .target_version
            .cmp(&right.definition().target_version)
            .then_with(|| left.definition().priority.cmp(&right.definition().priority))
            .then_with(|| left.definition().id.cmp(right.definition().id))
    });
    Ok(ordered)
}

fn validate_task_states(tasks: &[Box<dyn UpgradeTask>], state: &UpgradeState) -> TalosResult<()> {
    for task in tasks {
        let definition = task.definition();
        let Some(task_state) = state.tasks.get(definition.id) else {
            continue;
        };
        if task_state.revision != definition.revision
            || task_state.target_version != definition.target_version
        {
            return Err(TalosError::new(
                ErrorCode::InvalidRequest,
                format!(
                    "upgrade task '{}' changed from revision {} target {} to revision {} target {}",
                    definition.id,
                    task_state.revision,
                    task_state.target_version,
                    definition.revision,
                    definition.target_version
                ),
            ));
        }
    }
    Ok(())
}

fn log_skipped(
    definition: &UpgradeTaskDefinition,
    target_version: &Version,
    reason: &str,
    started_at: Instant,
) {
    tracing::info!(
        upgrade_id = definition.id,
        revision = definition.revision,
        target_version = %target_version,
        reason,
        elapsed_ms = started_at.elapsed().as_millis(),
        "upgrade task skipped"
    );
}

fn load_state(path: &Path) -> TalosResult<Option<UpgradeState>> {
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read(path).map_err(|error| io_error("read upgrade state", path, error))?;
    let mut state: UpgradeState = serde_json::from_slice(&raw).map_err(|error| {
        TalosError::new(
            ErrorCode::InvalidRequest,
            format!("invalid upgrade state at {}: {error}", path.display()),
        )
    })?;
    if state.schema_version != STATE_SCHEMA_VERSION {
        return Err(TalosError::new(
            ErrorCode::InvalidRequest,
            format!(
                "unsupported upgrade state schema version {} at {}",
                state.schema_version,
                path.display()
            ),
        ));
    }
    for task in state.tasks.values_mut() {
        if task.cleanup.updated_at == 0 {
            task.cleanup.updated_at = task.updated_at;
        }
    }
    Ok(Some(state))
}

fn write_state(target: &Path, state: &UpgradeState) -> TalosResult<()> {
    talos_fs::std_fs().write_json_pretty(target, state)
}

fn parse_version(value: &str, label: &str) -> TalosResult<Version> {
    Version::parse(value).map_err(|error| {
        TalosError::new(
            ErrorCode::InvalidRequest,
            format!("invalid {label} '{value}': {error}"),
        )
    })
}

fn unix_seconds() -> TalosResult<i64> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| {
            TalosError::internal_with_source(
                "system clock is before Unix epoch; technical details were withheld.",
                error,
            )
        })?;
    i64::try_from(duration.as_secs())
        .map_err(|_| TalosError::internal("upgrade completion timestamp overflow"))
}

fn io_error(operation: &str, path: &Path, error: std::io::Error) -> TalosError {
    {
        let diagnostic = format!("{operation} {}: {error}", path.display());
        TalosError::internal_with_source(
            "Upgrade filesystem operation failed; technical details were withheld.",
            error,
        )
        .context(diagnostic)
    }
}

#[cfg(test)]
#[path = "runner_tests.rs"]
mod tests;
