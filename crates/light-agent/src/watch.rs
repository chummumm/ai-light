//! Read-only reconciliation of already-known sessions. No prompts are copied or
//! uploaded. The rollout adapter is deliberately strict and is not a stable API.
use crate::{lifecycle::{self,Evidence}, owner::{self,Owner,Presence}, store};
use anyhow::{Context,Result};
use light_core::{fsutil::atomic_json, hooks::LocalSession, now_ms};
use serde::{Deserialize,Serialize};
use serde_json::Value;
use std::{collections::{BTreeMap,BTreeSet},fs::{self,File},io::{BufRead,BufReader,Read,Seek,SeekFrom},path::{Path,PathBuf},time::{Duration,Instant}};
const LINE:usize=1024*1024;
const WINDOW:u64=4*1024*1024;
#[derive(Default,Clone,Serialize,Deserialize)]struct Binding {
    #[serde(default)] path:Option<PathBuf>,
    #[serde(default)] owner:Option<Owner>,
    #[serde(default)] turn:String,
    #[serde(default)] retired:Vec<String>,
    #[serde(default)] calls:BTreeSet<String>,
}
#[derive(Default,Serialize,Deserialize)]struct Book {
    #[serde(default)] roots:BTreeSet<PathBuf>,
    #[serde(default)] sessions:BTreeMap<String,Binding>,
}
fn book_path()->Result<PathBuf>{Ok(store::state_dir()?.join("tracking.json"))}
fn book()->Result<Book>{match fs::read(book_path()?){Ok(b)=>Ok(serde_json::from_slice(&b).context("invalid lifecycle tracking data")?),Err(e)if e.kind()==std::io::ErrorKind::NotFound=>Ok(Book::default()),Err(e)=>Err(e.into())}}
fn root()->Option<PathBuf>{fs::canonicalize(std::env::var_os("CODEX_HOME").map(PathBuf::from).unwrap_or(store::home().ok()?.join(".codex"))).ok()}
fn allowed(path:&Path,roots:&BTreeSet<PathBuf>)->Option<PathBuf>{
    let p=fs::canonicalize(path).ok()?;
    if p.extension().and_then(|s|s.to_str())!=Some("jsonl")||!fs::metadata(&p).ok()?.is_file(){return None;}
    roots.iter().any(|r|p.starts_with(r.join("sessions"))||p.starts_with(r.join("archived_sessions"))).then_some(p)
}
pub fn remember_root()->Result<()> {
    store::update(|_|{
        let mut b=book()?;
        if let Some(r)=root(){b.roots.insert(r);}
        atomic_json(&book_path()?,&b)?;Ok(false)
    })
}
/// Called by the existing hook executable path; no Codex restart is necessary
/// for future invocations to use the new reducer after an atomic binary upgrade.
pub fn record(v:&Value,now:u64,heuristic:bool,run:&str)->Result<()> {
    let sid=v.get("session_id").and_then(Value::as_str).unwrap_or("");
    let event=v.get("hook_event_name").and_then(Value::as_str).unwrap_or("");
    let turn=v.get("turn_id").and_then(Value::as_str).unwrap_or("");
    let call=v.get("tool_use_id").and_then(Value::as_str).unwrap_or("");
    let parent=owner::capture();let home=root();
    store::update(|s|{
        let mut tracking=book().ok();
        if event=="UserPromptSubmit"&&!turn.is_empty()&&tracking.as_ref().and_then(|b|b.sessions.get(sid)).is_some_and(|b|b.retired.iter().any(|t|t==turn)){return Ok(false);}
        if event=="PreToolUse"&&!call.is_empty()&&s.sessions.get(sid).is_some_and(|s|s.event=="Stop")
            &&tracking.as_ref().and_then(|b|b.sessions.get(sid)).is_some_and(|b|b.calls.contains(call)){return Ok(false);}
        let changed=lifecycle::hook(s,v,now,heuristic,run)?;
        if (changed||event=="SessionStart")&&s.sessions.contains_key(sid)&&v.get("agent_id").and_then(Value::as_str).unwrap_or("").is_empty(){
            if let Some(b)=tracking.as_mut(){
                if let Some(home)=home.clone(){if b.roots.len()<16{b.roots.insert(home);}}
                let path=v.get("transcript_path").and_then(Value::as_str).and_then(|p|allowed(Path::new(p),&b.roots));
                let current=&s.sessions[sid];
                let binding=b.sessions.entry(sid.into()).or_default();
                if !binding.turn.is_empty()&&binding.turn!=current.turn{
                    binding.retired.push(binding.turn.clone());binding.calls.clear();
                    if binding.retired.len()>64{binding.retired.remove(0);}
                }
                binding.turn=current.turn.clone();
                if event=="PreToolUse"&&!call.is_empty()&&binding.calls.len()<4096{binding.calls.insert(call.into());}
                if path.is_some(){binding.path=path;}
                if parent.is_some(){binding.owner=parent;}
                // Bound private bookkeeping to the live registry plus one-hour
                // lifecycle tombstones already retained by the state reducer.
                b.sessions.retain(|id,_|s.sessions.contains_key(id));
                if atomic_json(&book_path()?,b).is_err(){store::log_issue("lifecycle metadata save failed (hook state retained)");}
            }
        }
        Ok(changed)
    })
}
fn notification(s:&mut light_core::hooks::LocalState,v:&Value,now:u64,heuristic:bool)->Result<bool> {
    if v.get("type").and_then(Value::as_str)!=Some("agent-turn-complete"){return Ok(false);}
    let turn=v.get("turn-id").and_then(Value::as_str).unwrap_or("");
    let thread=v.get("thread-id").and_then(Value::as_str).unwrap_or("");
    let ids:Vec<_>=s.sessions.values().filter(|s|!turn.is_empty()&&s.turn==turn&&(thread.is_empty()||s.id==thread)).map(|s|s.id.clone()).collect();
    if ids.len()==1 && matches!(s.sessions[&ids[0]].event.as_str(),"NotifyComplete"|"JournalComplete"|"JournalAborted"|"Interrupt"|"OwnerExited") {
        // A delayed/duplicate notification must not override an abort or a
        // definitive completion; in particular, it must not replay a timer.
        return Ok(false);
    }
    let mut changed=s.apply_notify(v,now,heuristic)?;
    if ids.len()==1&&s.sessions.get(&ids[0]).is_some_and(|s|s.event=="Stop"){
        s.revision=s.revision.saturating_add(1);let entry=s.sessions.get_mut(&ids[0]).unwrap();
        entry.event="NotifyComplete".into();entry.version=s.revision;changed=true;
    }
    Ok(changed)
}
pub fn notify(v:&Value,now:u64,heuristic:bool)->Result<()> {
    store::update(|s|notification(s,v,now,heuristic))
}
#[derive(Default)]struct Cursor {
    path:PathBuf, offset:u64, identity:(u64,u64), verified:bool, internal:bool,
    last:Option<Evidence>, discard:bool, tainted:bool,
}
#[cfg(unix)]fn file_id(m:&fs::Metadata)->(u64,u64){use std::os::unix::fs::MetadataExt;(m.dev(),m.ino())}
#[cfg(not(unix))]fn file_id(_: &fs::Metadata)->(u64,u64){(0,0)}
fn header(file:&File,sid:&str)->Result<bool>{
    let mut f=file.try_clone()?;f.seek(SeekFrom::Start(0))?;
    let mut line=Vec::new();BufReader::new(f).take((LINE+1)as u64).read_until(b'\n',&mut line)?;
    anyhow::ensure!(line.len()<=LINE&&line.last()==Some(&b'\n'),"incomplete or oversized rollout header");
    let v:Value=serde_json::from_slice(&line).context("unsupported rollout header")?;
    anyhow::ensure!(v.get("type").and_then(Value::as_str)==Some("session_meta")&&v.pointer("/payload/id").and_then(Value::as_str)==Some(sid),"rollout identity mismatch");
    let source=v.pointer("/payload/source");
    Ok(source.is_some_and(|v|v.get("subagent").is_some()||v.as_str()==Some("subagent")))
}
impl Cursor {
    fn poll(&mut self,path:&Path,sid:&str)->Result<bool>{
        let mut f=File::open(path)?;let meta=f.metadata()?;
        anyhow::ensure!(meta.is_file(),"rollout is not a regular file");
        let id=file_id(&meta);
        if !self.verified||self.path!=path||self.identity!=id||meta.len()<self.offset{
            *self=Self{path:path.to_owned(),identity:id,internal:header(&f,sid)?,verified:true,..Self::default()};
            self.offset=meta.len().saturating_sub(WINDOW);
            // Starting in the middle of a record cannot produce an event.
            self.discard=self.offset>0;
        }
        if self.internal{return Ok(true);}
        f.seek(SeekFrom::Start(self.offset))?;
        let mut reader=BufReader::new(f);let mut consumed=0u64;
        while consumed<WINDOW {
            let begin=self.offset;let mut line=Vec::new();
            let n=(&mut reader).take((LINE+1)as u64).read_until(b'\n',&mut line)?;
            if n==0{return Ok(!self.discard&&!self.tainted);}
            let newline=line.last()==Some(&b'\n');
            if !newline&&n<=LINE&&!self.discard{
                // Keep an incomplete record for the next poll. Do not parse it.
                self.offset=begin;return Ok(false);
            }
            self.offset+=n as u64;consumed+=n as u64;
            if self.discard{if newline{self.discard=false;}continue;}
            if n>LINE{self.discard=!newline;self.tainted=true;continue;}
            let v:Value=match serde_json::from_slice(&line){Ok(v)=>v,Err(_)=>{self.tainted=true;continue;}};
            if let Some(ev)=lifecycle::parse(&v){
                if ev.kind==lifecycle::Kind::Started{self.tainted=false;}
                lifecycle::advance(&mut self.last,ev);
            }
        }
        Ok(self.offset>=meta.len()&&!self.discard&&!self.tainted)
    }
}
#[derive(Default)]struct Watch {
    cursor:Cursor, owner:Option<Owner>, dead_since:Option<Instant>, dead_version:u64,
    last_bind:Option<Instant>, status:&'static str,
}
#[derive(Default)]pub struct Observer {
    watches:BTreeMap<String,Watch>, discovered:BTreeMap<String,PathBuf>,
    search:Vec<PathBuf>, last_search:Option<Instant>, last_report:Option<Instant>,
}
impl Observer {
    /// Filename-only discovery, restricted to IDs already in AI Light. Existing
    /// running tasks can be adopted without writing or restarting Codex.
    fn discover(&mut self,roots:&BTreeSet<PathBuf>,ids:&BTreeSet<String>){
        let now=Instant::now();
        if self.search.is_empty()&&self.last_search.is_none_or(|t|now.duration_since(t)>=Duration::from_secs(60)){
            for r in roots{self.search.push(r.join("sessions"));self.search.push(r.join("archived_sessions"));}
            self.last_search=Some(now);
        }
        // Date folders are shallow; bounded directory count keeps polling cheap.
        let mut count=0;
        while count<64 {
            let Some(dir)=self.search.pop()else{break;};count+=1;
            let Ok(entries)=fs::read_dir(&dir)else{continue;};
            for e in entries.flatten(){
                let Ok(t)=e.file_type()else{continue;};
                if t.is_symlink(){continue;}
                if t.is_dir(){
                    if roots.iter().any(|r|e.path().strip_prefix(r).ok().is_some_and(|p|p.components().count()<=5)){self.search.push(e.path());}
                }else if t.is_file(){
                    let name=e.file_name();let name=name.to_string_lossy();
                    if !name.ends_with(".jsonl"){continue;}
                    for sid in ids {
                        if name.ends_with(&format!("-{sid}.jsonl")) {
                            if let Some(path)=allowed(&e.path(),roots){self.discovered.insert(sid.clone(),path);}
                        }
                    }
                }
            }
        }
    }
    pub fn tick(&mut self,heuristic:bool)->Result<()> {
        let local=store::read_state()?;let mut b=book()?;if let Some(root)=root(){b.roots.insert(root);}
        let ids:BTreeSet<_>=local.sessions.keys().cloned().collect();
        self.watches.retain(|id,_|ids.contains(id));self.discovered.retain(|id,_|ids.contains(id));
        self.discover(&b.roots,&ids);
        enum Action{Journal(LocalSession,Evidence),Internal(LocalSession),Exit(LocalSession,Owner)}
        let mut actions=Vec::new();let now=now_ms();
        for session in local.sessions.values(){
            if session.id=="manual-test"||session.event=="OwnerExited"{continue;}
            let w=self.watches.entry(session.id.clone()).or_default();
            let binding=b.sessions.get(&session.id);
            if let Some(owner)=binding.and_then(|b|b.owner.clone()){
                if w.owner.as_ref()!=Some(&owner){w.owner=Some(owner);w.dead_since=None;}
            }
            let path=binding.and_then(|b|b.path.as_ref()).or_else(||self.discovered.get(&session.id)).and_then(|p|allowed(p,&b.roots));
            w.status="unverified";
            if let Some(path)=path {
                match w.cursor.poll(&path,&session.id){
                    Ok(true) if w.cursor.internal=>{actions.push(Action::Internal(session.clone()));w.status="internal";continue;}
                    Ok(true)=>{
                        if let Some(ev)=w.cursor.last.as_ref(){
                            w.status=if ev.turn!=session.turn{"different_turn"}else if ev.kind==lifecycle::Kind::Started{"journal_open"}else{"journal_terminal"};
                            actions.push(Action::Journal(session.clone(),ev.clone()));
                        }
                    }
                    Ok(false)=>w.status="journal_pending",
                    Err(_)=>w.status="journal_unavailable",
                }
                if w.owner.is_none()&&w.last_bind.is_none_or(|t|t.elapsed()>Duration::from_secs(60)){
                    w.last_bind=Some(Instant::now());w.owner=owner::holding(&path);
                }
            }
            if !matches!(session.state,light_core::model::LightState::Done|light_core::model::LightState::Off){
                match w.owner.as_ref().map(owner::presence).unwrap_or(Presence::Unknown){
                    Presence::Gone=>{
                        if w.dead_version!=session.version{w.dead_since=None;w.dead_version=session.version;}
                        let since=w.dead_since.get_or_insert_with(Instant::now);
                        if since.elapsed()>=Duration::from_secs(10){actions.push(Action::Exit(session.clone(),w.owner.clone().unwrap()));}
                    }
                    Presence::Alive=>{w.dead_since=None;if w.status=="unverified"{w.status="owner_alive";}}
                    Presence::Unknown=>w.dead_since=None,
                }
            }
        }
        store::update(|s|{
            let mut changed=false;
            let latest_tracking=book()?;
            for action in actions {
                changed|=match action {
                    Action::Journal(old,e)=>lifecycle::reconcile(s,&old,&e,now,heuristic),
                    Action::Internal(old)=>lifecycle::remove_known(s,&old),
                    Action::Exit(old,owner)=>{
                        let reassigned=latest_tracking.sessions.get(&old.id).and_then(|b|b.owner.as_ref()).is_some_and(|current|current!=&owner);
                        !reassigned&&owner::presence(&owner)==Presence::Gone&&lifecycle::owner_exited(s,&old,now)
                    },
                };
            }
            changed|=lifecycle::maintain(s,now);Ok(changed)
        })?;
        if self.last_report.is_none_or(|t|t.elapsed()>=Duration::from_secs(10)){
            self.last_report=Some(Instant::now());
            let rows:Vec<_>=self.watches.iter().map(|(id,w)|serde_json::json!({"session_id":id,"evidence":w.status})).collect();
            atomic_json(&store::state_dir()?.join("lifecycle-status.json"),&serde_json::json!({"updated_ms":now,"sessions":rows}))?;
        }
        Ok(())
    }
}
#[cfg(test)]mod tests{
    use super::*;
    fn tmp()->PathBuf{let p=std::env::temp_dir().join(format!("ailight-test-{}",uuid::Uuid::new_v4()));fs::create_dir_all(&p).unwrap();p}
    fn meta(id:&str)->String{format!("{{\"type\":\"session_meta\",\"payload\":{{\"id\":\"{id}\",\"source\":\"cli\"}}}}\n")}
    fn done()->String{"{\"type\":\"event_msg\",\"timestamp\":\"2026-09-27T00:00:00Z\",\"payload\":{\"type\":\"task_complete\",\"turn_id\":\"t\"}}\n".into()}
    #[test]fn partial_record_is_retried(){let dir=tmp();let path=dir.join("x.jsonl");let d=done();fs::write(&path,format!("{}{}",meta("s"),d.trim_end())).unwrap();let mut c=Cursor::default();assert!(!c.poll(&path,"s").unwrap());assert!(c.last.is_none());fs::write(&path,format!("{}{}",meta("s"),d)).unwrap();assert!(c.poll(&path,"s").unwrap());assert_eq!(c.last.as_ref().unwrap().kind,lifecycle::Kind::Complete);fs::remove_dir_all(dir).unwrap();}
    #[test]fn different_file_identity_rejected(){let dir=tmp();let path=dir.join("x.jsonl");fs::write(&path,format!("{}{}",meta("other"),done())).unwrap();assert!(Cursor::default().poll(&path,"s").is_err());fs::remove_dir_all(dir).unwrap();}
    #[test]fn subagent_meta_identified(){let dir=tmp();let path=dir.join("x.jsonl");fs::write(&path,"{\"type\":\"session_meta\",\"payload\":{\"id\":\"s\",\"source\":{\"subagent\":\"thread_title\"}}}\n").unwrap();let mut c=Cursor::default();assert!(c.poll(&path,"s").unwrap());assert!(c.internal);fs::remove_dir_all(dir).unwrap();}
    #[test]fn outside_root_rejected(){let dir=tmp();let path=dir.join("x.jsonl");fs::write(&path,meta("s")).unwrap();let roots=BTreeSet::from([dir.clone()]);assert!(allowed(&path,&roots).is_none());fs::remove_dir_all(dir).unwrap();}
    #[test]fn truncation_resets_cursor(){let dir=tmp();let path=dir.join("x.jsonl");fs::write(&path,format!("{}{}",meta("s"),done())).unwrap();let mut c=Cursor::default();assert!(c.poll(&path,"s").unwrap());fs::write(&path,meta("s")).unwrap();assert!(c.poll(&path,"s").unwrap());assert!(c.last.is_none());fs::remove_dir_all(dir).unwrap();}
    #[test]fn notification_latches_without_replay(){
        use light_core::hooks::LocalState;
        let mut s=LocalState::default();
        let hook=serde_json::json!({"hook_event_name":"UserPromptSubmit","session_id":"s","turn_id":"t"});
        lifecycle::hook(&mut s,&hook,100,true,"").unwrap();
        let n=serde_json::json!({"type":"agent-turn-complete","thread-id":"s","turn-id":"t","last-assistant-message":"done"});
        assert!(notification(&mut s,&n,200,true).unwrap());let revision=s.revision;
        assert_eq!(s.sessions["s"].event,"NotifyComplete");
        assert!(!notification(&mut s,&n,300,true).unwrap());assert_eq!(s.revision,revision);
        assert_eq!(s.sessions["s"].changed_ms,200);
    }
    #[test]fn late_notification_cannot_undo_interruption(){
        use light_core::hooks::LocalState;
        let mut s=LocalState::default();
        let hook=serde_json::json!({"hook_event_name":"Interrupt","session_id":"s","turn_id":"t"});
        lifecycle::hook(&mut s,&hook,100,true,"").unwrap();
        let n=serde_json::json!({"type":"agent-turn-complete","thread-id":"s","turn-id":"t"});
        assert!(!notification(&mut s,&n,200,true).unwrap());
        assert_eq!(s.sessions["s"].state,light_core::model::LightState::Waiting);
    }
}
