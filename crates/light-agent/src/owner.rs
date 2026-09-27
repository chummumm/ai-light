//! Linux process identity, not a process-name or CPU-activity timeout.
use serde::{Deserialize,Serialize};
#[derive(Debug,Clone,PartialEq,Eq,Serialize,Deserialize)]
pub struct Owner { pub pid:u32, pub start:u64, pub boot:String }
#[derive(Debug,Clone,Copy,PartialEq,Eq)]
pub enum Presence { Alive, Gone, Unknown }
fn stat(s:&str)->Option<(u32,u64,bool)> {
    let (_,tail)=s.rsplit_once(") ")?;
    let f:Vec<_>=tail.split_whitespace().collect();
    Some((f.get(1)?.parse().ok()?,f.get(19)?.parse().ok()?,matches!(*f.first()?,"Z"|"X")))
}
#[cfg(target_os="linux")]
fn identity(pid:u32)->Option<Owner> {
    let (_,start,zombie)=stat(&std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?)?;
    if zombie{return None;}
    Some(Owner{pid,start,boot:std::fs::read_to_string("/proc/sys/kernel/random/boot_id").ok()?.trim().into()})
}
#[cfg(target_os="linux")]
fn codex(pid:u32)->bool {
    std::fs::read_link(format!("/proc/{pid}/exe")).ok().and_then(|p|p.file_name().map(|n|n.to_string_lossy().into_owned()))
        .is_some_and(|name|name=="codex"||name=="codex.exe")
}
pub fn capture()->Option<Owner> {
    #[cfg(target_os="linux")] {
        let mut pid=std::process::id();
        for _ in 0..16 {
            let (parent,_,_)=stat(&std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?)?;
            if parent<=1||parent==pid{return None;} pid=parent;
            if codex(pid){return identity(pid);}
        }
    }
    None
}
pub fn presence(owner:&Owner)->Presence {
    #[cfg(target_os="linux")] {
        let Ok(boot)=std::fs::read_to_string("/proc/sys/kernel/random/boot_id")else{return Presence::Unknown;};
        if boot.trim()!=owner.boot{return Presence::Gone;}
        match std::fs::read_to_string(format!("/proc/{}/stat",owner.pid)) {
            Ok(s)=>match stat(&s){Some((_,start,z))=>if start==owner.start&&!z{Presence::Alive}else{Presence::Gone},None=>Presence::Unknown},
            Err(e) if e.kind()==std::io::ErrorKind::NotFound=>Presence::Gone,
            Err(_)=>Presence::Unknown,
        }
    }
    #[cfg(not(target_os="linux"))] {let _=owner;Presence::Unknown}
}
/// Migration: associate only a Codex process actually holding this exact file.
/// Failure to find one is UNKNOWN, never proof that the old task has ended.
pub fn holding(path:&std::path::Path)->Option<Owner> {
    #[cfg(target_os="linux")] {
        for p in std::fs::read_dir("/proc").ok()?.flatten().take(32768) {
            let Ok(pid)=p.file_name().to_string_lossy().parse::<u32>()else{continue;};
            if !codex(pid){continue;}
            let Some(before)=identity(pid)else{continue;};
            let Ok(fds)=std::fs::read_dir(p.path().join("fd"))else{continue;};
            for fd in fds.flatten().take(4096) {
                if std::fs::read_link(fd.path()).ok().as_deref()==Some(path) && identity(pid).as_ref()==Some(&before){return Some(before);}
            }
        }
    }
    #[cfg(not(target_os="linux"))] let _=path;
    None
}
#[cfg(test)]mod tests {
    use super::*;
    #[test]fn stat_with_spaces_in_name(){let mut f=vec!["S","42"];f.extend(std::iter::repeat("0").take(17));f.push("1234");assert_eq!(stat(&format!("88 (codex (worker)) {}",f.join(" "))),Some((42,1234,false)));}
    #[test]fn malformed_is_not_death(){assert_eq!(stat("bad"),None);}
    #[test]fn pid_reuse_identity_differs(){let a=Owner{pid:42,start:1,boot:"boot".into()};let b=Owner{start:2,..a.clone()};assert_ne!(a,b);}
    #[cfg(target_os="linux")]#[test]fn current_process_alive(){let me=identity(std::process::id()).unwrap();assert_eq!(presence(&me),Presence::Alive);let other=Owner{start:me.start+1,..me};assert_eq!(presence(&other),Presence::Gone);}
}
