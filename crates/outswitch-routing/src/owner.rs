use crate::packet::Key;
use anyhow::{Result,bail,ensure};
use std::{collections::BTreeSet,net::{IpAddr,Ipv4Addr,Ipv6Addr},ptr};
use windows_sys::Win32::{Foundation::CloseHandle,NetworkManagement::IpHelper::*,Networking::WinSock::*,System::Threading::*};
pub fn path(pid:u32)->Option<String>{unsafe{
    let handle=OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION,0,pid);if handle.is_null(){return None;}
    let mut text=vec![0;32768];let mut n=text.len()as u32;let ok=QueryFullProcessImageNameW(handle,0,text.as_mut_ptr(),&mut n);CloseHandle(handle);
    (ok!=0).then(||String::from_utf16_lossy(&text[..n as usize]))
}}
pub fn lookup(key:Key)->Result<Option<u32>>{unsafe{
    let family=if key.local.is_ipv4(){AF_INET}else{AF_INET6} as u32;let mut size=0;
    let get=|p:*mut std::ffi::c_void,s:*mut u32|if key.tcp{GetExtendedTcpTable(p,s,0,family,TCP_TABLE_OWNER_PID_ALL,0)}else{GetExtendedUdpTable(p,s,0,family,UDP_TABLE_OWNER_PID,0)};
    let first=get(ptr::null_mut(),&mut size);ensure!(first==0||first==122,"Cannot read application ownership: Windows error {first}");
    for _ in 0..4{
        let mut buf=vec![0u64;(size as usize+7)/8];let code=get(buf.as_mut_ptr().cast(),&mut size);if code==122{continue;}ensure!(code==0,"Cannot read application ownership: Windows error {code}");
        if buf.is_empty(){return Ok(None);}let p=buf.as_ptr().cast::<u8>();let count=ptr::read_unaligned(p.cast::<u32>())as usize;let mut matches=BTreeSet::new();
        let mut add=|pid:u32,local:IpAddr,port:u32,remote:Option<(IpAddr,u32)>|{
            if u16::from_be(port as u16)!=key.local.port(){return;}
            if local!=key.local.ip()&&!local.is_unspecified(){return;}
            if let Some((ip,port))=remote{if ip!=key.remote.ip()||u16::from_be(port as u16)!=key.remote.port(){return;}}
            matches.insert(pid);
        };
        macro_rules! rows{($ty:ty,$f:expr)=>{{let stride=std::mem::size_of::<$ty>();ensure!(4+count*stride<=buf.len()*8,"Invalid endpoint table");for i in 0..count{$f(ptr::read_unaligned(p.add(4+i*stride).cast::<$ty>()));}}}}
        if key.tcp&&key.local.is_ipv4(){rows!(MIB_TCPROW_OWNER_PID,|r:MIB_TCPROW_OWNER_PID|add(r.dwOwningPid,Ipv4Addr::from(r.dwLocalAddr.to_ne_bytes()).into(),r.dwLocalPort,Some((Ipv4Addr::from(r.dwRemoteAddr.to_ne_bytes()).into(),r.dwRemotePort))));}
        else if key.tcp{rows!(MIB_TCP6ROW_OWNER_PID,|r:MIB_TCP6ROW_OWNER_PID|add(r.dwOwningPid,Ipv6Addr::from(r.ucLocalAddr).into(),r.dwLocalPort,Some((Ipv6Addr::from(r.ucRemoteAddr).into(),r.dwRemotePort))));}
        else if key.local.is_ipv4(){rows!(MIB_UDPROW_OWNER_PID,|r:MIB_UDPROW_OWNER_PID|add(r.dwOwningPid,Ipv4Addr::from(r.dwLocalAddr.to_ne_bytes()).into(),r.dwLocalPort,None));}
        else{rows!(MIB_UDP6ROW_OWNER_PID,|r:MIB_UDP6ROW_OWNER_PID|add(r.dwOwningPid,Ipv6Addr::from(r.ucLocalAddr).into(),r.dwLocalPort,None));}
        // Never guess when SO_REUSEADDR gives a UDP endpoint multiple owners.
        ensure!(matches.len()<=1,"Ambiguous shared application endpoint");return Ok(matches.into_iter().next());
    }
    bail!("Application endpoint table changed repeatedly")
}}
