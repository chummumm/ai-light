//! Validated and narrowly allowlisted HID READ helpers. No arbitrary commands.
use anyhow::{bail, ensure, Result};
use crate::{power::PowerSample, protocol::{REPORT_ID, REPORT_LENGTH}};

pub fn read_report(address:u8)->Result<[u8;REPORT_LENGTH]> {
    ensure!(matches!(address,0x05|0x11|0x14),"register not allowlisted");
    let mut out=[0u8;REPORT_LENGTH];
    out[..7].copy_from_slice(&[REPORT_ID,94,94,3,2,address,3^2^address]);
    Ok(out)
}
pub fn parse_reply(input:&[u8],address:u8)->Result<Option<Vec<u8>>> {
    ensure!(matches!(address,0x05|0x11|0x14),"register not allowlisted");
    let b=if input.first()==Some(&REPORT_ID){&input[1..]}else{input};
    if b.len()<5||b[..2]!=[94,94]{return Ok(None);}
    let n=usize::from(b[2])+3;
    if n<6||n>64||n>b.len()||b[..n].iter().fold(0u8,|a,x|a^x)!=0{return Ok(None);}
    if b[3]==0x42 {ensure!(n==6,"malformed READ error");bail!("READ rejected: error {}",b[4]);}
    if b[3]!=0x82||b[4]!=address{return Ok(None);}
    let expected=if address==0x14{2}else{1};
    ensure!(n==6+expected,"unexpected length for READ 0x{address:02X}");
    Ok(Some(b[5..n-1].to_vec()))
}
pub fn hex(input:&[u8])->String {input.iter().map(|b|format!("{b:02X}")).collect::<Vec<_>>().join(" ")}
pub fn sample(raw14:&[u8])->Result<PowerSample> {
    let data=parse_reply(raw14,0x14)?.ok_or_else(||anyhow::anyhow!("no valid 0x14 reply"))?;
    let mv=u16::from_le_bytes([data[0],data[1]]);
    ensure!((2000..=5000).contains(&mv),"voltage outside client validation range: {mv} mV");
    Ok(PowerSample{voltage_mv:mv,raw_14:hex(raw14)})
}
#[cfg(test)] mod tests {
    use super::*;
    const R14:[u8;8]=[94,94,5,0x82,0x14,0x20,0x0f,0xbc];
    #[test] fn capture(){assert_eq!(sample(&R14).unwrap().voltage_mv,3872);}
    #[test] fn envelope(){let mut b=vec![2];b.extend(R14);b.resize(65,0);assert_eq!(sample(&b).unwrap().voltage_mv,3872);}
    #[test] fn allowlist(){for a in 0..=255{assert_eq!(read_report(a).is_ok(),matches!(a,0x05|0x11|0x14));}}
    #[test] fn checksum(){for a in [5,0x11,0x14]{let p=read_report(a).unwrap();assert_eq!(p[1..7].iter().fold(0,|a,b|a^b),0);}}
    #[test] fn truncated(){for n in 0..R14.len(){assert_eq!(parse_reply(&R14[..n],0x14).unwrap(),None);}}
    #[test] fn corrupt(){let mut b=R14;b[7]^=1;assert_eq!(parse_reply(&b,0x14).unwrap(),None);}
    #[test] fn mismatch(){assert_eq!(parse_reply(&R14,5).unwrap(),None);}
    #[test] fn error(){assert!(parse_reply(&[94,94,3,0x42,2,0x43],5).is_err());}
    #[test] fn one_byte_volume(){assert_eq!(parse_reply(&[94,94,4,0x82,5,100,0xe7],5).unwrap(),Some(vec![100]));}
}
