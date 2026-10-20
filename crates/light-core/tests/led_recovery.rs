//! Fault-injection regressions for the physical lamp retaining an old color.
//! Synthetic time only: no Bluetooth, network, wall-clock waits, or user state.
use light_core::{
    led_output::{LedCommand, LedDelivery, LedOutputSync},
    model::{Engine, LightState, Snapshot, WireSession},
    sound::{Action, SoundConfig, SoundEngine},
};

const BASE: u64 = 1_800_000_000_000;

fn command(state: LightState) -> LedCommand {
    LedCommand {
        state,
        period_ms: 3000,
        fade_ms: 1500,
        serial: "001122334455".into(),
    }
}

struct SimulatedLamp {
    state: LightState,
    drop_next_write: bool,
    writes: usize,
}

impl SimulatedLamp {
    fn write(&mut self, desired: &LedCommand) -> LedDelivery {
        self.writes += 1;
        if std::mem::take(&mut self.drop_next_write) {
            // The OS accepted the HID write, but the lamp received nothing.
            return LedDelivery::Unconfirmed;
        }
        self.state = desired.state;
        LedDelivery::Confirmed
    }
}

#[test]
fn missed_working_or_off_write_recovers_without_another_task_event() {
    for next_state in [LightState::Working, LightState::Off] {
        let mut sync = LedOutputSync::default();
        let mut lamp = SimulatedLamp {
            state: LightState::Done,
            drop_next_write: true,
            writes: 0,
        };
        sync.record_attempt(command(LightState::Done), BASE, LedDelivery::Confirmed);
        let desired = command(next_state);
        let changed_at = BASE + 1000;

        assert!(sync.should_send(&desired, changed_at));
        sync.record_attempt(desired.clone(), changed_at, lamp.write(&desired));
        assert_eq!(lamp.state, LightState::Done);

        // Keep precisely the same desired output. No pause, new snapshot,
        // reconnect notification, or user interaction helps the retry along.
        for elapsed in (100..=5000).step_by(100) {
            let now = changed_at + elapsed;
            if sync.should_send(&desired, now) {
                sync.record_attempt(desired.clone(), now, lamp.write(&desired));
            }
            if elapsed < 5000 {
                assert_eq!(lamp.state, LightState::Done);
            }
        }
        assert_eq!(lamp.state, next_state);
        assert_eq!(lamp.writes, 2);
    }
}

fn assert_bounded_retry(delivery: impl Fn() -> LedDelivery) {
    let mut sync = LedOutputSync::default();
    let desired = command(LightState::Off);
    let mut now = BASE;
    assert!(sync.should_send(&desired, now));
    for delay_ms in [5000, 10_000, 20_000, 30_000, 30_000, 30_000] {
        sync.record_attempt(desired.clone(), now, delivery());
        assert!(!sync.should_send(&desired, now + delay_ms - 1));
        assert!(sync.should_send(&desired, now + delay_ms));
        now += delay_ms;
    }
}

#[test]
fn missing_ack_retries_with_bounded_backoff() {
    assert_bounded_retry(|| LedDelivery::Unconfirmed);
}

#[test]
fn failed_writes_retry_with_bounded_backoff() {
    assert_bounded_retry(|| LedDelivery::Failed);
}

#[test]
fn changed_state_effect_or_device_bypasses_old_retry_and_starts_fresh() {
    let old = command(LightState::Done);
    let new_commands = [
        LedCommand {
            state: LightState::Off,
            ..old.clone()
        },
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
    for desired in new_commands {
        let mut sync = LedOutputSync::default();
        sync.record_attempt(old.clone(), BASE, LedDelivery::Unconfirmed);
        sync.record_attempt(old.clone(), BASE + 5000, LedDelivery::Failed);
        sync.record_attempt(old.clone(), BASE + 15_000, LedDelivery::Unconfirmed);
        let changed_at = BASE + 15_001;
        assert!(!sync.should_send(&old, changed_at));
        assert!(sync.should_send(&desired, changed_at));

        sync.record_attempt(desired.clone(), changed_at, LedDelivery::Unconfirmed);
        assert!(!sync.should_send(&desired, changed_at + 4999));
        assert!(sync.should_send(&desired, changed_at + 5000));
    }
}

#[test]
fn refresh_or_wake_invalidation_resends_even_cached_off_immediately() {
    for delivery in [
        LedDelivery::Confirmed,
        LedDelivery::Unconfirmed,
        LedDelivery::Failed,
    ] {
        let mut sync = LedOutputSync::default();
        let desired = command(LightState::Off);
        sync.record_attempt(desired.clone(), BASE, delivery);
        assert!(!sync.should_send(&desired, BASE + 1));

        sync.invalidate();
        sync.invalidate();
        assert!(sync.should_send(&desired, BASE + 1));
        sync.record_attempt(desired.clone(), BASE + 1, LedDelivery::Confirmed);
        assert!(!sync.should_send(&desired, BASE + 2));
    }
}

#[test]
fn confirmed_output_is_reapplied_after_thirty_seconds_if_lamp_drifts() {
    let mut sync = LedOutputSync::default();
    let desired = command(LightState::Working);
    let mut lamp = SimulatedLamp {
        state: LightState::Done,
        drop_next_write: false,
        writes: 0,
    };
    sync.record_attempt(desired.clone(), BASE, lamp.write(&desired));
    assert_eq!(lamp.state, LightState::Working);

    // A lamp-side reset or another controller can invalidate an earlier ACK.
    lamp.state = LightState::Done;
    assert!(!sync.should_send(&desired, BASE + 29_999));
    assert!(sync.should_send(&desired, BASE + 30_000));
    sync.record_attempt(desired.clone(), BASE + 30_000, lamp.write(&desired));
    assert_eq!(lamp.state, LightState::Working);
    assert_eq!(lamp.writes, 2);
    assert!(!sync.should_send(&desired, BASE + 59_999));
    assert!(sync.should_send(&desired, BASE + 60_000));
}

#[test]
fn repeated_probe_failures_do_not_postpone_an_unconfirmed_or_failed_retry() {
    for delivery in [LedDelivery::Unconfirmed, LedDelivery::Failed] {
        let mut sync = LedOutputSync::default();
        let desired = command(LightState::Off);
        sync.record_attempt(desired.clone(), BASE, delivery);
        for elapsed in [1000, 2000, 4999, 5000, 8000] {
            sync.transport_failed(BASE + elapsed);
            assert_eq!(sync.should_send(&desired, BASE + elapsed), elapsed >= 5000);
        }
    }
}

#[test]
fn first_probe_failure_invalidates_confirmation_without_sliding_the_deadline() {
    let mut sync = LedOutputSync::default();
    let desired = command(LightState::Working);
    sync.record_attempt(desired.clone(), BASE, LedDelivery::Confirmed);
    sync.transport_failed(BASE + 1000);
    for elapsed in [2000, 5999, 6000, 9000] {
        sync.transport_failed(BASE + elapsed);
        assert_eq!(sync.should_send(&desired, BASE + elapsed), elapsed >= 6000);
    }
}

#[test]
fn probe_failure_keeps_an_already_nearer_refresh_deadline() {
    let mut sync = LedOutputSync::default();
    let desired = command(LightState::Off);
    sync.record_attempt(desired.clone(), BASE, LedDelivery::Confirmed);
    sync.transport_failed(BASE + 28_000);
    assert!(!sync.should_send(&desired, BASE + 29_999));
    sync.transport_failed(BASE + 29_999);
    assert!(sync.should_send(&desired, BASE + 30_000));
}

#[test]
fn a_confirmation_resets_backoff_for_the_next_unconfirmed_write() {
    let mut sync = LedOutputSync::default();
    let desired = command(LightState::Working);
    sync.record_attempt(desired.clone(), BASE, LedDelivery::Failed);
    sync.record_attempt(desired.clone(), BASE + 5000, LedDelivery::Unconfirmed);
    sync.record_attempt(desired.clone(), BASE + 15_000, LedDelivery::Confirmed);
    assert!(!sync.should_send(&desired, BASE + 44_999));
    assert!(sync.should_send(&desired, BASE + 45_000));
    sync.record_attempt(desired.clone(), BASE + 45_000, LedDelivery::Unconfirmed);
    assert!(!sync.should_send(&desired, BASE + 49_999));
    assert!(sync.should_send(&desired, BASE + 50_000));
}

fn completion_snapshot(age_ms: u64) -> Snapshot {
    Snapshot {
        schema: 1,
        source_id: "fixture-vm".into(),
        generation: "fixture-generation".into(),
        revision: 1,
        sessions: vec![WireSession {
            id: "fixture-session".into(),
            turn: "fixture-turn".into(),
            version: 1,
            state: LightState::Done,
            age_ms,
            event: "Stop".into(),
        }],
    }
}

#[test]
fn led_retries_do_not_extend_completion_or_replay_its_sound_episode() {
    let mut engine = Engine::default();
    engine.accept(completion_snapshot(0), BASE, 300).unwrap();
    let episode = engine.view(BASE, 90).sessions[0].episode_id.clone();
    let mut sync = LedOutputSync::default();
    let mut sound = SoundEngine::new(BASE);
    let sound_config = SoundConfig::default();
    let mut led_attempts = Vec::new();
    let mut sound_plays = Vec::new();

    for elapsed in (0..=310_000).step_by(1000) {
        let now = BASE + elapsed;
        // Keep the source and battery healthy so an accidental sound replay
        // cannot pass unnoticed merely because an unrelated safety gate closed.
        if elapsed > 0 && elapsed % 15_000 == 0 {
            engine
                .accept(completion_snapshot(elapsed), now, 300)
                .unwrap();
        }
        if elapsed % 30_000 == 0 {
            sound.battery.observe(70, now);
        }
        let aggregate = engine.view(now, 90);
        assert_eq!(aggregate.sessions[0].episode_id, episode);
        assert_eq!(aggregate.sessions[0].entered_ms, BASE);
        let expected_state = if elapsed < 300_000 {
            LightState::Done
        } else {
            LightState::Off
        };
        assert_eq!(aggregate.state, expected_state);
        assert_eq!(
            aggregate.remaining_ms,
            if elapsed < 300_000 {
                Some(300_000 - elapsed)
            } else {
                None
            }
        );

        let desired = command(aggregate.state);
        if sync.should_send(&desired, now) {
            led_attempts.push((elapsed, desired.state));
            // Some firmware never acknowledges writes. LED convergence must
            // remain independent of task-event and buzzer scheduling.
            sync.record_attempt(desired, now, LedDelivery::Unconfirmed);
        }
        if let Some(action) = sound.tick(now, &aggregate.sessions, &sound_config, true) {
            if let Action::Play(playback) = &action {
                sound_plays.push((elapsed, playback.episode.clone()));
            }
            sound.complete(&action, now, None);
        }
    }

    assert!(
        led_attempts
            .iter()
            .filter(|(_, state)| *state == LightState::Done)
            .count()
            > 10
    );
    // The new Off target must preempt a still-pending retry of the old green.
    assert!(led_attempts.contains(&(300_000, LightState::Off)));
    assert!(led_attempts
        .iter()
        .all(|(at, state)| *at < 300_000 || *state == LightState::Off));
    assert_eq!(
        sound_plays,
        vec![
            (0, episode.clone()),
            (10_000, episode.clone()),
            (20_000, episode)
        ]
    );
}
