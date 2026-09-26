use crate::model::LightState;
use anyhow::{ensure, Result};

pub const VENDOR_ID: u16 = 0x07d7;
pub const PRODUCT_ID: u16 = 0x6b01;
pub const USAGE_PAGE: u16 = 0xff00;
pub const USAGE: u16 = 1;
pub const REPORT_ID: u8 = 2;
pub const REPORT_LENGTH: usize = 65;
// No user's Bluetooth address is embedded in the public build.
pub const DEFAULT_SERIAL: &str = "";

/// Generate volatile LED (0x04) commands, not persistent settings writes.
pub fn led_frame(channel: u8, action: u8, options: &[u8]) -> Result<Vec<u8>> {
    ensure!((1..=7).contains(&channel), "invalid channel mask");
    ensure!(action <= 2, "invalid LED action");
    ensure!(options.len() % 2 == 0, "TLV pairs must have even length");
    for pair in options.chunks_exact(2) {
        match pair[0] {
            1 => ensure!(pair[1] == 0xff, "exclusive option must be FF"),
            2 => ensure!(pair[1] == 0, "only continuous breathing is supported"),
            3 => ensure!((2..=50).contains(&pair[1]), "invalid period"),
            4 => ensure!(pair[1] <= 50, "invalid fade"),
            _ => anyhow::bail!("unsupported LED option"),
        }
    }
    let mut frame = vec![0x5e, 0x5e, (4 + options.len()) as u8, 0x04, channel, action];
    frame.extend_from_slice(options);
    let checksum = frame.iter().fold(0u8, |acc, byte| acc ^ byte);
    frame.push(checksum);
    ensure!(frame.len() <= 64, "LED frame too long");
    Ok(frame)
}

pub fn report(state: LightState, period_ms: u16, fade_ms: u16) -> Result<[u8; REPORT_LENGTH]> {
    ensure!((200..=5000).contains(&period_ms) && period_ms % 100 == 0, "period must be 200..5000ms in 100ms steps");
    ensure!(fade_ms <= period_ms / 2 && fade_ms % 100 == 0, "fade must be <= half the period, in 100ms steps");
    let frame = match state {
        LightState::Off => led_frame(7, 0, &[])?,
        LightState::Done => led_frame(1, 1, &[1, 0xff])?,
        LightState::Waiting => led_frame(2, 1, &[1, 0xff])?,
        LightState::Working | LightState::Error => {
            let channel = if state == LightState::Working { 2 } else { 4 };
            led_frame(channel, 2, &[1, 0xff, 2, 0, 3, (period_ms / 100) as u8, 4, (fade_ms / 100) as u8])?
        }
    };
    let mut output = [0u8; REPORT_LENGTH];
    output[0] = REPORT_ID;
    output[1..1 + frame.len()].copy_from_slice(&frame);
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn known_green() { assert_eq!(led_frame(1, 1, &[1,255]).unwrap(), [0x5e,0x5e,6,4,1,1,1,255,252]); }
    #[test] fn known_yellow() { assert_eq!(led_frame(2, 1, &[1,255]).unwrap(), [0x5e,0x5e,6,4,2,1,1,255,255]); }
    #[test] fn known_red() { assert_eq!(led_frame(4, 1, &[1,255]).unwrap(), [0x5e,0x5e,6,4,4,1,1,255,249]); }
    #[test] fn known_off() { assert_eq!(led_frame(7, 0, &[]).unwrap(), [0x5e,0x5e,4,4,7,0,7]); }
    #[test] fn documented_fade() { assert_eq!(led_frame(4,1,&[4,10]).unwrap(), [0x5e,0x5e,6,4,4,1,4,10,9]); }
    #[test] fn documented_breathing() { assert_eq!(led_frame(1,2,&[3,50,4,25]).unwrap(), [0x5e,0x5e,8,4,1,2,3,50,4,25,35]); }
    #[test] fn hid_envelope() {
        let r = report(LightState::Working, 3000, 1500).unwrap();
        assert_eq!(r.len(),65); assert_eq!(r[0],2); assert_eq!(&r[1..3], &[94,94]);
        let end = 4 + r[3] as usize;
        assert_eq!(r[1..end].iter().fold(0, |a,b| a ^ b),0);
        assert!(r[end..].iter().all(|x| *x == 0));
    }
    #[test] fn fade_bounds() { assert!(report(LightState::Working,3000,1600).is_err()); }
    #[test] fn period_steps() { assert!(report(LightState::Working,3050,1500).is_err()); }
    #[test] fn no_persistent_commands() {
        for state in [LightState::Off,LightState::Working,LightState::Waiting,LightState::Done,LightState::Error] {
            assert_eq!(report(state,3000,1500).unwrap()[4], 4);
        }
    }
}

/// Accept only a complete, checksum-valid response to the expected LED channel.
pub fn led_ack(input: &[u8], expected_channel: u8) -> Option<bool> {
    let body=if input.first()==Some(&REPORT_ID){&input[1..]}else{input};
    if body.len()<5 || body[..2]!=[0x5e,0x5e]{return None;}
    let end=3usize.checked_add(body[2] as usize)?;
    if end<6 || end>body.len(){return None;}
    if body[..end].iter().fold(0u8,|a,b|a^b)!=0{return None;}
    match body[3] {0x84 if body[4]==expected_channel=>Some(true),0x44=>Some(false),_=>None}
}
#[cfg(test)] mod ack_tests {
    use super::*;
    #[test] fn ack_valid(){assert_eq!(led_ack(&[2,94,94,3,0x84,2,0x85],2),Some(true));}
    #[test] fn other_channel_ignored(){assert_eq!(led_ack(&[94,94,3,0x84,2,0x85],1),None);}
    #[test] fn corrupt_ignored(){assert_eq!(led_ack(&[94,94,3,0x84,2,0x80],2),None);}
    #[test] fn short_ignored(){assert_eq!(led_ack(&[2],2),None);}
}
