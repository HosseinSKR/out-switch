//! Source-bound ICMP probes. Never change routes or adapter metrics.
use anyhow::{ensure, Context, Result};
use outswitch_common::Network;
use std::{net::Ipv4Addr, ptr};
use windows_sys::Win32::{Foundation::*, NetworkManagement::IpHelper::*, Networking::WinSock::*};

pub const TARGETS: [(&str, &str); 3] = [
    ("Cloudflare", "1.1.1.1"), ("Google", "8.8.8.8"), ("Shecan", "178.22.122.100"),
];

#[derive(Clone, Debug)]
pub struct Reply {
    pub sequence: u32,
    pub ms: Option<u32>,
    pub ttl: Option<u8>,
    pub bytes: u16,
    pub from: Option<Ipv4Addr>,
    pub error: String,
}

#[derive(Default, Debug)]
pub struct Summary {
    pub sent: usize, pub received: usize, pub lost: usize,
    pub min: Option<f64>, pub average: Option<f64>, pub max: Option<f64>,
    /// Mean absolute difference between consecutive successful probes.
    /// A loss breaks the consecutive sequence.
    pub jitter: Option<f64>,
}
impl Summary {
    pub fn from_replies(replies: &[Reply]) -> Self {
        let values: Vec<f64> = replies.iter().filter_map(|r|r.ms.map(f64::from)).collect();
        let diffs: Vec<f64> = replies.windows(2).filter_map(|pair|Some(
            (f64::from(pair[1].ms?) - f64::from(pair[0].ms?)).abs())).collect();
        Self { sent: replies.len(), received: values.len(), lost: replies.len()-values.len(),
            min: values.iter().copied().reduce(f64::min),
            max: values.iter().copied().reduce(f64::max),
            average: (!values.is_empty()).then(||values.iter().sum::<f64>()/values.len() as f64),
            jitter: (!diffs.is_empty()).then(||diffs.iter().sum::<f64>()/diffs.len() as f64) }
    }
    pub fn loss_percent(&self) -> f64 { if self.sent==0 {0.} else {100.*self.lost as f64/self.sent as f64} }
}

fn socket_address(ip: Ipv4Addr) -> SOCKADDR_INET {
    let mut address: SOCKADDR_IN = unsafe { std::mem::zeroed() };
    address.sin_family = AF_INET;
    address.sin_addr.S_un.S_addr = u32::from_ne_bytes(ip.octets());
    SOCKADDR_INET { Ipv4: address }
}

pub fn source(network: &Network) -> Result<Ipv4Addr> {
    ensure!(network.connected, "This network is disconnected");
    network.addresses.iter().filter_map(|a|a.parse::<Ipv4Addr>().ok())
        .find(|a|!a.is_unspecified()&&!a.is_loopback())
        .context("This network has no IPv4 address; these ping targets require IPv4")
}

/// Revalidate the adapter before each probe, including after hotplug or DHCP changes.
fn validate(network: &Network, source: Ipv4Addr, target: Ipv4Addr) -> Result<()> {
    let networks = super::scan()?;
    let current = networks.iter().find(|n|n.id==network.id).context("Network was removed")?;
    ensure!(current.connected && current.index==network.index && current.addresses.contains(&source.to_string()),
        "Network or source address changed; start a new test");
    ensure!(!networks.iter().any(|n|n.id!=network.id&&n.connected&&n.addresses.contains(&source.to_string())),
        "Another adapter shares this IP address; cannot isolate this ping test");
    unsafe {
        let mut interface: MIB_IPINTERFACE_ROW = std::mem::zeroed();
        InitializeIpInterfaceEntry(&mut interface);
        interface.Family=AF_INET; interface.InterfaceIndex=network.index;
        let code=GetIpInterfaceEntry(&mut interface);
        ensure!(code==0, "Cannot verify the selected adapter (Windows error {code})");
        ensure!(interface.WeakHostSend==0,
            "This adapter allows sending through other networks. An isolated ping test is unavailable");
        let mut route: MIB_IPFORWARD_ROW2 = std::mem::zeroed();
        let mut chosen_source: SOCKADDR_INET = std::mem::zeroed();
        let code=GetBestRoute2(ptr::null(),network.index,&socket_address(source),&socket_address(target),0,&mut route,&mut chosen_source);
        ensure!(code==0 && route.InterfaceIndex==network.index,
            "No route to {target} through this adapter (Windows error {code})");
    }
    Ok(())
}

struct Handle(HANDLE);
impl Drop for Handle { fn drop(&mut self) { unsafe { IcmpCloseHandle(self.0); } } }

pub fn probe(network: &Network, source: Ipv4Addr, target: Ipv4Addr, sequence: u32) -> Result<Reply> {
    validate(network,source,target)?;
    unsafe {
        let raw=IcmpCreateFile();
        ensure!(raw!=INVALID_HANDLE_VALUE && !raw.is_null(), "Cannot open ICMP: {}",std::io::Error::last_os_error());
        let handle=Handle(raw);
        let mut payload=[0x4Fu8;32];
        payload[..4].copy_from_slice(&sequence.to_le_bytes());
        // u64 storage gives the native reply structure its required alignment.
        let mut buffer=[0u64;128];
        let count=IcmpSendEcho2Ex(handle.0,ptr::null_mut(),None,ptr::null(),
            u32::from_ne_bytes(source.octets()),u32::from_ne_bytes(target.octets()),
            payload.as_ptr().cast(),payload.len() as u16,ptr::null(),
            buffer.as_mut_ptr().cast(),std::mem::size_of_val(&buffer) as u32,1500);
        let mut result=Reply {sequence,ms:None,ttl:None,bytes:0,from:None,error:String::new()};
        let status=if count==0 {GetLastError()} else {
            let reply=&*(buffer.as_ptr() as *const ICMP_ECHO_REPLY);
            result.from=Some(Ipv4Addr::from(reply.Address.to_ne_bytes()));
            if reply.Status==0 {
                ensure!(result.from==Some(target),"Reply came from an unexpected address");
                result.ms=Some(reply.RoundTripTime); result.ttl=Some(reply.Options.Ttl); result.bytes=reply.DataSize;
            }
            reply.Status
        };
        if status!=0 {
            result.error=match status {
                11010=>"Timed out after 1500 ms".into(),
                11002=>"Destination network unreachable".into(),
                11003=>"Destination host unreachable".into(),
                11004=>"Destination protocol unreachable".into(),
                11005=>"Destination port unreachable".into(),
                11013=>"TTL expired in transit".into(),
                _=>format!("ICMP error {status}: {}",std::io::Error::from_raw_os_error(status as i32)),
            };
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn reply(ms:Option<u32>) -> Reply { Reply {sequence:1,ms,ttl:None,bytes:32,from:None,error:String::new()} }
    #[test]
    fn loss_does_not_become_zero_latency_or_bridge_jitter() {
        let s=Summary::from_replies(&[reply(Some(10)),reply(Some(20)),reply(None),reply(Some(100)),reply(Some(80))]);
        assert_eq!((s.sent,s.received,s.lost),(5,4,1));
        assert_eq!((s.min,s.average,s.max,s.jitter),(Some(10.),Some(52.5),Some(100.),Some(15.)));
        assert_eq!(s.loss_percent(),20.);
    }
    #[test]
    fn empty_and_all_lost_have_no_latency() {
        let empty=Summary::from_replies(&[]);assert_eq!(empty.average,None);assert_eq!(empty.loss_percent(),0.);
        let lost=Summary::from_replies(&[reply(None),reply(None)]);
        assert_eq!(lost.loss_percent(),100.);assert_eq!((lost.min,lost.max,lost.jitter),(None,None,None));
        assert_eq!(Summary::from_replies(&[reply(Some(0))]).average,Some(0.));
    }
}
