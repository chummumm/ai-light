//! Unverifiable pre-observer records are UNKNOWN, not successful or still-live.
//! Only a migration projection changes; callbacks and journals can restore it.
use light_core::{hooks::{LocalSession,LocalState},model::LightState};
pub fn candidate(s:&LocalSession,has_tracking:bool)->bool {
    s.id!="manual-test" && s.state==LightState::Working && s.event=="UserPromptSubmit"
        && s.run_id.is_empty() && !has_tracking
}
pub fn set(s:&mut LocalState,expected:&LocalSession,unknown:bool)->bool {
    let Some(cur)=s.sessions.get_mut(&expected.id)else{return false;};
    if cur.version!=expected.version||cur.turn!=expected.turn||cur.unverified_legacy==unknown{return false;}
    if unknown && !candidate(cur,false){return false;}
    cur.unverified_legacy=unknown;
    s.revision=s.revision.saturating_add(1);cur.version=s.revision;
    // Do not change changed_ms, touched_ms, event, state or turn. No completion.
    true
}
#[cfg(test)]mod tests{
    use super::*;use serde_json::json;use light_core::model::Engine;
    fn start(id:&str)->LocalState{let mut s=LocalState::default();s.apply(&json!({"session_id":id,"hook_event_name":"UserPromptSubmit","turn_id":"t"}),100,true,"").unwrap();s}
    #[test]fn legacy_only(){let s=start("old");assert!(candidate(&s.sessions["old"],false));assert!(!candidate(&s.sessions["old"],true));}
    #[test]fn no_timeout_rule(){let mut s=start("old");s.sessions.get_mut("old").unwrap().touched_ms=u64::MAX;assert!(candidate(&s.sessions["old"],false));}
    #[test]fn tools_waits_errors_are_not_legacy_start(){for (state,event) in [(LightState::Working,"PostToolUse"),(LightState::Waiting,"PermissionRequest"),(LightState::Error,"PostToolUse")]{let mut s=start("s");let row=s.sessions.get_mut("s").unwrap();row.state=state;row.event=event.into();assert!(!candidate(row,false));}}
    #[test]fn retains_original_evidence(){let mut s=start("old");let old=s.sessions["old"].clone();assert!(set(&mut s,&old,true));let row=&s.sessions["old"];assert_eq!(row.state,old.state);assert_eq!(row.event,old.event);assert_eq!(row.turn,old.turn);assert_eq!(row.changed_ms,old.changed_ms);assert_eq!(row.touched_ms,old.touched_ms);let snap=s.snapshot("vm",200);snap.validate().unwrap();assert_eq!(snap.sessions[0].state,LightState::Off);assert_eq!(snap.sessions[0].event,"UnverifiedLegacy:UserPromptSubmit");}
    #[test]fn newer_hook_wins(){let mut s=start("old");let old=s.sessions["old"].clone();s.apply(&json!({"session_id":"old","hook_event_name":"PostToolUse","turn_id":"t"}),200,true,"").unwrap();assert!(!set(&mut s,&old,true));}
    #[test]fn real_callback_restores(){let mut s=start("old");let old=s.sessions["old"].clone();set(&mut s,&old,true);s.apply(&json!({"session_id":"old","hook_event_name":"PostToolUse","turn_id":"t"}),200,true,"").unwrap();assert!(!s.sessions["old"].unverified_legacy);assert_eq!(s.snapshot("vm",200).sessions[0].state,LightState::Working);}
    #[test]fn next_turn_restores(){let mut s=start("old");let old=s.sessions["old"].clone();set(&mut s,&old,true);s.apply(&json!({"session_id":"old","hook_event_name":"UserPromptSubmit","turn_id":"next"}),200,true,"").unwrap();assert!(!s.sessions["old"].unverified_legacy);assert_eq!(s.sessions["old"].turn,"next");}
    #[test]fn evidence_restores_without_timer_reset(){let mut s=start("old");let old=s.sessions["old"].clone();set(&mut s,&old,true);let old=s.sessions["old"].clone();assert!(set(&mut s,&old,false));assert_eq!(s.sessions["old"].changed_ms,100);}
    #[test]fn mask_idempotent(){let mut s=start("old");let old=s.sessions["old"].clone();set(&mut s,&old,true);let old=s.sessions["old"].clone();let rev=s.revision;assert!(!set(&mut s,&old,true));assert_eq!(s.revision,rev);}
    #[test]fn serialization_backward_compatible(){let s=start("old");let mut v=serde_json::to_value(&s).unwrap();v["sessions"]["old"].as_object_mut().unwrap().remove("unverified_legacy");let old:LocalState=serde_json::from_value(v).unwrap();assert!(!old.sessions["old"].unverified_legacy);}
    #[test]fn orphan_does_not_override_real_task(){let mut s=start("old");let old=s.sessions["old"].clone();set(&mut s,&old,true);s.apply(&json!({"session_id":"live","hook_event_name":"UserPromptSubmit","turn_id":"live-turn"}),200,true,"").unwrap();let mut e=Engine::default();e.accept(s.snapshot("vm",200),200,300).unwrap();assert_eq!(e.view(200,90).state,LightState::Working);s.apply(&json!({"session_id":"live","hook_event_name":"Stop","turn_id":"live-turn"}),300,true,"").unwrap();e.accept(s.snapshot("vm",300),300,300).unwrap();assert_eq!(e.view(300,90).state,LightState::Done);assert_eq!(s.sessions["old"].state,LightState::Working);assert!(s.sessions["old"].unverified_legacy);}
    #[test]fn unknown_alone_is_not_done(){let mut s=start("old");let old=s.sessions["old"].clone();set(&mut s,&old,true);let mut e=Engine::default();e.accept(s.snapshot("vm",200),200,300).unwrap();assert_eq!(e.view(200,90).state,LightState::Off);}
}
