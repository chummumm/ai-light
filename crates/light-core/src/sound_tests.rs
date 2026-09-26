use super::*;
fn session(id:&str,state:LightState,at:u64)->SessionView {
    SessionView{source:"vm".into(),id:id.into(),turn:id.into(),state,event:"test".into(),
        episode_id:id.into(),entered_ms:at,remaining_ms:None,online:true}
}
fn ready()->SoundEngine{let mut e=SoundEngine::default();e.battery.observe(60,0);e}
fn tick(e:&mut SoundEngine,t:u64,s:&[SessionView],c:&SoundConfig)->Option<Action>{
    let a=e.tick(t,s,c,true);if let Some(a)=&a{e.complete(a,t,None);}a
}
fn plays(e:&mut SoundEngine,s:&[SessionView],c:&SoundConfig,end:u64)->Vec<(u64,Playback)> {
    let mut out=vec![];for t in (0..=end).step_by(100){if let Some(Action::Play(p))=tick(e,t,s,c){out.push((t,p));}}out
}
#[test] fn volume_mapping(){assert_eq!(raw_volume(0).unwrap(),0);assert_eq!(raw_volume(50).unwrap(),25);assert_eq!(raw_volume(100).unwrap(),50);}
#[test] fn all_generated_drive_values_safe(){for i in 0..=100{assert!(raw_volume(i).unwrap()<=50);}assert!(raw_volume(101).is_err());}
#[test] fn nonzero_ui_not_rounded_to_silent(){assert_eq!(raw_volume(1).unwrap(),1);}
#[test] fn frame_short(){assert_eq!(&beep_report(Some(BeepMode::Short))[..7],&[2,94,94,3,5,1,7]);}
#[test] fn frame_stop(){assert_eq!(&beep_report(None)[..7],&[2,94,94,3,5,0,6]);}
#[test] fn frame_volume(){assert_eq!(&volume_report(50).unwrap()[..8],&[2,94,94,4,3,5,50,48]);assert!(volume_report(100).is_err());}
#[test] fn frame_zero_padding(){let b=volume_report(35).unwrap();assert!(b[8..].iter().all(|b|*b==0));}
#[test] fn valid_ack(){assert_eq!(ack(&[2,94,94,3,0x85,1,0x87],5,1),Some(true));}
#[test] fn mismatched_ack(){assert_eq!(ack(&[2,94,94,3,0x85,1,0x87],5,2),None);}
#[test] fn bad_ack_checksum(){assert_eq!(ack(&[2,94,94,3,0x85,1,0x86],5,1),None);}
#[test] fn defaults_validate(){SoundConfig::default().validate().unwrap();}
#[test] fn rule_bounds(){let mut c=SoundConfig::default();c.done.interval_seconds=4;assert!(c.validate().is_err());c.done.interval_seconds=5;c.error.volume=Some(101);assert!(c.validate().is_err());}
#[test] fn count_bounds(){let mut c=SoundConfig::default();c.done.limit=Limit::Count{count:0};assert!(c.validate().is_err());}
#[test] fn duration_bounds(){let mut c=SoundConfig::default();c.done.limit=Limit::Duration{seconds:3601};assert!(c.validate().is_err());}
#[test] fn settings_defaults_merge(){let c:SoundConfig=serde_json::from_str(r#"{"enabled":false}"#).unwrap();assert!(!c.enabled);assert_eq!(c.waiting.interval_seconds,15);}
#[test] fn unknown_at_start(){assert_eq!(BatteryGuard::default().gate(0),Gate::Unknown);}
#[test] fn low_boundary(){let mut b=BatteryGuard::default();b.observe(20,0);assert_eq!(b.gate(0),Gate::Low);}
#[test] fn zero_battery_is_low_not_unknown(){let mut b=BatteryGuard::default();b.observe(0,0);assert_eq!(b.gate(0),Gate::Low);}
#[test] fn fresh_21_is_ready(){let mut b=BatteryGuard::default();b.observe(21,0);assert_eq!(b.gate(0),Gate::Ready);}
#[test] fn recovery_hysteresis(){let mut b=BatteryGuard::default();b.observe(20,0);b.observe(24,1000);assert_eq!(b.gate(1000),Gate::Low);b.observe(25,2000);assert_eq!(b.gate(2000),Gate::Recovering);b.observe(25,11999);assert_eq!(b.gate(11999),Gate::Recovering);b.observe(25,12000);assert_eq!(b.gate(12000),Gate::Ready);}
#[test] fn duplicate_samples_not_recovery(){let mut b=BatteryGuard::default();b.observe(10,0);b.observe(30,1000);b.observe(30,1000);assert_eq!(b.gate(11000),Gate::Recovering);}
#[test] fn recovery_dip_resets(){let mut b=BatteryGuard::default();b.observe(10,0);b.observe(30,1000);b.observe(24,9000);b.observe(30,11000);assert_eq!(b.gate(11000),Gate::Recovering);b.observe(30,21000);assert_eq!(b.gate(21000),Gate::Ready);}
#[test] fn stale_guard(){let mut b=BatteryGuard::default();b.observe(90,0);assert_eq!(b.gate(BATTERY_FRESH_MS+1),Gate::Unknown);}
#[test] fn failed_guard(){let mut b=BatteryGuard::default();b.observe(90,0);b.fail();assert_eq!(b.gate(1),Gate::Unknown);}
#[test] fn stale_recovery_not_combined(){let mut b=BatteryGuard::default();b.observe(10,0);b.observe(30,1000);b.observe(30,100000);assert_eq!(b.gate(100000),Gate::Recovering);}
#[test] fn done_three_rounds_in_thirty_seconds(){let mut e=ready();let p=plays(&mut e,&[session("a",LightState::Done,0)],&SoundConfig::default(),35000);assert_eq!(p.iter().map(|x|x.0).collect::<Vec<_>>(),vec![0,10000,20000]);}
#[test] fn waiting_delay(){let mut e=ready();let p=plays(&mut e,&[session("a",LightState::Waiting,0)],&SoundConfig::default(),35000);assert_eq!(p.iter().map(|x|x.0).collect::<Vec<_>>(),vec![2000,17000,32000]);}
#[test] fn auto_recovery_before_error_delay(){let mut e=ready();let c=SoundConfig::default();let s=vec![session("a",LightState::Error,0)];assert_eq!(tick(&mut e,0,&s,&c),None);assert_eq!(tick(&mut e,2000,&[session("b",LightState::Working,2000)],&c),None);assert_eq!(tick(&mut e,3000,&[session("b",LightState::Working,2000)],&c),None);}
#[test] fn once_has_bounded_stop(){let mut e=ready();let mut c=SoundConfig::default();c.done.limit=Limit::Once;let s=[session("a",LightState::Done,0)];assert!(matches!(tick(&mut e,0,&s,&c),Some(Action::Play(_))));assert_eq!(tick(&mut e,4999,&s,&c),None);assert_eq!(tick(&mut e,5000,&s,&c),Some(Action::Stop));assert_eq!(tick(&mut e,6000,&s,&c),None);}
#[test] fn count_limit(){let mut e=ready();let mut c=SoundConfig::default();c.done.limit=Limit::Count{count:2};let p=plays(&mut e,&[session("a",LightState::Done,0)],&c,35000);assert_eq!(p.len(),2);}
#[test] fn until_state_repeats(){let mut e=ready();let mut c=SoundConfig::default();c.done.limit=Limit::UntilState;let p=plays(&mut e,&[session("a",LightState::Done,0)],&c,35000);assert_eq!(p.len(),4);}
#[test] fn mute_same_episode_never_rearms(){let mut e=ready();let c=SoundConfig::default();let s=[session("a",LightState::Done,0)];tick(&mut e,0,&s,&c);e.stop_current();assert_eq!(tick(&mut e,1000,&s,&c),Some(Action::Stop));for t in [5000,10000,15000]{assert_eq!(tick(&mut e,t,&s,&c),None);}}
#[test] fn new_episode_after_mute(){let mut e=ready();let c=SoundConfig::default();tick(&mut e,0,&[session("a",LightState::Done,0)],&c);e.stop_current();tick(&mut e,100,&[session("a",LightState::Done,0)],&c);assert!(matches!(tick(&mut e,1000,&[session("b",LightState::Done,1000)],&c),Some(Action::Play(_))));}
#[test] fn priority_no_low_backlog(){let mut e=ready();let mut c=SoundConfig::default();c.error.delay_seconds=0;let low=session("a",LightState::Done,0);let high=session("b",LightState::Error,0);let a=tick(&mut e,0,&[low.clone(),high],&c).unwrap();assert!(matches!(a,Action::Play(Playback{mode:BeepMode::Triple,..})));tick(&mut e,1000,&[low.clone()],&c);assert_eq!(tick(&mut e,2000,&[low],&c),None);}
#[test] fn low_cancels_and_no_replay_after_recovery(){let mut e=ready();let c=SoundConfig::default();let s=[session("a",LightState::Done,0)];tick(&mut e,0,&s,&c);e.battery.observe(20,100);assert_eq!(tick(&mut e,100,&s,&c),Some(Action::Stop));e.battery.observe(30,1000);tick(&mut e,1000,&s,&c);tick(&mut e,6000,&s,&c);e.battery.observe(30,11000);assert_eq!(tick(&mut e,11000,&s,&c),None);}
#[test] fn unknown_no_start_or_replay(){let mut e=SoundEngine::default();let c=SoundConfig::default();let s=[session("a",LightState::Done,0)];assert_eq!(tick(&mut e,0,&s,&c),None);e.battery.observe(60,1000);assert_eq!(tick(&mut e,1000,&s,&c),None);}
#[test] fn disabled_no_start(){let mut e=ready();let mut c=SoundConfig::default();c.enabled=false;assert_eq!(tick(&mut e,0,&[session("a",LightState::Done,0)],&c),None);}
#[test] fn zero_volume_never_plays(){let mut e=ready();let mut c=SoundConfig::default();c.done.volume=Some(0);assert!(plays(&mut e,&[session("a",LightState::Done,0)],&c,30000).is_empty());}
#[test] fn disable_reenable_no_replay(){let mut e=ready();let mut c=SoundConfig::default();let s=[session("a",LightState::Done,0)];tick(&mut e,0,&s,&c);c.enabled=false;assert_eq!(tick(&mut e,100,&s,&c),Some(Action::Stop));c.enabled=true;assert_eq!(tick(&mut e,200,&s,&c),None);}
#[test] fn old_notification_no_replay(){let mut e=ready();let c=SoundConfig::default();assert_eq!(tick(&mut e,20000,&[session("a",LightState::Done,0)],&c),None);}
#[test] fn offline_no_replay(){let mut e=ready();let c=SoundConfig::default();let mut s=session("a",LightState::Done,0);tick(&mut e,0,&[s.clone()],&c);s.online=false;s.state=LightState::Off;tick(&mut e,1000,&[s.clone()],&c);s.online=true;s.state=LightState::Done;assert_eq!(tick(&mut e,2000,&[s],&c),None);}
#[test] fn test_low_denied(){let mut e=ready();e.battery.observe(10,1);assert!(e.preview(BeepMode::Short,50,1,&SoundConfig::default(),true).is_err());}
#[test] fn test_master_off_denied(){let mut e=ready();let mut c=SoundConfig::default();c.enabled=false;assert!(e.preview(BeepMode::Short,50,0,&c,true).is_err());}
#[test] fn test_current_sound_denied(){let mut e=ready();let c=SoundConfig::default();tick(&mut e,0,&[session("a",LightState::Done,0)],&c);assert!(e.preview(BeepMode::Short,50,0,&c,true).is_err());}
#[test] fn test_only_one_round(){let mut e=ready();let c=SoundConfig::default();e.preview(BeepMode::Double,60,0,&c,true).unwrap();let p=plays(&mut e,&[],&c,8000);assert_eq!(p.len(),1);assert_eq!(p[0].1.volume_percent,60);}
#[test] fn duplicate_test_denied(){let mut e=ready();let c=SoundConfig::default();e.preview(BeepMode::Short,50,0,&c,true).unwrap();assert!(e.preview(BeepMode::Short,50,0,&c,true).is_err());}
#[test] fn no_overlap_at_minimum_interval(){let mut e=ready();let mut c=SoundConfig::default();c.done.interval_seconds=5;let p=plays(&mut e,&[session("a",LightState::Done,0)],&c,20000);for w in p.windows(2){assert!(w[1].0-w[0].0>=5000);}}
#[test] fn time_gap_cancels(){let mut e=ready();let c=SoundConfig::default();let s=[session("a",LightState::Done,0)];tick(&mut e,0,&s,&c);assert_eq!(tick(&mut e,20000,&s,&c),Some(Action::Stop));assert_eq!(e.battery.gate(20000),Gate::Unknown);}
#[test] fn transport_error_not_retried(){let mut e=ready();let c=SoundConfig::default();let s=[session("a",LightState::Done,0)];let a=e.tick(0,&s,&c,true).unwrap();e.complete(&a,0,Some("uncertain write".into()));assert_eq!(tick(&mut e,100,&s,&c),Some(Action::Stop));assert_eq!(tick(&mut e,1000,&s,&c),None);assert_eq!(e.view(1000,&c,true).sent_rounds,0);}
#[test] fn failed_stop_does_not_loop(){let mut e=ready();let c=SoundConfig::default();e.stop_current();let a=e.tick(0,&[],&c,true).unwrap();e.complete(&a,0,Some("offline".into()));assert_eq!(tick(&mut e,100,&[],&c),None);}
#[test] fn settings_do_not_restart_episode(){let mut e=ready();let c=SoundConfig::default();let s=[session("a",LightState::Done,0)];tick(&mut e,0,&s,&c);e.reconfigure();tick(&mut e,1000,&s,&c);assert_eq!(tick(&mut e,2000,&s,&c),None);}
#[test] fn stop_before_first_scheduler_tick(){let mut e=ready();let c=SoundConfig::default();let s=[session("a",LightState::Done,0)];e.stop_visible(0,&s,&c);assert_eq!(tick(&mut e,0,&s,&c),Some(Action::Stop));assert_eq!(tick(&mut e,100,&s,&c),None);assert!(matches!(tick(&mut e,1000,&[session("b",LightState::Done,1000)],&c),Some(Action::Play(_))));}
#[test] fn all_beep_frames_validate(){for m in [None,Some(BeepMode::Short),Some(BeepMode::Double),Some(BeepMode::Triple),Some(BeepMode::Long)]{let r=beep_report(m);assert_eq!(r[1..7].iter().fold(0u8,|a,b|a^b),0);assert!(r[7..].iter().all(|b|*b==0));}}
#[test] fn whole_sound_config_roundtrip(){let c=SoundConfig::default();let value=serde_json::to_string(&c).unwrap();assert_eq!(serde_json::from_str::<SoundConfig>(&value).unwrap(),c);}
#[test] fn restart_does_not_replay_even_recent_prior_event(){let mut e=SoundEngine::new(1000);e.battery.observe(60,1000);let c=SoundConfig::default();assert_eq!(tick(&mut e,1100,&[session("old",LightState::Done,900)],&c),None);assert!(matches!(tick(&mut e,1200,&[session("new",LightState::Done,1200)],&c),Some(Action::Play(_))));}
