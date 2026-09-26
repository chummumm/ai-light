//! Deterministic, original traffic-light artwork. No Node, fonts or image library.
use std::{fs,io,path::Path};
fn crc32(data:&[u8])->u32 {
    let mut c=!0u32;
    for b in data { c^=u32::from(*b); for _ in 0..8 {c=(c>>1)^if c&1!=0{0xedb88320}else{0};} }
    !c
}
fn chunk(out:&mut Vec<u8>,tag:&[u8;4],data:&[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let start=out.len();out.extend_from_slice(tag);out.extend_from_slice(data);
    out.extend_from_slice(&crc32(&out[start..]).to_be_bytes());
}
fn png(size:usize,mode:&str)->Vec<u8> {
    let mut raw=vec![0u8;size*(size*4+1)];
    let colors=[[226f64,120.,103.],[230.,184.,106.],[126.,194.,153.]];
    for y in 0..size {for x in 0..size {
        let u=(x as f64+0.5)/size as f64;let v=(y as f64+0.5)/size as f64;
        let mut c=[17u8,24,28,0];
        let qx=((u-0.5).abs()-0.23).max(0.);let qy=((v-0.5).abs()-0.32).max(0.);
        if qx*qx+qy*qy<0.11*0.11 {c=[32+(12.*(1.-u)).round() as u8,41+(10.*(1.-u)).round() as u8,46+(10.*(1.-u)).round() as u8,255];}
        for i in 0..3 {
            let cy=0.26+i as f64*0.24;let r=((u-0.5).powi(2)+(v-cy).powi(2)).sqrt();
            if r<0.103 {
                c=[58,68,73,255];
                if mode=="all" || mode==["red","yellow","green"][i] {
                    for k in 0..3 {c[k]=(colors[i][k]*(0.9+0.16*(1.-r/0.103))).round().min(255.) as u8;}
                }
            }
        }
        let p=y*(size*4+1)+1+x*4;raw[p..p+4].copy_from_slice(&c);
    }}
    // zlib stream containing uncompressed DEFLATE blocks (PNG accepts these).
    let mut z=vec![0x78,0x01];
    let chunks=raw.chunks(65535);let total=chunks.len();
    for (i,b) in chunks.enumerate() {z.push(if i+1==total{1}else{0});let n=b.len() as u16;z.extend_from_slice(&n.to_le_bytes());z.extend_from_slice(&(!n).to_le_bytes());z.extend_from_slice(b);}
    let (mut a,mut b)=(1u32,0u32);for x in &raw{a=(a+u32::from(*x))%65521;b=(b+a)%65521;}z.extend_from_slice(&((b<<16)|a).to_be_bytes());
    let mut out=vec![137,80,78,71,13,10,26,10];let mut hdr=Vec::new();hdr.extend_from_slice(&(size as u32).to_be_bytes());hdr.extend_from_slice(&(size as u32).to_be_bytes());hdr.extend_from_slice(&[8,6,0,0,0]);
    chunk(&mut out,b"IHDR",&hdr);chunk(&mut out,b"IDAT",&z);chunk(&mut out,b"IEND",&[]);out
}
fn write(path:&Path,data:&[u8])->io::Result<()> {if fs::read(path).ok().as_deref()!=Some(data){fs::write(path,data)?;}Ok(())}
pub fn generate()->io::Result<()> {
    let dir=Path::new("icons");fs::create_dir_all(dir)?;
    for n in [32,128,256]{write(&dir.join(format!("{n}x{n}.png")),&png(n,"all"))?;}
    for mode in ["red","yellow","green","off"]{write(&dir.join(format!("tray-{mode}.png")),&png(32,mode))?;}
    let sizes=[16usize,24,32,48,64,128,256];let images:Vec<_>=sizes.iter().map(|n|png(*n,"all")).collect();
    let mut ico=vec![0,0,1,0,sizes.len() as u8,0];let mut offset=6+16*sizes.len();
    for (n,img) in sizes.iter().zip(&images){let d=if *n==256{0}else{*n as u8};ico.extend_from_slice(&[d,d,0,0,1,0,32,0]);ico.extend_from_slice(&(img.len() as u32).to_le_bytes());ico.extend_from_slice(&(offset as u32).to_le_bytes());offset+=img.len();}
    for img in images{ico.extend_from_slice(&img);}
    write(&dir.join("icon.ico"),&ico)
}
