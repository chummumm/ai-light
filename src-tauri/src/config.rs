use anyhow::{ensure, Context, Result};
use light_core::{fsutil::atomic_json,protocol::DEFAULT_SERIAL};
use serde::{Deserialize,Serialize};
use std::{fs,net::IpAddr,path::{Path,PathBuf}};

#[derive(Debug,Clone,Serialize,Deserialize)]
#[serde(default)]
pub struct Config {
    pub serial:String,
    pub bind_host:String,
    pub port:u16,
    pub allowed_sources:Vec<String>,
    pub period_ms:u16,
    pub fade_ms:u16,
    pub done_seconds:u32,
    pub source_timeout_seconds:u32,
    pub battery_poll_seconds:u32,
    #[serde(alias="power_inference_enabled")]
    pub voltage_enabled:bool,
    pub sound:light_core::sound::SoundConfig,
    pub theme:String,
    pub reduce_motion:bool,
    pub exit_off:bool,
}
impl Default for Config {
    fn default()->Self{Self{
        serial:DEFAULT_SERIAL.into(),bind_host:"0.0.0.0".into(),port:17322,allowed_sources:vec![],
        period_ms:3000,fade_ms:1500,done_seconds:300,source_timeout_seconds:90,
        battery_poll_seconds:30,voltage_enabled:true,sound:light_core::sound::SoundConfig::default(),theme:"dark".into(),reduce_motion:false,exit_off:true,
    }}
}
impl Config {
    pub fn validate(&self)->Result<()> {
        ensure!(self.serial.is_empty() || (self.serial.len()==12 && self.serial.bytes().all(|b|b.is_ascii_hexdigit())),"设备序列号必须是 12 位十六进制蓝牙地址");
        let _:IpAddr=self.bind_host.parse().context("监听地址必须是 IP 地址")?;
        ensure!(self.port>=1024,"端口必须在 1024..65535 之间");
        ensure!(self.allowed_sources.len()<=16,"最多允许 16 个来源地址");
        for ip in &self.allowed_sources {let _:IpAddr=ip.parse().context("来源白名单包含无效 IP")?;}
        ensure!((60..=3600).contains(&self.done_seconds),"完成计时必须是 60..3600 秒");
        ensure!((30..=600).contains(&self.source_timeout_seconds),"来源超时必须是 30..600 秒");
        ensure!((15..=600).contains(&self.battery_poll_seconds),"电量轮询必须是 15..600 秒");
        ensure!(matches!(self.theme.as_str(),"dark"|"light"|"system"),"无效主题");
        light_core::protocol::report(light_core::model::LightState::Working,self.period_ms,self.fade_ms)?;
        self.sound.validate()?;
        Ok(())
    }
}
#[derive(Serialize,Deserialize)]struct Secrets{token:String}
pub fn data_dir()->Result<PathBuf>{
    Ok(PathBuf::from(std::env::var_os("LOCALAPPDATA").context("LOCALAPPDATA is not set; this desktop application requires Windows")?).join("AILight"))
}
pub fn load(dir:&Path)->Result<(Config,String,bool)> {
    fs::create_dir_all(dir)?;
    let config_path=dir.join("settings.json");let fresh=!config_path.exists();
    let cfg=if fresh{Config::default()}else{serde_json::from_slice(&fs::read(&config_path)?).context("settings.json 无效；原文件未覆盖")?};
    cfg.validate()?;
    let secret_path=dir.join("secrets.json");
    let secrets:Secrets=if secret_path.exists(){serde_json::from_slice(&fs::read(&secret_path)?).context("secrets.json 无效；原密钥未覆盖")?}else{
        Secrets{token:format!("{}{}",uuid::Uuid::new_v4().simple(),uuid::Uuid::new_v4().simple())}
    };
    ensure!(secrets.token.len()==64&&secrets.token.bytes().all(|b|b.is_ascii_hexdigit()),"Invalid local access token");
    if fresh{atomic_json(&config_path,&cfg)?;}
    if !secret_path.exists(){atomic_json(&secret_path,&secrets)?;}
    Ok((cfg,secrets.token,fresh))
}
