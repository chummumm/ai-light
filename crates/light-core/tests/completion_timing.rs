//! Countdown regressions use synthetic time and the shipped 90-second timeout.
//! No network, Bluetooth, wall-clock sleeps, or user task state is accessed.
use light_core::model::{Engine, LightState, Snapshot, WireSession};

const BASE: u64 = 1_800_000_000_000;
const SOURCE_TIMEOUT: u32 = 90;
fn snap(source: &str, revision: u64, turn: &str, state: LightState, age: u64) -> Snapshot {
    Snapshot { schema: 1, source_id: source.into(), generation: "fixture-generation".into(),
        revision, sessions: vec![WireSession { id: "fixture-session".into(), turn: turn.into(),
        version: revision, state, age_ms: age, event: "Fixture".into() }] }
}
fn at(e: &Engine, elapsed: u64) -> LightState { e.view(BASE + elapsed, SOURCE_TIMEOUT).state }

#[test]
fn completion_survives_source_timeout_until_five_minutes() {
    let mut e=Engine::default();
    e.accept(snap("vm",1,"t",LightState::Done,0),BASE,300).unwrap();
    let v=e.view(BASE+90_001,SOURCE_TIMEOUT);
    assert_eq!(v.state,LightState::Done);
    assert_eq!(v.remaining_ms,Some(209_999));
    assert!(!v.sources[0].online);
    assert!(!v.sessions[0].online);
    assert_eq!(at(&e,299_999),LightState::Done);
    assert_eq!(at(&e,300_000),LightState::Off);
}
#[test]
fn unfinished_states_still_expire_when_source_disconnects() {
    for state in [LightState::Working,LightState::Waiting,LightState::Error] {
        let mut e=Engine::default();
        e.accept(snap("vm",1,"t",state,0),BASE,300).unwrap();
        assert_eq!(at(&e,90_000),state);
        assert_eq!(at(&e,90_001),LightState::Off);
    }
}
#[test]
fn duplicate_heartbeats_do_not_extend_completion() {
    let mut e=Engine::default();
    e.accept(snap("vm",1,"t",LightState::Done,0),BASE,300).unwrap();
    for elapsed in (15_000..=285_000).step_by(15_000) {
        e.accept(snap("vm",1,"t",LightState::Done,elapsed),BASE+elapsed,300).unwrap();
    }
    assert_eq!(at(&e,299_999),LightState::Done);
    assert_eq!(at(&e,300_000),LightState::Off);
}
#[test]
fn duplicate_new_revision_does_not_restart_timer() {
    let mut e=Engine::default();
    e.accept(snap("vm",1,"t",LightState::Done,0),BASE,300).unwrap();
    e.accept(snap("vm",2,"t",LightState::Done,0),BASE+290_000,300).unwrap();
    assert_eq!(at(&e,300_000),LightState::Off);
}
#[test]
fn late_completion_keeps_original_age_instead_of_replaying() {
    let mut e=Engine::default();
    e.accept(snap("vm",1,"t",LightState::Done,240_000),BASE,300).unwrap();
    assert_eq!(at(&e,59_999),LightState::Done);
    assert_eq!(at(&e,60_000),LightState::Off);
}
#[test]
fn already_expired_completion_never_lights() {
    let mut e=Engine::default();
    e.accept(snap("vm",1,"t",LightState::Done,310_000),BASE,300).unwrap();
    assert_eq!(at(&e,0),LightState::Off);
    e.reconfigure_done_timer(BASE+1,600).unwrap();
    assert_eq!(at(&e,1),LightState::Off);
}
#[test]
fn lengthening_live_timer_uses_original_completion_time() {
    let mut e=Engine::default();
    e.accept(snap("vm",1,"t",LightState::Done,0),BASE,60).unwrap();
    e.reconfigure_done_timer(BASE+30_000,300).unwrap();
    assert_eq!(e.view(BASE+30_000,SOURCE_TIMEOUT).remaining_ms,Some(270_000));
    assert_eq!(at(&e,299_999),LightState::Done);
    assert_eq!(at(&e,300_000),LightState::Off);
}
#[test]
fn shortening_timer_does_not_start_a_new_duration() {
    let mut e=Engine::default();
    e.accept(snap("vm",1,"t",LightState::Done,0),BASE,300).unwrap();
    e.reconfigure_done_timer(BASE+90_000,60).unwrap();
    assert_eq!(at(&e,90_000),LightState::Off);
}
#[test]
fn editing_after_expiration_does_not_replay() {
    let mut e=Engine::default();
    e.accept(snap("vm",1,"t",LightState::Done,0),BASE,60).unwrap();
    e.reconfigure_done_timer(BASE+60_000,300).unwrap();
    assert_eq!(at(&e,60_000),LightState::Off);
}
#[test]
fn invalid_duration_is_rejected_without_mutating_timer() {
    let mut e=Engine::default();
    e.accept(snap("vm",1,"t",LightState::Done,0),BASE,300).unwrap();
    for duration in [0,3601] { assert!(e.reconfigure_done_timer(BASE+30_000,duration).is_err()); }
    assert_eq!(at(&e,299_999),LightState::Done);
    assert_eq!(at(&e,300_000),LightState::Off);
}
#[test]
fn new_work_replaces_done_and_ignores_old_deadline() {
    let mut e=Engine::default();
    e.accept(snap("vm",1,"t",LightState::Done,0),BASE,300).unwrap();
    e.accept(snap("vm",2,"next",LightState::Working,0),BASE+290_000,300).unwrap();
    assert_eq!(at(&e,310_000),LightState::Working);
}
#[test]
fn explicit_snapshot_removal_still_cancels_completion() {
    let mut e=Engine::default();
    e.accept(snap("vm",1,"t",LightState::Done,0),BASE,300).unwrap();
    let mut empty=snap("vm",2,"t",LightState::Off,0);empty.sessions.clear();
    e.accept(empty,BASE+10_000,300).unwrap();
    assert_eq!(at(&e,10_000),LightState::Off);
}
#[test]
fn explicit_forget_still_cancels_completion() {
    let mut e=Engine::default();
    e.accept(snap("vm",1,"t",LightState::Done,0),BASE,300).unwrap();
    e.forget_source("vm");
    assert_eq!(at(&e,1),LightState::Off);
}
#[test]
fn online_work_on_other_source_keeps_priority() {
    let mut e=Engine::default();
    e.accept(snap("vm-a",1,"t",LightState::Done,0),BASE,300).unwrap();
    e.accept(snap("vm-b",1,"u",LightState::Working,0),BASE+100_000,300).unwrap();
    assert_eq!(at(&e,100_001),LightState::Working);
    assert_eq!(at(&e,190_001),LightState::Done);
    assert_eq!(at(&e,300_000),LightState::Off);
}
#[test]
fn config_edit_never_changes_episode_or_online_flag() {
    let mut e=Engine::default();
    e.accept(snap("vm",1,"t",LightState::Done,0),BASE,300).unwrap();
    let before=e.view(BASE+100_000,SOURCE_TIMEOUT).sessions[0].clone();
    e.reconfigure_done_timer(BASE+100_000,600).unwrap();
    let after=e.view(BASE+100_000,SOURCE_TIMEOUT).sessions[0].clone();
    assert_eq!(before.episode_id,after.episode_id);
    assert_eq!(before.entered_ms,after.entered_ms);
    assert_eq!(before.online,after.online);
    assert_eq!(after.remaining_ms,Some(500_000));
}
#[test]
fn heartbeat_after_offline_does_not_restart_timer() {
    let mut e=Engine::default();
    e.accept(snap("vm",1,"t",LightState::Done,0),BASE,300).unwrap();
    assert_eq!(at(&e,100_000),LightState::Done);
    e.accept(snap("vm",1,"t",LightState::Done,200_000),BASE+200_000,300).unwrap();
    let v=e.view(BASE+200_001,SOURCE_TIMEOUT);
    assert!(v.sources[0].online);
    assert_eq!(v.remaining_ms,Some(99_999));
    assert_eq!(at(&e,300_000),LightState::Off);
}
#[test]
fn cached_green_does_not_enable_offline_sound() {
    use light_core::sound::{SoundConfig, SoundEngine, top_key, Action};
    let mut e=Engine::default();
    e.accept(snap("vm",1,"t",LightState::Done,0),BASE,300).unwrap();
    let view=e.view(BASE+90_001,SOURCE_TIMEOUT);
    let cfg=SoundConfig::default();
    assert_eq!(view.state,LightState::Done);
    assert!(top_key(&view.sessions,&cfg).is_none());
    let mut sound=SoundEngine::new(BASE);
    sound.battery.observe(70,BASE+90_001);
    assert!(!matches!(sound.tick(BASE+90_001,&view.sessions,&cfg,true),Some(Action::Play(_))));
}
#[test]
fn shortening_then_lengthening_does_not_revive_expired_timer() {
    let mut e=Engine::default();
    e.accept(snap("vm",1,"t",LightState::Done,0),BASE,300).unwrap();
    e.reconfigure_done_timer(BASE+100_000,60).unwrap();
    e.reconfigure_done_timer(BASE+100_001,600).unwrap();
    assert_eq!(at(&e,100_001),LightState::Off);
}
