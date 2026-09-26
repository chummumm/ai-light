use crate::{config::Config,platform,worker::Reply};
use light_core::{battery::BatteryReading,model::{Aggregate,Engine,LightState,Snapshot},power::{PowerSample,MAX_GAP_MS},sound::{SoundEngine,SoundView,top_key}};
use serde::Serialize;
use std::{collections::VecDeque,path::PathBuf,sync::{atomic::{AtomicBool,Ordering},Mutex,MutexGuard},time::Instant};

#[derive(Debug,Clone,Serialize)]pub struct LogEntry{pub at_ms:u64,pub kind:String,pub message:String}
#[derive(Debug,Clone,Serialize,Default)]pub struct HardwareView {
    pub present:Option<bool>,pub device_name:Option<String>,pub api_success:Option<bool>,
    pub acknowledged:Option<bool>,pub last_checked_ms:Option<u64>,pub last_output_ms:Option<u64>,pub error:Option<String>,
}
#[derive(Debug,Clone,Serialize,Default)]pub struct BatteryView {
    pub reading:Option<BatteryReading>,pub updated_ms:Option<u64>,pub attempted_ms:Option<u64>,
    pub stale:bool,pub error:Option<String>,
}
#[derive(Debug,Default)]pub struct PowerData {
    pub reading:Option<PowerSample>,pub updated_ms:Option<u64>,pub attempted_ms:Option<u64>,pub error:Option<String>,
}
#[derive(Debug,Clone,Serialize)]pub struct PowerView {
    pub reading:Option<PowerSample>,pub updated_ms:Option<u64>,pub attempted_ms:Option<u64>,
    pub error:Option<String>,pub stale:bool,pub enabled:bool,
}
#[derive(Debug,Clone,Serialize)]pub struct ReceiverView{pub listening:bool,pub address:String,pub error:Option<String>}
#[derive(Debug,Clone,Serialize)]pub struct Preview{pub state:LightState,pub until_ms:u64}
#[derive(Debug,Serialize)]pub struct View {
    pub version:&'static str,pub now_ms:u64,pub config:Config,pub autostart:bool,
    pub aggregate:Aggregate,pub output:LightState,pub paused:bool,pub preview:Option<Preview>,
    pub hardware:HardwareView,pub battery:BatteryView,pub power:PowerView,pub sound:SoundView,pub receiver:ReceiverView,
    pub logs:Vec<LogEntry>,pub data_dir:String,
}
pub struct Data {
    pub config:Config,pub engine:Engine,pub paused:bool,pub preview:Option<Preview>,
    pub hardware:HardwareView,pub battery:BatteryView,pub power:PowerData,pub sound:SoundEngine,pub receiver:ReceiverView,
    pub logs:VecDeque<LogEntry>,
}
pub struct Shared {
    pub data:Mutex<Data>,pub dir:PathBuf,pub token:String,
    pub stop:AtomicBool,pub hardware_stopped:AtomicBool,pub sound_cancel:AtomicBool,
    pub force_output:AtomicBool,pub force_battery:AtomicBool,pub force_power:AtomicBool,
    start:Instant,anchor_ms:u64,
}
impl Shared {
    pub fn new(config:Config,dir:PathBuf,token:String)->Self {
        let address=format!("{}:{}",config.bind_host,config.port);let anchor_ms=light_core::now_ms();
        Self{data:Mutex::new(Data{config,engine:Engine::default(),paused:false,preview:None,
            hardware:HardwareView::default(),battery:BatteryView{stale:true,..BatteryView::default()},power:PowerData::default(),sound:SoundEngine::new(anchor_ms),
            receiver:ReceiverView{listening:false,address,error:None},logs:VecDeque::new()}),dir,token,
            stop:AtomicBool::new(false),hardware_stopped:AtomicBool::new(false),sound_cancel:AtomicBool::new(false),force_output:AtomicBool::new(false),
            force_battery:AtomicBool::new(false),force_power:AtomicBool::new(false),start:Instant::now(),anchor_ms}
    }
    pub fn now(&self)->u64{self.anchor_ms.saturating_add(self.start.elapsed().as_millis().min(u64::MAX as u128) as u64)}
    pub fn lock(&self)->MutexGuard<'_,Data>{self.data.lock().unwrap_or_else(|p|p.into_inner())}
    pub fn log(&self,kind:&str,message:impl Into<String>){let now=self.now();self.lock().log(now,kind,message.into());}
    pub fn view(&self)->View {
        let now=self.now();let mut d=self.lock();
        let aggregate=d.engine.view(now,d.config.source_timeout_seconds);
        let output=d.output(&aggregate,now);
        let mut battery=d.battery.clone();
        let ttl=if d.config.sound.enabled{light_core::sound::BATTERY_FRESH_MS}else{u64::from(d.config.battery_poll_seconds)*2000+5000};
        battery.stale=battery.error.is_some()||d.hardware.present==Some(false)||battery.updated_ms.is_none_or(|t|now<t||now.saturating_sub(t)>ttl);
        let power=PowerView{reading:d.power.reading.clone(),updated_ms:d.power.updated_ms,attempted_ms:d.power.attempted_ms,
            error:d.power.error.clone(),enabled:d.config.voltage_enabled,
            stale:!d.config.voltage_enabled||d.power.error.is_some()||d.hardware.present==Some(false)
                ||d.power.updated_ms.is_none_or(|t|now<t||now.saturating_sub(t)>MAX_GAP_MS)};
        let sound=d.sound.view(now,&d.config.sound,d.connected());
        View{version:env!("CARGO_PKG_VERSION"),now_ms:now,config:d.config.clone(),autostart:platform::autostart_enabled(),
            aggregate,output,paused:d.paused,preview:d.preview.clone(),hardware:d.hardware.clone(),battery,power,sound,
            receiver:d.receiver.clone(),logs:d.logs.iter().rev().take(120).cloned().collect(),data_dir:self.dir.to_string_lossy().into_owned()}
    }
    pub fn accept(&self,snapshot:Snapshot)->anyhow::Result<()> {
        let now=self.now();let mut d=self.lock();let done=d.config.done_seconds;
        let before=d.engine.view(now,d.config.source_timeout_seconds);
        let old_sound=top_key(&before.sessions,&d.config.sound);
        let important=d.engine.accept(snapshot,now,done)?;
        if important{d.preview=None;d.sound.cancel_test();}
        let after=d.engine.view(now,d.config.source_timeout_seconds);
        if old_sound!=top_key(&after.sessions,&d.config.sound)||important{self.sound_cancel.store(true,Ordering::SeqCst);}
        if before.state!=after.state{d.log(now,"state",format!("{} → {}",before.state.label(),after.state.label()));}
        Ok(())
    }
    pub fn hardware_result(&self,r:&Reply,sent:bool){
        let now=self.now();let mut d=self.lock();
        if let Some(p)=r.present{d.hardware.present=Some(p);}
        if r.device_name.is_some(){d.hardware.device_name=r.device_name.clone();}
        d.hardware.last_checked_ms=Some(now);
        if sent{d.hardware.api_success=r.api_success;d.hardware.acknowledged=r.acknowledged;if r.api_success==Some(true){d.hardware.last_output_ms=Some(now);}}
        if d.hardware.error!=r.error {if let Some(error)=&r.error{d.log(now,"hardware",error.clone());}}
        d.hardware.error=r.error.clone();
    }
    pub fn request_refresh(&self){self.force_output.store(true,Ordering::Relaxed);self.force_battery.store(true,Ordering::Relaxed);self.force_power.store(true,Ordering::Relaxed);}
}
impl Data {
    pub fn connected(&self)->bool{self.hardware.present==Some(true)&&self.hardware.error.is_none()}
    pub fn output(&mut self,aggregate:&Aggregate,now:u64)->LightState {
        if self.preview.as_ref().is_some_and(|p|now>=p.until_ms){self.preview=None;}
        if self.paused{LightState::Off}else{self.preview.as_ref().map(|p|p.state).unwrap_or(aggregate.state)}
    }
    pub fn log(&mut self,now:u64,kind:&str,message:String){
        self.logs.push_back(LogEntry{at_ms:now,kind:kind.into(),message:message.chars().take(500).collect()});
        while self.logs.len()>200{self.logs.pop_front();}
    }
}
