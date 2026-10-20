//! Physical effect regressions: repeating a breathing command resets its phase.
//! Synthetic time only; no Bluetooth, network, sleeps, or real user task state.
use light_core::{
    led_output::{LedCommand, LedDelivery, LedOutputSync},
    model::{Engine, LightState, Snapshot, WireSession},
    sound::{Action, SoundConfig, SoundEngine},
};

const BASE: u64 = 1_800_000_000_137;
const TEN_MINUTES: u64 = 600_000;

fn command(state: LightState) -> LedCommand {
    LedCommand {
        state,
        period_ms: 3000,
        fade_ms: 1500,
        serial: "001122334455".into(),
    }
}

#[derive(Clone, Copy)]
struct Transmission {
    applied: bool,
    ack: Option<bool>,
}
const APPLIED_WITHOUT_ACK: Transmission = Transmission {
    applied: true,
    ack: None,
};
const LOST_WITHOUT_ACK: Transmission = Transmission {
    applied: false,
    ack: None,
};
const REJECTED: Transmission = Transmission {
    applied: false,
    ack: Some(false),
};

struct SimulatedLamp {
    state: LightState,
    period_ms: u64,
    effect_started_ms: Option<u64>,
    attempts: Vec<(u64, LedCommand)>,
}

impl SimulatedLamp {
    fn new(state: LightState) -> Self {
        Self {
            state,
            period_ms: 0,
            effect_started_ms: None,
            attempts: Vec::new(),
        }
    }

    fn write(&mut self, desired: &LedCommand, now: u64, result: Transmission) -> LedDelivery {
        self.attempts.push((now, desired.clone()));
        // Command application and acknowledgement are independent. In particular,
        // a missing ACK must not make an already-running effect look unapplied.
        if result.applied {
            self.state = desired.state;
            self.period_ms = u64::from(desired.period_ms);
            self.effect_started_ms = match desired.state {
                LightState::Working | LightState::Error => Some(now),
                _ => None,
            };
        }
        match result.ack {
            Some(true) => LedDelivery::Confirmed,
            Some(false) => LedDelivery::Failed,
            None => LedDelivery::Unconfirmed,
        }
    }

    fn phase_ms(&self, now: u64) -> Option<u64> {
        self.effect_started_ms
            .map(|start| (now - start) % self.period_ms)
    }

    fn power_cycle(&mut self) {
        self.state = LightState::Off;
        self.effect_started_ms = None;
    }
}

fn dispatch(
    sync: &mut LedOutputSync,
    lamp: &mut SimulatedLamp,
    desired: &LedCommand,
    now: u64,
    result: Transmission,
) -> bool {
    if !sync.should_send(desired, now) {
        return false;
    }
    let delivery = lamp.write(desired, now, result);
    sync.record_attempt(desired.clone(), now, delivery);
    true
}

fn assert_undisturbed_breathing(ack: Option<bool>) {
    for state in [LightState::Working, LightState::Error] {
        for period_ms in [2300, 3000, 5000] {
            let desired = LedCommand {
                period_ms,
                fade_ms: period_ms / 200 * 100,
                ..command(state)
            };
            let mut sync = LedOutputSync::default();
            let mut lamp = SimulatedLamp::new(LightState::Done);
            for elapsed in (0..=TEN_MINUTES).step_by(100) {
                let now = BASE + elapsed;
                dispatch(
                    &mut sync,
                    &mut lamp,
                    &desired,
                    now,
                    Transmission { applied: true, ack },
                );
                assert_eq!(lamp.state, state);
                assert_eq!(lamp.effect_started_ms, Some(BASE));
                assert_eq!(lamp.phase_ms(now), Some(elapsed % u64::from(period_ms)));
                assert_eq!(
                    lamp.attempts.len(),
                    1,
                    "normal breathing must never be restarted"
                );
            }
        }
    }
}

#[test]
fn acknowledged_breathing_runs_ten_minutes_without_a_repeated_command() {
    assert_undisturbed_breathing(Some(true));
}

#[test]
fn applied_breathing_without_ack_runs_ten_minutes_without_a_repeated_command() {
    assert_undisturbed_breathing(None);
}

#[test]
fn unknown_lost_write_stays_unconfirmed_until_an_explicit_refresh_recovers_it() {
    for state in [LightState::Working, LightState::Off] {
        let desired = command(state);
        let mut sync = LedOutputSync::default();
        let mut lamp = SimulatedLamp::new(LightState::Done);
        assert!(dispatch(
            &mut sync,
            &mut lamp,
            &desired,
            BASE,
            LOST_WITHOUT_ACK
        ));

        for elapsed in [5000, 15_000, 35_000, 60_000, TEN_MINUTES] {
            assert!(!dispatch(
                &mut sync,
                &mut lamp,
                &desired,
                BASE + elapsed,
                APPLIED_WITHOUT_ACK
            ));
            assert_eq!(lamp.state, LightState::Done);
        }
        assert_eq!(lamp.attempts.len(), 1);
        // No state readback exists: lack of ACK cannot promise automatic recovery.
        sync.invalidate();
        assert!(dispatch(
            &mut sync,
            &mut lamp,
            &desired,
            BASE + TEN_MINUTES,
            APPLIED_WITHOUT_ACK
        ));
        assert_eq!(lamp.state, state);
        assert_eq!(lamp.attempts.len(), 2);
        assert!(!sync.should_send(&desired, BASE + TEN_MINUTES * 2));
    }
}

#[test]
fn explicit_rejections_retry_with_bounded_backoff() {
    let desired = command(LightState::Off);
    let mut sync = LedOutputSync::default();
    let mut lamp = SimulatedLamp::new(LightState::Done);
    let mut now = BASE;
    for delay_ms in [5000, 10_000, 20_000, 30_000, 30_000, 30_000] {
        assert!(dispatch(&mut sync, &mut lamp, &desired, now, REJECTED));
        assert!(!sync.should_send(&desired, now + delay_ms - 1));
        assert!(sync.should_send(&desired, now + delay_ms));
        assert_eq!(lamp.state, LightState::Done);
        now += delay_ms;
    }
    assert_eq!(lamp.attempts.len(), 6);
}

#[test]
fn successful_or_unconfirmed_application_after_failures_stops_all_retries() {
    for ack in [Some(true), None] {
        let desired = command(LightState::Working);
        let mut sync = LedOutputSync::default();
        let mut lamp = SimulatedLamp::new(LightState::Done);
        for elapsed in [0, 5000, 15_000] {
            assert!(dispatch(
                &mut sync,
                &mut lamp,
                &desired,
                BASE + elapsed,
                REJECTED
            ));
        }
        let recovered_at = BASE + 35_000;
        assert!(dispatch(
            &mut sync,
            &mut lamp,
            &desired,
            recovered_at,
            Transmission { applied: true, ack }
        ));
        for elapsed in (100..=TEN_MINUTES).step_by(100) {
            assert!(!dispatch(
                &mut sync,
                &mut lamp,
                &desired,
                recovered_at + elapsed,
                APPLIED_WITHOUT_ACK
            ));
            assert_eq!(lamp.effect_started_ms, Some(recovered_at));
            assert_eq!(lamp.phase_ms(recovered_at + elapsed), Some(elapsed % 3000));
        }
        assert_eq!(lamp.attempts.len(), 4);
    }
}

#[test]
fn new_target_preempts_failed_old_target_without_a_delayed_replay() {
    let old = command(LightState::Done);
    let desired = command(LightState::Error);
    let mut sync = LedOutputSync::default();
    let mut lamp = SimulatedLamp::new(LightState::Off);
    for elapsed in [0, 5000, 15_000] {
        assert!(dispatch(
            &mut sync,
            &mut lamp,
            &old,
            BASE + elapsed,
            REJECTED
        ));
    }
    let changed_at = BASE + 15_001;
    assert!(!sync.should_send(&old, changed_at));
    assert!(dispatch(
        &mut sync,
        &mut lamp,
        &desired,
        changed_at,
        APPLIED_WITHOUT_ACK
    ));
    assert!(!dispatch(
        &mut sync,
        &mut lamp,
        &desired,
        BASE + 35_000,
        APPLIED_WITHOUT_ACK
    ));
    assert_eq!(lamp.state, LightState::Error);
    assert_eq!(lamp.effect_started_ms, Some(changed_at));
    assert_eq!(lamp.attempts.len(), 4);
}

#[test]
fn a_changed_target_does_not_inherit_the_previous_failure_backoff() {
    let old = command(LightState::Done);
    let desired = command(LightState::Off);
    let mut sync = LedOutputSync::default();
    let mut lamp = SimulatedLamp::new(LightState::Done);
    for elapsed in [0, 5000, 15_000] {
        assert!(dispatch(
            &mut sync,
            &mut lamp,
            &old,
            BASE + elapsed,
            REJECTED
        ));
    }
    let changed_at = BASE + 15_001;
    assert!(dispatch(
        &mut sync, &mut lamp, &desired, changed_at, REJECTED
    ));
    assert!(!sync.should_send(&desired, changed_at + 4999));
    assert!(dispatch(
        &mut sync,
        &mut lamp,
        &desired,
        changed_at + 5000,
        APPLIED_WITHOUT_ACK
    ));
    assert_eq!(lamp.state, LightState::Off);
    assert!(!sync.should_send(&desired, BASE + TEN_MINUTES));
}

#[test]
fn changed_breathing_parameters_or_device_apply_once() {
    for state in [LightState::Working, LightState::Error] {
        let old = command(state);
        let changed_commands = [
            LedCommand {
                period_ms: 4000,
                ..old.clone()
            },
            LedCommand {
                fade_ms: 1000,
                ..old.clone()
            },
            LedCommand {
                serial: "5566778899aa".into(),
                ..old.clone()
            },
        ];
        for desired in changed_commands {
            let mut sync = LedOutputSync::default();
            let mut lamp = SimulatedLamp::new(LightState::Off);
            assert!(dispatch(
                &mut sync,
                &mut lamp,
                &old,
                BASE,
                APPLIED_WITHOUT_ACK
            ));
            assert!(dispatch(
                &mut sync,
                &mut lamp,
                &desired,
                BASE + 1300,
                APPLIED_WITHOUT_ACK
            ));
            assert!(!dispatch(
                &mut sync,
                &mut lamp,
                &desired,
                BASE + TEN_MINUTES,
                APPLIED_WITHOUT_ACK
            ));
            assert_eq!(lamp.effect_started_ms, Some(BASE + 1300));
            assert_eq!(lamp.attempts.len(), 2);
        }
    }
}

#[test]
fn static_effects_ignore_breathing_parameter_edits_until_breathing_is_requested() {
    for state in [LightState::Off, LightState::Done, LightState::Waiting] {
        for ack in [Some(true), None] {
            let old = command(state);
            let changed = LedCommand {
                period_ms: 4000,
                fade_ms: 1000,
                ..old.clone()
            };
            let mut sync = LedOutputSync::default();
            let mut lamp = SimulatedLamp::new(LightState::Off);
            assert!(dispatch(
                &mut sync,
                &mut lamp,
                &old,
                BASE,
                Transmission { applied: true, ack }
            ));
            for elapsed in [1, 5000, 30_000, TEN_MINUTES] {
                assert!(!dispatch(
                    &mut sync,
                    &mut lamp,
                    &changed,
                    BASE + elapsed,
                    APPLIED_WITHOUT_ACK
                ));
            }
            assert_eq!(lamp.attempts.len(), 1);
            let breathing = LedCommand {
                state: LightState::Working,
                ..changed
            };
            assert!(dispatch(
                &mut sync,
                &mut lamp,
                &breathing,
                BASE + TEN_MINUTES + 1,
                APPLIED_WITHOUT_ACK
            ));
            assert_eq!(lamp.period_ms, 4000);
        }
    }
}

#[test]
fn a_different_device_still_receives_an_unchanged_static_effect() {
    for state in [LightState::Off, LightState::Done, LightState::Waiting] {
        let old = command(state);
        let desired = LedCommand {
            serial: "5566778899aa".into(),
            ..old.clone()
        };
        let mut sync = LedOutputSync::default();
        let mut lamp = SimulatedLamp::new(LightState::Off);
        assert!(dispatch(
            &mut sync,
            &mut lamp,
            &old,
            BASE,
            APPLIED_WITHOUT_ACK
        ));
        assert!(dispatch(
            &mut sync,
            &mut lamp,
            &desired,
            BASE + 1,
            APPLIED_WITHOUT_ACK
        ));
        assert!(!sync.should_send(&desired, BASE + TEN_MINUTES));
        assert_eq!(lamp.attempts.len(), 2);
        assert_eq!(lamp.attempts[1].1.serial, desired.serial);
    }
}

fn snapshot(state: LightState, turn: &str, revision: u64, age_ms: u64) -> Snapshot {
    Snapshot {
        schema: 1,
        source_id: "fixture-vm".into(),
        generation: "fixture-generation".into(),
        revision,
        sessions: vec![WireSession {
            id: "fixture-session".into(),
            turn: turn.into(),
            version: revision,
            state,
            age_ms,
            event: "Fixture".into(),
        }],
    }
}

#[test]
fn heartbeats_and_a_new_working_turn_do_not_restart_the_same_breathing_effect() {
    let mut engine = Engine::default();
    engine
        .accept(snapshot(LightState::Working, "first", 1, 0), BASE, 300)
        .unwrap();
    let original_episode = engine.view(BASE, 90).sessions[0].episode_id.clone();
    let mut sync = LedOutputSync::default();
    let mut lamp = SimulatedLamp::new(LightState::Off);
    for elapsed in (0..=120_000).step_by(1000) {
        let now = BASE + elapsed;
        if elapsed % 15_000 == 0 {
            let (turn, revision, age) = if elapsed < 60_000 {
                ("first", 1, elapsed)
            } else {
                ("second", 2, elapsed - 60_000)
            };
            engine
                .accept(snapshot(LightState::Working, turn, revision, age), now, 300)
                .unwrap();
        }
        let aggregate = engine.view(now, 90);
        dispatch(
            &mut sync,
            &mut lamp,
            &command(aggregate.state),
            now,
            APPLIED_WITHOUT_ACK,
        );
        assert_eq!(lamp.effect_started_ms, Some(BASE));
    }
    assert_ne!(
        engine.view(BASE + 120_000, 90).sessions[0].episode_id,
        original_episode
    );
    assert_eq!(lamp.attempts.len(), 1);
}

#[test]
fn unknown_and_repeated_connected_observations_are_not_refresh_requests() {
    for ack in [Some(true), None] {
        let desired = command(LightState::Working);
        let mut sync = LedOutputSync::default();
        let mut lamp = SimulatedLamp::new(LightState::Off);
        assert!(!sync.observe_connection(None));
        assert!(!sync.observe_connection(Some(true)));
        assert!(dispatch(
            &mut sync,
            &mut lamp,
            &desired,
            BASE,
            Transmission { applied: true, ack }
        ));
        for elapsed in [1, 5000, 30_000, TEN_MINUTES] {
            assert!(!sync.observe_connection(None));
            assert!(!sync.observe_connection(Some(true)));
            assert!(!dispatch(
                &mut sync,
                &mut lamp,
                &desired,
                BASE + elapsed,
                APPLIED_WITHOUT_ACK
            ));
        }
        assert_eq!(lamp.effect_started_ms, Some(BASE));
        assert_eq!(lamp.attempts.len(), 1);
    }
}

#[test]
fn known_disconnection_blocks_output_even_after_refresh_or_unknown_observations() {
    let mut sync = LedOutputSync::default();
    let mut lamp = SimulatedLamp::new(LightState::Done);
    assert!(!sync.observe_connection(Some(false)));
    for state in [LightState::Working, LightState::Error, LightState::Off] {
        sync.invalidate();
        assert!(!sync.observe_connection(None));
        assert!(!sync.observe_connection(Some(false)));
        assert!(!dispatch(
            &mut sync,
            &mut lamp,
            &command(state),
            BASE,
            APPLIED_WITHOUT_ACK
        ));
    }
    assert!(lamp.attempts.is_empty());
    assert!(sync.observe_connection(Some(true)));
    assert!(dispatch(
        &mut sync,
        &mut lamp,
        &command(LightState::Off),
        BASE + 1000,
        APPLIED_WITHOUT_ACK
    ));
    assert_eq!(lamp.state, LightState::Off);
    assert_eq!(lamp.attempts.len(), 1);
}

#[test]
fn reconnect_applies_only_the_latest_target_once_with_or_without_prior_ack() {
    for ack in [Some(true), None] {
        let mut sync = LedOutputSync::default();
        let mut lamp = SimulatedLamp::new(LightState::Off);
        assert!(!sync.observe_connection(Some(true)));
        assert!(dispatch(
            &mut sync,
            &mut lamp,
            &command(LightState::Working),
            BASE,
            Transmission { applied: true, ack }
        ));
        assert!(!sync.observe_connection(Some(false)));
        lamp.power_cycle();
        for (i, state) in [LightState::Done, LightState::Waiting, LightState::Error]
            .into_iter()
            .enumerate()
        {
            assert!(!dispatch(
                &mut sync,
                &mut lamp,
                &command(state),
                BASE + 1000 + i as u64,
                APPLIED_WITHOUT_ACK
            ));
        }
        let desired = command(LightState::Error);
        let reconnected_at = BASE + 20_000;
        assert!(sync.observe_connection(Some(true)));
        assert!(dispatch(
            &mut sync,
            &mut lamp,
            &desired,
            reconnected_at,
            APPLIED_WITHOUT_ACK
        ));
        for elapsed in [1, 5000, 30_000, TEN_MINUTES] {
            assert!(!sync.observe_connection(Some(true)));
            assert!(!sync.observe_connection(None));
            assert!(!dispatch(
                &mut sync,
                &mut lamp,
                &desired,
                reconnected_at + elapsed,
                APPLIED_WITHOUT_ACK
            ));
        }
        assert_eq!(lamp.state, LightState::Error);
        assert_eq!(lamp.effect_started_ms, Some(reconnected_at));
        assert_eq!(
            lamp.attempts
                .iter()
                .map(|(_, cmd)| cmd.state)
                .collect::<Vec<_>>(),
            vec![LightState::Working, LightState::Error]
        );
    }
}

#[test]
fn reconnect_preempts_a_failure_cooldown_without_repeated_recovery_writes() {
    let desired = command(LightState::Working);
    let mut sync = LedOutputSync::default();
    let mut lamp = SimulatedLamp::new(LightState::Off);
    assert!(dispatch(&mut sync, &mut lamp, &desired, BASE, REJECTED));
    assert!(!sync.observe_connection(Some(false)));
    assert!(!sync.should_send(&desired, BASE + 1000));
    assert!(sync.observe_connection(Some(true)));
    assert!(dispatch(
        &mut sync,
        &mut lamp,
        &desired,
        BASE + 2000,
        APPLIED_WITHOUT_ACK
    ));
    assert!(!sync.observe_connection(Some(true)));
    assert!(!sync.should_send(&desired, BASE + 5000));
    assert_eq!(lamp.effect_started_ms, Some(BASE + 2000));
    assert_eq!(lamp.attempts.len(), 2);
}

#[test]
fn explicit_refresh_or_actual_resume_reapplies_the_same_effect_once_including_off() {
    for state in [LightState::Working, LightState::Error, LightState::Off] {
        for ack in [Some(true), None] {
            let desired = command(state);
            let mut sync = LedOutputSync::default();
            let mut lamp = SimulatedLamp::new(LightState::Done);
            assert!(dispatch(
                &mut sync,
                &mut lamp,
                &desired,
                BASE,
                Transmission { applied: true, ack }
            ));
            sync.invalidate();
            sync.invalidate();
            let refreshed_at = BASE + 1300;
            assert!(dispatch(
                &mut sync,
                &mut lamp,
                &desired,
                refreshed_at,
                APPLIED_WITHOUT_ACK
            ));
            assert!(!dispatch(
                &mut sync,
                &mut lamp,
                &desired,
                refreshed_at + TEN_MINUTES,
                APPLIED_WITHOUT_ACK
            ));
            assert_eq!(lamp.state, state);
            if matches!(state, LightState::Working | LightState::Error) {
                assert_eq!(lamp.effect_started_ms, Some(refreshed_at));
            }
            assert_eq!(lamp.attempts.len(), 2);
        }
    }
}

#[test]
fn recovery_does_not_extend_completion_or_replay_its_sound_episode() {
    let mut engine = Engine::default();
    engine
        .accept(snapshot(LightState::Done, "done", 1, 0), BASE, 300)
        .unwrap();
    let episode = engine.view(BASE, 90).sessions[0].episode_id.clone();
    let mut sync = LedOutputSync::default();
    let mut lamp = SimulatedLamp::new(LightState::Working);
    let mut sound = SoundEngine::new(BASE);
    let sound_config = SoundConfig::default();
    let mut sound_plays = Vec::new();

    for elapsed in (0..=310_000).step_by(1000) {
        let now = BASE + elapsed;
        if elapsed > 0 && elapsed % 15_000 == 0 {
            engine
                .accept(snapshot(LightState::Done, "done", 1, elapsed), now, 300)
                .unwrap();
        }
        if elapsed % 30_000 == 0 {
            sound.battery.observe(70, now);
        }
        if matches!(elapsed, 60_000 | 180_000 | 299_000) {
            sync.invalidate();
        }
        if elapsed == 120_000 {
            assert!(!sync.observe_connection(Some(false)));
            lamp.power_cycle();
        }
        if elapsed == 125_000 {
            assert!(sync.observe_connection(Some(true)));
        }

        let aggregate = engine.view(now, 90);
        assert_eq!(aggregate.sessions[0].episode_id, episode);
        assert_eq!(aggregate.sessions[0].entered_ms, BASE);
        assert_eq!(
            aggregate.state,
            if elapsed < 300_000 {
                LightState::Done
            } else {
                LightState::Off
            }
        );
        assert_eq!(
            aggregate.remaining_ms,
            if elapsed < 300_000 {
                Some(300_000 - elapsed)
            } else {
                None
            }
        );

        // Fail one second before the deadline: Off must preempt the green retry
        // that would otherwise be due four seconds after the deadline.
        let result = if elapsed == 299_000 {
            REJECTED
        } else {
            APPLIED_WITHOUT_ACK
        };
        dispatch(&mut sync, &mut lamp, &command(aggregate.state), now, result);
        let connected = !(120_000..125_000).contains(&elapsed);
        if let Some(action) = sound.tick(now, &aggregate.sessions, &sound_config, connected) {
            if let Action::Play(playback) = &action {
                sound_plays.push((elapsed, playback.episode.clone()));
            }
            sound.complete(&action, now, None);
        }
    }

    assert_eq!(
        lamp.attempts
            .iter()
            .map(|(at, cmd)| (at - BASE, cmd.state))
            .collect::<Vec<_>>(),
        vec![
            (0, LightState::Done),
            (60_000, LightState::Done),
            (125_000, LightState::Done),
            (180_000, LightState::Done),
            (299_000, LightState::Done),
            (300_000, LightState::Off),
        ]
    );
    assert_eq!(lamp.state, LightState::Off);
    assert_eq!(
        sound_plays,
        vec![
            (0, episode.clone()),
            (10_000, episode.clone()),
            (20_000, episode)
        ]
    );
}

#[test]
fn completion_expiring_while_disconnected_reconnects_directly_to_off() {
    let mut engine = Engine::default();
    engine
        .accept(snapshot(LightState::Done, "done", 1, 0), BASE, 300)
        .unwrap();
    let mut sync = LedOutputSync::default();
    let mut lamp = SimulatedLamp::new(LightState::Off);
    assert!(dispatch(
        &mut sync,
        &mut lamp,
        &command(engine.view(BASE, 90).state),
        BASE,
        APPLIED_WITHOUT_ACK
    ));
    assert!(!sync.observe_connection(Some(false)));
    let now = BASE + 310_000;
    let aggregate = engine.view(now, 90);
    assert_eq!(aggregate.state, LightState::Off);
    assert!(!dispatch(
        &mut sync,
        &mut lamp,
        &command(aggregate.state),
        now,
        APPLIED_WITHOUT_ACK
    ));
    assert!(sync.observe_connection(Some(true)));
    assert!(dispatch(
        &mut sync,
        &mut lamp,
        &command(aggregate.state),
        now,
        APPLIED_WITHOUT_ACK
    ));
    assert_eq!(lamp.state, LightState::Off);
    assert_eq!(
        lamp.attempts
            .iter()
            .map(|(_, cmd)| cmd.state)
            .collect::<Vec<_>>(),
        vec![LightState::Done, LightState::Off]
    );
}
