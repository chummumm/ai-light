use crate::store::{self, Config};
use anyhow::{Context, Result};
use light_core::{fsutil::{atomic_bytes, atomic_json}, hooks::HOOK_EVENTS, now_ms};
use serde_json::{json, Value};
use std::{fs, path::{Path,PathBuf}, process::Command};

fn bin_path()->Result<PathBuf>{Ok(store::home()?.join(".local/bin/light-agent"))}
fn codex_home()->Result<PathBuf>{Ok(std::env::var_os("CODEX_HOME").map(PathBuf::from).unwrap_or(store::home()?.join(".codex")))}
fn shell_quote(s:&str)->String{format!("'{}'",s.replace('\'',"'\"'\"'"))}
fn command_line()->Result<String>{Ok(format!("{} hook",shell_quote(&bin_path()?.to_string_lossy())))}
fn notify_line()->Result<String>{
    let path=serde_json::to_string(&bin_path()?.to_string_lossy().into_owned())?;
    Ok(format!("notify = [{path}, \"notify\"]"))
}
#[derive(Debug,Clone,Copy,PartialEq,Eq)]
enum NotifyMerge { Installed, AlreadyInstalled, PreservedExisting, Removed, Absent }
fn root_notify_present(text:&str)->bool{
    for line in text.trim_start_matches('\u{feff}').lines(){
        let t=line.trim_start();
        if t.starts_with('['){break;}
        let code=t.split('#').next().unwrap_or("").trim();
        if let Some((key,_))=code.split_once('='){
            let key=key.trim().trim_matches('"').trim_matches('\'');
            if key=="notify"{return true;}
        }
    }
    false
}
fn merge_notify_text(text:&str, ours:&str, remove:bool)->(String,NotifyMerge){
    let (bom,body)=if let Some(rest)=text.strip_prefix('\u{feff}'){("\u{feff}",rest)}else{("",text)};
    let prefix=format!("{ours}\n");
    if remove {
        if body.starts_with(&prefix){return (format!("{bom}{}", &body[prefix.len()..]),NotifyMerge::Removed);}
        if body.trim()==ours{return (bom.to_owned(),NotifyMerge::Removed);}
        return (text.to_owned(),NotifyMerge::Absent);
    }
    if body.starts_with(&prefix)||body.trim()==ours{return (text.to_owned(),NotifyMerge::AlreadyInstalled);}
    if root_notify_present(body){return (text.to_owned(),NotifyMerge::PreservedExisting);}
    (format!("{bom}{ours}\n{body}"),NotifyMerge::Installed)
}
fn merge_notify(path:&Path,remove:bool)->Result<NotifyMerge>{
    let existed=path.exists();
    let text=if existed{fs::read_to_string(path).with_context(||format!("read {}",path.display()))?}else{String::new()};
    let ours=notify_line()?;
    let (next,state)=merge_notify_text(&text,&ours,remove);
    if next!=text{
        if existed{
            let backup=path.with_file_name(format!("config.toml.ai-light-backup-{}",now_ms()));
            fs::copy(path,&backup)?;
            println!("Existing Codex config backed up to {}",backup.display());
        }
        atomic_bytes(path,next.as_bytes())?;
    }
    Ok(state)
}
fn systemd_quote(s:&str)->String{format!("\"{}\"",s.replace('\\',"\\\\").replace('"',"\\\"").replace('%',"%%").replace('$',"$$"))}
fn merge_hooks(path:&Path, remove:bool)->Result<()> {
    let mut root:Value=if path.exists(){serde_json::from_slice(&fs::read(path)?).context("existing hooks.json is invalid; left untouched")?}else{json!({"hooks":{}})};
    let object=root.as_object_mut().context("hooks.json root must be an object")?;
    let hooks=object.entry("hooks").or_insert_with(||json!({})).as_object_mut().context("hooks must be an object")?;
    let our_command=command_line()?;
    for event in HOOK_EVENTS {
        let groups=hooks.entry((*event).to_string()).or_insert_with(||json!([])).as_array_mut().context("hook event must be an array")?;
        for group in groups.iter_mut() {
            if let Some(items)=group.get_mut("hooks").and_then(Value::as_array_mut) {
                items.retain(|item| item.get("command").and_then(Value::as_str)!=Some(our_command.as_str()));
            }
        }
        groups.retain(|group|!group.get("hooks").and_then(Value::as_array).is_some_and(|x|x.is_empty()));
        if !remove {groups.push(json!({"hooks":[{"type":"command","command":our_command,"timeout":2}]}));}
    }
    if path.exists(){
        let backup=path.with_file_name(format!("hooks.json.ai-light-backup-{}",now_ms()));
        fs::copy(path,&backup)?;
        println!("Existing hook configuration backed up to {}",backup.display());
    }
    atomic_json(path,&root)?;
    Ok(())
}
pub fn install(client:&Path, service:bool)->Result<()> {
    anyhow::ensure!(cfg!(target_os="linux"),"run this command inside Ubuntu/Linux");
    let imported:Config=serde_json::from_slice(&fs::read(client).context("read exported client.json")?)?;
    imported.validate()?;
    let config_dir=store::config_dir()?;store::private_dir(&config_dir)?;
    store::private_dir(&store::state_dir()?)?;
    let mut cfg=imported;
    if let Ok(previous)=store::load_config(){cfg.source_id=previous.source_id;}
    atomic_json(&config_dir.join("client.json"),&cfg)?;
    store::initialize_state()?;
    let bin=bin_path()?;fs::create_dir_all(bin.parent().context("missing bin parent")?)?;
    let current=std::env::current_exe()?;
    if fs::canonicalize(&bin).ok()!=fs::canonicalize(&current).ok() {atomic_bytes(&bin,&fs::read(current)?)?;}
    #[cfg(unix)]{use std::os::unix::fs::PermissionsExt;fs::set_permissions(&bin,fs::Permissions::from_mode(0o755))?;}
    let hookdir=codex_home()?;fs::create_dir_all(&hookdir)?;
    merge_hooks(&hookdir.join("hooks.json"),false)?;
    match merge_notify(&hookdir.join("config.toml"),false)? {
        NotifyMerge::Installed=>println!("Installed Codex agent-turn-complete notify fallback."),
        NotifyMerge::AlreadyInstalled=>println!("Codex completion notify fallback is already installed."),
        NotifyMerge::PreservedExisting=>println!("Existing Codex notify setting left unchanged; Stop hooks still work, but codex exec completion fallback was not installed."),
        _=>{}
    }
    let unit_dir=store::home()?.join(".config/systemd/user");fs::create_dir_all(&unit_dir)?;
    let unit=format!("[Unit]\nDescription=AI Light Rust state relay\nAfter=network.target\n\n[Service]\nType=simple\nExecStart={} relay\nRestart=on-failure\nRestartSec=3\nUMask=0077\nNoNewPrivileges=true\n\n[Install]\nWantedBy=default.target\n",systemd_quote(&bin.to_string_lossy()));
    atomic_bytes(&unit_dir.join("ai-light-relay.service"),unit.as_bytes())?;
    if service {
        for args in [&["--user","daemon-reload"][..],&["--user","enable","--now","ai-light-relay.service"][..],&["--user","restart","ai-light-relay.service"][..]] {
            let status=Command::new("systemctl").args(args).status().context("systemctl unavailable; run install with --no-service and start light-agent relay manually")?;
            anyhow::ensure!(status.success(),"systemctl failed; files are installed, but check the user systemd session");
        }
    }
    println!("Installed: {}",bin.display());
    println!("Restart Codex, open /hooks, and review/trust the AI Light command hooks.");
    println!("No provider/model/AGENTS.md settings were modified. Do not bypass hook trust.");
    println!("Check: {} check",bin.display());
    Ok(())
}
pub fn uninstall()->Result<()> {
    anyhow::ensure!(cfg!(target_os="linux"),"run this command in Linux");
    let _=Command::new("systemctl").args(["--user","disable","--now","ai-light-relay.service"]).status();
    let codex=codex_home()?;
    let hooks=codex.join("hooks.json");if hooks.exists(){merge_hooks(&hooks,true)?;}
    let notify=codex.join("config.toml");if notify.exists(){let _=merge_notify(&notify,true);}
    let _=fs::remove_file(store::home()?.join(".config/systemd/user/ai-light-relay.service"));
    let _=Command::new("systemctl").args(["--user","daemon-reload"]).status();
    let _=fs::remove_file(bin_path()?);
    println!("Removed own service/hooks/notify fallback/binary. Configuration and state were retained.");
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn quotes_spaces_and_apostrophes(){assert_eq!(shell_quote("a b'c"),"'a b'\"'\"'c'");}
    #[test] fn escapes_systemd_specifiers(){assert_eq!(systemd_quote("/home/a%b/$x"),"\"/home/a%%b/$x\"");}
    #[test] fn notify_added_without_reformatting_existing_config(){
        let original="# keep me\nmodel = \"gpt-test\"\n\n[features]\ncodex_hooks = true\n";
        let ours="notify = [\"/home/u/.local/bin/light-agent\", \"notify\"]";
        let (next,state)=merge_notify_text(original,ours,false);
        assert_eq!(state,NotifyMerge::Installed);
        assert!(next.starts_with(&format!("{ours}\n# keep me\n")));
        assert!(next.contains("[features]\ncodex_hooks = true"));
    }
    #[test] fn existing_notify_is_preserved(){
        let original="notify = [\"other-notifier\"]\nmodel = \"x\"\n";
        let ours="notify = [\"/home/u/.local/bin/light-agent\", \"notify\"]";
        let (next,state)=merge_notify_text(original,ours,false);
        assert_eq!(state,NotifyMerge::PreservedExisting);assert_eq!(next,original);
    }
    #[test] fn uninstall_removes_only_our_prepended_notify(){
        let ours="notify = [\"/home/u/.local/bin/light-agent\", \"notify\"]";
        let text=format!("{ours}\n# comment\nmodel = \"x\"\n");
        let (next,state)=merge_notify_text(&text,ours,true);
        assert_eq!(state,NotifyMerge::Removed);assert_eq!(next,"# comment\nmodel = \"x\"\n");
        let other="notify = [\"other\"]\n";
        let (next,state)=merge_notify_text(other,ours,true);
        assert_eq!(state,NotifyMerge::Absent);assert_eq!(next,other);
    }
}
