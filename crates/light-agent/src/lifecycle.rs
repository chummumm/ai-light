//! Evidence-based lifecycle rules. Silence and relay heartbeats are not completion.
use anyhow::Result;
use light_core::{hooks::{LocalState, LocalSession, looks_like_question}, model::LightState};
use serde_json::{json, Value};

fn text<'a>(v:&'a Value,k:&str)->&'a str { v.get(k).and_then(Value::as_str).unwrap_or("") }
pub fn terminal(s:&LocalSession)->bool {
    matches!(s.event.as_str(),"Stop"|"Interrupt"|"JournalComplete"|"JournalAborted"|"OwnerExited")
}
/// All command-hook input goes through this gate before the legacy reducer.
pub fn hook(s:&mut LocalState,v:&Value,now:u64,heuristic:bool,run:&str)->Result<bool> {
    let event=text(v,"hook_event_name");
    if let Some(old)=s.sessions.get(text(v,"session_id")) {
        let turn=text(v,"turn_id");
        if event=="UserPromptSubmit" && !turn.is_empty() && turn==old.turn {
            // Redelivery must not reopen a finished turn or reset its timer.
            return Ok(false);
        }
        if terminal(old) && !matches!(event,"UserPromptSubmit"|"SessionStart"|"SessionEnd") {
            // A new prompt, not a late tool callback, starts the next turn.
            return Ok(false);
        }
    }
    s.apply(v,now,heuristic,run)
}
#[derive(Debug,Clone,Copy,PartialEq,Eq)]
pub enum Kind { Started, Complete, Aborted }
#[derive(Debug,Clone)]
pub struct Evidence {
    pub turn:String, pub kind:Kind, pub at:u64, pub question:bool, pub failed:bool,
}
/// Only UTC timestamps emitted by the supported rollout format. Unknown formats
/// are ignored, never replaced with "now" (which would replay old completion).
pub fn timestamp(s:&str)->Option<u64> {
    let b=s.as_bytes();
    if b.len()<20 || !s.ends_with('Z') || b[4]!=b'-'||b[7]!=b'-'||b[10]!=b'T'||b[13]!=b':'||b[16]!=b':' {return None;}
    fn n(b:&[u8])->Option<u64>{if !b.iter().all(u8::is_ascii_digit){return None;} std::str::from_utf8(b).ok()?.parse().ok()}
    let y=n(&b[0..4])?;let m=n(&b[5..7])?;let d=n(&b[8..10])?;
    let h=n(&b[11..13])?;let min=n(&b[14..16])?;let sec=n(&b[17..19])?;
    if !(1970..=9999).contains(&y)||!(1..=12).contains(&m)||h>23||min>59||sec>59{return None;}
    let leap=|x:u64|x%4==0&&(x%100!=0||x%400==0);
    let months=[31,if leap(y){29}else{28},31,30,31,30,31,31,30,31,30,31];
    if d==0||d>months[(m-1)as usize]{return None;}
    let mut days=0;for year in 1970..y{days+=if leap(year){366}else{365};}
    days+=months[..(m-1)as usize].iter().sum::<u64>()+d-1;
    let mut ms=0;
    if b.len()>20 {
        if b[19]!=b'.'||b.len()==21{return None;}
        let frac=&b[20..b.len()-1];if !frac.iter().all(u8::is_ascii_digit){return None;}
        for i in 0..3 {ms=ms*10+frac.get(i).map_or(0,|v|(v-b'0')as u64);}
    }
    Some(((days*24+h)*60+min)*60*1000+sec*1000+ms)
}
pub fn parse(v:&Value)->Option<Evidence> {
    if text(v,"type")!="event_msg" {return None;}
    let p=v.get("payload")?;
    let kind=match text(p,"type") {
        "task_started"|"turn_started"=>Kind::Started,
        "task_complete"|"turn_complete"=>Kind::Complete,
        "turn_aborted"=>Kind::Aborted,_=>return None,
    };
    let turn=text(p,"turn_id");
    if turn.is_empty()||turn.len()>128||turn.chars().any(char::is_control){return None;}
    Some(Evidence{turn:turn.into(),kind,at:timestamp(text(v,"timestamp"))?,
        question:looks_like_question(text(p,"last_agent_message")),
        failed:p.get("error").is_some_and(|v|!v.is_null())})
}
/// Ordered lifecycle evidence; a late old completion cannot replace a newer start.
pub fn advance(current:&mut Option<Evidence>,next:Evidence) {
    if let Some(old)=current.as_ref() {
        if next.at<old.at{return;}
        if next.turn!=old.turn && next.kind!=Kind::Started{return;}
        if next.turn==old.turn && old.kind!=Kind::Started && next.kind==Kind::Started{return;}
    }
    *current=Some(next);
}
/// Compare-and-swap: filesystem I/O is outside the state lock. Never apply its
/// result to a session that received a newer hook while the file was read.
pub fn reconcile(s:&mut LocalState,expected:&LocalSession,e:&Evidence,now:u64,heuristic:bool)->bool {
    let Some(cur)=s.sessions.get(&expected.id) else{return false;};
    if cur.version!=expected.version||cur.turn!=expected.turn||e.at>now.saturating_add(5000){return false;}
    if e.turn!=cur.turn {
        // Only an explicit newer start can introduce a new turn, never an old
        // completion observed during migration. No new sessions are created.
        if e.kind!=Kind::Started||e.at<=cur.touched_ms{return false;}
        let v=json!({"hook_event_name":"UserPromptSubmit","session_id":cur.id,"turn_id":e.turn});
        return hook(s,&v,e.at.min(now),heuristic,"").unwrap_or(false);
    }
    if e.kind==Kind::Started||terminal(cur){return false;}
    let state=if e.kind==Kind::Aborted{LightState::Waiting}
        else if e.failed{LightState::Error}
        else if heuristic&&e.question{LightState::Waiting}else{LightState::Done};
    s.emit(&expected.id,state,e.at.min(now));
    s.sessions.get_mut(&expected.id).unwrap().event=if e.kind==Kind::Aborted{"JournalAborted"}else{"JournalComplete"}.into();
    true
}
pub fn remove_known(s:&mut LocalState,expected:&LocalSession)->bool {
    if !s.sessions.get(&expected.id).is_some_and(|v|v.version==expected.version&&v.turn==expected.turn){return false;}
    s.sessions.remove(&expected.id);s.revision=s.revision.saturating_add(1);true
}
pub fn owner_exited(s:&mut LocalState,expected:&LocalSession,now:u64)->bool {
    let Some(cur)=s.sessions.get(&expected.id)else{return false;};
    if cur.version!=expected.version||cur.turn!=expected.turn||terminal(cur){return false;}
    // Losing the process is not successful completion. Retain a diagnostic record.
    s.emit(&expected.id,LightState::Off,now);
    s.sessions.get_mut(&expected.id).unwrap().event="OwnerExited".into();true
}
pub fn maintain(s:&mut LocalState,now:u64)->bool {
    let before=s.sessions.len();
    s.sessions.retain(|id,v| {
        let manual=id=="manual-test"&&v.event=="Manual"&&matches!(v.state,LightState::Working|LightState::Waiting|LightState::Error);
        !(manual&&now.saturating_sub(v.changed_ms)>=60_000 ||
            matches!(v.state,LightState::Off|LightState::Done)&&now.saturating_sub(v.touched_ms)>3_600_000)
    });
    if before!=s.sessions.len(){s.revision=s.revision.saturating_add(1);true}else{false}
}
#[cfg(test)]mod tests {
    use super::*;
    fn v(event:&str,turn:&str)->Value{json!({"hook_event_name":event,"session_id":"s","turn_id":turn,"tool_name":"Bash","tool_use_id":"x"})}
    fn start()->LocalState{let mut s=LocalState::default();hook(&mut s,&v("UserPromptSubmit","t"),100,true,"").unwrap();s}
    fn e(kind:Kind,at:u64)->Evidence{Evidence{turn:"t".into(),kind,at,question:false,failed:false}}
    #[test]fn late_tool_cannot_reopen(){for event in ["PostToolUse","PreToolUse","PermissionRequest"]{let mut s=start();hook(&mut s,&v("Stop","t"),200,true,"").unwrap();assert!(!hook(&mut s,&v(event,"t"),300,true,"").unwrap());assert_eq!(s.sessions["s"].state,LightState::Done);}}
    #[test]fn duplicate_prompt_cannot_reopen(){let mut s=start();hook(&mut s,&v("Stop","t"),200,true,"").unwrap();assert!(!hook(&mut s,&v("UserPromptSubmit","t"),300,true,"").unwrap());}
    #[test]fn next_turn_can_start(){let mut s=start();hook(&mut s,&v("Stop","t"),200,true,"").unwrap();assert!(hook(&mut s,&v("UserPromptSubmit","u"),300,true,"").unwrap());assert_eq!(s.sessions["s"].state,LightState::Working);}
    #[test]fn long_silent_work_survives(){let mut s=start();assert!(!maintain(&mut s,86_400_000));assert_eq!(s.sessions["s"].state,LightState::Working);}
    #[test]fn manual_expires_without_affecting_work(){let mut s=start();s.emit("manual-test",LightState::Working,100);assert!(maintain(&mut s,60_100));assert!(s.sessions.contains_key("s"));assert!(!s.sessions.contains_key("manual-test"));}
    #[test]fn expiry_runs_without_hooks(){let mut s=start();s.emit("old",LightState::Done,0);assert!(maintain(&mut s,3_600_001));assert!(s.sessions.contains_key("s"));}
    #[test]fn journal_recovers_missed_stop(){let mut s=start();let old=s.sessions["s"].clone();assert!(reconcile(&mut s,&old,&e(Kind::Complete,200),1000,true));assert_eq!(s.sessions["s"].changed_ms,200);assert_eq!(s.sessions["s"].state,LightState::Done);}
    #[test]fn stale_io_cannot_change_new_turn(){let mut s=start();let old=s.sessions["s"].clone();hook(&mut s,&v("UserPromptSubmit","u"),300,true,"").unwrap();assert!(!reconcile(&mut s,&old,&e(Kind::Complete,200),1000,true));assert_eq!(s.sessions["s"].turn,"u");}
    #[test]fn old_completion_does_not_finish_other_turn(){let mut s=start();let old=s.sessions["s"].clone();let mut ev=e(Kind::Complete,200);ev.turn="other".into();assert!(!reconcile(&mut s,&old,&ev,1000,true));}
    #[test]fn duplicate_journal_preserves_timestamp(){let mut s=start();let old=s.sessions["s"].clone();reconcile(&mut s,&old,&e(Kind::Complete,200),1000,true);let old=s.sessions["s"].clone();assert!(!reconcile(&mut s,&old,&e(Kind::Complete,200),2000,true));assert_eq!(s.sessions["s"].changed_ms,200);}
    #[test]fn question_remains_waiting(){let mut s=start();let old=s.sessions["s"].clone();let mut ev=e(Kind::Complete,200);ev.question=true;reconcile(&mut s,&old,&ev,1000,true);assert_eq!(s.sessions["s"].state,LightState::Waiting);}
    #[test]fn error_is_not_success(){let mut s=start();let old=s.sessions["s"].clone();let mut ev=e(Kind::Complete,200);ev.failed=true;reconcile(&mut s,&old,&ev,1000,true);assert_eq!(s.sessions["s"].state,LightState::Error);}
    #[test]fn exited_owner_is_off_not_done(){let mut s=start();let old=s.sessions["s"].clone();assert!(owner_exited(&mut s,&old,1000));assert_eq!(s.sessions["s"].state,LightState::Off);}
    #[test]fn dead_old_owner_cannot_stop_new_work(){let mut s=start();let old=s.sessions["s"].clone();hook(&mut s,&v("UserPromptSubmit","u"),200,true,"").unwrap();assert!(!owner_exited(&mut s,&old,1000));}
    #[test]fn source_heartbeat_does_not_prune_work(){let mut s=start();let _=s.snapshot("vm",86_400_000);assert!(!maintain(&mut s,86_400_000));}
    #[test]fn late_old_completion_does_not_replace_new_start(){let mut a=Some(e(Kind::Started,200));let mut old=e(Kind::Complete,300);old.turn="old".into();advance(&mut a,old);assert_eq!(a.unwrap().kind,Kind::Started);}
    #[test]fn utc_dates(){assert_eq!(timestamp("1970-01-01T00:00:00Z"),Some(0));assert_eq!(timestamp("2000-03-01T00:00:00.123456Z"),Some(951868800123));assert_eq!(timestamp("2026-09-27T02:42:55Z"),Some(1790476975000));assert!(timestamp("2025-02-29T00:00:00Z").is_none());assert!(timestamp("x").is_none());}
    #[test]fn parser_ignores_unrecognized_and_missing_ids(){assert!(parse(&json!({"type":"event_msg","payload":{"type":"stream_error"}})).is_none());assert!(parse(&json!({"type":"event_msg","timestamp":"2026-09-27T00:00:00Z","payload":{"type":"task_complete"}})).is_none());}
    #[test]fn parser_discards_content(){let x=json!({"type":"event_msg","timestamp":"2026-09-27T00:00:00Z","payload":{"type":"task_complete","turn_id":"t","last_agent_message":"SECRET"}});let ev=parse(&x).unwrap();assert!(!format!("{ev:?}").contains("SECRET"));}
}
