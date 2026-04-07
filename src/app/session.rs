use std::io;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::{mpsc, Notify};
use tracing::info;

use super::state::AgentPanelScope;
use crate::config::Config;
use crate::events::AppEvent;
use crate::persist::{
    self, LoadResult, PersistenceWorker, RestoreReport, RestoreStatus, SessionSnapshot,
};
use crate::workspace::Workspace;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RestoreOutcome {
    NoSnapshot,
    CleanRestore,
    PartialRestore,
    RestoreFailed,
    NewerSnapshotIgnored,
}

pub(crate) struct StartupSessionState {
    pub workspaces: Vec<Workspace>,
    pub active: Option<usize>,
    pub selected: usize,
    pub agent_panel_scope: AgentPanelScope,
    pub sidebar_width: u16,
    pub restore_outcome: RestoreOutcome,
    pub diagnostics: SessionDiagnostics,
}

pub(crate) fn load_startup_session(
    config: &Config,
    no_session: bool,
    event_tx: mpsc::Sender<AppEvent>,
    render_notify: Arc<Notify>,
    render_dirty: Arc<AtomicBool>,
) -> StartupSessionState {
    if no_session {
        return StartupSessionState::empty(
            config.ui.sidebar_width,
            RestoreOutcome::NoSnapshot,
            None,
        );
    }

    match persist::load() {
        LoadResult::NoSnapshot => {
            StartupSessionState::empty(config.ui.sidebar_width, RestoreOutcome::NoSnapshot, None)
        }
        LoadResult::Loaded(snapshot) => {
            let report = persist::restore(
                &snapshot,
                24,
                80,
                config.advanced.scrollback_limit_bytes,
                event_tx,
                render_notify,
                render_dirty,
            );
            StartupSessionState::from_restore_report(snapshot, report, config.ui.sidebar_width)
        }
        LoadResult::NewerSnapshotIgnored { version } => StartupSessionState::empty(
            config.ui.sidebar_width,
            RestoreOutcome::NewerSnapshotIgnored,
            Some(format!(
                "session snapshot is from newer herdr version {version}; autosave paused until you make a structural session change"
            )),
        ),
        LoadResult::Failed { message } => StartupSessionState::empty(
            config.ui.sidebar_width,
            RestoreOutcome::RestoreFailed,
            Some(format!(
                "{message}; autosave paused until you make a structural session change"
            )),
        ),
    }
}

impl StartupSessionState {
    fn empty(
        sidebar_width: u16,
        restore_outcome: RestoreOutcome,
        diagnostic: Option<String>,
    ) -> Self {
        Self {
            workspaces: Vec::new(),
            active: None,
            selected: 0,
            agent_panel_scope: AgentPanelScope::CurrentWorkspace,
            sidebar_width,
            restore_outcome,
            diagnostics: SessionDiagnostics::new(diagnostic),
        }
    }

    fn from_restore_report(
        snapshot: SessionSnapshot,
        report: RestoreReport,
        default_sidebar_width: u16,
    ) -> Self {
        let sidebar_width = snapshot.sidebar_width.unwrap_or(default_sidebar_width);
        let workspaces = report.workspaces;
        let active = snapshot.active.filter(|&i| i < workspaces.len());
        let selected = if workspaces.is_empty() {
            0
        } else {
            snapshot.selected.min(workspaces.len().saturating_sub(1))
        };

        let (restore_outcome, diagnostic, log_message) = match report.status {
            RestoreStatus::Clean => (RestoreOutcome::CleanRestore, None, "session restored"),
            RestoreStatus::Partial => (
                RestoreOutcome::PartialRestore,
                Some(
                    "session restored partially; autosave paused until you make a structural session change"
                        .to_string(),
                ),
                "session restored partially",
            ),
            RestoreStatus::Failed => (
                RestoreOutcome::RestoreFailed,
                Some(
                    "session restore failed; keeping existing session.json until you make a structural session change"
                        .to_string(),
                ),
                "session file found but no workspaces restored",
            ),
        };

        info!(count = workspaces.len(), "{log_message}");

        Self {
            workspaces,
            active,
            selected,
            agent_panel_scope: snapshot.agent_panel_scope,
            sidebar_width,
            restore_outcome,
            diagnostics: SessionDiagnostics::new(diagnostic),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct SessionDiagnostics {
    startup: Option<String>,
    shutdown_listener: Option<String>,
    persistence_failure: Option<String>,
}

impl SessionDiagnostics {
    pub(crate) fn new(startup: Option<String>) -> Self {
        Self {
            startup,
            shutdown_listener: None,
            persistence_failure: None,
        }
    }

    pub(crate) fn current(&self) -> Option<String> {
        self.persistence_failure
            .clone()
            .or_else(|| self.shutdown_listener.clone())
            .or_else(|| self.startup.clone())
    }

    pub(crate) fn shutdown_listener_failed(&mut self, error: &str) {
        self.shutdown_listener = Some(format!(
            "graceful shutdown persistence unavailable: {error}"
        ));
    }

    pub(crate) fn persistence_failed(&mut self, action: &'static str, error: &str) {
        self.persistence_failure = Some(format!("{action} failed: {error}"));
    }

    pub(crate) fn persistence_succeeded(&mut self) {
        self.startup = None;
        self.persistence_failure = None;
    }
}

#[derive(Debug, Default)]
pub(crate) struct SessionEditTracker {
    has_authoritative_change: bool,
}

impl SessionEditTracker {
    pub(crate) fn mark_authoritative_change(&mut self) {
        self.has_authoritative_change = true;
    }

    pub(crate) fn has_authoritative_change(&self) -> bool {
        self.has_authoritative_change
    }
}

pub(crate) struct SessionPersistenceController {
    enabled: bool,
    worker: Option<PersistenceWorker>,
    autosave_interval: Option<Duration>,
    next_save: Option<Instant>,
    armed: bool,
    restore_outcome: RestoreOutcome,
}

impl SessionPersistenceController {
    pub(crate) fn new(
        no_session: bool,
        autosave_interval_secs: u64,
        worker: Option<PersistenceWorker>,
        restore_outcome: RestoreOutcome,
    ) -> Self {
        let enabled = !no_session;
        let armed = enabled
            && matches!(
                restore_outcome,
                RestoreOutcome::NoSnapshot | RestoreOutcome::CleanRestore
            );
        let autosave_interval = (enabled && autosave_interval_secs > 0)
            .then_some(Duration::from_secs(autosave_interval_secs));
        let next_save = armed
            .then(|| autosave_interval)
            .flatten()
            .map(|interval| Instant::now() + interval);

        Self {
            enabled,
            worker,
            autosave_interval,
            next_save,
            armed,
            restore_outcome,
        }
    }

    #[cfg(test)]
    pub(crate) fn is_armed(&self) -> bool {
        self.armed
    }

    pub(crate) fn next_save_deadline(&self) -> Option<Instant> {
        self.next_save
    }

    pub(crate) fn arm_if_needed(&mut self, now: Instant, has_authoritative_change: bool) -> bool {
        if !self.enabled || self.armed || !has_authoritative_change {
            return false;
        }

        info!(
            restore_outcome = ?self.restore_outcome,
            "session persistence armed after structural change"
        );
        self.armed = true;
        if self.next_save.is_none() {
            self.next_save = self.autosave_interval.map(|interval| now + interval);
        }
        true
    }

    pub(crate) fn autosave_due(&self, now: Instant) -> bool {
        self.next_save.is_some_and(|deadline| now >= deadline)
    }

    pub(crate) fn enqueue_snapshot(&mut self, snapshot: SessionSnapshot) -> Option<u64> {
        if !self.enabled || !self.armed {
            return None;
        }

        let worker = self.worker.as_ref()?;
        Some(if snapshot.workspaces.is_empty() {
            worker.enqueue_clear()
        } else {
            worker.enqueue_save(snapshot)
        })
    }

    pub(crate) fn reschedule_after_save_attempt(&mut self, now: Instant) {
        self.next_save = self.autosave_interval.map(|interval| now + interval);
    }

    pub(crate) fn finalize(&mut self, snapshot: Option<SessionSnapshot>) -> io::Result<()> {
        if self.armed {
            if let Some(snapshot) = snapshot {
                let _ = self.enqueue_snapshot(snapshot);
            }
        }

        let Some(worker) = self.worker.take() else {
            return Ok(());
        };

        worker.shutdown().map_err(io::Error::other)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::state::AgentPanelScope;
    use crate::persist::{SessionSnapshot, WorkspaceSnapshot};
    use std::path::PathBuf;

    fn startup_snapshot() -> SessionSnapshot {
        SessionSnapshot {
            version: 3,
            workspaces: vec![WorkspaceSnapshot {
                id: Some("w-1".to_string()),
                custom_name: Some("one".to_string()),
                identity_cwd: PathBuf::from("/tmp"),
                tabs: Vec::new(),
                active_tab: 0,
            }],
            active: Some(5),
            selected: 4,
            agent_panel_scope: AgentPanelScope::AllWorkspaces,
            sidebar_width: Some(31),
        }
    }

    #[test]
    fn startup_session_partial_restore_clamps_selection_and_sets_warning() {
        let snapshot = startup_snapshot();
        let state = StartupSessionState::from_restore_report(
            snapshot,
            RestoreReport {
                workspaces: Vec::new(),
                status: RestoreStatus::Failed,
            },
            26,
        );

        assert_eq!(state.active, None);
        assert_eq!(state.selected, 0);
        assert_eq!(state.agent_panel_scope, AgentPanelScope::AllWorkspaces);
        assert_eq!(state.sidebar_width, 31);
        assert_eq!(state.restore_outcome, RestoreOutcome::RestoreFailed);
        assert_eq!(
            state.diagnostics.current().as_deref(),
            Some(
                "session restore failed; keeping existing session.json until you make a structural session change"
            )
        );
    }

    #[test]
    fn session_diagnostics_preserve_shutdown_warning_after_save_success() {
        let mut diagnostics = SessionDiagnostics::new(Some(
            "session restored partially; autosave paused until you make a structural session change"
                .to_string(),
        ));

        diagnostics.shutdown_listener_failed("registration failed");
        diagnostics.persistence_failed("save session", "disk full");
        assert_eq!(
            diagnostics.current().as_deref(),
            Some("save session failed: disk full")
        );

        diagnostics.persistence_succeeded();
        assert_eq!(
            diagnostics.current().as_deref(),
            Some("graceful shutdown persistence unavailable: registration failed")
        );
    }

    #[test]
    fn persistence_controller_arms_only_after_authoritative_change() {
        let mut controller =
            SessionPersistenceController::new(false, 60, None, RestoreOutcome::PartialRestore);
        let now = Instant::now();

        assert!(!controller.arm_if_needed(now, false));
        assert!(controller.next_save_deadline().is_none());

        assert!(controller.arm_if_needed(now, true));
        assert!(controller.next_save_deadline().is_some());
    }
}
