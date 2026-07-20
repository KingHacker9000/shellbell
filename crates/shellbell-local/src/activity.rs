use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ShellType {
    Bash,
    Zsh,
    Fish,
}

impl std::fmt::Display for ShellType {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Bash => "bash",
            Self::Zsh => "zsh",
            Self::Fish => "fish",
        })
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Off,
    Persistent,
    Once,
}

impl std::fmt::Display for Mode {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Off => "off",
            Self::Persistent => "persistent",
            Self::Once => "once",
        })
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionState {
    Disarmed,
    Armed,
    Active,
    Settling,
    /// The externally documented state; the implementation normally records this as
    /// `burst_notified` while retaining ACTIVE or SETTLING timing information.
    Notified,
}

impl std::fmt::Display for SessionState {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Disarmed => "disarmed",
            Self::Armed => "armed",
            Self::Active => "active",
            Self::Settling => "settling",
            Self::Notified => "notified",
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Session {
    pub id: Uuid,
    pub shell_pid: u32,
    pub shell_type: ShellType,
    pub tty: Option<String>,
    pub mode: Mode,
    pub state: SessionState,
    pub burst_id: Option<Uuid>,
    pub accumulated_active_ms: u64,
    #[serde(skip)]
    pub command_started_mono_ms: Option<u64>,
    #[serde(skip)]
    pub settling_deadline_mono_ms: Option<u64>,
    pub settling_deadline_wall_ms: Option<i64>,
    pub burst_notified: bool,
    pub label: Option<String>,
    pub targets: Vec<String>,
    pub minimum_active_ms: u64,
    pub idle_for_ms: u64,
    pub last_seen_wall_ms: i64,
}

impl Session {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: Uuid,
        shell_pid: u32,
        shell_type: ShellType,
        tty: Option<String>,
        minimum_active_ms: u64,
        idle_for_ms: u64,
        targets: Vec<String>,
        now_wall_ms: i64,
    ) -> Self {
        Self {
            id,
            shell_pid,
            shell_type,
            tty,
            mode: Mode::Off,
            state: SessionState::Disarmed,
            burst_id: None,
            accumulated_active_ms: 0,
            command_started_mono_ms: None,
            settling_deadline_mono_ms: None,
            settling_deadline_wall_ms: None,
            burst_notified: false,
            label: None,
            targets,
            minimum_active_ms,
            idle_for_ms,
            last_seen_wall_ms: now_wall_ms,
        }
    }

    pub fn displayed_active_ms(&self, now_mono_ms: u64) -> u64 {
        self.accumulated_active_ms.saturating_add(
            self.command_started_mono_ms
                .map(|started| now_mono_ms.saturating_sub(started))
                .unwrap_or_default(),
        )
    }

    fn reset_burst(&mut self) {
        self.burst_id = None;
        self.accumulated_active_ms = 0;
        self.command_started_mono_ms = None;
        self.settling_deadline_mono_ms = None;
        self.settling_deadline_wall_ms = None;
        self.burst_notified = false;
    }

    fn reset_after_burst(&mut self) {
        self.reset_burst();
        if self.mode == Mode::Off {
            self.state = SessionState::Disarmed;
        } else {
            self.state = SessionState::Armed;
        }
    }

    pub fn recover_live_timing(&mut self, now_mono_ms: u64, now_wall_ms: i64) {
        self.command_started_mono_ms = None;
        self.settling_deadline_mono_ms = None;
        match self.state {
            SessionState::Active | SessionState::Notified => {
                // The daemon cannot know when foreground control returned while it was down.
                // Discard the incomplete burst instead of inventing a completion time.
                self.reset_after_burst();
            }
            SessionState::Settling => {
                if let Some(wall_deadline) = self.settling_deadline_wall_ms {
                    let remaining = wall_deadline.saturating_sub(now_wall_ms);
                    self.settling_deadline_mono_ms = Some(
                        now_mono_ms.saturating_add(u64::try_from(remaining).unwrap_or_default()),
                    );
                } else {
                    self.reset_after_burst();
                }
            }
            SessionState::Disarmed | SessionState::Armed => {}
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityAction {
    pub event_id: Uuid,
    pub session_id: Option<Uuid>,
    pub message: Option<String>,
    pub targets: Vec<String>,
    pub automatic: bool,
    pub active_duration_ms: Option<u64>,
}

#[derive(Debug, Default)]
pub struct ActivityEngine {
    sessions: BTreeMap<Uuid, Session>,
}

impl ActivityEngine {
    pub fn from_sessions(sessions: impl IntoIterator<Item = Session>) -> Self {
        Self {
            sessions: sessions
                .into_iter()
                .map(|session| (session.id, session))
                .collect(),
        }
    }

    pub fn sessions(&self) -> impl Iterator<Item = &Session> {
        self.sessions.values()
    }

    pub fn get(&self, id: Uuid) -> Option<&Session> {
        self.sessions.get(&id)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn session_open(
        &mut self,
        id: Uuid,
        shell_pid: u32,
        shell_type: ShellType,
        tty: Option<String>,
        minimum_active_ms: u64,
        idle_for_ms: u64,
        targets: Vec<String>,
        now_wall_ms: i64,
    ) {
        if let Some(session) = self.sessions.get_mut(&id) {
            session.shell_pid = shell_pid;
            session.shell_type = shell_type;
            session.tty = tty;
            session.last_seen_wall_ms = now_wall_ms;
        } else {
            self.sessions.insert(
                id,
                Session::new(
                    id,
                    shell_pid,
                    shell_type,
                    tty,
                    minimum_active_ms,
                    idle_for_ms,
                    targets,
                    now_wall_ms,
                ),
            );
        }
    }

    pub fn session_close(&mut self, id: Uuid) -> bool {
        self.sessions.remove(&id).is_some()
    }

    pub fn arm(
        &mut self,
        id: Uuid,
        mode: Mode,
        minimum_active_ms: u64,
        idle_for_ms: u64,
        targets: Vec<String>,
        now_wall_ms: i64,
    ) -> bool {
        let Some(session) = self.sessions.get_mut(&id) else {
            return false;
        };
        session.mode = mode;
        session.minimum_active_ms = minimum_active_ms;
        session.idle_for_ms = idle_for_ms;
        session.targets = targets;
        session.last_seen_wall_ms = now_wall_ms;
        session.reset_burst();
        session.state = SessionState::Armed;
        true
    }

    #[allow(clippy::too_many_arguments)]
    pub fn arm_and_start(
        &mut self,
        id: Uuid,
        mode: Mode,
        minimum_active_ms: u64,
        idle_for_ms: u64,
        targets: Vec<String>,
        now_mono_ms: u64,
        now_wall_ms: i64,
    ) -> bool {
        if !self.arm(
            id,
            mode,
            minimum_active_ms,
            idle_for_ms,
            targets,
            now_wall_ms,
        ) {
            return false;
        }

        self.command_start(id, now_mono_ms, now_wall_ms)
    }

    pub fn disarm(&mut self, id: Uuid, now_wall_ms: i64) -> bool {
        let Some(session) = self.sessions.get_mut(&id) else {
            return false;
        };
        session.mode = Mode::Off;
        session.state = SessionState::Disarmed;
        session.last_seen_wall_ms = now_wall_ms;
        session.reset_burst();
        true
    }

    pub fn set_label(&mut self, id: Uuid, label: Option<String>, now_wall_ms: i64) -> bool {
        let Some(session) = self.sessions.get_mut(&id) else {
            return false;
        };
        session.label = label;
        session.last_seen_wall_ms = now_wall_ms;
        true
    }

    pub fn command_start(&mut self, id: Uuid, now_mono_ms: u64, now_wall_ms: i64) -> bool {
        let Some(session) = self.sessions.get_mut(&id) else {
            return false;
        };
        session.last_seen_wall_ms = now_wall_ms;
        match session.state {
            SessionState::Armed => {
                session.burst_id = Some(Uuid::new_v4());
                session.accumulated_active_ms = 0;
                session.burst_notified = false;
                session.command_started_mono_ms = Some(now_mono_ms);
                session.state = SessionState::Active;
            }
            SessionState::Settling => {
                session.settling_deadline_mono_ms = None;
                session.settling_deadline_wall_ms = None;
                session.command_started_mono_ms = Some(now_mono_ms);
                session.state = SessionState::Active;
            }
            SessionState::Disarmed | SessionState::Active | SessionState::Notified => {}
        }
        true
    }

    pub fn prompt_ready(&mut self, id: Uuid, now_mono_ms: u64, now_wall_ms: i64) -> bool {
        let Some(session) = self.sessions.get_mut(&id) else {
            return false;
        };
        session.last_seen_wall_ms = now_wall_ms;
        if session.state == SessionState::Active {
            if let Some(started) = session.command_started_mono_ms.take() {
                session.accumulated_active_ms = session
                    .accumulated_active_ms
                    .saturating_add(now_mono_ms.saturating_sub(started));
            }
            session.state = SessionState::Settling;
            session.settling_deadline_mono_ms =
                Some(now_mono_ms.saturating_add(session.idle_for_ms));
            session.settling_deadline_wall_ms = Some(
                now_wall_ms.saturating_add(i64::try_from(session.idle_for_ms).unwrap_or(i64::MAX)),
            );
        }
        true
    }

    pub fn manual_ring(
        &mut self,
        session_id: Option<Uuid>,
        message: Option<String>,
        targets: Vec<String>,
        now_mono_ms: u64,
        now_wall_ms: i64,
    ) -> Option<ActivityAction> {
        let Some(id) = session_id else {
            return Some(ActivityAction {
                event_id: Uuid::new_v4(),
                session_id: None,
                message,
                targets,
                automatic: false,
                active_duration_ms: None,
            });
        };
        let Some(session) = self.sessions.get_mut(&id) else {
            return Some(ActivityAction {
                event_id: Uuid::new_v4(),
                session_id: Some(id),
                message,
                targets,
                automatic: false,
                active_duration_ms: None,
            });
        };
        session.last_seen_wall_ms = now_wall_ms;
        let associated = session.burst_id.is_some()
            && matches!(session.state, SessionState::Active | SessionState::Settling);
        if associated && session.burst_notified {
            return None;
        }
        let active_duration_ms = associated.then(|| session.displayed_active_ms(now_mono_ms));
        if associated {
            session.burst_notified = true;
            if session.mode == Mode::Once {
                session.mode = Mode::Off;
                session.state = SessionState::Disarmed;
                session.reset_burst();
            }
        }
        Some(ActivityAction {
            event_id: Uuid::new_v4(),
            session_id: Some(id),
            message,
            targets,
            automatic: false,
            active_duration_ms,
        })
    }

    pub fn tick(&mut self, now_mono_ms: u64, now_wall_ms: i64) -> Vec<ActivityAction> {
        let due: Vec<Uuid> = self
            .sessions
            .values()
            .filter(|session| {
                session.state == SessionState::Settling
                    && session
                        .settling_deadline_mono_ms
                        .is_some_and(|deadline| deadline <= now_mono_ms)
            })
            .map(|session| session.id)
            .collect();
        let mut actions = Vec::new();
        for id in due {
            let session = self.sessions.get_mut(&id).expect("due session exists");
            session.last_seen_wall_ms = now_wall_ms;
            let qualifies = session.accumulated_active_ms >= session.minimum_active_ms;
            if qualifies && !session.burst_notified {
                let label = session.label.as_deref().filter(|value| !value.is_empty());
                actions.push(ActivityAction {
                    event_id: Uuid::new_v4(),
                    session_id: Some(session.id),
                    message: Some(
                        label
                            .map(|value| format!("{value} is ready"))
                            .unwrap_or_else(|| "Shell is ready".to_owned()),
                    ),
                    targets: session.targets.clone(),
                    automatic: true,
                    active_duration_ms: Some(session.accumulated_active_ms),
                });
                session.burst_notified = true;
                if session.mode == Mode::Once {
                    session.mode = Mode::Off;
                }
            }
            session.reset_after_burst();
        }
        actions
    }

    pub fn cleanup_stale(&mut self, older_than_wall_ms: i64) -> Vec<Uuid> {
        let stale: Vec<Uuid> = self
            .sessions
            .values()
            .filter(|session| session.last_seen_wall_ms < older_than_wall_ms)
            .map(|session| session.id)
            .collect();
        for id in &stale {
            self.sessions.remove(id);
        }
        stale
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINUTE: u64 = 60_000;

    fn setup(mode: Mode) -> (ActivityEngine, Uuid) {
        let id = Uuid::new_v4();
        let mut engine = ActivityEngine::default();
        engine.session_open(
            id,
            123,
            ShellType::Bash,
            Some("/dev/pts/1".into()),
            2 * MINUTE,
            45_000,
            vec!["phone".into()],
            1_000,
        );
        assert!(engine.arm(id, mode, 2 * MINUTE, 45_000, vec!["phone".into()], 1_000));
        (engine, id)
    }

    fn command(engine: &mut ActivityEngine, id: Uuid, start: u64, end: u64) {
        assert!(engine.command_start(id, start, start as i64));
        assert!(engine.prompt_ready(id, end, end as i64));
    }

    #[test]
    fn arm_and_start_tracks_work_in_the_same_submission() {
        let id = Uuid::new_v4();
        let mut engine = ActivityEngine::default();

        engine.session_open(
            id,
            123,
            ShellType::Bash,
            Some("/dev/pts/1".into()),
            5_000,
            5_000,
            vec!["pc".into()],
            1_000,
        );

        assert!(engine.arm_and_start(
            id,
            Mode::Once,
            5_000,
            5_000,
            vec!["pc".into()],
            1_000,
            1_000,
        ));

        let active = engine.get(id).unwrap();
        assert_eq!(active.mode, Mode::Once);
        assert_eq!(active.state, SessionState::Active);
        assert_eq!(active.command_started_mono_ms, Some(1_000));

        assert!(engine.prompt_ready(id, 8_000, 8_000));

        let actions = engine.tick(13_000, 13_000);

        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].active_duration_ms, Some(7_000));
        assert_eq!(engine.get(id).unwrap().mode, Mode::Off);
        assert_eq!(engine.get(id).unwrap().state, SessionState::Disarmed);
    }

    #[test]
    fn short_initial_arm_boundary_does_not_consume_once_mode() {
        let id = Uuid::new_v4();
        let mut engine = ActivityEngine::default();

        engine.session_open(
            id,
            123,
            ShellType::Bash,
            None,
            5_000,
            5_000,
            vec!["pc".into()],
            1_000,
        );

        assert!(engine.arm_and_start(
            id,
            Mode::Once,
            5_000,
            5_000,
            vec!["pc".into()],
            1_000,
            1_000,
        ));

        assert!(engine.prompt_ready(id, 1_050, 1_050));
        assert!(engine.tick(6_050, 6_050).is_empty());

        let rearmed = engine.get(id).unwrap();
        assert_eq!(rearmed.mode, Mode::Once);
        assert_eq!(rearmed.state, SessionState::Armed);

        assert!(engine.command_start(id, 7_000, 7_000));
        assert!(engine.prompt_ready(id, 13_000, 13_000));

        let actions = engine.tick(18_000, 18_000);

        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].active_duration_ms, Some(6_000));
        assert_eq!(engine.get(id).unwrap().mode, Mode::Off);
        assert_eq!(engine.get(id).unwrap().state, SessionState::Disarmed);
    }

    #[test]
    fn persistent_burst_rings_and_rearms_for_later_bursts() {
        let (mut engine, id) = setup(Mode::Persistent);
        command(&mut engine, id, 0, 130_000);
        let first = engine.tick(175_000, 175_000);
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].active_duration_ms, Some(130_000));
        assert_eq!(engine.get(id).unwrap().state, SessionState::Armed);
        command(&mut engine, id, 200_000, 321_000);
        assert_eq!(engine.tick(366_000, 366_000).len(), 1);
    }

    #[test]
    fn once_disarms_after_automatic_ring() {
        let (mut engine, id) = setup(Mode::Once);
        command(&mut engine, id, 0, 120_000);
        assert_eq!(engine.tick(165_000, 165_000).len(), 1);
        assert_eq!(engine.get(id).unwrap().mode, Mode::Off);
        assert_eq!(engine.get(id).unwrap().state, SessionState::Disarmed);
    }

    #[test]
    fn once_disarms_after_associated_manual_ring() {
        let (mut engine, id) = setup(Mode::Once);
        engine.command_start(id, 0, 0);
        assert!(
            engine
                .manual_ring(Some(id), Some("done".into()), vec![], 10_000, 10_000)
                .is_some()
        );
        assert_eq!(engine.get(id).unwrap().state, SessionState::Disarmed);
        assert!(engine.prompt_ready(id, 20_000, 20_000));
        assert!(engine.tick(100_000, 100_000).is_empty());
    }

    #[test]
    fn command_during_settling_cancels_timer_and_accumulates() {
        let (mut engine, id) = setup(Mode::Persistent);
        command(&mut engine, id, 0, 70_000);
        engine.command_start(id, 90_000, 90_000);
        assert!(engine.tick(115_000, 115_000).is_empty());
        engine.prompt_ready(id, 150_000, 150_000);
        let actions = engine.tick(195_000, 195_000);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].active_duration_ms, Some(130_000));
    }

    #[test]
    fn short_burst_resets_silently_without_consuming_once() {
        let (mut engine, id) = setup(Mode::Once);
        command(&mut engine, id, 0, 30_000);
        assert!(engine.tick(75_000, 75_000).is_empty());
        assert_eq!(engine.get(id).unwrap().mode, Mode::Once);
        assert_eq!(engine.get(id).unwrap().state, SessionState::Armed);
    }

    #[test]
    fn manual_ring_suppresses_only_current_persistent_burst() {
        let (mut engine, id) = setup(Mode::Persistent);
        engine.command_start(id, 0, 0);
        assert!(
            engine
                .manual_ring(Some(id), None, vec![], 50_000, 50_000)
                .is_some()
        );
        assert!(
            engine
                .manual_ring(Some(id), None, vec![], 51_000, 51_000)
                .is_none()
        );
        engine.prompt_ready(id, 130_000, 130_000);
        assert!(engine.tick(175_000, 175_000).is_empty());
        command(&mut engine, id, 200_000, 330_000);
        assert_eq!(engine.tick(375_000, 375_000).len(), 1);
    }

    #[test]
    fn standalone_manual_ring_does_not_consume_mode() {
        let (mut engine, id) = setup(Mode::Persistent);
        assert!(engine.manual_ring(Some(id), None, vec![], 0, 0).is_some());
        assert_eq!(engine.get(id).unwrap().mode, Mode::Persistent);
        assert_eq!(engine.get(id).unwrap().state, SessionState::Armed);
    }

    #[test]
    fn disarm_cancels_settling() {
        let (mut engine, id) = setup(Mode::Persistent);
        command(&mut engine, id, 0, 130_000);
        engine.disarm(id, 140_000);
        assert!(engine.tick(200_000, 200_000).is_empty());
    }

    #[test]
    fn duplicate_boundaries_are_idempotent() {
        let (mut engine, id) = setup(Mode::Persistent);
        engine.command_start(id, 10_000, 10_000);
        engine.command_start(id, 50_000, 50_000);
        engine.prompt_ready(id, 130_000, 130_000);
        let deadline = engine.get(id).unwrap().settling_deadline_mono_ms;
        engine.prompt_ready(id, 140_000, 140_000);
        assert_eq!(engine.get(id).unwrap().settling_deadline_mono_ms, deadline);
        let action = engine.tick(175_000, 175_000);
        assert_eq!(action[0].active_duration_ms, Some(120_000));
        assert!(engine.tick(200_000, 200_000).is_empty());
    }

    #[test]
    fn multiple_sessions_are_independent_and_stale_cleanup_is_scoped() {
        let (mut engine, first) = setup(Mode::Persistent);
        let second = Uuid::new_v4();
        let stale = Uuid::new_v4();
        engine.session_open(second, 124, ShellType::Fish, None, 10, 5, vec![], 20_000);
        engine.arm(second, Mode::Once, 10, 5, vec![], 20_000);
        engine.session_open(stale, 125, ShellType::Zsh, None, 10, 5, vec![], 1);
        command(&mut engine, first, 0, 130_000);
        command(&mut engine, second, 30_000, 30_010);
        assert_eq!(engine.tick(175_000, 175_000).len(), 2);
        assert_eq!(engine.cleanup_stale(170_000), vec![stale]);
        assert!(engine.get(first).is_some());
    }

    #[test]
    fn restart_discards_active_burst_but_recovers_settling_deadline() {
        let (mut engine, id) = setup(Mode::Persistent);
        engine.command_start(id, 0, 0);
        let mut active = engine.get(id).unwrap().clone();
        active.recover_live_timing(10, 10);
        assert_eq!(active.state, SessionState::Armed);
        command(&mut engine, id, 0, 130_000);
        let mut settling = engine.get(id).unwrap().clone();
        settling.recover_live_timing(1_000, 150_000);
        assert_eq!(settling.settling_deadline_mono_ms, Some(26_000));
        let mut recovered = ActivityEngine::from_sessions([settling]);
        assert_eq!(recovered.tick(26_000, 175_000).len(), 1);
    }
}
