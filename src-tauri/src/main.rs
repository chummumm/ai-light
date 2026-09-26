#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod battery;
mod config;
mod devices;
mod hardware;
mod platform;
mod process;
mod server;
mod state;
mod worker;

use anyhow::{Context,Result};
use light_core::{fsutil::atomic_json,model::LightState,sound::BeepMode};
use state::{Shared,View};
use std::{net::IpAddr,sync::{Arc,atomic::Ordering},time::{Duration,Instant}};
use tauri::{AppHandle,Manager,State,menu::{Menu,MenuItem,CheckMenuItem,PredefinedMenuItem},tray::{MouseButton,MouseButtonState,TrayIconBuilder,TrayIconEvent}};

type UiResult<T>=std::result::Result<T,String>;
fn message(e:anyhow::Error)->String{format!("{e:#}")}
#[tauri::command]
fn get_view(shared:State<'_,Arc<Shared>>)->View{shared.view()}
#[tauri::command]
fn find_devices()->UiResult<Vec<devices::Device>> { devices::discover().map_err(message) }
#[tauri::command]
fn set_preview(shared:State<'_,Arc<Shared>>,state:LightState,seconds:u32)->UiResult<()> {
    let max=if state==LightState::Done{3600}else{60};
    if seconds==0||seconds>max{return Err("无效预览时长".into());}
    let now=shared.now();let mut d=shared.lock();
    if d.paused{return Err("先恢复灯光输出，再测试灯效".into());}
    d.preview=Some(state::Preview{state,until_ms:now+seconds as u64*1000});
    d.log(now,"preview",format!("{} · 测试 {} 秒",state.label(),seconds));
    Ok(())
}
#[tauri::command]
fn clear_preview(shared:State<'_,Arc<Shared>>){shared.lock().preview=None;}
#[tauri::command]
fn set_paused(shared:State<'_,Arc<Shared>>,paused:bool){
    let mut d=shared.lock();d.paused=paused;d.preview=None;
    d.log(shared.now(),"output",if paused{"已暂停灯光输出"}else{"已恢复自动灯光"}.into());
}
fn persist_settings(shared:&Shared,mut settings:config::Config)->Result<()> {
    settings.serial=settings.serial.to_lowercase();settings.validate()?;
    let mut d=shared.lock();
    atomic_json(&shared.dir.join("settings.json"),&settings)?;
    let changed_device=d.config.serial!=settings.serial;
    let sound_changed=d.config.sound!=settings.sound;
    if sound_changed||changed_device {
        let now=shared.now();let snapshot=d.engine.view(now,d.config.source_timeout_seconds);let cfg=d.config.sound.clone();
        d.sound.stop_visible(now,&snapshot.sessions,&cfg);d.sound.reconfigure();shared.sound_cancel.store(true,Ordering::SeqCst);
    }
    if changed_device {
        d.battery=state::BatteryView{stale:true,..state::BatteryView::default()};
        d.hardware=state::HardwareView::default();d.power=state::PowerData::default();
        d.sound=light_core::sound::SoundEngine::new(shared.now());
    }
    d.config=settings;
    d.log(shared.now(),"settings","设置已保存；声音设置改变不会补播旧提醒".into());
    drop(d);shared.request_refresh();Ok(())
}
#[tauri::command]
fn save_settings(shared:State<'_,Arc<Shared>>,settings:config::Config)->UiResult<()> {persist_settings(&shared,settings).map_err(message)}
fn sound_enabled(shared:&Shared,enabled:bool)->Result<()> {
    let mut c=shared.lock().config.clone();c.sound.enabled=enabled;persist_settings(shared,c)
}
#[tauri::command]
fn set_sound_enabled(shared:State<'_,Arc<Shared>>,enabled:bool)->UiResult<()> {sound_enabled(&shared,enabled).map_err(message)}
fn mute_current(shared:&Shared){
    let now=shared.now();let mut d=shared.lock();let snapshot=d.engine.view(now,d.config.source_timeout_seconds);let cfg=d.config.sound.clone();
    d.sound.stop_visible(now,&snapshot.sessions,&cfg);shared.sound_cancel.store(true,Ordering::SeqCst);
    d.log(now,"sound","已停止本次声音提醒；灯光继续，新事件仍可提醒".into());
}
#[tauri::command]
fn stop_sound(shared:State<'_,Arc<Shared>>){mute_current(&shared);}
#[tauri::command]
fn test_sound(shared:State<'_,Arc<Shared>>,mode:BeepMode,volume:u8)->UiResult<()> {
    let mut d=shared.lock();let cfg=d.config.sound.clone();let connected=d.connected();
    d.sound.preview(mode,volume,shared.now(),&cfg,connected).map_err(message)?;
    shared.sound_cancel.store(true,Ordering::SeqCst);
    d.log(shared.now(),"sound",format!("请求试听一轮 {:?}，音量档位 {}%",mode,volume));Ok(())
}
#[tauri::command]
fn set_autostart(shared:State<'_,Arc<Shared>>,enabled:bool)->UiResult<()> {
    platform::set_autostart(enabled).map_err(message)?;
    shared.log("settings",if enabled{"已启用当前用户登录自启"}else{"已关闭登录自启"});Ok(())
}
#[tauri::command]
fn refresh_device(shared:State<'_,Arc<Shared>>){shared.request_refresh();}
#[tauri::command]
fn refresh_battery(shared:State<'_,Arc<Shared>>){shared.force_battery.store(true,Ordering::Relaxed);shared.force_power.store(true,Ordering::Relaxed);}
#[tauri::command]
fn export_client(shared:State<'_,Arc<Shared>>,host:String,source_id:String)->UiResult<String>{
    (||->Result<String>{
        let ip:IpAddr=host.trim().parse().context("请输入 Ubuntu 能访问到的 Windows 宿主机 IP")?;
        anyhow::ensure!(!ip.is_unspecified()&&!ip.is_multicast(),"不能使用 0.0.0.0、:: 或组播地址");
        anyhow::ensure!(!source_id.is_empty()&&source_id.len()<=128&&!source_id.chars().any(char::is_control),"无效来源标识");
        let host=match ip{IpAddr::V4(ip)=>ip.to_string(),IpAddr::V6(ip)=>format!("[{ip}]")};
        let port=shared.lock().config.port;
        let data=serde_json::json!({"url":format!("http://{host}:{port}"),"token":shared.token,"source_id":source_id,"question_heuristic":true});
        let path=shared.dir.join("exports/client.json");atomic_json(&path,&data)?;
        shared.log("export","已导出连接配置（包含私密访问凭据）");Ok(path.to_string_lossy().into_owned())
    })().map_err(message)
}
#[tauri::command]
fn export_diagnostics(shared:State<'_,Arc<Shared>>)->UiResult<String>{
    let path=shared.dir.join("exports/diagnostics.json");atomic_json(&path,&shared.view()).map_err(message)?;Ok(path.to_string_lossy().into_owned())
}
#[tauri::command]
fn open_folder(shared:State<'_,Arc<Shared>>,kind:String)->UiResult<()> {
    let path=match kind.as_str(){"data"=>shared.dir.clone(),"exports"=>shared.dir.join("exports"),_=>return Err("无效目录类型".into())};
    std::fs::create_dir_all(&path).map_err(|e|e.to_string())?;platform::open_folder(&path).map_err(message)
}
#[tauri::command]
fn bluetooth_settings()->UiResult<()> {
    #[cfg(windows)]{std::process::Command::new("explorer.exe").arg("ms-settings:bluetooth").spawn().map_err(|e|e.to_string())?;Ok(())}
    #[cfg(not(windows))]{Err("Windows only".into())}
}
#[tauri::command]
fn quit(app:AppHandle,shared:State<'_,Arc<Shared>>){begin_exit(&app,shared.inner().clone());}
fn show(app:&AppHandle){if let Some(w)=app.get_webview_window("main"){let _=w.unminimize();let _=w.show();let _=w.set_focus();}}
fn begin_exit(app:&AppHandle,shared:Arc<Shared>){
    if shared.stop.swap(true,Ordering::SeqCst){return;}
    shared.sound_cancel.store(true,Ordering::SeqCst);shared.log("lifecycle","正在退出，停止声音并尝试熄灯");
    let app=app.clone();
    std::thread::spawn(move||{
        let start=Instant::now();
        while !shared.hardware_stopped.load(Ordering::Relaxed)&&start.elapsed()<Duration::from_millis(2600){std::thread::sleep(Duration::from_millis(40));}
        app.exit(0);
    });
}
struct SoundTray{toggle:CheckMenuItem<tauri::Wry>}
fn tray(app:&mut tauri::App)->Result<()> {
    let open=MenuItem::with_id(app,"open","打开 AI Light",true,None::<&str>)?;
    let pause=MenuItem::with_id(app,"pause","暂停 / 恢复灯光输出",true,None::<&str>)?;
    let refresh=MenuItem::with_id(app,"refresh","重新连接 / 刷新电量",true,None::<&str>)?;
    let sound=CheckMenuItem::with_id(app,"sound-toggle","蜂鸣器总开关",true,app.state::<Arc<Shared>>().lock().config.sound.enabled,None::<&str>)?;
    let mute=MenuItem::with_id(app,"sound-mute","停止本次声音提醒",true,None::<&str>)?;
    app.manage(SoundTray{toggle:sound.clone()});
    let separator=PredefinedMenuItem::separator(app)?;
    let exit=MenuItem::with_id(app,"quit","退出 AI Light",true,None::<&str>)?;
    let menu=Menu::with_items(app,&[&open,&pause,&sound,&mute,&refresh,&separator,&exit])?;
    TrayIconBuilder::with_id("main")
        .icon(app.default_window_icon().context("missing app icon")?.clone())
        .tooltip("AI Light · 空闲")
        .menu(&menu).show_menu_on_left_click(false)
        .on_tray_icon_event(|tray,event|{
            if matches!(event,TrayIconEvent::Click{button:MouseButton::Left,button_state:MouseButtonState::Up,..}|TrayIconEvent::DoubleClick{button:MouseButton::Left,..}){show(tray.app_handle());}
        })
        .on_menu_event(|app,event|{
            let s=app.state::<Arc<Shared>>();
            match event.id.as_ref(){
                "open"=>show(app),
                "pause"=>{let mut d=s.lock();d.paused=!d.paused;d.preview=None;},
                "refresh"=>s.request_refresh(),
                "sound-mute"=>mute_current(&s),
                "sound-toggle"=>{let enabled=!s.lock().config.sound.enabled;if let Err(e)=sound_enabled(&s,enabled){s.log("settings",format!("声音设置失败：{e}"));}},
                "quit"=>begin_exit(app,s.inner().clone()),_=>{}
            }
        }).build(app)?;
    Ok(())
}
fn update_tray(app:AppHandle,shared:Arc<Shared>){std::thread::spawn(move||{
    let mut last=String::new();
    while !shared.stop.load(Ordering::Relaxed){
        let v=shared.view();
        if let Some(menu)=app.try_state::<SoundTray>(){let _=menu.toggle.set_checked(v.sound.enabled);}
        let battery=if v.battery.stale{String::new()}else{v.battery.reading.as_ref().and_then(|b|b.percent).map(|p|format!(" · 电量 {p}%")).unwrap_or_default()};
        let sound=if !v.sound.enabled{" · 蜂鸣关闭"}else if v.sound.low_battery{" · 低电量禁鸣"}else{""};
        let text=format!("AI Light · {}{}{}{}",v.output.label(),if v.paused{"（输出暂停）"}else{""},battery,sound);
        if text!=last{
            if let Some(tray)=app.tray_by_id("main"){
                let _=tray.set_tooltip(Some(text.as_str()));
                let bytes:&[u8]=match v.output{
                    LightState::Working|LightState::Waiting=>include_bytes!("../icons/tray-yellow.png"),
                    LightState::Done=>include_bytes!("../icons/tray-green.png"),
                    LightState::Error=>include_bytes!("../icons/tray-red.png"),
                    LightState::Off=>include_bytes!("../icons/tray-off.png"),
                };
                if let Ok(icon)=tauri::image::Image::from_bytes(bytes){let _=tray.set_icon(Some(icon));}
            }last=text;
        }
        std::thread::sleep(Duration::from_secs(1));
    }
});}
fn launch()->Result<()> {
    let dir=config::data_dir()?;let (mut cfg,token,fresh)=config::load(&dir)?;
    if cfg.serial.is_empty() {
        if let Ok(found)=devices::discover() {if found.len()==1 {cfg.serial=found[0].serial.clone();atomic_json(&dir.join("settings.json"),&cfg)?;}}
    }
    let shared=Arc::new(Shared::new(cfg,dir,token));
    let background=std::env::args().any(|a|a=="--background");let setup_shared=shared.clone();
    let app=tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app,_,_|show(app)))
        .manage(shared.clone())
        .invoke_handler(tauri::generate_handler![get_view,find_devices,set_preview,clear_preview,set_paused,save_settings,set_autostart,refresh_device,refresh_battery,set_sound_enabled,stop_sound,test_sound,export_client,export_diagnostics,open_folder,bluetooth_settings,quit])
        .on_window_event(|window,event|{
            if let tauri::WindowEvent::CloseRequested{api,..}=event{
                let shared=window.state::<Arc<Shared>>();
                if !shared.stop.load(Ordering::Relaxed){api.prevent_close();let _=window.hide();}
            }
        })
        .setup(move|app|{
            tray(app)?;
            if fresh||platform::autostart_enabled(){if let Err(e)=platform::set_autostart(true){setup_shared.log("settings",format!("自启登记失败：{e}"));}}
            setup_shared.log("lifecycle","Rust 桌面程序已启动，等待来源同步");
            server::start(setup_shared.clone());hardware::start(setup_shared.clone());
            update_tray(app.handle().clone(),setup_shared.clone());
            if !background{show(app.handle());}Ok(())
        }).build(tauri::generate_context!())?;
    app.run(move|handle,event|{
        if let tauri::RunEvent::ExitRequested{api,..}=event{
            if !shared.stop.load(Ordering::Relaxed){api.prevent_exit();begin_exit(handle,shared.clone());}
        }
    });Ok(())
}
fn main(){
    if std::env::args().any(|a|a=="--hardware-worker"){if worker::run().is_err(){std::process::exit(2);}return;}
    if let Err(e)=launch(){
        let text=format!("AI Light 启动失败：{e:#}");
        if let Ok(dir)=config::data_dir(){let _=std::fs::create_dir_all(&dir);let _=std::fs::write(dir.join("startup-error.log"),&text);}
        eprintln!("{text}");
        #[cfg(windows)]{
            #[link(name="user32")]extern "system"{fn MessageBoxW(window:isize,text:*const u16,caption:*const u16,kind:u32)->i32;}
            let msg:Vec<u16>=text.encode_utf16().chain(Some(0)).collect();let title:Vec<u16>="AI Light".encode_utf16().chain(Some(0)).collect();
            unsafe{MessageBoxW(0,msg.as_ptr(),title.as_ptr(),0x10);}
        }
        std::process::exit(1);
    }
}
