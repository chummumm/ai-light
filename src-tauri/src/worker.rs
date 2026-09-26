//! Isolated Windows operations. The parent owns scheduling and cancellation.
use anyhow::{ensure, Result};
use light_core::{battery::BatteryReading,model::LightState,power::PowerSample,sound::BeepMode};
use serde::{Deserialize,Serialize};
use std::io::{Read,Write};

#[derive(Debug,Serialize,Deserialize)]
#[serde(tag="operation",rename_all="snake_case")]
pub enum Request {
    Hid{serial:String,state:Option<LightState>,period_ms:u16,fade_ms:u16,#[serde(default)]power:bool},
    Battery{serial:String},
    Buzzer{serial:String,mode:Option<BeepMode>,raw_volume:Option<u8>,verify_capability:bool,not_after_ms:u64},
}
#[derive(Debug,Default,Serialize,Deserialize)]
pub struct Reply {
    pub present:Option<bool>,pub device_name:Option<String>,pub api_success:Option<bool>,pub acknowledged:Option<bool>,
    pub battery:Option<BatteryReading>,pub power:Option<PowerSample>,pub power_error:Option<String>,
    pub buzzer_api_success:Option<bool>,pub buzzer_acknowledged:Option<bool>,pub applied_volume:Option<u8>,pub capabilities:Option<u8>,
    pub error:Option<String>,
}
pub fn run()->Result<()> {
    let mut bytes=Vec::new();std::io::stdin().lock().take(16385).read_to_end(&mut bytes)?;
    ensure!(bytes.len()<=16384,"worker request too large");
    let request:Request=serde_json::from_slice(&bytes)?;
    let result=match request {
        Request::Battery{serial}=>crate::battery::read(&serial).map(|b|Reply{battery:Some(b),..Reply::default()}),
        other=>native(other),
    };
    let reply=result.unwrap_or_else(|e|Reply{error:Some(format!("{e:#}")),..Reply::default()});
    std::io::stdout().lock().write_all(&serde_json::to_vec(&reply)?)?;Ok(())
}
#[cfg(not(windows))]
fn native(_:Request)->Result<Reply>{anyhow::bail!("Windows HID worker only")}
#[cfg(windows)]
fn native(request:Request)->Result<Reply> {
    use anyhow::Context;
    use hidapi::{HidApi,HidDevice};
    use light_core::{protocol,telemetry,sound};
    use std::{ffi::c_void,time::{Duration,Instant}};
    let serial=match &request {Request::Hid{serial,..}|Request::Buzzer{serial,..}=>serial,_=>unreachable!()};
    ensure!(serial.len()==12&&serial.bytes().all(|b|b.is_ascii_hexdigit()),"invalid serial");
    let api=HidApi::new().context("enumerate Windows HID")?;
    let candidates:Vec<_>=api.device_list().filter(|d|
        d.vendor_id()==protocol::VENDOR_ID&&d.product_id()==protocol::PRODUCT_ID&&
        d.usage_page()==protocol::USAGE_PAGE&&d.usage()==protocol::USAGE&&
        d.serial_number().is_some_and(|s|s.eq_ignore_ascii_case(serial))
    ).collect();
    if candidates.is_empty(){return Ok(Reply{present:Some(false),error:Some("未发现指定的蓝牙 HID 灯".into()),..Reply::default()});}
    ensure!(candidates.len()==1,"ambiguous HID collection; refusing output");
    let target=candidates[0];
    let mut reply=Reply{present:Some(true),device_name:target.product_string().map(str::to_owned),..Reply::default()};
    if matches!(request,Request::Hid{state:None,power:false,..}){return Ok(reply);}
    #[link(name="kernel32")]
    extern "system" {
        fn CreateFileW(name:*const u16,access:u32,share:u32,security:*const c_void,disposition:u32,flags:u32,template:isize)->isize;
        fn CloseHandle(handle:isize)->i32;
    }
    #[link(name="hid")]
    extern "system" {fn HidD_SetOutputReport(handle:isize,buffer:*const c_void,length:u32)->u8;}
    struct Handle(isize);
    impl Drop for Handle{fn drop(&mut self){unsafe{CloseHandle(self.0);}}}
    let path=target.path().to_str().context("HID path UTF-8")?;
    let wide:Vec<u16>=path.encode_utf16().chain(Some(0)).collect();
    let handle=unsafe{CreateFileW(wide.as_ptr(),0xc0000000,3,std::ptr::null(),3,0,0)};
    ensure!(handle!=0&&handle!=-1,"HID open: {}",std::io::Error::last_os_error());
    let handle=Handle(handle);
    // Preserve the independently verified transport paths for this firmware.
    let input=api.open_path(target.path()).context("HID input/WriteFile handle")?;
    fn drain(input:&HidDevice)->Result<()> {
        let mut buf=[0;65];for _ in 0..32{if input.read_timeout(&mut buf,0)?==0{return Ok(());}}
        anyhow::bail!("input queue not idle; another controller may be running")
    }
    fn control(h:&Handle,r:&[u8;65])->Result<()> {
        let ok=unsafe{HidD_SetOutputReport(h.0,r.as_ptr() as *const c_void,65)};
        ensure!(ok!=0,"HidD_SetOutputReport: {}",std::io::Error::last_os_error());Ok(())
    }
    fn output(input:&HidDevice,r:&[u8;65])->Result<()> {
        let n=input.write(r).context("HID WriteFile output")?;
        ensure!(n==65,"short HID output: {n}/65");Ok(())
    }
    fn read_register(h:&Handle,input:&HidDevice,addr:u8)->Result<Vec<u8>> {
        drain(input)?;control(h,&telemetry::read_report(addr)?)?;
        let start=Instant::now();let mut b=[0;65];
        while start.elapsed()<Duration::from_millis(800){
            let n=input.read_timeout(&mut b,40)?;if n==0{continue;}
            if telemetry::parse_reply(&b[..n],addr)?.is_some(){return Ok(b[..n].to_vec());}
        }
        anyhow::bail!("READ 0x{addr:02X}: no validated response")
    }
    fn read_byte(h:&Handle,input:&HidDevice,addr:u8)->Result<u8> {
        let raw=read_register(h,input,addr)?;
        let data=telemetry::parse_reply(&raw,addr)?.context("missing register response")?;Ok(data[0])
    }
    fn check_deadline(deadline:u64)->Result<()> {
        ensure!(light_core::now_ms()<=deadline,"sound request expired; not sent late");Ok(())
    }
    match request {
        Request::Hid{state,period_ms,fade_ms,power,..}=>{
            if let Some(state)=state {
                drain(&input)?;let out=protocol::report(state,period_ms,fade_ms)?;
                if let Err(e)=control(&handle,&out){reply.api_success=Some(false);reply.error=Some(e.to_string());return Ok(reply);}
                reply.api_success=Some(true);
                let start=Instant::now();let mut b=[0;65];
                while start.elapsed()<Duration::from_millis(300){
                    match input.read_timeout(&mut b,40) {
                        Ok(n) if n>0=>if let Some(a)=protocol::led_ack(&b[..n],out[5]){reply.acknowledged=Some(a);break;},
                        Ok(_)=>{},Err(_)=>break,
                    }
                }
                if reply.acknowledged==Some(false){reply.error=Some("LED 命令返回错误".into());}
            }
            if power {
                match read_register(&handle,&input,0x14).and_then(|b|telemetry::sample(&b)){
                    Ok(p)=>reply.power=Some(p),Err(e)=>reply.power_error=Some(format!("{e:#}"))
                }
            }
        }
        Request::Buzzer{mode,raw_volume,verify_capability,not_after_ms,..}=>{
            let outcome=(||->Result<()> {
                check_deadline(not_after_ms)?;
                if mode.is_none(){output(&input,&sound::beep_report(None))?;reply.buzzer_api_success=Some(true);return Ok(());}
                if verify_capability {
                    let caps=read_byte(&handle,&input,0x11)?;reply.capabilities=Some(caps);
                    ensure!(caps&0x20!=0,"设备未声明蜂鸣器能力 0x20，未发送声音");
                }
                if let Some(raw)=raw_volume {
                    ensure!(raw<=50,"raw sound level must be <=50");
                    let current=read_byte(&handle,&input,0x05)?;
                    if current!=raw {
                        check_deadline(not_after_ms)?;drain(&input)?;
                        output(&input,&sound::volume_report(raw)?)?;
                        std::thread::sleep(Duration::from_millis(100));
                        let actual=read_byte(&handle,&input,0x05)?;
                        ensure!(actual==raw,"蜂鸣原始值读回不符：要求 {raw}，读回 {actual}；本次未鸣叫");
                    }
                    reply.applied_volume=Some(raw);
                }
                check_deadline(not_after_ms)?;drain(&input)?;
                output(&input,&sound::beep_report(mode))?;
                reply.buzzer_api_success=Some(true);
                let start=Instant::now();let mut b=[0;65];
                while start.elapsed()<Duration::from_millis(200){
                    let n=input.read_timeout(&mut b,40)?;if n==0{continue;}
                    if let Some(a)=sound::ack(&b[..n],5,mode.unwrap().code()){reply.buzzer_acknowledged=Some(a);break;}
                }
                ensure!(reply.buzzer_acknowledged!=Some(false),"设备返回蜂鸣命令错误");
                Ok(())
            })();
            if let Err(e)=outcome{reply.error=Some(format!("{e:#}"));}
        }
        Request::Battery{..}=>unreachable!(),
    }
    Ok(reply)
}
