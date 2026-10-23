//! Per-executable TCP/UDP relay using the separately distributed signed WinDivert driver.
mod divert; mod owner; mod packet; mod relay; mod traffic;
use anyhow::{Result,ensure};
use outswitch_common::*;
use outswitch_core::{select_network,RoutingEngine,Selection};
use relay::{Counters,Policy,Runtime,Target};
use std::{collections::{BTreeMap,BTreeSet},net::IpAddr,sync::{Arc,atomic::Ordering},time::Instant};
use windows_sys::Win32::{Foundation::CloseHandle,Security::{GetTokenInformation,TOKEN_ELEVATION,TOKEN_QUERY,TokenElevation},System::Threading::{GetCurrentProcess,OpenProcessToken}};
pub fn availability_error()->Option<String>{
    let dir=match divert::directory(){Ok(d)=>d,Err(e)=>return Some(e.to_string())};
    for file in ["WinDivert.dll","WinDivert64.sys"]{if !dir.join(file).is_file(){return Some(format!("Missing {file}. Repair or reinstall the complete Out-Switch package."));}}
    unsafe{
        let mut token=std::ptr::null_mut();if OpenProcessToken(GetCurrentProcess(),TOKEN_QUERY,&mut token)==0{return Some("Cannot check service permissions".into());}
        let mut info:TOKEN_ELEVATION=std::mem::zeroed();let mut n=0;let ok=GetTokenInformation(token,TokenElevation,(&mut info as *mut TOKEN_ELEVATION).cast(),std::mem::size_of::<TOKEN_ELEVATION>()as u32,&mut n);CloseHandle(token);
        if ok==0||info.TokenIsElevated==0{return Some("Install Out-Switch to enable routing through its background service. The portable monitor has no administrator permissions.".into());}
    }None
}
/// Explicit driver smoke test: a false filter cannot intercept any traffic.
pub fn probe()->Result<()>{let api=divert::Api::load()?;let driver=api.open("false")?;drop(driver);Ok(())}
pub struct WindowsRoutingEngine{
    runtime:Option<Runtime>,automatic:BTreeSet<String>,plan:BTreeMap<String,Policy>,pub statuses:BTreeMap<String,String>,selected:BTreeMap<String,String>,
    counters:Counters,last:BTreeMap<String,(u64,u64,Instant)>,
}
impl Default for WindowsRoutingEngine{fn default()->Self{Self{runtime:None,automatic:BTreeSet::new(),plan:BTreeMap::new(),statuses:BTreeMap::new(),selected:BTreeMap::new(),counters:BTreeMap::new(),last:BTreeMap::new()}}}
impl WindowsRoutingEngine{
    pub fn enabled(&self)->bool{self.runtime.is_some()}
    pub fn failure(&self)->Option<String>{self.runtime.as_ref().and_then(Runtime::failure)}
    pub fn notice(&self)->Option<String>{self.runtime.as_ref().and_then(Runtime::notice)}
    pub fn measurements(&mut self,apps:&mut [Application]){
        let now=Instant::now();for (name,traffic)in &self.counters{
            if !self.plan.contains_key(name){continue;}
            let up=traffic.uploaded.load(Ordering::Relaxed);let down=traffic.downloaded.load(Ordering::Relaxed);
            if let Some(app)=apps.iter_mut().find(|a|identity(&a.path)==*name){
                let (old_up,old_down,at)=self.last.get(name).copied().unwrap_or((up,down,now));let elapsed=now.duration_since(at).as_secs_f64().max(0.001);
                app.uploaded=Some(up);app.downloaded=Some(down);app.upload_bps=Some(up.saturating_sub(old_up)as f64/elapsed);app.download_bps=Some(down.saturating_sub(old_down)as f64/elapsed);
                // The original socket is reflected locally; its IP is not proof of upstream egress.
                if self.plan.contains_key(name){app.observed_networks=traffic.observed.lock().unwrap().iter().cloned().collect();}
            }self.last.insert(name.clone(),(up,down,now));
        }
    }
}
impl RoutingEngine for WindowsRoutingEngine{
    fn apply(&mut self,rules:&[Rule],networks:&[Network])->Result<()>{
        let automatic:BTreeSet<String>=rules.iter().filter(|r|r.network.is_none()).map(|r|identity(&r.path)).collect();
        let mut plan=BTreeMap::new();let mut statuses=BTreeMap::new();let mut selected=BTreeMap::new();let own=std::env::current_exe()?.to_string_lossy().into_owned();
        for rule in rules{
            rule.validate()?;ensure!(identity(&rule.path)!=identity(&own),"The Out-Switch service cannot route itself");let name=identity(&rule.path);
            let (target,blocked,status)=match select_network(rule,networks,self.selected.get(&name).map(String::as_str)){
                Selection::Automatic=>{ensure!(rule.upload_mbps.is_none()&&rule.download_mbps.is_none()&&rule.priority==Priority::Normal,"Choose a network before enabling bandwidth limits or QoS");statuses.insert(name,"Automatic — Windows chooses".into());continue;},
                Selection::Network(id)=>{
                    let n=networks.iter().find(|n|n.id==id).unwrap();ensure!(n.strong_host,"{}: strong-host sending is required for adapter isolation",n.label);
                    let usable:Vec<IpAddr>=n.addresses.iter().filter_map(|a|a.parse().ok()).filter(|ip:&IpAddr|!ip.is_loopback()&&!ip.is_unspecified()&&match ip{IpAddr::V4(a)=>!a.is_link_local(),IpAddr::V6(a)=>(a.segments()[0]&0xffc0)!=0xfe80}).collect();
                    ensure!(!usable.is_empty(),"{} has no usable address",n.label);
                    for ip in &usable{ensure!(!networks.iter().any(|other|other.id!=n.id&&other.connected&&other.addresses.contains(&ip.to_string())),"{} has a source address shared with another adapter",n.label);}
                    selected.insert(name.clone(),id.clone());
                    (Some(Target{id,index:n.index,ipv6_index:n.ipv6_index,v4:usable.iter().find(|a|a.is_ipv4()).copied(),v6:usable.iter().find(|a|a.is_ipv6()).copied()}),false,format!("{} — relay ready for new TCP / UDP connections",n.label))
                },
                Selection::Unavailable(_)=>(None,true,"Selected network unavailable — new traffic blocked".into()),
                Selection::ApprovalRequired(_)=>(None,true,"Choose the fallback network to resume this application".into()),
            };
            plan.insert(name.clone(),Policy{rule:rule.clone(),target,blocked});statuses.insert(name,status);
        }
        if plan!=self.plan||automatic!=self.automatic||(!plan.is_empty()&&self.runtime.is_none()){
            for key in plan.keys(){let counter=self.counters.entry(key.clone()).or_insert_with(||Arc::new(traffic::Traffic::default()));if plan.get(key)!=self.plan.get(key){counter.reset_waits();counter.observed.lock().unwrap().clear();}}
            if plan.is_empty(){self.runtime.take();}
            else if let Some(runtime)=&self.runtime{runtime.update(plan.clone(),automatic.clone(),self.counters.clone());}
            else{self.runtime=Some(Runtime::start(plan.clone(),automatic.clone(),self.counters.clone())?);}self.plan=plan;self.automatic=automatic;
        }
        self.statuses=statuses;self.selected=selected;Ok(())
    }
    fn clear(&mut self)->Result<()>{self.runtime.take();self.plan.clear();self.automatic.clear();self.statuses.clear();self.selected.clear();Ok(())}
}
