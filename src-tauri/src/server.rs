use crate::state::Shared;
use axum::{extract::{ConnectInfo,DefaultBodyLimit,State},http::{HeaderMap,StatusCode},routing::{get,post},Json,Router};
use light_core::{model::Snapshot,network::{allowed_peer,token_matches}};
use serde_json::{json,Value};
use std::{net::{IpAddr,SocketAddr},sync::{Arc,atomic::Ordering},time::Duration};

type ApiResult=Result<Json<Value>,(StatusCode,Json<Value>)>;
fn error(code:StatusCode,message:&str)->(StatusCode,Json<Value>){(code,Json(json!({"error":message})))}
fn authorize(s:&Shared,ip:IpAddr,headers:&HeaderMap)->Result<(),(StatusCode,Json<Value>)>{
    if s.stop.load(Ordering::Relaxed){return Err(error(StatusCode::SERVICE_UNAVAILABLE,"shutting down"));}
    if headers.contains_key("origin"){return Err(error(StatusCode::FORBIDDEN,"browser origins are not allowed"));}
    if !allowed_peer(ip,&s.lock().config.allowed_sources){return Err(error(StatusCode::FORBIDDEN,"source address not allowed"));}
    let token=headers.get("authorization").and_then(|h|h.to_str().ok()).and_then(|h|h.strip_prefix("Bearer ")).unwrap_or("");
    if !token_matches(&s.token,token){return Err(error(StatusCode::UNAUTHORIZED,"invalid access token"));}
    Ok(())
}
async fn status(State(s):State<Arc<Shared>>,ConnectInfo(peer):ConnectInfo<SocketAddr>,headers:HeaderMap)->ApiResult{
    authorize(&s,peer.ip(),&headers)?;
    let v=s.view();
    Ok(Json(json!({"version":v.version,"output":v.output,"paused":v.paused,"receiver":v.receiver,
        "sources":v.aggregate.sources,"hardware":v.hardware,"battery":v.battery,
        "timing":{"done_seconds":v.config.done_seconds,"source_timeout_seconds":v.config.source_timeout_seconds,
            "remaining_ms":v.aggregate.remaining_ms}})))
}
async fn sync(State(s):State<Arc<Shared>>,ConnectInfo(peer):ConnectInfo<SocketAddr>,headers:HeaderMap,Json(snapshot):Json<Snapshot>)->ApiResult{
    authorize(&s,peer.ip(),&headers)?;
    s.accept(snapshot).map_err(|_|error(StatusCode::BAD_REQUEST,"invalid or excessive snapshot"))?;
    Ok(Json(json!({"ok":true})))
}
pub fn start(shared:Arc<Shared>){
    tauri::async_runtime::spawn(async move{
        let config=shared.lock().config.clone();
        let address=match config.bind_host.parse::<IpAddr>(){Ok(ip)=>SocketAddr::new(ip,config.port),Err(e)=>{shared.lock().receiver.error=Some(e.to_string());return;}};
        let listener=match tokio::net::TcpListener::bind(address).await{
            Ok(l)=>l,Err(e)=>{shared.lock().receiver.error=Some(e.to_string());shared.log("network",format!("监听失败：{e}"));return;}
        };
        shared.lock().receiver.listening=true;
        shared.log("network",format!("本地接收端已启动 · {address}"));
        let app=Router::new().route("/v1/status",get(status)).route("/v1/sync",post(sync))
            .layer(DefaultBodyLimit::max(256*1024)).with_state(shared.clone());
        let stop=shared.clone();
        let result=axum::serve(listener,app.into_make_service_with_connect_info::<SocketAddr>()).with_graceful_shutdown(async move{
            while !stop.stop.load(Ordering::Relaxed){tokio::time::sleep(Duration::from_millis(100)).await;}
        }).await;
        shared.lock().receiver.listening=false;
        if let Err(e)=result{shared.lock().receiver.error=Some(e.to_string());}
    });
}
