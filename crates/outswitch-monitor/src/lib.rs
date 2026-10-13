use anyhow::{bail,Result};
use outswitch_common::*;
use std::{collections::BTreeMap, net::{Ipv4Addr,Ipv6Addr},ptr};
use windows_sys::Win32::{Foundation::CloseHandle,NetworkManagement::IpHelper::*,Networking::WinSock::*,System::Threading::*};

fn process_path(pid:u32)->Option<String>{unsafe{
    let handle=OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION,0,pid);if handle.is_null(){return None;}
    let mut buf=vec![0u16;32768];let mut len=buf.len() as u32;
    let ok=QueryFullProcessImageNameW(handle,0,buf.as_mut_ptr(),&mut len);CloseHandle(handle);
    (ok!=0).then(||String::from_utf16_lossy(&buf[..len as usize]))
}}

#[cfg(test)]mod tests{
    use super::*;
    #[test]fn detects_real_tcp_and_udp_sockets_for_this_process(){
        let listener=std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let client=std::net::TcpStream::connect(listener.local_addr().unwrap()).unwrap();let (_server,_)=listener.accept().unwrap();
        let _udp=std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let apps=snapshot(&[]).unwrap();let current=apps.iter().find(|a|a.pids.contains(&std::process::id())).expect("Current process must be found");
        assert!(current.tcp_connections>=2);assert!(current.udp_endpoints>=1);assert!(current.observed_networks.is_empty());assert!(current.download_bps.is_none());drop(client);
    }
}
unsafe fn table(tcp:bool,family:u32)->Result<Vec<u64>>{
    let mut size=0;let get=|p:*mut std::ffi::c_void,s:*mut u32|if tcp{GetExtendedTcpTable(p,s,0,family,TCP_TABLE_OWNER_PID_ALL,0)}else{GetExtendedUdpTable(p,s,0,family,UDP_TABLE_OWNER_PID,0)};
    let code=get(ptr::null_mut(),&mut size);if code!=0&&code!=122{bail!("Endpoint table: {code}");}
    for _ in 0..4 {let mut buf=vec![0u64;(size as usize+7)/8];let code=get(buf.as_mut_ptr().cast(),&mut size);
        if code==122{continue;}if code!=0{bail!("Endpoint table: {code}");}return Ok(buf);}
    bail!("Endpoint table changed repeatedly")
}
pub fn snapshot(networks:&[Network])->Result<Vec<Application>>{unsafe{
    let mut apps:BTreeMap<String,Application>=BTreeMap::new();let mut paths:BTreeMap<u32,Option<String>>=BTreeMap::new();
    let mut add=|pid:u32,ip:String,tcp:bool| {
        let Some(path)=paths.entry(pid).or_insert_with(||process_path(pid)).clone() else{return;};
        let key=identity(&path);let app=apps.entry(key).or_insert_with(||Application{path:path.clone(),name:path.rsplit('\\').next().unwrap_or(&path).trim_end_matches(".exe").into(),..Default::default()});
        if !app.pids.contains(&pid){app.pids.push(pid);}
        if tcp{app.tcp_connections+=1;}else{app.udp_endpoints+=1;}
        if ip!="0.0.0.0"&&ip!="::"&&!app.local_addresses.contains(&ip){app.local_addresses.push(ip.clone());}
        // TCP local address is evidence of a bound connection, not a packet capture.
        if tcp {for n in networks {if n.addresses.contains(&ip)&&!app.observed_networks.contains(&n.id){app.observed_networks.push(n.id.clone());}}}
    };
    for family in [AF_INET as u32,AF_INET6 as u32]{
        for tcp in [true,false]{
            let buffer=table(tcp,family)?;if buffer.is_empty(){continue;}
            let p=buffer.as_ptr().cast::<u8>();let count=*(p as *const u32) as usize;
            macro_rules! rows {($ty:ty,$body:expr)=>{{let size=std::mem::size_of::<$ty>();anyhow::ensure!(4+count*size<=buffer.len()*8,"Invalid endpoint table");for i in 0..count {let row=ptr::read_unaligned(p.add(4+i*size) as *const $ty);$body(row);}}}}
            if tcp&&family==AF_INET as u32 {rows!(MIB_TCPROW_OWNER_PID,|r:MIB_TCPROW_OWNER_PID|{if r.dwState==5{add(r.dwOwningPid,Ipv4Addr::from(r.dwLocalAddr.to_ne_bytes()).to_string(),true);}});}
            else if tcp {rows!(MIB_TCP6ROW_OWNER_PID,|r:MIB_TCP6ROW_OWNER_PID|{if r.dwState==5{add(r.dwOwningPid,Ipv6Addr::from(r.ucLocalAddr).to_string(),true);}});}
            else if family==AF_INET as u32 {rows!(MIB_UDPROW_OWNER_PID,|r:MIB_UDPROW_OWNER_PID|add(r.dwOwningPid,Ipv4Addr::from(r.dwLocalAddr.to_ne_bytes()).to_string(),false));}
            else {rows!(MIB_UDP6ROW_OWNER_PID,|r:MIB_UDP6ROW_OWNER_PID|add(r.dwOwningPid,Ipv6Addr::from(r.ucLocalAddr).to_string(),false));}
        }
    }
    Ok(apps.into_values().collect())
}}
