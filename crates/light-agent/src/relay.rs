use crate::store;
use anyhow::{Context, Result};
use light_core::now_ms;
use std::time::{Duration,Instant};

fn client()->Result<reqwest::blocking::Client>{
    Ok(reqwest::blocking::Client::builder()
        .no_proxy() // VM traffic must not leak through unrelated HTTP proxies.
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(2)).timeout(Duration::from_secs(4))
        .build()?)
}
pub fn check()->Result<()> {
    let c=store::load_config()?;
    let response=client()?.get(format!("{}/v1/status",c.url.trim_end_matches('/'))).bearer_auth(&c.token).send()?;
    anyhow::ensure!(response.status().is_success(),"Windows returned HTTP {}",response.status());
    println!("{}",response.text()?); Ok(())
}
pub fn run()->Result<()> {
    let _lock=store::relay_lock().context("another light-agent relay is running")?;
    store::initialize_state()?;
    let cfg=store::load_config()?;
    let http=client()?;
    let mut last_sent:Option<(String,u64)>=None;
    let mut last_heartbeat=Instant::now()-Duration::from_secs(15);
    let mut last_failed=false;
    println!("AI Light relay started (state only; no prompt or model traffic).");
    loop {
        let local=store::read_state()?;
        let current=(local.generation.clone(),local.revision);
        if last_sent.as_ref()!=Some(&current) || last_heartbeat.elapsed()>=Duration::from_secs(15) {
            let snapshot=local.snapshot(&cfg.source_id,now_ms());
            let sent=http.post(format!("{}/v1/sync",cfg.url.trim_end_matches('/')))
                .bearer_auth(&cfg.token).json(&snapshot).send();
            match sent {
                Ok(response) if response.status().is_success()=>{
                    last_sent=Some(current);last_heartbeat=Instant::now();
                    if last_failed {println!("Windows receiver is reachable again.");}
                    last_failed=false;
                }
                Ok(response)=>{
                    if !last_failed {eprintln!("Receiver HTTP {}. Check token, source allowlist and port.",response.status());}
                    last_failed=true;std::thread::sleep(Duration::from_secs(3));
                }
                Err(_)=>{
                    if !last_failed {eprintln!("Receiver unreachable. Latest state retained locally; retrying.");}
                    last_failed=true;std::thread::sleep(Duration::from_secs(3));
                }
            }
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}
