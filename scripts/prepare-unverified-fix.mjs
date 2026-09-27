import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
const files = new Map();
function load(p) { if (!files.has(p)) files.set(p, fs.readFileSync(p,'utf8')); return files.get(p); }
function edit(p,a,b) { const s=load(p); if (s.split(a).length!==2) throw Error(`non-unique/missing anchor in ${p}: ${a.slice(0,90)}`); files.set(p,s.replace(a,b)); }
function append(p,s) { files.set(p,load(p)+s); }
function add(p,s) { if(fs.existsSync(p))throw Error(`already exists ${p}`);files.set(p,s); }
for (const [p,sha] of Object.entries({
 'crates/light-core/src/hooks.rs':'0c1bde5cf2b02fae0f31d77ea2a13b54521395e6',
 'crates/light-agent/src/watch.rs':'7bd023aed60307654b14c8f657256b8337e32546',
 'ui/sessions-ui.js':'766931a67c48b9f9b8118684c47c61fab861b1ba'
})) {
 const bytes=fs.readFileSync(p);const actual=crypto.createHash('sha1').update(`blob ${bytes.length}\0`).update(bytes).digest('hex');
 if(actual!==sha)throw Error(`base changed: ${p}`);
}
const h='crates/light-core/src/hooks.rs';
edit(h,'    #[serde(default)] pub run_id: String,','    #[serde(default)] pub run_id: String,\n    // Projection only: retain the original event, turn, state and timestamps.\n    #[serde(default)] pub unverified_legacy: bool,');
edit(h,'            pending_names:BTreeSet::new(), error_latched:false }','            pending_names:BTreeSet::new(), error_latched:false, unverified_legacy:false }');
edit(h,'        s.state=new_state; s.event=event.to_owned(); s.touched_ms=now;','        s.unverified_legacy=false;\n        s.state=new_state; s.event=event.to_owned(); s.touched_ms=now;');
edit(h,'        s.state=state; s.event="Manual".into(); s.changed_ms=now; s.touched_ms=now; s.version=self.revision;','        s.unverified_legacy=false;\n        s.state=state; s.event="Manual".into(); s.changed_ms=now; s.touched_ms=now; s.version=self.revision;');
edit(h,'                state:s.state,age_ms:now.saturating_sub(s.changed_ms),event:s.event.clone()}).collect() }',`                state:if s.unverified_legacy {LightState::Off}else{s.state},
                age_ms:now.saturating_sub(s.changed_ms),
                event:if s.unverified_legacy {format!("UnverifiedLegacy:{}",s.event)}else{s.event.clone()}}).collect() }`);
edit('crates/light-agent/src/main.rs','mod lifecycle;','mod lifecycle;\nmod legacy;');
add('crates/light-agent/src/legacy.rs',String.raw`//! Unverifiable pre-observer records are UNKNOWN, not successful or still-live.
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
`);
const w='crates/light-agent/src/watch.rs';
edit(w,'use crate::{lifecycle::{self,Evidence}, owner::{self,Owner,Presence}, store};','use crate::{legacy,lifecycle::{self,Evidence}, owner::{self,Owner,Presence}, store};');
edit(w,'    Ok(source.is_some_and(|v|v.get("subagent").is_some()||v.as_str()==Some("subagent")))',`    // Only the explicitly named title generator is synthetic. A thread_spawn
    // subagent (or an unknown source) must never be silently removed.
    Ok(source.and_then(|v|v.get("subagent")).and_then(Value::as_str)==Some("thread_title"))`);
edit(w,'    search:Vec<PathBuf>, last_search:Option<Instant>, last_report:Option<Instant>,',`    search:Vec<PathBuf>, last_search:Option<Instant>, last_report:Option<Instant>,
    search_failed:bool, search_roots:BTreeSet<PathBuf>, search_ids:BTreeSet<String>,
    legacy_initialized:bool, legacy_candidates:BTreeMap<String,(u64,String)>,`);
const before=load(w);const a=before.indexOf('    fn discover(');const z=before.indexOf('    pub fn tick(',a);
if(a<0||z<a)throw Error('discover region');
files.set(w,before.slice(0,a)+String.raw`    fn discover(&mut self,roots:&BTreeSet<PathBuf>,ids:&BTreeSet<String>)->bool{
        let now=Instant::now();
        if self.search_roots!=*roots || self.search_ids!=*ids {
            self.search.clear();self.last_search=None;self.search_roots=roots.clone();self.search_ids=ids.clone();
        }
        if self.search.is_empty()&&self.last_search.is_none_or(|t|now.duration_since(t)>=Duration::from_secs(60)){
            self.search_failed=roots.is_empty();
            for r in roots{self.search.push(r.join("sessions"));self.search.push(r.join("archived_sessions"));}
            self.last_search=Some(now);
        }
        let mut count=0;
        while count<64 {
            let Some(dir)=self.search.pop()else{break;};count+=1;
            let entries=match fs::read_dir(&dir){
                Ok(e)=>e,
                Err(e) if e.kind()==std::io::ErrorKind::NotFound && roots.iter().any(|r|dir==r.join("archived_sessions"))=>continue,
                Err(_)=>{self.search_failed=true;continue;}
            };
            for entry in entries{
                let e=match entry{Ok(e)=>e,Err(_)=>{self.search_failed=true;continue;}};
                let t=match e.file_type(){Ok(t)=>t,Err(_)=>{self.search_failed=true;continue;}};
                if t.is_symlink(){self.search_failed=true;continue;}
                if t.is_dir(){
                    if roots.iter().any(|r|e.path().strip_prefix(r).ok().is_some_and(|p|p.components().count()<=5)){self.search.push(e.path());}
                    else{self.search_failed=true;}
                }else if t.is_file(){
                    let name=e.file_name();let name=name.to_string_lossy();
                    if !name.ends_with(".jsonl"){continue;}
                    for sid in ids {
                        if name.ends_with(&format!("-{sid}.jsonl")) {
                            if let Some(path)=allowed(&e.path(),roots){self.discovered.insert(sid.clone(),path);}
                            else{self.search_failed=true;}
                        }
                    }
                }
            }
        }
        self.search.is_empty()&&!self.search_failed&&!roots.is_empty()
    }
`+before.slice(z));
edit(w,'        self.discover(&b.roots,&ids);\n        enum Action{Journal(LocalSession,Evidence),Internal(LocalSession),Exit(LocalSession,Owner)}',`        if !self.legacy_initialized {
            self.legacy_initialized=true;
            self.legacy_candidates=local.sessions.values().filter(|s|legacy::candidate(s,b.sessions.contains_key(&s.id)))
                .map(|s|(s.id.clone(),(s.version,s.turn.clone()))).collect();
        }
        let discovery_complete=self.discover(&b.roots,&ids);
        enum Action{Journal(LocalSession,Evidence),Internal(LocalSession),Exit(LocalSession,Owner),Unverified(LocalSession)}`);
edit(w,'            w.status="unverified";\n            if let Some(path)=path {',`            w.status=if discovery_complete{"journal_missing"}else{"discovery_incomplete"};
            let has_path=path.is_some();
            if let Some(path)=path {`);
edit(w,'                    Ok(true)=>{\n                        if let Some(ev)=w.cursor.last.as_ref(){',`                    Ok(true)=>{
                        w.status="journal_no_lifecycle";
                        if let Some(ev)=w.cursor.last.as_ref(){`);
edit(w,'                    Presence::Alive=>{w.dead_since=None;if w.status=="unverified"{w.status="owner_alive";}}',`                    Presence::Alive=>{w.dead_since=None;if !has_path{w.status="owner_alive";}}`);
edit(w,`                    Presence::Unknown=>w.dead_since=None,
                }
            }
        }
        store::update(|s|{`,`                    Presence::Unknown=>w.dead_since=None,
                }
            }
            let initial=self.legacy_candidates.get(&session.id).is_some_and(|(v,t)|*v==session.version&&t==&session.turn);
            if !session.unverified_legacy && initial && discovery_complete && !has_path && w.owner.is_none() && binding.is_none() {
                actions.push(Action::Unverified(session.clone()));
            }
            if session.unverified_legacy{w.status="legacy_unverified";}
        }
        store::update(|s|{`);
edit(w,'                    Action::Journal(old,e)=>lifecycle::reconcile(s,&old,&e,now,heuristic),',`                    Action::Journal(old,e)=>{
                        let changed=lifecycle::reconcile(s,&old,&e,now,heuristic);
                        changed || (old.unverified_legacy&&e.at<=now.saturating_add(5000)&&e.turn==old.turn&&legacy::set(s,&old,false))
                    },
                    Action::Unverified(old)=>{
                        // Recheck metadata under the state lock: a live hook may
                        // have supplied a binding during the directory scan.
                        !latest_tracking.sessions.contains_key(&old.id)&&legacy::set(s,&old,true)
                    },`);
edit(w,'            let rows:Vec<_>=self.watches.iter().map(|(id,w)|serde_json::json!({"session_id":id,"evidence":w.status})).collect();',`            let observed=store::read_state()?;
            let rows:Vec<_>=self.watches.iter().filter_map(|(id,w)|observed.sessions.get(id).map(|s|{
                let unknown=s.unverified_legacy;
                serde_json::json!({"session_id":id,"evidence":if unknown{"legacy_unverified"}else{w.status},
                    "reason":if unknown{Some("pre_observer_start_without_tracking_or_journal")}else{None},
                    "included_in_output":!unknown,"recorded_state":s.state,"recorded_event":s.event,
                    "discovery_complete":discovery_complete,"tracking_present":b.sessions.contains_key(id),
                    "owner_pid":w.owner.as_ref().map(|o|o.pid),"journal_turn_id":w.cursor.last.as_ref().map(|e|&e.turn)})
            })).collect();`);
append(w,String.raw`
#[cfg(test)]mod legacy_watch_tests{
    use super::*;
    fn tmp()->PathBuf{let p=std::env::temp_dir().join(format!("ailight-legacy-test-{}",uuid::Uuid::new_v4()));fs::create_dir_all(&p).unwrap();p}
    #[test]fn real_subagent_is_not_internal(){let dir=tmp();let p=dir.join("x.jsonl");fs::write(&p,"{\"type\":\"session_meta\",\"payload\":{\"id\":\"child\",\"source\":{\"subagent\":{\"thread_spawn\":{\"parent_thread_id\":\"main\",\"depth\":1}}}}}\n").unwrap();let mut c=Cursor::default();assert!(c.poll(&p,"child").unwrap());assert!(!c.internal);fs::remove_dir_all(dir).unwrap();}
    #[test]fn unknown_subagent_is_not_internal(){let dir=tmp();let p=dir.join("x.jsonl");for source in [serde_json::json!("subagent"),serde_json::json!({"subagent":"unknown"})]{fs::write(&p,format!("{}\n",serde_json::json!({"type":"session_meta","payload":{"id":"s","source":source}}))).unwrap();let mut c=Cursor::default();assert!(c.poll(&p,"s").unwrap());assert!(!c.internal);}fs::remove_dir_all(dir).unwrap();}
    #[test]fn vscode_main_is_not_internal(){let dir=tmp();let p=dir.join("x.jsonl");fs::write(&p,"{\"type\":\"session_meta\",\"payload\":{\"id\":\"s\",\"cli_version\":\"0.157.1\",\"source\":\"vscode\"}}\n").unwrap();let mut c=Cursor::default();assert!(c.poll(&p,"s").unwrap());assert!(!c.internal);fs::remove_dir_all(dir).unwrap();}
    #[test]fn discovery_missing_required_root_is_not_absence(){let dir=tmp();let mut o=Observer::default();assert!(!o.discover(&BTreeSet::from([dir.clone()]),&BTreeSet::from(["s".into()])));fs::remove_dir_all(dir).unwrap();}
    #[test]fn discovery_clean_search_and_optional_archive(){let dir=tmp();fs::create_dir(dir.join("sessions")).unwrap();let mut o=Observer::default();assert!(o.discover(&BTreeSet::from([dir.clone()]),&BTreeSet::from(["s".into()])));assert!(o.discovered.is_empty());fs::remove_dir_all(dir).unwrap();}
    #[test]fn discovery_finds_matching_only(){let dir=tmp();let nested=dir.join("sessions/2026/09/27");fs::create_dir_all(&nested).unwrap();fs::write(nested.join("rollout-t-s.jsonl"),"{}").unwrap();fs::write(nested.join("rollout-t-other.jsonl"),"{}").unwrap();let mut o=Observer::default();assert!(o.discover(&BTreeSet::from([dir.clone()]),&BTreeSet::from(["s".into()])));assert_eq!(o.discovered.len(),1);assert!(o.discovered.contains_key("s"));fs::remove_dir_all(dir).unwrap();}
}
`);
const u='ui/sessions-ui.js';
edit(u,'export function counts(sessions = []) {',`export const isUnverified = s => typeof s?.event === 'string' && s.event.startsWith('UnverifiedLegacy:');
export function counts(sessions = []) {`);
edit(u,'    if (Object.hasOwn(c, s.state)) c[s.state]++;','    if (isUnverified(s)) c.unknown++;\n    else if (Object.hasOwn(c, s.state)) c[s.state]++;');
edit(u,'未参与 ${c.off + c.unknown}`;','未参与 ${c.off} · 未核实 ${c.unknown}`;');
edit(u,'  const names = { working:',`  if (isUnverified(s)) return '未核实历史记录 · 原事件 ' + s.event.slice('UnverifiedLegacy:'.length) + ' · 不计入当前灯态；未认定完成';
  const names = { working:`);
edit(u,'  return `${sessions.length} 条状态记录；记录数不等于运行任务数`;',"  const n = counts(sessions).unknown;\n  return n ? sessions.length + ' 条记录；另有 ' + n + ' 条未核实，灯光仅代表已纳入任务' : sessions.length + ' 条状态记录；记录数不等于运行任务数';");
edit(u,'    node.append(details);',`    node.append(details);
    const unknown = counts(records).unknown;
    if (unknown) {
      const warning = document.createElement('p'); warning.className = 'empty';
      warning.textContent = '存在未核实历史记录：不计入当前灯态，但不代表已完成。新 hook 或匹配的回合日志会恢复追踪。';
      node.append(warning);
    }`);
add('tests/ui-unverified.test.mjs',String.raw`import test from 'node:test';
import assert from 'node:assert/strict';
import { counts, summary, caption, detail, isUnverified } from '../ui/sessions-ui.js';
const old={state:'off',event:'UnverifiedLegacy:UserPromptSubmit'};
test('unknown projection not counted as working or completed',()=>{assert.equal(counts([old]).unknown,1);assert.equal(counts([old]).off,0);assert.equal(counts([old]).done,0);});
test('mixed real work and unknown',()=>{const x=counts([old,{state:'working'}]);assert.equal(x.working,1);assert.equal(x.unknown,1);});
test('summary separates unknown',()=>assert.match(summary([old]),/未核实 1/));
test('detail explicitly does not imply completion',()=>assert.match(detail(old,1),/未认定完成/));
test('caption warns scope of light output',()=>assert.match(caption([old]),/灯光仅代表已纳入任务/));
test('ordinary records unchanged',()=>{assert.equal(isUnverified({event:'PostToolUse'}),false);assert.equal(counts([{state:'done'}]).done,1);});
`);
edit('Cargo.toml','version = "0.3.4"','version = "0.3.5"');
for(const n of ['ai-light','light-agent','light-core']) edit('Cargo.lock',`name = "${n}"\nversion = "0.3.4"`,`name = "${n}"\nversion = "0.3.5"`);
edit('src-tauri/tauri.conf.json','"version": "0.3.4"','"version": "0.3.5"');
edit('ui/index.html','id="version">v0.3.4<','id="version">v0.3.5<');
edit('CHANGELOG.md','# Changelog\n','# Changelog\n\n## 0.3.5 — Honest unverified legacy records\n\n- Quarantine only pre-observer, unbound UserPromptSubmit/Working records after an error-free search of registered roots finds no journal. Preserve original state and timestamps; exclusion is not task success. Fresh accepted hooks or matching journal evidence restore participation.\n- Unknown legacy records no longer indefinitely override real completion. UI explicitly shows unverified count and limits the meaning of green to participating tasks.\n- Never filter all subagents as internal: only an explicitly identified thread_title helper is removed. Already registered real/unknown subagents remain.\n- Add filesystem, state-race, projection and UI regression tests. No process interruption, state clearing, new hook registration or dependency upgrades.\n');
add('docs/UNVERIFIED-LEGACY.md',`# 0.3.5：未核实旧记录不等于正在工作\n\n## 适用问题\n\n0.3.4 会一直保留只有旧 UserPromptSubmit、没有 tracking、没有匹配日志的 working。Windows 因此可能长期黄灯。没有日志不证明任务已完成，也不能证明它是标题生成器。\n\n## 新规则\n\n观察器启动时登记迁移候选；只针对缺少 tracking 的旧 Working/UserPromptSubmit、没有 run_id 的记录。在所有已登记根目录完成无错误搜索、没有日志路径、也没有关联进程时，把候选单列为 legacy_unverified。权限错误、搜索尚未结束、根目录不可用、已有绑定或日志时不执行此降级。没有按“几分钟没活动”结束真实任务的超时规则。\n\n原 state/event/turn/changed_ms/touched_ms 保留，仅增加 unverified_legacy 投影标志；Windows 接收 state=off 和 UnverifiedLegacy:原事件。它不是 Done、不是取消、不触发完成蜂鸣。用户界面显示“未核实”，而不是把它混进工作计数。由于无法从丢失的证据恢复真相，绿灯只代表参与追踪的任务已完成，不代表这些旧记录也被证明完成。\n\n后续收到该会话有效的新 hook，或找到匹配回合生命周期记录，会自动恢复。状态锁内重查版本、回合及 tracking，避免扫描期间覆盖真实任务的新事件。已有绑定的长时间推理不会降级。只识别 thread_title 为内部任务，不把所有子代理一概删除；现有主任务 hooks 仍按父会话聚合，不承诺额外发现全部未登记子代理。\n\n## 升级\n\n使用运行 Codex 的同一用户：解压本版 Ubuntu 包，运行 ./light-agent upgrade。只重启 AI Light relay，不清空 state，不改 Codex 配置或 hooks，不终止 Codex；已有任务继续运行。\n\nWindows 升级到本版可看到明确的“未核实”分类。旧 Windows 0.3.2—0.3.4 会把它显示成不参与或空闲，但不会再被这条旧 working 卡住。\n\n检查：~/.local/bin/light-agent lifecycle-status。legacy_unverified 的 reason 应为 pre_observer_start_without_tracking_or_journal。没有找到身份就不会称其为“子代理”或“已完成”。\n`);
append('docs/SESSION-LIFECYCLE.md','\n\n## 0.3.5 补充：无证据旧记录\n\n旧版“未核实仍参与灯态”的规则在仅有旧 UserPromptSubmit 且从未绑定的迁移记录上不再适用。见 [未核实旧记录](UNVERIFIED-LEGACY.md)：单列未知、保留原始状态、允许恢复；绝不把无日志推断为完成。\n');
for(const [p,s] of files){fs.mkdirSync(path.dirname(p),{recursive:true});fs.writeFileSync(p,s);}
console.log(`Prepared ${files.size} source files; no dependencies changed.`);
