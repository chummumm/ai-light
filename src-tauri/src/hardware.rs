//! Serialized HID owner for LED, voltage and buzzer; separate bounded GATT reader.
use crate::{process,state::Shared,worker::{Request,Reply}};
use light_core::{model::LightState,power::SAMPLE_INTERVAL_MS,sound::{Action,Gate,raw_volume}};
use std::{sync::{Arc,atomic::Ordering},time::{Duration,Instant}};

pub fn start(shared:Arc<Shared>){
    let battery=shared.clone();std::thread::spawn(move||battery_loop(&battery));
    std::thread::spawn(move||output_loop(&shared));
}
fn output_loop(shared:&Shared){
    let mut last_sent:Option<(LightState,u16,u16,String)>=None;
    let mut device_serial=String::new();
    let mut verified_volume:Option<u8>=None;
    let mut verified_capability=false;
    let mut last_probe=Instant::now()-Duration::from_secs(10);
    let mut next_voltage=Instant::now();let mut retry=Instant::now();
    let mut last_wall=light_core::now_ms();
    while !shared.stop.load(Ordering::Relaxed){
        let now=shared.now();let wall=light_core::now_ms();
        if wall<last_wall||wall.saturating_sub(last_wall)>10_000 {
            let mut d=shared.lock();d.sound.suspend();d.battery.stale=true;d.power.updated_ms=None;
            shared.force_battery.store(true,Ordering::Relaxed);shared.sound_cancel.store(true,Ordering::SeqCst);
        }
        last_wall=wall;
        let (cfg,output,action)={
            let mut d=shared.lock();let v=d.engine.view(now,d.config.source_timeout_seconds);
            let output=d.output(&v,now);let cfg=d.config.clone();let connected=d.connected();
            shared.sound_cancel.store(false,Ordering::SeqCst);
            let action=d.sound.tick(now,&v.sessions,&cfg.sound,connected);
            (cfg,output,action)
        };
        if cfg.serial.is_empty() {
            {let mut d=shared.lock();d.hardware.present=Some(false);d.hardware.error=Some("请在设备页扫描并选择已配对的灯".into());}
            std::thread::sleep(Duration::from_millis(500));continue;
        }
        if device_serial!=cfg.serial {
            if !device_serial.is_empty(){
                let _=process::call(&Request::Buzzer{serial:device_serial.clone(),mode:None,raw_volume:None,verify_capability:false,not_after_ms:light_core::now_ms()+800},Duration::from_millis(900),None);
            }
            verified_volume=None;verified_capability=false;last_sent=None;next_voltage=Instant::now();
            device_serial=cfg.serial.clone();
        }
        if let Some(action)=action {
            let (mode,raw,verify)=match &action {
                Action::Stop=>(None,None,false),
                Action::Play(p)=>{
                    let raw=raw_volume(p.volume_percent).expect("validated sound volume");
                    (Some(p.mode),if verified_volume==Some(raw){None}else{Some(raw)},!verified_capability)
                }
            };
            let request=Request::Buzzer{serial:cfg.serial.clone(),mode,raw_volume:raw,verify_capability:verify,not_after_ms:light_core::now_ms()+2800};
            let cancel=if matches!(action,Action::Stop){None}else{Some(&shared.sound_cancel)};
            let result=process::call(&request,Duration::from_millis(3000),cancel);
            if shared.stop.load(Ordering::Relaxed){break;}
            let canceled=matches!(action,Action::Play(_))&&shared.sound_cancel.load(Ordering::SeqCst);
            let mut d=shared.lock();
            if d.config.serial==cfg.serial {
                if canceled {
                    d.sound.stop_current();verified_volume=None;verified_capability=false;
                } else {
                    let r=result.unwrap_or_else(|e|Reply{error:Some(e.to_string()),..Reply::default()});
                    if let Some(p)=r.present{d.hardware.present=Some(p);}
                    if let Some(raw)=r.applied_volume{verified_volume=Some(raw);}
                    if r.capabilities.is_some_and(|c|c&0x20!=0){verified_capability=true;}
                    let error=r.error.clone().or_else(||if r.buzzer_api_success!=Some(true){Some("未确认蜂鸣器系统写入成功".into())}else{None});
                    if error.is_some(){verified_volume=None;verified_capability=false;}
                    let now=shared.now();
                    if let Some(e)=&error{d.log(now,"sound",format!("声音输出：{e}"));}
                    else if let Action::Play(p)=&action{d.log(now,"sound",format!("发送 {:?} · 音量档位 {}%（系统写入成功，听感需实机确认）",p.mode,p.volume_percent));}
                    d.sound.complete(&action,now,error);
                }
            }
            drop(d);
            std::thread::sleep(Duration::from_millis(50));continue;
        }
        let force=shared.force_output.swap(false,Ordering::Relaxed);
        if force{verified_volume=None;verified_capability=false;}
        let forced_voltage=shared.force_power.swap(false,Ordering::Relaxed);
        let wanted=(output,cfg.period_ms,cfg.fade_ms,cfg.serial.clone());
        let send=force||(last_sent.as_ref()!=Some(&wanted)&&Instant::now()>=retry);
        let voltage=cfg.voltage_enabled&&(forced_voltage||Instant::now()>=next_voltage);
        if send||voltage||last_probe.elapsed()>=Duration::from_secs(8) {
            let request=Request::Hid{serial:cfg.serial.clone(),state:if send{Some(output)}else{None},period_ms:cfg.period_ms,fade_ms:cfg.fade_ms,power:voltage};
            let r=process::call(&request,Duration::from_secs(3),Some(&shared.stop)).unwrap_or_else(|e|Reply{error:Some(e.to_string()),..Reply::default()});
            if shared.stop.load(Ordering::Relaxed){break;}
            if send {
                if r.api_success==Some(true)&&r.acknowledged!=Some(false){last_sent=Some(wanted);}
                else{last_sent=None;retry=Instant::now()+Duration::from_secs(5);}
            }
            if r.present==Some(false)||r.error.is_some(){
                verified_volume=None;verified_capability=false;last_sent=None;retry=Instant::now()+Duration::from_secs(5);
            }
            if shared.lock().config.serial==cfg.serial {shared.hardware_result(&r,send);}
            if voltage {
                let now=shared.now();let mut d=shared.lock();
                next_voltage=Instant::now()+Duration::from_millis(SAMPLE_INTERVAL_MS);
                if d.config.serial==cfg.serial&&d.config.voltage_enabled {
                    d.power.attempted_ms=Some(now);
                    let valid_power=r.power_error.is_none()&&r.error.is_none();
                    if let Some(sample)=r.power.filter(|_|valid_power){
                        d.power.reading=Some(sample);d.power.updated_ms=Some(now);d.power.error=None;
                    } else {
                        let error=r.power_error.or(r.error).unwrap_or_else(||"未收到有效电压应答".into());
                        if d.power.error.as_ref()!=Some(&error){d.log(now,"power",error.clone());}
                        d.power.error=Some(error);next_voltage=Instant::now()+Duration::from_secs(30);
                    }
                }
            }
            last_probe=Instant::now();
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let cfg=shared.lock().config.clone();
    let _=process::call(&Request::Buzzer{serial:cfg.serial.clone(),mode:None,raw_volume:None,verify_capability:false,not_after_ms:light_core::now_ms()+800},Duration::from_millis(900),None);
    if cfg.exit_off {
        let _=process::call(&Request::Hid{serial:cfg.serial,state:Some(LightState::Off),period_ms:cfg.period_ms,fade_ms:cfg.fade_ms,power:false},Duration::from_millis(1300),None);
    }
    shared.hardware_stopped.store(true,Ordering::Relaxed);
}
fn battery_loop(shared:&Shared){
    let mut last=Instant::now()-Duration::from_secs(600);let mut failed=false;
    while !shared.stop.load(Ordering::Relaxed){
        let (cfg,recovering)={let d=shared.lock();(d.config.clone(),d.sound.battery.recovery_poll(shared.now()))};
        if cfg.serial.is_empty(){std::thread::sleep(Duration::from_millis(500));continue;}
        let interval=if cfg.sound.enabled{if recovering{10}else{cfg.battery_poll_seconds.min(30)}}else{cfg.battery_poll_seconds};
        if shared.force_battery.swap(false,Ordering::Relaxed)||last.elapsed()>=Duration::from_secs(u64::from(interval)){
            let result=process::call(&Request::Battery{serial:cfg.serial.clone()},Duration::from_secs(12),Some(&shared.stop));
            if shared.stop.load(Ordering::Relaxed){break;}
            let mut d=shared.lock();
            if d.config.serial!=cfg.serial{last=Instant::now()-Duration::from_secs(600);continue;}
            let now=shared.now();d.battery.attempted_ms=Some(now);let old_gate=d.sound.battery.gate(now);
            match result {
                Ok(Reply{battery:Some(reading),error:None,..}) if reading.percent.is_some()=>{
                    if failed{shared.force_output.store(true,Ordering::Relaxed);}failed=false;
                    d.sound.battery.observe(reading.percent.unwrap(),now);
                    d.battery.reading=Some(reading);d.battery.updated_ms=Some(now);d.battery.error=None;
                }
                Ok(r)=>{failed=true;d.sound.battery.fail();d.battery.error=Some(r.error.unwrap_or_else(||"没有有效电量读数".into()));}
                Err(e)=>{failed=true;d.sound.battery.fail();d.battery.error=Some(e.to_string());}
            }
            let gate=d.sound.battery.gate(now);
            if gate!=Gate::Ready {d.sound.stop_current();shared.sound_cancel.store(true,Ordering::SeqCst);}
            if old_gate!=gate {d.log(now,"battery",match gate{Gate::Ready=>"电量保护已解除；不补播旧声音",Gate::Low=>"低电量 ≤20%，取消蜂鸣提醒",Gate::Recovering=>"电量恢复确认中（需两次 ≥25%）",Gate::Unknown=>"电量未确认，暂停蜂鸣"}.into());}
            last=Instant::now();
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}
