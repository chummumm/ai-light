use anyhow::Result;
use std::path::Path;
const RUN_KEY:&str="Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const RUN_NAME:&str="AILightRust";
#[cfg(windows)]
pub fn autostart_enabled()->bool {
    use winreg::{RegKey,enums::HKEY_CURRENT_USER};
    RegKey::predef(HKEY_CURRENT_USER).open_subkey(RUN_KEY)
        .and_then(|k|k.get_value::<String,_>(RUN_NAME)).map(|s|!s.is_empty()).unwrap_or(false)
}
#[cfg(not(windows))] pub fn autostart_enabled()->bool{false}
#[cfg(windows)]
pub fn set_autostart(enabled:bool)->Result<()> {
    use winreg::{RegKey,enums::HKEY_CURRENT_USER};
    let (key,_)=RegKey::predef(HKEY_CURRENT_USER).create_subkey(RUN_KEY)?;
    if enabled {
        key.set_value(RUN_NAME,&format!("\"{}\" --background",std::env::current_exe()?.display()))?;
    } else if let Err(e)=key.delete_value(RUN_NAME) {if e.kind()!=std::io::ErrorKind::NotFound {return Err(e.into());}}
    Ok(())
}
#[cfg(not(windows))] pub fn set_autostart(_:bool)->Result<()>{anyhow::bail!("Windows only")}
pub fn open_folder(path:&Path)->Result<()> {
    #[cfg(windows)] {std::process::Command::new("explorer.exe").arg(path).spawn()?;Ok(())}
    #[cfg(not(windows))] {let _=path;anyhow::bail!("Windows desktop only")}
}
