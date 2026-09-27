use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum LightState { #[default] Off, Done, Working, Waiting, Error }
impl LightState {
    pub fn label(self) -> &'static str {
        match self { Self::Off => "空闲", Self::Done => "任务完成", Self::Working => "工作中", Self::Waiting => "等待你处理", Self::Error => "执行异常" }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireSession {
    pub id: String,
    pub turn: String,
    pub version: u64,
    pub state: LightState,
    pub age_ms: u64,
    pub event: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub schema: u8,
    pub source_id: String,
    pub generation: String,
    pub revision: u64,
    pub sessions: Vec<WireSession>,
}
impl Snapshot {
    pub fn validate(&self) -> Result<()> {
        ensure!(self.schema == 1, "unsupported snapshot schema");
        ensure!(valid_id(&self.source_id,128) && valid_id(&self.generation,64), "invalid source identity");
        ensure!(self.sessions.len() <= 128, "too many sessions (max 128)");
        let mut seen = BTreeSet::new();
        for s in &self.sessions {
            ensure!(valid_id(&s.id,128) && s.turn.len() <= 128 && !s.turn.chars().any(char::is_control), "invalid session identity");
            ensure!(s.version <= self.revision, "session version exceeds snapshot revision");
            ensure!(seen.insert(&s.id), "duplicate session id");
            ensure!(s.event.len() <= 64 && !s.event.chars().any(char::is_control), "invalid event label");
        }
        Ok(())
    }
}
fn valid_id(s: &str, max: usize) -> bool { !s.is_empty() && s.len() <= max && !s.chars().any(char::is_control) }

#[derive(Debug, Clone)]
struct RemoteSession {
    wire: WireSession,
    expires_ms: Option<u64>,
    accepted_ms: u64,
    episode_version: u64,
    entered_ms: u64,
}
#[derive(Debug, Clone)]
struct RemoteSource {
    generation: String,
    revision: u64,
    seen_ms: u64,
    sessions: BTreeMap<String, RemoteSession>,
}
#[derive(Debug, Clone, Serialize)]
pub struct SessionView {
    pub source: String, pub id: String, pub turn: String,
    pub state: LightState, pub event: String,
    pub episode_id: String, pub entered_ms: u64,
    pub remaining_ms: Option<u64>, pub online: bool,
}
#[derive(Debug, Clone, Serialize)]
pub struct SourceView { pub id: String, pub online: bool, pub last_seen_ms: u64, pub sessions: usize }
#[derive(Debug, Clone, Serialize)]
pub struct Aggregate {
    pub state: LightState,
    pub remaining_ms: Option<u64>,
    pub sessions: Vec<SessionView>,
    pub sources: Vec<SourceView>,
}
#[derive(Debug, Default)]
pub struct Engine { sources: BTreeMap<String, RemoteSource> }
impl Engine {
    /// Full latest-state sync, not event replay. Equal revisions are heartbeat only.
    /// Returns true when a new waiting/error event should cancel a local preview.
    pub fn accept(&mut self, snapshot: Snapshot, now: u64, done_seconds: u32) -> Result<bool> {
        snapshot.validate()?;
        ensure!((1..=3600).contains(&done_seconds), "invalid completion timer");
        ensure!(self.sources.contains_key(&snapshot.source_id) || self.sources.len() < 32, "source limit reached");
        let source = self.sources.entry(snapshot.source_id).or_insert_with(|| RemoteSource {
            generation: snapshot.generation.clone(), revision: 0, seen_ms: now, sessions: BTreeMap::new(),
        });
        if source.generation != snapshot.generation {
            source.generation = snapshot.generation;
            source.revision = 0;
            source.sessions.clear();
        }
        if snapshot.revision < source.revision { return Ok(false); }
        source.seen_ms = now;
        if snapshot.revision == source.revision && !source.sessions.is_empty() { return Ok(false); }
        let mut important = false;
        let mut next = BTreeMap::new();
        for wire in snapshot.sessions {
            let old = source.sessions.remove(&wire.id);
            if old.as_ref().is_some_and(|s| s.wire.version >= wire.version) {
                next.insert(wire.id.clone(), old.unwrap());
                continue;
            }
            let continuing = old.as_ref().filter(|s| s.wire.turn == wire.turn && s.wire.state == wire.state);
            let entered_ms = continuing.map_or_else(||now.saturating_sub(wire.age_ms),|s|s.entered_ms);
            let episode_version = continuing.map_or(wire.version,|s|s.episode_version);
            important |= continuing.is_none() && matches!(wire.state, LightState::Waiting | LightState::Error);
            let expires_ms = if wire.state == LightState::Done {
                // A transport refresh must not restart the same completion timer.
                continuing.and_then(|s|s.expires_ms).or_else(||Some(now.saturating_add((done_seconds as u64 * 1000).saturating_sub(wire.age_ms))))
            } else { None };
            next.insert(wire.id.clone(), RemoteSession { wire, expires_ms, accepted_ms: now, episode_version, entered_ms });
        }
        source.sessions = next;
        source.revision = snapshot.revision;
        Ok(important)
    }
    pub fn view(&self, now: u64, source_timeout_seconds: u32) -> Aggregate {
        let mut result = Aggregate { state: LightState::Off, remaining_ms: None, sessions: Vec::new(), sources: Vec::new() };
        let mut newest = 0;
        for (source_id, source) in &self.sources {
            let online = now.saturating_sub(source.seen_ms) <= source_timeout_seconds as u64 * 1000;
            result.sources.push(SourceView { id: source_id.clone(), online, last_seen_ms: source.seen_ms, sessions: source.sessions.len() });
            for session in source.sessions.values() {
                let expired = session.expires_ms.is_some_and(|deadline| now >= deadline);
                // A received completion has a local deadline independent of relay liveness.
                // Preserve the real online flag: offline sources must not re-enable sound.
                let retain_completion = session.wire.state == LightState::Done;
                let state = if !expired && (online || retain_completion) { session.wire.state } else { LightState::Off };
                let remaining = session.expires_ms.map(|t| t.saturating_sub(now));
                if state > result.state || (state == result.state && session.accepted_ms >= newest) {
                    result.state = state;
                    result.remaining_ms = if state == LightState::Done { remaining } else { None };
                    newest = session.accepted_ms;
                }
                result.sessions.push(SessionView {
                    source: source_id.clone(), id: session.wire.id.clone(), turn: session.wire.turn.clone(),
                    episode_id: serde_json::to_string(&(source_id,&source.generation,&session.wire.id,&session.wire.turn,session.episode_version)).expect("string tuple serialization"),
                    entered_ms:session.entered_ms,
                    state, event: session.wire.event.clone(), remaining_ms: remaining, online,
                });
            }
        }
        result
    }
    /// Adjust pending completion deadlines from the original completion time.
    /// Changing settings must not revive expired reminders or reset sound episodes.
    pub fn reconfigure_done_timer(&mut self, now: u64, done_seconds: u32) -> Result<()> {
        ensure!((1..=3600).contains(&done_seconds), "invalid completion timer");
        let duration_ms = u64::from(done_seconds) * 1000;
        for source in self.sources.values_mut() {
            for session in source.sessions.values_mut() {
                if session.wire.state == LightState::Done
                    && session.expires_ms.is_some_and(|deadline| now < deadline)
                {
                    session.expires_ms = Some(session.entered_ms.saturating_add(duration_ms));
                }
            }
        }
        Ok(())
    }
    pub fn forget_source(&mut self, id: &str) { self.sources.remove(id); }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn snap(rev:u64, state:LightState, age:u64) -> Snapshot {
        Snapshot { schema:1, source_id:"vm".into(), generation:"g1".into(), revision:rev,
            sessions:vec![WireSession { id:"s1".into(), turn:"t1".into(), version:rev, state,age_ms:age,event:"Test".into() }] }
    }
    #[test] fn done_expires_at_five_minutes() {
        let mut e=Engine::default(); e.accept(snap(1,LightState::Done,0),1000,300).unwrap();
        assert_eq!(e.view(300_999,90).state,LightState::Done);
        assert_eq!(e.view(301_000,90).state,LightState::Off);
    }
    #[test] fn duplicate_does_not_extend_done() {
        let mut e=Engine::default(); e.accept(snap(1,LightState::Done,0),0,300).unwrap();
        e.accept(snap(1,LightState::Done,0),299_000,300).unwrap();
        assert_eq!(e.view(300_000,90).state,LightState::Off);
    }
    #[test] fn delayed_done_is_not_restarted() {
        let mut e=Engine::default(); e.accept(snap(1,LightState::Done,310_000),1000,300).unwrap();
        assert_eq!(e.view(1000,90).state,LightState::Off);
    }
    #[test] fn old_timer_cannot_stop_new_work() {
        let mut e=Engine::default(); e.accept(snap(1,LightState::Done,0),0,300).unwrap();
        e.accept(snap(2,LightState::Working,0),290_000,300).unwrap();
        assert_eq!(e.view(310_000,90).state,LightState::Working);
    }
    #[test] fn stale_revision_ignored() {
        let mut e=Engine::default(); e.accept(snap(2,LightState::Working,0),0,300).unwrap();
        e.accept(snap(1,LightState::Done,0),1,300).unwrap();
        assert_eq!(e.view(1,90).state,LightState::Working);
    }
    #[test] fn offline_is_not_an_error() {
        let mut e=Engine::default(); e.accept(snap(1,LightState::Working,0),0,300).unwrap();
        assert_eq!(e.view(90_001,90).state,LightState::Off);
    }
    #[test] fn heartbeat_restores_current_work() {
        let mut e=Engine::default(); e.accept(snap(1,LightState::Working,0),0,300).unwrap();
        e.accept(snap(1,LightState::Working,100_000),100_000,300).unwrap();
        assert_eq!(e.view(100_001,90).state,LightState::Working);
    }
    #[test] fn priorities_match_product_rules() {
        assert!(LightState::Error > LightState::Waiting);
        assert!(LightState::Waiting > LightState::Working);
        assert!(LightState::Working > LightState::Done);
    }
    #[test] fn unrelated_revision_preserves_deadline() {
        let mut e=Engine::default(); e.accept(snap(1,LightState::Done,0),0,300).unwrap();
        let mut s=snap(2,LightState::Done,0); s.sessions[0].version=1;
        s.sessions.push(WireSession{id:"s2".into(),turn:"t2".into(),version:2,state:LightState::Off,age_ms:0,event:"SessionEnd".into()});
        e.accept(s,290_000,600).unwrap();
        assert_eq!(e.view(300_000,90).state,LightState::Off);
    }
    #[test] fn new_generation_accepted() {
        let mut e=Engine::default(); e.accept(snap(20,LightState::Working,0),0,300).unwrap();
        let mut s=snap(1,LightState::Waiting,0); s.generation="g2".into();
        e.accept(s,1,300).unwrap(); assert_eq!(e.view(1,90).state,LightState::Waiting);
    }
    #[test] fn full_snapshot_removes_closed_sessions() {
        let mut e=Engine::default(); e.accept(snap(1,LightState::Error,0),0,300).unwrap();
        let mut s=snap(2,LightState::Off,0); s.sessions.clear(); e.accept(s,1,300).unwrap();
        assert_eq!(e.view(1,90).state,LightState::Off);
    }
    #[test] fn duplicate_session_invalid() {
        let mut s=snap(1,LightState::Done,0); s.sessions.push(s.sessions[0].clone()); assert!(s.validate().is_err());
    }
}
#[cfg(test)] mod sound_episode_tests {
    use super::*;
    fn snap(rev:u64,turn:&str,state:LightState)->Snapshot {Snapshot{schema:1,source_id:"vm".into(),generation:"g".into(),revision:rev,
        sessions:vec![WireSession{id:"s".into(),turn:turn.into(),version:rev,state,age_ms:0,event:"hook".into()}]}}
    #[test] fn same_state_new_version_preserves_episode(){let mut e=Engine::default();e.accept(snap(1,"t",LightState::Waiting),0,300).unwrap();let a=e.view(0,90).sessions[0].episode_id.clone();assert!(!e.accept(snap(2,"t",LightState::Waiting),1000,300).unwrap());assert_eq!(a,e.view(1000,90).sessions[0].episode_id);assert_eq!(e.view(1000,90).sessions[0].entered_ms,0);}
    #[test] fn new_turn_changes_episode(){let mut e=Engine::default();e.accept(snap(1,"t",LightState::Done),0,300).unwrap();let a=e.view(0,90).sessions[0].episode_id.clone();e.accept(snap(2,"u",LightState::Done),1000,300).unwrap();assert_ne!(a,e.view(1000,90).sessions[0].episode_id);}
    #[test] fn repeated_done_does_not_extend_timer(){let mut e=Engine::default();e.accept(snap(1,"t",LightState::Done),0,300).unwrap();e.accept(snap(2,"t",LightState::Done),290000,300).unwrap();assert_eq!(e.view(300000,90).state,LightState::Off);}
    #[test] fn recover_then_fail_new_episode(){let mut e=Engine::default();e.accept(snap(1,"t",LightState::Error),0,300).unwrap();let a=e.view(0,90).sessions[0].episode_id.clone();e.accept(snap(2,"t",LightState::Working),1000,300).unwrap();e.accept(snap(3,"t",LightState::Error),2000,300).unwrap();assert_ne!(a,e.view(2000,90).sessions[0].episode_id);}
}
