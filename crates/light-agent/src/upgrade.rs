//! Upgrade only the relay executable. Never clear sessions, touch Codex hooks,
//! restart Codex, or modify its model/provider settings.
use crate::{store,watch};
use anyhow::{Context,Result};
use std::{fs,io::Write,process::Command};
pub fn run(no_service:bool)->Result<()> {
    anyhow::ensure!(cfg!(target_os="linux"),"run upgrade inside Linux as the Codex user");
    store::load_config()?;
    let source=std::env::current_exe()?;
    let dest=store::home()?.join(".local/bin/light-agent");
    anyhow::ensure!(dest.is_file(),"install the agent before using upgrade");
    watch::remember_root()?;
    if fs::canonicalize(&source)?!=fs::canonicalize(&dest)? {
        let dir=dest.parent().context("missing binary directory")?;
        let temp=dir.join(format!(".light-agent-{}",uuid::Uuid::new_v4()));
        let result=(||->Result<()>{
            let mut options=fs::OpenOptions::new();options.write(true).create_new(true);
            #[cfg(unix)] {use std::os::unix::fs::OpenOptionsExt;options.mode(0o755);}
            let mut out=options.open(&temp)?;
            out.write_all(&fs::read(&source)?)?;out.sync_all()?;drop(out);
            #[cfg(unix)] {use std::os::unix::fs::PermissionsExt;fs::set_permissions(&temp,fs::Permissions::from_mode(0o755))?;}
            fs::rename(&temp,&dest)?;
            #[cfg(unix)] fs::File::open(dir)?.sync_all()?;
            Ok(())
        })();
        if result.is_err(){let _=fs::remove_file(&temp);}result?;
    }
    if !no_service {
        let status=Command::new("systemctl").args(["--user","restart","ai-light-relay.service"]).status()?;
        anyhow::ensure!(status.success(),"binary upgraded; relay restart failed (Codex and state were left untouched)");
    }
    println!("Agent upgraded atomically. Codex tasks, hooks, client settings and state.json were preserved.");
    if no_service{println!("Restart only your existing light-agent relay using its current process manager.");}
    println!("Existing sessions are reconciled from matching lifecycle evidence; missing evidence is not treated as completion.");
    Ok(())
}
