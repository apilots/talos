use std::sync::Arc;

use chrono::{DateTime, Days, Utc};
use talos_config::parse_timezone;
use talos_error::{TalosError, TalosResult};
use talos_store::profile::ProfileStore;
use talos_store::project::ProjectStore;
use talos_store::session::SessionStore;
use talos_types::{ProfileDailyUsage, ProfileModelUsage, ProfileReadResponse};

use crate::Actor;

/// Builds the authenticated user's Profile read model from dedicated stores.
///
/// Gateway services call this API after authentication. It owns user scope and
/// calendar-window policy, but does not expose database records or SQL details.
#[derive(Clone)]
pub struct ProfileService {
    sessions: Arc<dyn SessionStore>,
    projects: Arc<dyn ProjectStore>,
    profiles: Arc<dyn ProfileStore>,
    timezone: String,
}

impl ProfileService {
    pub fn new(
        sessions: Arc<dyn SessionStore>,
        projects: Arc<dyn ProjectStore>,
        profiles: Arc<dyn ProfileStore>,
        timezone: String,
    ) -> Self {
        Self {
            sessions,
            projects,
            profiles,
            timezone,
        }
    }

    pub async fn read_profile(&self, actor: &Actor) -> TalosResult<ProfileReadResponse> {
        self.read_profile_at(actor, Utc::now()).await
    }

    pub(super) async fn read_profile_at(
        &self,
        actor: &Actor,
        now: DateTime<Utc>,
    ) -> TalosResult<ProfileReadResponse> {
        let timezone = parse_timezone(&self.timezone)?;
        let as_of_date = now.with_timezone(&timezone).date_naive();
        let start_date = as_of_date.checked_sub_days(Days::new(363)).ok_or_else(|| {
            TalosError::InternalError("profile date window underflow".to_string())
        })?;
        let start_date = start_date.format("%Y-%m-%d").to_string();
        let as_of_date = as_of_date.format("%Y-%m-%d").to_string();
        let (sessions, project_count, usage, daily_usage, model_usage) = tokio::try_join!(
            self.sessions.summarize_sessions(&actor.user_id),
            self.projects.count_projects_for_owner(&actor.user_id),
            self.profiles.usage_summary(&actor.user_id),
            self.profiles
                .daily_usage(&actor.user_id, &start_date, &as_of_date),
            self.profiles.model_usage(&actor.user_id, 3),
        )?;

        Ok(ProfileReadResponse {
            session_count: sessions.session_count,
            message_count: sessions.message_count,
            project_count,
            usage: usage.usage,
            usage_available: usage.usage_available,
            usage_session_count: usage.usage_session_count,
            usage_started_at: usage
                .usage_started_at_ms
                .map(|timestamp| timestamp.div_euclid(1_000)),
            as_of_date,
            daily_usage: daily_usage
                .into_iter()
                .map(|usage| ProfileDailyUsage {
                    date: usage.date,
                    usage: usage.usage,
                    usage_available: usage.usage_available,
                })
                .collect(),
            model_usage: model_usage
                .into_iter()
                .map(|usage| ProfileModelUsage {
                    model_ref: usage.model_ref,
                    turn_count: usage.turn_count,
                    usage: usage.usage,
                    usage_available: usage.usage_available,
                })
                .collect(),
        })
    }
}
