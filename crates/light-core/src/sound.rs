//! Clock-driven, transport-independent sound scheduling.
//! No timers are attached to GUI windows, no model calls, no sound backlog.
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use crate::{model::{LightState, SessionView}, protocol::{REPORT_ID, REPORT_LENGTH}};

pub const LOW_PERCENT: u8 = 20;
pub const RECOVER_PERCENT: u8 = 25;
pub const RECOVERY_SPACING_MS: u64 = 10_000;
pub const BATTERY_FRESH_MS: u64 = 90_000;
pub const ROUND_LEASE_MS: u64 = 5_000;
const MAX_NEW_EVENT_AGE_MS: u64 = 15_000;
const TICK_GAP_MS: u64 = 10_000;

#[derive(Debug,Clone,Copy,Serialize,Deserialize,PartialEq,Eq,Default)]
#[serde(rename_all="snake_case")]
pub enum BeepMode { #[default] Short, Double, Triple, Long }
impl BeepMode {
    pub fn code(self)->u8 {match self{Self::Short=>1,Self::Double=>2,Self::Triple=>3,Self::Long=>4}}
}
#[derive(Debug,Clone,Serialize,Deserialize,PartialEq,Eq)]
#[serde(tag="kind",rename_all="snake_case")]
pub enum Limit { Once, Duration{seconds:u32}, Count{count:u32}, UntilState }
#[derive(Debug,Clone,Serialize,Deserialize,PartialEq,Eq)]
pub struct SoundRule {
    pub enabled:bool,
    pub mode:BeepMode,
    pub volume:Option<u8>,
    pub delay_seconds:u32,
    pub interval_seconds:u32,
    pub limit:Limit,
}
impl SoundRule {
    pub fn validate(&self)->Result<()> {
        ensure!(self.volume.is_none_or(|v|v<=100),"音量必须为 0..100");
        ensure!(self.delay_seconds<=300,"首次延迟必须为 0..300 秒");
        ensure!((5..=3600).contains(&self.interval_seconds),"重复间隔必须为 5..3600 秒");
        match self.limit {
            Limit::Duration{seconds}=>ensure!((5..=3600).contains(&seconds),"提醒时长必须为 5..3600 秒"),
            Limit::Count{count}=>ensure!((1..=1000).contains(&count),"提醒次数必须为 1..1000"),_=>{}
        }
        Ok(())
    }
}
#[derive(Debug,Clone,Serialize,Deserialize,PartialEq,Eq)]
#[serde(default)]
pub struct SoundConfig {
    pub enabled:bool,
    pub default_volume:u8,
    pub working:SoundRule,
    pub waiting:SoundRule,
    pub error:SoundRule,
    pub done:SoundRule,
}
impl Default for SoundConfig {
    fn default()->Self {
        Self {
            enabled:true,default_volume:70,
            working:SoundRule{enabled:false,mode:BeepMode::Short,volume:None,delay_seconds:0,interval_seconds:60,limit:Limit::Once},
            waiting:SoundRule{enabled:true,mode:BeepMode::Double,volume:None,delay_seconds:2,interval_seconds:15,limit:Limit::Duration{seconds:180}},
            error:SoundRule{enabled:true,mode:BeepMode::Triple,volume:Some(80),delay_seconds:3,interval_seconds:10,limit:Limit::Duration{seconds:120}},
            done:SoundRule{enabled:true,mode:BeepMode::Short,volume:Some(50),delay_seconds:0,interval_seconds:10,limit:Limit::Duration{seconds:30}},
        }
    }
}
impl SoundConfig {
    pub fn validate(&self)->Result<()> {
        ensure!(self.default_volume<=100,"默认音量必须为 0..100");
        for r in [&self.working,&self.waiting,&self.error,&self.done]{r.validate()?;}
        Ok(())
    }
    pub fn rule(&self,state:LightState)->Option<&SoundRule> {
        match state {LightState::Working=>Some(&self.working),LightState::Waiting=>Some(&self.waiting),LightState::Error=>Some(&self.error),LightState::Done=>Some(&self.done),_=>None}
    }
}
/// Relative control position, not a linear SPL percentage. Verified hardware:
/// register 50 is loudest; 100 is silent. Never generate a raw value >50.
pub fn raw_volume(percent:u8)->Result<u8> {
    ensure!(percent<=100,"invalid volume");
    Ok(((u16::from(percent)+1)/2) as u8)
}
fn report(code:u8,data:&[u8])->[u8;REPORT_LENGTH] {
    let mut b=[0;REPORT_LENGTH];b[0]=REPORT_ID;b[1]=94;b[2]=94;
    b[3]=(data.len()+2) as u8;b[4]=code;b[5..5+data.len()].copy_from_slice(data);
    b[5+data.len()]=b[1..5+data.len()].iter().fold(0,|a,b|a^b);b
}
pub fn beep_report(mode:Option<BeepMode>)->[u8;REPORT_LENGTH] {report(5,&[mode.map_or(0,BeepMode::code)])}
/// The only persistent write exposed here is volume address 0x05.
pub fn volume_report(raw:u8)->Result<[u8;REPORT_LENGTH]> {
    ensure!(raw<=50,"raw buzzer drive must be 0..50");Ok(report(3,&[5,raw]))
}
pub fn ack(input:&[u8],command:u8,echo:u8)->Option<bool> {
    let b=if input.first()==Some(&REPORT_ID){&input[1..]}else{input};
    if b.len()<6||b[..2]!=[94,94]{return None;}
    let n=3+usize::from(b[2]);
    if n!=6||n>b.len()||b[..n].iter().fold(0u8,|a,x|a^x)!=0{return None;}
    if b[3]==(command|0x80) {if b[4]==echo{Some(true)}else{None}}
    else if b[3]==(command|0x40) {Some(false)} else {None}
}
#[derive(Debug,Clone,Copy,Serialize,PartialEq,Eq)]
#[serde(rename_all="snake_case")]
pub enum Gate { Ready, Low, Recovering, Unknown }
#[derive(Debug,Clone,Default)]
pub struct BatteryGuard {
    percent:Option<u8>,updated_ms:Option<u64>,failed:bool,
    low_latched:bool,recovery_first:Option<u64>,
}
impl BatteryGuard {
    pub fn observe(&mut self,percent:u8,at:u64) {
        if percent>100{self.fail();return;}
        if self.updated_ms.is_some_and(|t|at<=t){return;}
        let gap=self.updated_ms.is_some_and(|t|at.saturating_sub(t)>BATTERY_FRESH_MS);
        if gap{self.recovery_first=None;}
        self.percent=Some(percent);self.updated_ms=Some(at);self.failed=false;
        if percent<=LOW_PERCENT {self.low_latched=true;self.recovery_first=None;}
        else if self.low_latched {
            if percent<RECOVER_PERCENT{self.recovery_first=None;}
            else if let Some(first)=self.recovery_first {
                if at.saturating_sub(first)>=RECOVERY_SPACING_MS {self.low_latched=false;self.recovery_first=None;}
            } else {self.recovery_first=Some(at);}
        }
    }
    pub fn fail(&mut self){self.failed=true;self.recovery_first=None;}
    pub fn gate(&self,now:u64)->Gate {
        if self.failed||self.updated_ms.is_none_or(|t|now<t||now.saturating_sub(t)>BATTERY_FRESH_MS){return Gate::Unknown;}
        if self.low_latched {if self.recovery_first.is_some(){Gate::Recovering}else{Gate::Low}}
        else{Gate::Ready}
    }
    pub fn is_low(&self)->bool{self.low_latched}
    pub fn recovery_poll(&self,now:u64)->bool{self.gate(now)!=Gate::Ready}
}
#[derive(Debug,Clone,PartialEq,Eq)]
pub struct Playback {pub mode:BeepMode,pub volume_percent:u8,pub episode:String}
#[derive(Debug,Clone,PartialEq,Eq)]
pub enum Action { Stop, Play(Playback) }
#[derive(Debug,Clone)]
struct Episode {
    state:LightState,rule:SoundRule,volume:u8,next:u64,deadline:Option<u64>,
    attempts:u32,sent:u32,finished:bool,reason:String,
}
#[derive(Debug,Clone)]
struct Audition {play:Playback,sent:bool,until:u64}
#[derive(Debug,Clone,Serialize)]
pub struct SoundView {
    pub enabled:bool,pub gate:Gate,pub low_battery:bool,pub status:String,pub reason:String,
    pub active_state:Option<LightState>,pub mode:Option<BeepMode>,pub sent_rounds:u32,
    pub next_ms:Option<u64>,pub until_ms:Option<u64>,pub volume_percent:Option<u8>,pub raw_drive:Option<u8>,
    pub can_test:bool,pub last_sent_ms:Option<u64>,pub error:Option<String>,
}
#[derive(Debug,Default)]
pub struct SoundEngine {
    pub battery:BatteryGuard,
    episodes:BTreeMap<String,Episode>,selected:Option<String>,audition:Option<Audition>,
    stop_pending:bool,playing_until:Option<u64>,last_tick:Option<u64>,
    last_sent_ms:Option<u64>,last_error:Option<String>,not_before_ms:u64,
}
fn candidate<'a>(sessions:&'a [SessionView],cfg:&SoundConfig)->Option<&'a SessionView> {
    sessions.iter().filter(|s|s.online && cfg.rule(s.state).is_some_and(|r|r.enabled))
        .max_by(|a,b|(a.state,a.entered_ms,&a.episode_id).cmp(&(b.state,b.entered_ms,&b.episode_id)))
}
pub fn top_key(sessions:&[SessionView],cfg:&SoundConfig)->Option<String>{candidate(sessions,cfg).map(|s|s.episode_id.clone())}
impl SoundEngine {
    pub fn new(not_before_ms:u64)->Self {Self{not_before_ms,..Self::default()}}
    fn finish_all(&mut self,reason:&str) {
        for e in self.episodes.values_mut(){e.finished=true;e.reason=reason.into();}
        if self.playing_until.is_some()||self.audition.is_some(){self.stop_pending=true;}
        self.audition=None;
    }
    pub fn reconfigure(&mut self) {self.finish_all("设置已更改；不补播旧提醒");self.stop_pending=true;}
    pub fn stop_current(&mut self) {
        if let Some(e)=self.selected.as_ref().and_then(|k|self.episodes.get_mut(k)){e.finished=true;e.reason="已停止本次提醒".into();}
        self.audition=None;self.stop_pending=true;
    }
    pub fn stop_visible(&mut self,now:u64,sessions:&[SessionView],cfg:&SoundConfig) {
        self.sync(now,sessions,cfg);self.selected=top_key(sessions,cfg);
        self.finish_all("已停止本次提醒");self.stop_pending=true;
    }
    pub fn cancel_test(&mut self){if self.audition.take().is_some(){self.stop_pending=true;}}
    pub fn transport_failed(&mut self,message:String){self.finish_all("设备输出失败；本轮不重试鸣叫");self.last_error=Some(message);self.stop_pending=true;}
    pub fn suspend(&mut self){self.finish_all("休眠或时间跳变；旧声音不补播");self.battery.fail();self.stop_pending=true;}
    pub fn preview(&mut self,mode:BeepMode,volume:u8,now:u64,cfg:&SoundConfig,connected:bool)->Result<()> {
        ensure!(volume>0&&volume<=100,"试听音量必须为 1..100");
        ensure!(cfg.enabled,"先打开蜂鸣器总开关");
        ensure!(self.battery.gate(now)==Gate::Ready,"电量未确认、过期或处于低电量保护，禁止试听");
        ensure!(connected,"设备未连接，不能试听");
        ensure!(self.selected.as_ref().and_then(|k|self.episodes.get(k)).is_none_or(|e|e.finished),"先停止本次任务提醒，再试听");
        ensure!(self.audition.is_none()&&self.playing_until.is_none(),"当前声音尚未结束，请先停止或等待本轮结束");
        self.last_error=None;self.stop_pending=true;
        self.audition=Some(Audition{play:Playback{mode,volume_percent:volume,episode:"@preview".into()},sent:false,until:now+ROUND_LEASE_MS+1500});
        Ok(())
    }
    fn sync(&mut self,now:u64,sessions:&[SessionView],cfg:&SoundConfig) {
        let keys:BTreeSet<_>=sessions.iter().map(|s|s.episode_id.clone()).collect();
        self.episodes.retain(|k,_|keys.contains(k));
        for s in sessions {
            if let Some(e)=self.episodes.get_mut(&s.episode_id) {
                if !s.online||s.state==LightState::Off {e.finished=true;e.reason="状态结束或来源失联".into();}
                continue;
            }
            if !s.online {continue;}
            if let Some(rule)=cfg.rule(s.state) {
                let volume=rule.volume.unwrap_or(cfg.default_volume);
                let first=s.entered_ms.saturating_add(u64::from(rule.delay_seconds)*1000);
                let deadline=match rule.limit{Limit::Duration{seconds}=>Some(first.saturating_add(u64::from(seconds)*1000)),_=>None};
                let old=s.entered_ms<self.not_before_ms||now.saturating_sub(s.entered_ms)>MAX_NEW_EVENT_AGE_MS;
                self.episodes.insert(s.episode_id.clone(),Episode{state:s.state,rule:rule.clone(),volume,next:first,deadline,
                    attempts:0,sent:0,finished:old||!rule.enabled||volume==0,
                    reason:if old{"较早状态不补播"}else if !rule.enabled||volume==0{"此状态声音已关闭"}else{"等待首次提醒"}.into()});
            }
        }
    }
    /// At most one operation; the hardware owner serializes sound, LED and READs.
    pub fn tick(&mut self,now:u64,sessions:&[SessionView],cfg:&SoundConfig,connected:bool)->Option<Action> {
        self.sync(now,sessions,cfg);
        if self.last_tick.is_some_and(|t|now<t||now.saturating_sub(t)>TICK_GAP_MS){self.suspend();}
        self.last_tick=Some(now);
        let gate=self.battery.gate(now);
        if !cfg.enabled {self.finish_all("蜂鸣器总开关已关闭");}
        else if gate!=Gate::Ready {self.finish_all(match gate{Gate::Low=>"低电量，已取消本轮声音",Gate::Recovering=>"等待电量连续恢复到 25%",_=>"电量未确认或已过期；不补播旧声音"});}
        else if !connected{self.finish_all("蓝牙不可用；旧声音不补播");}
        let key=top_key(sessions,cfg);
        if self.selected!=key {
            if let Some(old)=self.selected.as_ref().and_then(|k|self.episodes.get_mut(k)){old.finished=true;old.reason="已被新状态接管".into();}
            if self.playing_until.is_some(){self.stop_pending=true;}
            self.selected=key;
        }
        for (k,e) in &mut self.episodes {
            if Some(k)!=self.selected.as_ref(){e.finished=true;e.reason="其他提醒优先，不排队补播".into();}
        }
        if let Some(e)=self.selected.as_ref().and_then(|k|self.episodes.get_mut(k)) {
            if e.deadline.is_some_and(|t|now>=t){e.finished=true;e.reason="提醒时限已到".into();if self.playing_until.is_some(){self.stop_pending=true;}}
        }
        if self.playing_until.is_some_and(|t|now>=t){self.stop_pending=true;}
        if self.audition.as_ref().is_some_and(|a|now>=a.until){self.audition=None;self.stop_pending=true;}
        if self.stop_pending {self.stop_pending=false;self.playing_until=None;return Some(Action::Stop);}
        if !cfg.enabled||gate!=Gate::Ready||!connected{return None;}
        if let Some(a)=&mut self.audition {
            if !a.sent {a.sent=true;self.playing_until=Some(now+ROUND_LEASE_MS);return Some(Action::Play(a.play.clone()));}
            return None;
        }
        let key=self.selected.clone()?;
        let e=self.episodes.get_mut(&key)?;
        if e.finished||now<e.next||self.playing_until.is_some(){return None;}
        e.attempts=e.attempts.saturating_add(1);
        e.next=now.saturating_add(u64::from(e.rule.interval_seconds)*1000);
        e.reason="按规则提醒".into();
        match e.rule.limit {
            Limit::Once=>{e.finished=true;e.reason="单次提醒已调度".into();},
            Limit::Count{count} if e.attempts>=count=>{e.finished=true;e.reason="提醒次数已到".into();},_=>{}
        }
        self.playing_until=Some(now+ROUND_LEASE_MS);
        Some(Action::Play(Playback{mode:e.rule.mode,volume_percent:e.volume,episode:key}))
    }
    pub fn complete(&mut self,action:&Action,now:u64,error:Option<String>) {
        if let Some(error)=error {
            if matches!(action,Action::Stop){self.playing_until=None;self.last_error=Some(error);}
            else{self.transport_failed(error);}
            return;
        }
        if let Action::Play(p)=action {
            self.last_sent_ms=Some(now);self.last_error=None;
            if let Some(e)=self.episodes.get_mut(&p.episode){e.sent=e.sent.saturating_add(1);}
        }
    }
    pub fn view(&self,now:u64,cfg:&SoundConfig,connected:bool)->SoundView {
        let gate=self.battery.gate(now);
        let e=self.selected.as_ref().and_then(|k|self.episodes.get(k));
        let (status,reason)=if !cfg.enabled{("disabled","蜂鸣器总开关已关闭".into())}
            else if gate!=Gate::Ready{(match gate{Gate::Low=>"low",Gate::Recovering=>"recovering",_=>"unknown"},match gate{Gate::Low=>"低电量：蜂鸣与试听已禁用",Gate::Recovering=>"需连续两次 ≥25%，间隔至少 10 秒",_=>"等待新鲜的有效电量读数"}.into())}
            else if !connected{("offline","设备未连接，停止声音输出".into())}
            else if self.audition.is_some(){("preview","只试听一轮；不改变灯光和任务".into())}
            else if let Some(e)=e {
                if self.playing_until.is_some(){("playing",e.reason.clone())}
                else if e.finished{("quiet",e.reason.clone())}
                else {("scheduled",e.reason.clone())}
            } else {("idle","已开启，等待新任务状态".into())};
        let volume=self.audition.as_ref().map(|a|a.play.volume_percent).or_else(||e.map(|e|e.volume));
        SoundView {enabled:cfg.enabled,gate,low_battery:self.battery.is_low(),status:status.into(),reason,
            active_state:e.map(|e|e.state),mode:self.audition.as_ref().map(|a|a.play.mode).or_else(||e.map(|e|e.rule.mode)),
            sent_rounds:e.map_or(0,|e|e.sent),next_ms:e.filter(|e|!e.finished&&cfg.enabled&&gate==Gate::Ready&&connected).map(|e|e.next),
            until_ms:self.audition.as_ref().map(|a|a.until).or_else(||e.and_then(|e|e.deadline)),
            volume_percent:volume,raw_drive:volume.and_then(|v|raw_volume(v).ok()),
            can_test:cfg.enabled&&gate==Gate::Ready&&connected&&self.audition.is_none()&&self.playing_until.is_none()&&e.is_none_or(|e|e.finished),
            last_sent_ms:self.last_sent_ms,error:self.last_error.clone()}
    }
}
#[cfg(test)]
#[path="sound_tests.rs"]
mod tests;
