use std::net::IpAddr;

/// Reject public Internet clients by default. An allowlist restricts peers to
/// exactly the configured VM addresses plus local loopback.
pub fn allowed_peer(peer: IpAddr, allowlist: &[String]) -> bool {
    if peer.is_loopback() { return true; }
    if !allowlist.is_empty() { return allowlist.iter().any(|a|a.parse::<IpAddr>().ok()==Some(peer)); }
    match peer {
        IpAddr::V4(a)=>a.is_private() || a.is_link_local(),
        IpAddr::V6(a)=> (a.segments()[0] & 0xfe00 == 0xfc00) || (a.segments()[0] & 0xffc0 == 0xfe80),
    }
}
/// Fixed-length comparison; never log Authorization headers or token values.
pub fn token_matches(expected:&str, received:&str)->bool {
    if expected.len()!=received.len(){return false;}
    expected.bytes().zip(received.bytes()).fold(0u8,|a,(x,y)|a|(x^y))==0
}
#[cfg(test)] mod tests {
    use super::*;
    #[test] fn public_refused(){assert!(!allowed_peer("8.8.8.8".parse().unwrap(),&[]));}
    #[test] fn vm_allowed(){assert!(allowed_peer("192.168.3.14".parse().unwrap(),&[]));}
    #[test] fn allowlist_restricts(){assert!(!allowed_peer("192.168.3.15".parse().unwrap(),&["192.168.3.14".into()]));}
    #[test] fn ipv6_ula(){assert!(allowed_peer("fd00::1".parse().unwrap(),&[]));}
    #[test] fn compare(){assert!(token_matches("abcd","abcd"));assert!(!token_matches("abcd","abce"));}
}
