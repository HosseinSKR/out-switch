use anyhow::{bail, Result};
use outswitch_common::Network;
use std::{collections::BTreeMap, ffi::CStr, net::{IpAddr, Ipv4Addr, Ipv6Addr}, ptr, time::Instant};
use windows_sys::Win32::{NetworkManagement::{IpHelper::*, Ndis::*}, Networking::WinSock::*};
pub mod ping;

unsafe fn wide(p: *const u16) -> String {
    if p.is_null() { return String::new(); } let mut len=0; while *p.add(len)!=0 && len<32768 {len+=1;}
    String::from_utf16_lossy(std::slice::from_raw_parts(p,len))
}
pub unsafe fn address(p: *const SOCKADDR) -> Option<String> {
    if p.is_null() { return None; }
    match (*p).sa_family {
        AF_INET => Some(Ipv4Addr::from((*(p as *const SOCKADDR_IN)).sin_addr.S_un.S_addr.to_ne_bytes()).to_string()),
        AF_INET6 => Some(Ipv6Addr::from((*(p as *const SOCKADDR_IN6)).sin6_addr.u.Byte).to_string()), _=>None
    }
}
pub fn scan() -> Result<Vec<Network>> { unsafe {
    let mut bytes=16_384u32;
    let mut buffer=vec![0u64;(bytes as usize+7)/8];
    for _ in 0..4 {
        let code=GetAdaptersAddresses(AF_UNSPEC as u32,GAA_FLAG_INCLUDE_GATEWAYS,ptr::null(),buffer.as_mut_ptr().cast(),&mut bytes);
        if code==111 {buffer.resize((bytes as usize+7)/8,0);continue;}
        if code!=0 {bail!("GetAdaptersAddresses: {code}");}
        let mut p=buffer.as_ptr() as *const IP_ADAPTER_ADDRESSES_LH;
        let mut result=Vec::new();
        while !p.is_null() {
            let a=&*p;
            if a.IfType!=IF_TYPE_SOFTWARE_LOOPBACK {
                let id=CStr::from_ptr(a.AdapterName.cast()).to_string_lossy().into_owned();
                let mut n=Network {id, index:a.Anonymous1.Anonymous.IfIndex, ipv6_index:a.Ipv6IfIndex, name:wide(a.FriendlyName), description:wide(a.Description),
                    connected:a.OperStatus==IfOperStatusUp, link_bps:a.ReceiveLinkSpeed,
                    kind:match a.IfType {71=>"Wi-Fi",6=>"Ethernet",23=>"PPP",131=>"Tunnel / VPN",_=>"Virtual / other"}.into(), ..Default::default()};
                let mut unicast=a.FirstUnicastAddress;
                while !unicast.is_null() {
                    let u=&*unicast;
                    if u.DadState==IpDadStatePreferred { if let Some(ip)=address(u.Address.lpSockaddr) {n.addresses.push(ip);} }
                    unicast=u.Next;
                }
                let mut gateway=a.FirstGatewayAddress;
                while !gateway.is_null() {let g=&*gateway;if let Some(ip)=address(g.Address.lpSockaddr){n.gateways.push(ip);}gateway=g.Next;}
                let mut dns=a.FirstDnsServerAddress;
                while !dns.is_null() {let d=&*dns;if let Some(ip)=address(d.Address.lpSockaddr){n.dns.push(ip);}dns=d.Next;}
                let mut row:MIB_IF_ROW2=std::mem::zeroed(); row.InterfaceLuid=a.Luid;
                if GetIfEntry2(&mut row)==0 {n.received=row.InOctets;n.sent=row.OutOctets;}
                n.strong_host=true;
                for family in [AF_INET,AF_INET6] {
                    if !n.addresses.iter().any(|s| s.contains(':')==(family==AF_INET6)){continue;}
                    let mut iprow:MIB_IPINTERFACE_ROW=std::mem::zeroed(); iprow.Family=family;iprow.InterfaceLuid=a.Luid;
                    if GetIpInterfaceEntry(&mut iprow)!=0 || iprow.WeakHostSend!=0 {n.strong_host=false;}
                }
                result.push(n);
            }
            p=a.Next;
        }
        result.sort_by(|a,b|(b.connected&&!b.gateways.is_empty()).cmp(&(a.connected&&!a.gateways.is_empty())).then(a.id.cmp(&b.id)));return Ok(result);
    }
    bail!("Adapter list changed repeatedly; retry")
} }

pub struct NetworkManager { last: BTreeMap<String,(u64,u64,Instant)> }
impl Default for NetworkManager {fn default()->Self{Self{last:BTreeMap::new()}}}
impl NetworkManager {
    pub fn snapshot(&mut self)->Result<Vec<Network>> {
        let mut networks=scan()?;let now=Instant::now();
        for n in &mut networks {
            if let Some((rx,tx,at))=self.last.insert(n.id.clone(),(n.received,n.sent,now)) {
                let secs=now.duration_since(at).as_secs_f64().max(0.001);
                n.down_bps=n.received.saturating_sub(rx) as f64/secs;n.up_bps=n.sent.saturating_sub(tx) as f64/secs;
            }
        }
        self.last.retain(|id,_|networks.iter().any(|n|n.id==*id)); Ok(networks)
    }
}
/// User-triggered TCP connectivity probe, no request payload and no cloud API.
pub fn test(n:&Network)->Result<f64> {
    let ip=n.addresses.iter().filter_map(|a|a.parse::<IpAddr>().ok()).find(|a|a.is_ipv4()).ok_or_else(||anyhow::anyhow!("No usable IPv4 address"))?;
    let socket=socket2::Socket::new(socket2::Domain::IPV4,socket2::Type::STREAM,Some(socket2::Protocol::TCP))?;
    socket.bind(&std::net::SocketAddr::new(ip,0).into())?;
    let start=Instant::now();
    socket.connect_timeout(&"1.1.1.1:443".parse::<std::net::SocketAddr>()?.into(),std::time::Duration::from_secs(3))?;
    Ok(start.elapsed().as_secs_f64()*1000.)
}

