//! Bounded child isolates blocking Bluetooth calls. Job close kills its workers.
use anyhow::{bail,Context,Result};
use std::{io::{Read,Write},process::{Child,Command,Stdio},sync::atomic::{AtomicBool,Ordering},time::{Duration,Instant}};
use crate::worker::{Request,Reply};

#[cfg(windows)]
struct Job(windows::Win32::Foundation::HANDLE);
#[cfg(windows)]
impl Job {
    fn new(child:&Child)->Result<Self>{
        use std::{mem::size_of,os::windows::io::AsRawHandle};
        use windows::{core::PCWSTR,Win32::{Foundation::HANDLE,System::JobObjects::*}};
        unsafe {
            let handle=CreateJobObjectW(None,PCWSTR::null())?;
            let job=Self(handle);
            let mut info=JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags=JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            SetInformationJobObject(handle,JobObjectExtendedLimitInformation,&info as *const _ as *const _,size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32)?;
            AssignProcessToJobObject(handle,HANDLE(child.as_raw_handle()))?;
            Ok(job)
        }
    }
}
#[cfg(windows)]impl Drop for Job{fn drop(&mut self){unsafe{let _=windows::Win32::Foundation::CloseHandle(self.0);}}}

pub fn call(request:&Request,timeout:Duration,cancel:Option<&AtomicBool>)->Result<Reply> {
    let mut command=Command::new(std::env::current_exe()?);
    command.arg("--hardware-worker").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
    #[cfg(windows)]{use std::os::windows::process::CommandExt;command.creation_flags(0x08000000);}
    let mut child=command.spawn().context("spawn isolated hardware worker")?;
    #[cfg(windows)]let _job=match Job::new(&child){Ok(job)=>job,Err(e)=>{let _=child.kill();let _=child.wait();return Err(e.context("isolate hardware worker in Job Object"));}};
    let stdout=child.stdout.take().context("missing worker stdout")?;
    let reader=std::thread::spawn(move||->Result<Vec<u8>>{
        let mut bytes=Vec::new();
        stdout.take(65537).read_to_end(&mut bytes)?;
        anyhow::ensure!(bytes.len()<=65536,"hardware reply too large");
        Ok(bytes)
    });
    let status_result=(||->Result<()>{
        {let mut input=child.stdin.take().context("missing worker stdin")?;input.write_all(&serde_json::to_vec(request)?)?;}
        let start=Instant::now();
        loop {
            if let Some(status)=child.try_wait()? {
                if !status.success(){bail!("hardware worker exited with {status}");}
                break;
            }
            if cancel.is_some_and(|s|s.load(Ordering::Relaxed)){bail!("hardware request cancelled");}
            if start.elapsed()>=timeout{bail!("蓝牙调用超时，已终止本次硬件进程");}
            std::thread::sleep(Duration::from_millis(40));
        }
        Ok(())
    })();
    if status_result.is_err(){let _=child.kill();let _=child.wait();}
    let bytes=reader.join().map_err(|_|anyhow::anyhow!("hardware pipe reader panicked"))?;
    status_result?;
    serde_json::from_slice(&bytes?).context("invalid hardware response")
}
