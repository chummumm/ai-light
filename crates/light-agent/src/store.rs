use anyhow::{bail, Context, Result};
use fs2::FileExt;
use light_core::{fsutil::atomic_json, hooks::LocalState, now_ms};
use serde::{Deserialize, Serialize};
use std::{fs::{self, File, OpenOptions}, path::PathBuf, time::{Duration,Instant}};

#[derive(Clone, Serialize, Deserialize)]
pub struct Config {
    pub url: String,
    pub token: String,
    pub source_id: String,
    #[serde(default="yes")] pub question_heuristic: bool,
}
fn yes()->bool{true}
pub fn home()->Result<PathBuf>{Ok(PathBuf::from(std::env::var_os("HOME").context("HOME is not set")?))}
pub fn config_dir()->Result<PathBuf>{Ok(std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or(home()?.join(".config")).join("ai-light"))}
pub fn state_dir()->Result<PathBuf>{Ok(std::env::var_os("XDG_STATE_HOME").map(PathBuf::from).unwrap_or(home()?.join(".local/state")).join("ai-light"))}
pub fn private_dir(dir:&std::path::Path)->Result<()> {
    fs::create_dir_all(dir)?;
    #[cfg(unix)] { use std::os::unix::fs::PermissionsExt; fs::set_permissions(dir,fs::Permissions::from_mode(0o700))?; }
    Ok(())
}
pub fn load_config()->Result<Config>{
    let path=config_dir()?.join("client.json");
    let cfg:Config=serde_json::from_slice(&fs::read(&path).with_context(||format!("read {}; run install first",path.display()))?)?;
    cfg.validate()?; Ok(cfg)
}
impl Config {
    pub fn validate(&self)->Result<()> {
        let u=reqwest::Url::parse(&self.url)?;
        anyhow::ensure!(matches!(u.scheme(),"http"|"https") && u.host_str().is_some(),"client URL must be http(s)");
        anyhow::ensure!(u.username().is_empty() && u.password().is_none() && u.query().is_none() && u.fragment().is_none(),"invalid URL credentials or suffix");
        anyhow::ensure!(u.path()=="/" || u.path().is_empty(),"use a base URL without a path");
        anyhow::ensure!(self.token.len()>=32 && self.token.len()<=256 && self.token.bytes().all(|b|b.is_ascii_hexdigit()),"invalid token");
        anyhow::ensure!(!self.source_id.is_empty() && self.source_id.len()<=128 && !self.source_id.chars().any(char::is_control),"invalid source id");
        Ok(())
    }
}
fn locked_file(name:&str, wait:Duration)->Result<File>{
    let dir=state_dir()?; private_dir(&dir)?;
    let mut opts=OpenOptions::new();opts.create(true).read(true).write(true).truncate(false);
    #[cfg(unix)] {use std::os::unix::fs::OpenOptionsExt;opts.mode(0o600);}
    let f=opts.open(dir.join(name))?;
    let start=Instant::now();
    loop {
        match f.try_lock_exclusive() {
            Ok(())=>return Ok(f),
            Err(e) if e.kind()==std::io::ErrorKind::WouldBlock => {
                if start.elapsed()>=wait {bail!("state busy");}
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(e)=>return Err(e.into()),
        }
    }
}
pub fn relay_lock()->Result<File>{locked_file("relay.lock",Duration::ZERO)}
pub fn update<F>(f:F)->Result<()> where F:FnOnce(&mut LocalState)->Result<bool> {
    let _lock=locked_file("state.lock",Duration::from_millis(200))?;
    let path=state_dir()?.join("state.json");
    let mut s=read_state()?;
    if f(&mut s)? {atomic_json(&path,&s)?;}
    Ok(())
}
pub fn read_state()->Result<LocalState>{
    let path=state_dir()?.join("state.json");
    match fs::read(&path) {
        Ok(bytes)=>serde_json::from_slice(&bytes).context("invalid state.json (not overwritten; inspect or back it up before resetting)"),
        Err(e) if e.kind()==std::io::ErrorKind::NotFound => Ok(LocalState::default()),
        Err(e)=>Err(e.into()),
    }
}
pub fn initialize_state()->Result<()> {
    let _lock=locked_file("state.lock",Duration::from_secs(1))?;
    let path=state_dir()?.join("state.json");
    if !path.exists() {atomic_json(&path,&LocalState::default())?;}
    Ok(())
}
pub fn log_issue(message:&str) {
    if let Ok(dir)=state_dir() {
        let _=private_dir(&dir);
        let path=dir.join("hook-errors.log");
        if fs::metadata(&path).map(|m|m.len()>128*1024).unwrap_or(false) {let _=fs::rename(&path,dir.join("hook-errors.previous.log"));}
        use std::io::Write;
        if let Ok(mut f)=OpenOptions::new().create(true).append(true).open(path) {let _=writeln!(f,"{} {}",now_ms(),message);}
    }
}
