use crate::store::{self, Config};
use anyhow::{Context, Result};
use light_core::{fsutil::{atomic_bytes, atomic_json}, hooks::HOOK_EVENTS, now_ms};
use serde_json::{json, Value};
use std::{fs, path::{Path,PathBuf}, process::Command};

fn bin_path()->Result<PathBuf>{Ok(store::home()?.join(".local/bin/light-agent"))}
fn codex_home()->Result<PathBuf>{Ok(std::env::var_os("CODEX_HOME").map(PathBuf::from).unwrap_or(store::home()?.join(".codex")))}
fn shell_quote(s:&str)->String{format!("'{}'",s.replace('\'',"'\"'\"'"))}
fn command_line()->Result<String>{Ok(format!("{} hook",shell_quote(&bin_path()?.to_string_lossy())))}
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
    let hooks=codex_home()?.join("hooks.json");if hooks.exists(){merge_hooks(&hooks,true)?;}
    let _=fs::remove_file(store::home()?.join(".config/systemd/user/ai-light-relay.service"));
    let _=Command::new("systemctl").args(["--user","daemon-reload"]).status();
    let _=fs::remove_file(bin_path()?);
    println!("Removed own service/hooks/binary. Configuration and state were retained.");
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn quotes_spaces_and_apostrophes(){assert_eq!(shell_quote("a b'c"),"'a b'\"'\"'c'");}
    #[test] fn escapes_systemd_specifiers(){assert_eq!(systemd_quote("/home/a%b/$x"),"\"/home/a%%b/$$x\"");}
}
