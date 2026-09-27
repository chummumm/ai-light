mod install;
mod relay;
mod store;
mod lifecycle;
mod legacy;
mod owner;
mod watch;
mod upgrade;
use anyhow::{Context, Result};
use light_core::{hooks::LocalState, model::LightState, now_ms};
use std::{io::Read, path::Path, process::Command};

fn main(){
    let args:Vec<String>=std::env::args().skip(1).collect();
    if args.first().map(String::as_str)==Some("hook") {
        if let Err(e)=hook(){store::log_issue(&format!("hook not recorded: {e}"));}
        println!("{{}}");return;
    }
    if args.first().map(String::as_str)==Some("notify") {
        if let Err(e)=notify(args.get(1).map(String::as_str)){store::log_issue(&format!("notify not recorded: {e}"));}
        return;
    }
    if let Err(e)=run(&args){eprintln!("AI Light: {e:#}");std::process::exit(1);}
}
fn hook()->Result<()> {
    let mut data=Vec::new();std::io::stdin().lock().take(1024*1024+1).read_to_end(&mut data)?;
    anyhow::ensure!(data.len()<=1024*1024,"hook input exceeded 1 MiB limit");
    let value:serde_json::Value=serde_json::from_slice(&data)?;
    let cfg=store::load_config()?;
    let run=std::env::var("AILIGHT_RUN_ID").unwrap_or_default();
    watch::record(&value,now_ms(),cfg.question_heuristic,&run)
}
fn notify(raw:Option<&str>)->Result<()> {
    let raw=raw.context("missing Codex notify JSON payload")?;
    anyhow::ensure!(raw.len()<=1024*1024,"notify input exceeded 1 MiB limit");
    let value:serde_json::Value=serde_json::from_str(raw)?;
    let cfg=store::load_config()?;
    watch::notify(&value,now_ms(),cfg.question_heuristic)
}
fn parse_state(s:&str)->Result<LightState>{
    match s {"working"=>Ok(LightState::Working),"waiting"=>Ok(LightState::Waiting),"done"=>Ok(LightState::Done),"error"=>Ok(LightState::Error),"off"=>Ok(LightState::Off),_=>anyhow::bail!("expected working|waiting|done|error|off")}
}
fn run(args:&[String])->Result<()> {
    match args.first().map(String::as_str) {
        Some("install")=>{
            let index=args.iter().position(|s|s=="--client").context("usage: light-agent install --client ./client.json [--no-service]")?;
            let path=args.get(index+1).context("missing --client path")?;
            install::install(Path::new(path),!args.iter().any(|s|s=="--no-service"))?;
            watch::remember_root()
        }
        Some("upgrade")=>upgrade::run(args.iter().any(|s|s=="--no-service")),
        Some("uninstall")=>install::uninstall(),
        Some("relay")=>relay::run(),
        Some("check")=>relay::check(),
        Some("emit")=>{
            let state=parse_state(args.get(1).context("missing state")?)?;
            store::load_config()?;
            store::update(|s|{s.emit("manual-test",state,now_ms());Ok(true)})?;
            println!("Local test state updated. Active manual tests expire after 60 seconds; real tasks are never expired by silence.");Ok(())
        }
        Some("clear")=>{store::update(|s|{s.clear();Ok(true)})?;println!("Local sessions cleared.");Ok(())}
        Some("status")=>{let c=store::load_config()?;println!("{}",serde_json::to_string_pretty(&store::read_state()?.snapshot(&c.source_id,now_ms()))?);Ok(())}
        Some("lifecycle-status")=>{
            let bytes=std::fs::read(store::state_dir()?.join("lifecycle-status.json")).context("lifecycle report not yet available")?;
            let v:serde_json::Value=serde_json::from_slice(&bytes)?;println!("{}",serde_json::to_string_pretty(&v)?);Ok(())
        }
        Some("codex")=>{
            store::load_config()?;
            let run_id=uuid::Uuid::new_v4().to_string();ctrlc::set_handler(||{})?;
            let pass=&args[1..];let pass=if pass.first().map(String::as_str)==Some("--"){&pass[1..]}else{pass};
            let status=Command::new("codex").args(pass).env("AILIGHT_RUN_ID",&run_id).status().context("launch codex from PATH")?;
            let code=status.code().unwrap_or({
                #[cfg(unix)] {use std::os::unix::process::ExitStatusExt;128+status.signal().unwrap_or(1)}
                #[cfg(not(unix))] {1}
            });
            if code!=0 && code!=130 {let _=store::update(|s:&mut LocalState|{s.mark_process_error(&run_id,now_ms());Ok(true)});}
            std::process::exit(code);
        }
        _=>{
            println!("AI Light Rust agent {}\n\n  install --client PATH [--no-service]\n  upgrade [--no-service] (preserves running Codex tasks and state)\n  relay\n  hook\n  notify JSON\n  check\n  emit working|waiting|done|error|off\n  status\n  lifecycle-status\n  clear\n  codex -- [arguments]\n  uninstall",env!("CARGO_PKG_VERSION"));Ok(())
        }
    }
}
