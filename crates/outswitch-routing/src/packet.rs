use std::net::{IpAddr,Ipv4Addr,Ipv6Addr,SocketAddr};
#[derive(Clone,Copy,Debug,PartialEq,Eq,PartialOrd,Ord)]
pub struct Key{pub local:SocketAddr,pub remote:SocketAddr,pub tcp:bool}
#[derive(Clone,Copy,Debug)]
pub struct Packet{pub key:Key,pub transport:usize,pub payload:usize,pub syn:bool}
fn word(p:&[u8],i:usize)->u16{u16::from_be_bytes([p[i],p[i+1]])}
impl Packet{
    pub fn parse(p:&[u8])->Option<Self>{
        let (src,dst,mut offset,mut protocol)=match p.first()?>>4{
            4=>{if p.len()<20{return None;}let n=(p[0]&15)as usize*4;
                if n<20||n>p.len()||word(p,2)as usize!=p.len()||word(p,6)&0x3fff!=0{return None;}
                (IpAddr::V4(Ipv4Addr::new(p[12],p[13],p[14],p[15])),IpAddr::V4(Ipv4Addr::new(p[16],p[17],p[18],p[19])),n,p[9])},
            6=>{if p.len()<40||40+word(p,4)as usize!=p.len(){return None;}
                (IpAddr::V6(Ipv6Addr::from(<[u8;16]>::try_from(&p[8..24]).ok()?)),IpAddr::V6(Ipv6Addr::from(<[u8;16]>::try_from(&p[24..40]).ok()?)),40,p[6])},
            _=>return None,
        };
        for _ in 0..8{if ![0,43,60].contains(&protocol){break;}if offset+2>p.len(){return None;}let n=(p[offset+1]as usize+1)*8;protocol=p[offset];offset=offset.checked_add(n)?;if offset>p.len(){return None;}}
        let tcp=protocol==6;let payload=if tcp {
            if offset+20>p.len(){return None;}let n=(p[offset+12]>>4)as usize*4;if n<20{return None;}offset+n
        }else if protocol==17{
            if offset+8>p.len()||offset+word(p,offset+4)as usize!=p.len(){return None;}offset+8
        }else{return None;};
        if payload>p.len(){return None;}
        Some(Self{key:Key{local:SocketAddr::new(src,word(p,offset)),remote:SocketAddr::new(dst,word(p,offset+2)),tcp},transport:offset,payload,syn:tcp&&p[offset+13]&0x12==2})
    }
    pub fn rewrite(&self,p:&mut[u8],source:SocketAddr,destination:SocketAddr){
        match (source.ip(),destination.ip()){
            (IpAddr::V4(a),IpAddr::V4(b))=>{p[12..16].copy_from_slice(&a.octets());p[16..20].copy_from_slice(&b.octets());},
            (IpAddr::V6(a),IpAddr::V6(b))=>{p[8..24].copy_from_slice(&a.octets());p[24..40].copy_from_slice(&b.octets());},_=>unreachable!(),
        }
        p[self.transport..self.transport+2].copy_from_slice(&source.port().to_be_bytes());p[self.transport+2..self.transport+4].copy_from_slice(&destination.port().to_be_bytes());
    }
}
pub fn udp_reply(key:Key,payload:&[u8])->Option<Vec<u8>>{
    let header=if key.local.is_ipv4(){20}else{40};if payload.len()>65535-header-8{return None;}
    let mut p=vec![0;header+8+payload.len()];let size=p.len();
    if header==20 {p[0]=0x45;p[2..4].copy_from_slice(&(size as u16).to_be_bytes());p[8]=64;p[9]=17;}
    else{p[0]=0x60;p[4..6].copy_from_slice(&((size-40)as u16).to_be_bytes());p[6]=17;p[7]=64;}
    p[header+4..header+6].copy_from_slice(&((payload.len()+8)as u16).to_be_bytes());p[header+8..].copy_from_slice(payload);
    Packet{key,transport:header,payload:header+8,syn:false}.rewrite(&mut p,key.remote,key.local);Some(p)
}
pub fn dscp(p:&mut[u8],value:u8){if p[0]>>4==4{p[1]=(value<<2)|(p[1]&3);}else{let traffic=((p[0]&15)<<4)|(p[1]>>4);let next=(value<<2)|(traffic&3);p[0]=(p[0]&0xf0)|(next>>4);p[1]=(p[1]&15)|(next<<4);}}
/// A reset in the server-to-client direction, derived only from TCP header state.
pub fn tcp_reset(meta:Packet,original:&[u8])->Vec<u8>{
    let h=if meta.key.local.is_ipv4(){20}else{40};let mut p=vec![0;h+20];let size=p.len();
    if h==20{p[0]=0x45;p[2..4].copy_from_slice(&(size as u16).to_be_bytes());p[8]=64;p[9]=6;}
    else{p[0]=0x60;p[4..6].copy_from_slice(&20u16.to_be_bytes());p[6]=6;p[7]=64;}
    let t=meta.transport;p[h+12]=0x50;
    if original[t+13]&16!=0{p[h+4..h+8].copy_from_slice(&original[t+8..t+12]);p[h+13]=4;}
    else{
        let seq=u32::from_be_bytes(original[t+4..t+8].try_into().unwrap());
        let advance=(original.len()-meta.payload)as u32+u32::from(original[t+13]&2!=0)+u32::from(original[t+13]&1!=0);
        p[h+8..h+12].copy_from_slice(&seq.wrapping_add(advance).to_be_bytes());p[h+13]=20;
    }
    Packet{key:meta.key,transport:h,payload:h+20,syn:false}.rewrite(&mut p,meta.key.remote,meta.key.local);p
}
#[cfg(test)]mod tests{
    use super::*;
    #[test]fn udp_roundtrip_both_families(){for (a,b)in [("192.0.2.1:1234","1.1.1.1:53"),("[2001:db8::1]:1234","[2001:4860:4860::8888]:53")]{let key=Key{local:a.parse().unwrap(),remote:b.parse().unwrap(),tcp:false};let mut p=udp_reply(key,b"opaque data").unwrap();let meta=Packet::parse(&p).unwrap();assert_eq!(meta.key.local,key.remote);assert_eq!(meta.key.remote,key.local);assert_eq!(&p[meta.payload..],b"opaque data");meta.rewrite(&mut p,key.local,key.remote);assert_eq!(Packet::parse(&p).unwrap().key,key);}}
    #[test]fn malformed_lengths_and_fragments_rejected(){let key=Key{local:"192.0.2.1:1".parse().unwrap(),remote:"1.1.1.1:53".parse().unwrap(),tcp:false};let mut p=udp_reply(key,b"x").unwrap();p[6]=0x20;assert!(Packet::parse(&p).is_none());p[6]=0;p[24]=0xff;assert!(Packet::parse(&p).is_none());for n in 0..28{assert!(Packet::parse(&p[..n]).is_none());}}
    #[test]fn marking_preserves_ecn_and_ipv6_flow_label(){let mut p=[0x6a,0xb5];dscp(&mut p,46);assert_eq!(p,[0x6b,0xb5]);let mut p=[0x45,3];dscp(&mut p,8);assert_eq!(p[1],35);}
    #[test]fn tcp_reflection_preserves_sequence_and_payload_and_reset_uses_ack(){
        for (local,remote)in [("192.0.2.1:51000","1.1.1.1:443"),("[2001:db8::1]:51000","[2001:4860::1]:443")]{
            let key=Key{local:local.parse().unwrap(),remote:remote.parse().unwrap(),tcp:true};let h=if key.local.is_ipv4(){20}else{40};
            let mut p=vec![0;h+24];let len=p.len()as u16;
            if h==20{p[0]=0x45;p[2..4].copy_from_slice(&len.to_be_bytes());p[9]=6;}else{p[0]=0x60;p[4..6].copy_from_slice(&24u16.to_be_bytes());p[6]=6;}
            p[h+12]=0x50;p[h+13]=0x18;p[h+4..h+8].copy_from_slice(&100u32.to_be_bytes());p[h+8..h+12].copy_from_slice(&200u32.to_be_bytes());p[h+20..].copy_from_slice(b"TLS!");
            Packet{key,transport:h,payload:h+20,syn:false}.rewrite(&mut p,key.local,key.remote);
            let original=p.clone();let meta=Packet::parse(&p).unwrap();let reset=tcp_reset(meta,&p);let parsed=Packet::parse(&reset).unwrap();assert_eq!(parsed.key.local,key.remote);assert_eq!(&reset[h+4..h+8],&200u32.to_be_bytes());assert_eq!(reset[h+13],4);
            meta.rewrite(&mut p,key.remote,key.local);assert_eq!(&p[h+4..],&original[h+4..]);let reverse=Packet::parse(&p).unwrap();reverse.rewrite(&mut p,key.local,key.remote);assert_eq!(p,original);
            p[h+13]=2;let reset=tcp_reset(Packet::parse(&p).unwrap(),&p);assert_eq!(&reset[h+8..h+12],&105u32.to_be_bytes());assert_eq!(reset[h+13],20);
        }
    }
}
