//! Codex hooks -> minimal local state. Raw prompts, tool arguments and responses
//! are inspected in memory only and are never written to the state snapshot.
use crate::model::{LightState, Snapshot, WireSession};
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub const HOOK_EVENTS: &[&str] = &[
    "SessionStart", "UserPromptSubmit", "PreToolUse", "PermissionRequest",
    "PostToolUse", "Stop", "Interrupt", "SessionEnd",
];
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalSession {
    pub id: String, pub turn: String, pub version: u64, pub state: LightState,
    pub event: String, pub changed_ms: u64, pub touched_ms: u64,
    #[serde(default)] pub run_id: String,
    #[serde(default)] pending_ids: BTreeSet<String>,
    #[serde(default)] pending_names: BTreeSet<String>,
    #[serde(default)] error_latched: bool,
}
impl LocalSession {
    fn new(id: String, turn: String, now: u64) -> Self {
        Self { id, turn, version:0, state:LightState::Off, event:"SessionStart".into(),
            changed_ms:now, touched_ms:now, run_id:String::new(), pending_ids:BTreeSet::new(),
            pending_names:BTreeSet::new(), error_latched:false }
    }
    fn waiting(&self) -> bool { !self.pending_ids.is_empty() || !self.pending_names.is_empty() }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalState {
    pub generation: String,
    pub revision: u64,
    pub sessions: BTreeMap<String, LocalSession>,
}
impl Default for LocalState {
    fn default() -> Self { Self { generation:uuid::Uuid::new_v4().to_string(), revision:0, sessions:BTreeMap::new() } }
}
impl LocalState {
    pub fn apply(&mut self, input: &Value, now: u64, question_heuristic: bool, run_id: &str) -> Result<bool> {
        let event = text(input, "hook_event_name");
        if !HOOK_EVENTS.contains(&event) { return Ok(false); }
        // Some versions include agent_id on non-lifecycle hooks. Do not turn a
        // child completion into the main task's completion.
        if !text(input,"agent_id").is_empty() { return Ok(false); }
        let id = identifier(text(input,"session_id"));
        if id.is_empty() { return Ok(false); }
        let turn = identifier(text(input,"turn_id"));
        self.sessions.retain(|_, s| {
            !(matches!(s.state, LightState::Off | LightState::Done) && now.saturating_sub(s.touched_ms) > 3_600_000)
        });
        if event == "SessionEnd" {
            // Preserve a completed turn's original countdown after normal CLI exit.
            if self.sessions.get(&id).is_some_and(|s| s.state == LightState::Done) { return Ok(false); }
            if self.sessions.remove(&id).is_some() { self.revision = self.revision.saturating_add(1); return Ok(true); }
            return Ok(false);
        }
        if event == "SessionStart" && self.sessions.contains_key(&id) { return Ok(false); }
        ensure!(self.sessions.contains_key(&id) || self.sessions.len() < 128, "local session limit reached");
        let s = self.sessions.entry(id.clone()).or_insert_with(|| LocalSession::new(id,turn.clone(),now));
        if !run_id.is_empty() { s.run_id = identifier(run_id); }
        if event != "UserPromptSubmit" && !s.turn.is_empty() && !turn.is_empty() && s.turn != turn {
            return Ok(false);
        }
        if event == "UserPromptSubmit" {
            if !turn.is_empty() && turn == s.turn && s.event == event { return Ok(false); }
            s.turn = turn;
            s.pending_ids.clear(); s.pending_names.clear(); s.error_latched = false;
        } else if s.turn.is_empty() && !turn.is_empty() { s.turn = turn; }
        let old_state = s.state;
        let tool = identifier(text(input,"tool_name"));
        let tool_id = identifier(text(input,"tool_use_id"));
        let mut new_state = old_state;
        match event {
            "SessionStart" => {},
            "UserPromptSubmit" => new_state = LightState::Working,
            "PermissionRequest" => {
                if !tool_id.is_empty() { s.pending_ids.insert(tool_id); }
                else { s.pending_names.insert(tool); }
                new_state = LightState::Waiting;
            }
            "PreToolUse" => {
                s.error_latched = false;
                if is_question_tool(&tool) {
                    if !tool_id.is_empty() { s.pending_ids.insert(tool_id); }
                    else { s.pending_names.insert(tool); }
                }
                new_state = if s.waiting() { LightState::Waiting } else { LightState::Working };
            }
            "PostToolUse" => {
                s.pending_ids.remove(&tool_id); s.pending_names.remove(&tool);
                if explicit_tool_failure(input.get("tool_response").unwrap_or(&Value::Null)) { s.error_latched=true; }
                new_state = if s.error_latched { LightState::Error }
                    else if s.waiting() { LightState::Waiting } else { LightState::Working };
            }
            "Interrupt" => {
                s.pending_ids.clear(); s.pending_names.clear(); s.error_latched=false;
                new_state=LightState::Waiting;
            }
            "Stop" => {
                new_state = if s.error_latched { LightState::Error }
                    else if s.waiting() || (question_heuristic && looks_like_question(text(input,"last_assistant_message"))) { LightState::Waiting }
                    else { LightState::Done };
                if s.event == "Stop" && new_state == old_state { return Ok(false); }
            }
            _ => {}
        }
        if new_state != old_state || event == "UserPromptSubmit" { s.changed_ms=now; }
        s.state=new_state; s.event=event.to_owned(); s.touched_ms=now;
        self.revision=self.revision.saturating_add(1); s.version=self.revision;
        Ok(true)
    }

    /// Completion fallback for Codex's user-level `notify` callback.
    ///
    /// This path never creates a session. A completion is accepted only when
    /// hooks have already registered the same session/turn locally.
    pub fn apply_notify(&mut self, input: &Value, now: u64, question_heuristic: bool) -> Result<bool> {
        if text(input, "type") != "agent-turn-complete" { return Ok(false); }
        let turn = identifier(text(input, "turn-id"));
        if turn.is_empty() { return Ok(false); }
        let thread = identifier(text(input, "thread-id"));
        let id = if !thread.is_empty() {
            let Some(session) = self.sessions.get(&thread) else { return Ok(false); };
            if !session.turn.is_empty() && session.turn != turn { return Ok(false); }
            thread
        } else {
            // Older notify payloads lacked thread-id. Use turn-id only if it
            // identifies exactly one already-known session.
            let mut matches = self.sessions.values().filter(|s| s.turn == turn).map(|s| s.id.clone());
            let Some(first) = matches.next() else { return Ok(false); };
            if matches.next().is_some() { return Ok(false); }
            first
        };
        let last = input.get("last-assistant-message").and_then(Value::as_str).unwrap_or("");
        let mut stop = serde_json::Map::new();
        stop.insert("hook_event_name".into(), Value::String("Stop".into()));
        stop.insert("session_id".into(), Value::String(id));
        stop.insert("turn_id".into(), Value::String(turn));
        stop.insert("last_assistant_message".into(), Value::String(last.to_owned()));
        self.apply(&Value::Object(stop), now, question_heuristic, "")
    }

    pub fn emit(&mut self, id: &str, state: LightState, now: u64) {
        self.revision=self.revision.saturating_add(1);
        let s=self.sessions.entry(id.to_owned()).or_insert_with(|| LocalSession::new(id.to_owned(),"manual".into(),now));
        s.state=state; s.event="Manual".into(); s.changed_ms=now; s.touched_ms=now; s.version=self.revision;
        s.error_latched=false; s.pending_ids.clear(); s.pending_names.clear();
    }
    pub fn clear(&mut self) { self.sessions.clear(); self.revision=self.revision.saturating_add(1); }
    pub fn mark_process_error(&mut self, run: &str, now: u64) {
        let ids:Vec<String>=self.sessions.values().filter(|s| s.run_id==run).map(|s|s.id.clone()).collect();
        if ids.is_empty() { self.emit(&format!("process-{run}"),LightState::Error,now); }
        else { for id in ids { self.emit(&id,LightState::Error,now); } }
    }
    pub fn snapshot(&self, source: &str, now: u64) -> Snapshot {
        Snapshot { schema:1, source_id:source.to_owned(), generation:self.generation.clone(), revision:self.revision,
            sessions:self.sessions.values().map(|s|WireSession{id:s.id.clone(),turn:s.turn.clone(),version:s.version,
                state:s.state,age_ms:now.saturating_sub(s.changed_ms),event:s.event.clone()}).collect() }
    }
}
fn text<'a>(v: &'a Value, key: &str) -> &'a str { v.get(key).and_then(Value::as_str).unwrap_or("") }
fn identifier(s: &str) -> String {
    let mut out=String::new();
    for c in s.chars().filter(|c| !c.is_control()) { if out.len()+c.len_utf8()>128 {break;} out.push(c); }
    out
}
fn is_question_tool(tool: &str) -> bool {
    matches!(tool, "request_user_input" | "AskUserQuestion") || tool.ends_with("__request_user_input")
}
pub fn looks_like_question(message: &str) -> bool {
    let tail:String=message.chars().rev().take(600).collect::<String>().chars().rev().collect();
    let line=tail.trim().trim_end_matches(['*','`',' ']).to_lowercase();
    if line.ends_with('?') || line.ends_with('？') { return true; }
    ["请确认后", "请你确认", "等待你确认", "需要你提供", "请提供", "请选择", "please confirm", "which option would you", "would you like me to"].iter().any(|s|line.contains(s))
}
pub fn explicit_tool_failure(v: &Value) -> bool {
    for key in ["isError","is_error"] { if v.get(key).and_then(Value::as_bool)==Some(true) { return true; } }
    for key in ["exit_code","exitCode","returncode"] {
        if v.get(key).and_then(Value::as_i64).is_some_and(|n| n!=0) { return true; }
    }
    if matches!(v.get("status").and_then(Value::as_str), Some("failed" | "error")) { return true; }
    if let Some(s)=v.as_str() {
        if s.len() <= 64*1024 {
            if let Ok(inner)=serde_json::from_str::<Value>(s) {
                if !inner.is_string() && explicit_tool_failure(&inner) { return true; }
            }
            for line in s.lines().take(100) {
                for prefix in ["Process exited with code ", "Exit code: ", "exit_code: "] {
                    if let Some(tail)=line.trim().strip_prefix(prefix) {
                        if tail.split_whitespace().next().and_then(|x|x.parse::<i32>().ok()).is_some_and(|n|n!=0) { return true; }
                    }
                }
            }
        }
    }
    false
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn event(name:&str) -> Value { json!({"hook_event_name":name,"session_id":"s","turn_id":"t","tool_name":"Bash","tool_use_id":"x"}) }
    fn state(s:&LocalState)->LightState{s.sessions["s"].state}
    #[test] fn prompt_working() {let mut s=LocalState::default();s.apply(&event("UserPromptSubmit"),0,true,"").unwrap();assert_eq!(state(&s),LightState::Working);}
    #[test] fn approval_waiting() {let mut s=LocalState::default();s.apply(&event("PermissionRequest"),0,true,"").unwrap();assert_eq!(state(&s),LightState::Waiting);}
    #[test] fn question_tool_waits() {let mut s=LocalState::default();let mut e=event("PreToolUse");e["tool_name"]=json!("request_user_input");s.apply(&e,0,true,"").unwrap();assert_eq!(state(&s),LightState::Waiting);}
    #[test] fn answer_resumes() {let mut s=LocalState::default();s.apply(&event("PermissionRequest"),0,true,"").unwrap();s.apply(&event("PostToolUse"),1,true,"").unwrap();assert_eq!(state(&s),LightState::Working);}
    #[test] fn failure_is_red() {let mut s=LocalState::default();let mut e=event("PostToolUse");e["tool_response"]=json!({"exit_code":1});s.apply(&e,0,true,"").unwrap();assert_eq!(state(&s),LightState::Error);s.apply(&event("Stop"),1,true,"").unwrap();assert_eq!(state(&s),LightState::Error);}
    #[test] fn new_tool_recovers() {let mut s=LocalState::default();s.emit("s",LightState::Error,0);s.sessions.get_mut("s").unwrap().turn="t".into();s.apply(&event("PreToolUse"),1,true,"").unwrap();assert_eq!(state(&s),LightState::Working);}
    #[test] fn normal_stop_done() {let mut s=LocalState::default();s.apply(&event("Stop"),0,true,"").unwrap();assert_eq!(state(&s),LightState::Done);}
    #[test] fn stop_question_waits() {let mut s=LocalState::default();let mut e=event("Stop");e["last_assistant_message"]=json!("是否继续？");s.apply(&e,0,true,"").unwrap();assert_eq!(state(&s),LightState::Waiting);}
    #[test] fn heuristic_can_be_disabled() {let mut s=LocalState::default();let mut e=event("Stop");e["last_assistant_message"]=json!("是否继续？");s.apply(&e,0,false,"").unwrap();assert_eq!(state(&s),LightState::Done);}
    #[test] fn child_stop_ignored() {let mut s=LocalState::default();s.apply(&event("UserPromptSubmit"),0,true,"").unwrap();s.apply(&event("SubagentStop"),1,true,"").unwrap();assert_eq!(state(&s),LightState::Working);}
    #[test] fn old_turn_does_not_complete_new_task() {let mut s=LocalState::default();let mut e=event("UserPromptSubmit");e["turn_id"]=json!("new");s.apply(&e,0,true,"").unwrap();s.apply(&event("Stop"),1,true,"").unwrap();assert_eq!(state(&s),LightState::Working);}
    #[test] fn duplicate_stop_keeps_timestamp() {let mut s=LocalState::default();s.apply(&event("Stop"),1,true,"").unwrap();s.apply(&event("Stop"),10000,true,"").unwrap();assert_eq!(s.sessions["s"].changed_ms,1);}
    #[test] fn interrupt_not_error() {let mut s=LocalState::default();s.apply(&event("Interrupt"),0,true,"").unwrap();assert_eq!(state(&s),LightState::Waiting);}
    #[test] fn normal_exit_preserves_done_timer() {let mut s=LocalState::default();s.apply(&event("Stop"),1,true,"").unwrap();s.apply(&event("SessionEnd"),5,true,"").unwrap();assert_eq!(s.sessions["s"].changed_ms,1);}
    fn notify(turn:&str)->Value {json!({"type":"agent-turn-complete","thread-id":"s","turn-id":turn,"last-assistant-message":"done"})}
    #[test] fn notify_completes_registered_turn() {let mut s=LocalState::default();s.apply(&event("UserPromptSubmit"),1,true,"").unwrap();assert!(s.apply_notify(&notify("t"),2,true).unwrap());assert_eq!(state(&s),LightState::Done);}
    #[test] fn notify_unknown_thread_is_ignored() {let mut s=LocalState::default();s.apply(&event("UserPromptSubmit"),1,true,"").unwrap();let mut n=notify("t");n["thread-id"]=json!("background-title");assert!(!s.apply_notify(&n,2,true).unwrap());assert_eq!(state(&s),LightState::Working);}
    #[test] fn notify_wrong_turn_is_ignored() {let mut s=LocalState::default();s.apply(&event("UserPromptSubmit"),1,true,"").unwrap();assert!(!s.apply_notify(&notify("other"),2,true).unwrap());assert_eq!(state(&s),LightState::Working);}
    #[test] fn legacy_notify_without_thread_matches_unique_turn() {let mut s=LocalState::default();s.apply(&event("UserPromptSubmit"),1,true,"").unwrap();let mut n=notify("t");n.as_object_mut().unwrap().remove("thread-id");assert!(s.apply_notify(&n,2,true).unwrap());assert_eq!(state(&s),LightState::Done);}
    #[test] fn notify_question_waits() {let mut s=LocalState::default();s.apply(&event("UserPromptSubmit"),1,true,"").unwrap();let mut n=notify("t");n["last-assistant-message"]=json!("请确认是否继续？");s.apply_notify(&n,2,true).unwrap();assert_eq!(state(&s),LightState::Waiting);}
    #[test] fn duplicate_notify_does_not_extend_done_timer() {let mut s=LocalState::default();s.apply(&event("UserPromptSubmit"),1,true,"").unwrap();s.apply_notify(&notify("t"),2,true).unwrap();s.apply_notify(&notify("t"),9000,true).unwrap();assert_eq!(s.sessions["s"].changed_ms,2);}
    #[test] fn secrets_not_serialized() {let mut s=LocalState::default();let mut e=event("UserPromptSubmit");e["prompt"]=json!("SECRET_PASSWORD");e["cwd"]=json!("/private/path");s.apply(&e,0,true,"").unwrap();let text=serde_json::to_string(&s).unwrap();assert!(!text.contains("SECRET_PASSWORD"));assert!(!text.contains("/private/path"));}
    #[test] fn explicit_failure_only() {assert!(!explicit_tool_failure(&json!("there is an error handler in this file")));assert!(explicit_tool_failure(&json!("Process exited with code 2")));assert!(!explicit_tool_failure(&json!({"exit_code":0})));}
}
