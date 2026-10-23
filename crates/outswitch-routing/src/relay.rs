use crate::{divert::{Address,Api,Driver},owner,packet::{self,Key,Packet},traffic::{self,Traffic}};
use anyhow::{Result,Context,ensure};
use outswitch_common::{identity,Rule};
use socket2::{Domain,Protocol,Socket,Type};
use std::{collections::{BTreeMap,BTreeSet},io::{Read,Write},net::{IpAddr,Ipv4Addr,Ipv6Addr,SocketAddr,TcpListener,TcpStream,UdpSocket,Shutdown},os::windows::io::AsRawSocket,sync::{Arc,Mutex,RwLock,mpsc::{self,SyncSender},atomic::{AtomicBool,Ordering}},thread::{self,JoinHandle},time::{Instant,Duration}};
use windows_sys::Win32::Networking::WinSock::{setsockopt,IPPROTO_IP,IPPROTO_IPV6,IP_UNICAST_IF,IPV6_UNICAST_IF,SOCKET_ERROR};

const MAX_FLOWS:usize=256;
#[derive(Clone,Debug,PartialEq)]
pub struct Policy{pub rule:Rule,pub target:Option<Target>,pub blocked:bool}
#[derive(Clone,Debug,PartialEq)]
pub struct Target{pub id:String,pub index:u32,pub ipv6_index:u32,pub v4:Option<IpAddr>,pub v6:Option<IpAddr>}
pub type Counters=BTreeMap<String,Arc<Traffic>>;
struct Flow{
    key:Key,executable:String,synthetic:u16,original:Address,policy:Policy,traffic:Arc<Traffic>,closed:AtomicBool,
    last:Mutex<Instant>,udp:Mutex<Option<SyncSender<Vec<u8>>>>,accepted:AtomicBool,created:Instant,reset:Mutex<Option<Vec<u8>>>,
}
impl Flow{fn count(&self,download:bool,bytes:usize){self.traffic.count(download,bytes);if let Some(target)=&self.policy.target{self.traffic.observed.lock().unwrap().insert(target.id.clone());}}}
struct Tables{flows:BTreeMap<Key,Arc<Flow>>,reverse:BTreeMap<(bool,u16),Arc<Flow>>,upstreams:BTreeMap<Key,u8>,pass:BTreeMap<Key,Instant>,next:u16}
struct Settings{policies:BTreeMap<String,Policy>,automatic:BTreeSet<String>,counters:Counters}
struct Shared{
    stop:AtomicBool,driver:Arc<Driver>,settings:RwLock<Settings>,
    tables:Mutex<Tables>,error:Mutex<Option<String>>,workers:Mutex<Vec<JoinHandle<()>>>,ports:[u16;2],
}
pub struct Runtime{shared:Arc<Shared>,capture:Option<JoinHandle<()>>,listeners:Vec<JoinHandle<()>>}
impl Shared{
    fn error(&self,error:impl ToString){let mut e=self.error.lock().unwrap();if e.is_none(){*e=Some(error.to_string());}}
    fn spawn(self:&Arc<Self>,f:impl FnOnce()+Send+'static)->Result<()>{
        let mut workers=self.workers.lock().unwrap();
        let mut i=0;while i<workers.len(){if workers[i].is_finished(){let job=workers.swap_remove(i);if job.join().is_err(){self.error("A traffic worker stopped unexpectedly");}}else{i+=1;}}
        ensure!(workers.len()<MAX_FLOWS*3,"Connection worker limit reached; reconnect after other transfers finish");
        workers.push(thread::Builder::new().name("OutSwitch relay".into()).spawn(f)?);Ok(())
    }
    fn inject(&self,p:&mut[u8],a:&mut Address)->Result<()>{self.driver.send(p,a)}
}
fn loopback(v4:bool)->IpAddr{if v4{Ipv4Addr::LOCALHOST.into()}else{Ipv6Addr::LOCALHOST.into()}}
fn listener(v4:bool)->Result<TcpListener>{
    let s=Socket::new(if v4{Domain::IPV4}else{Domain::IPV6},Type::STREAM,Some(Protocol::TCP))?;
    if !v4{s.set_only_v6(true)?;}
    s.bind(&SocketAddr::new(loopback(v4),0).into())?;s.listen(128)?;s.set_nonblocking(true)?;Ok(s.into())
}
impl Runtime{
    pub fn start(policies:BTreeMap<String,Policy>,automatic:BTreeSet<String>,counters:Counters)->Result<Self>{
        // All potentially failing setup occurs before capturing any user packets.
        for ip in ["192.0.2.1:1","[2001:db8::1]:1"]{for tcp in [true,false]{let endpoint:SocketAddr=ip.parse()?;owner::lookup(Key{local:endpoint,remote:endpoint,tcp})?;}}
        let v4=listener(true)?;let v6=listener(false)?;let ports=[v4.local_addr()?.port(),v6.local_addr()?.port()];
        let filter=format!("outbound and !impostor and (( !loopback and (tcp or udp)) or (loopback and tcp and (tcp.SrcPort == {} or tcp.SrcPort == {})))",ports[0],ports[1]);
        let api=Api::load()?;let driver=api.open(&filter)?;
        let shared=Arc::new(Shared{stop:AtomicBool::new(false),driver,settings:RwLock::new(Settings{policies,automatic,counters}),tables:Mutex::new(Tables{flows:BTreeMap::new(),reverse:BTreeMap::new(),upstreams:BTreeMap::new(),pass:BTreeMap::new(),next:10000}),error:Mutex::new(None),workers:Mutex::new(vec![]),ports});
        let mut runtime=Self{shared:shared.clone(),capture:None,listeners:vec![]};
        for (index,listener)in [v4,v6].into_iter().enumerate(){let s=shared.clone();runtime.listeners.push(thread::Builder::new().name("OutSwitch accept".into()).spawn(move||{
            while !s.stop.load(Ordering::Relaxed){match listener.accept(){
                Ok((stream,peer))=>{
                    let flow=s.tables.lock().unwrap().reverse.get(&(index==0,peer.port())).cloned();
                    // Injected synthetic peers have no real outbound socket. Reject
                    // a local process that tried to bind/guess a synthetic port.
                    let synthetic_peer=stream.local_addr().ok().is_some_and(|remote|matches!(owner::lookup(Key{local:peer,remote,tcp:true}),Ok(None)));
                    if let Some(flow)=flow.filter(|f|synthetic_peer&&peer.ip()==loopback(index==0)&&!f.closed.load(Ordering::Relaxed)&&!f.accepted.swap(true,Ordering::Relaxed)){
                        let worker=s.clone();let failed=flow.clone();if let Err(e)=s.spawn(move||tcp_relay(worker,flow,stream)){failed.closed.store(true,Ordering::Relaxed);s.error(e);}
                    }
                },Err(e)if e.kind()==std::io::ErrorKind::WouldBlock=>thread::sleep(Duration::from_millis(10)),
                Err(e)=>{s.error(e);break;},
            }}
        })?);}
        let s=shared.clone();runtime.capture=Some(thread::Builder::new().name("OutSwitch packets".into()).spawn(move||{
            let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||capture(&s)));
            if result.is_err(){s.error("Packet processing stopped unexpectedly");}
            s.stop.store(true,Ordering::Relaxed);s.driver.shutdown();
        })?);Ok(runtime)
    }
    pub fn failure(&self)->Option<String>{if self.shared.stop.load(Ordering::Relaxed){Some(self.shared.error.lock().unwrap().clone().unwrap_or_else(||"Routing worker stopped".into()))}else{None}}
    pub fn notice(&self)->Option<String>{if self.shared.stop.load(Ordering::Relaxed){None}else{self.shared.error.lock().unwrap().take()}}
    pub fn update(&self,policies:BTreeMap<String,Policy>,automatic:BTreeSet<String>,counters:Counters){
        let mut settings=self.shared.settings.write().unwrap();let mut tables=self.shared.tables.lock().unwrap();
        for flow in tables.flows.values(){if policy_for(&flow.executable,&policies,&automatic).map(|(_,p)|p)!=Some(&flow.policy){close_flow(&self.shared,flow);}}
        // Rules can make a formerly automatic executable eligible immediately.
        tables.pass.clear();*settings=Settings{policies,automatic,counters};
    }
}
fn close_flow(s:&Shared,flow:&Flow){
    if !flow.closed.swap(true,Ordering::Relaxed){if let Some(mut reset)=flow.reset.lock().unwrap().clone(){let mut address=flow.original;address.inbound();let _=s.inject(&mut reset,&mut address);}}
}
impl Drop for Runtime{fn drop(&mut self){
    for flow in self.shared.tables.lock().unwrap().flows.values(){close_flow(&self.shared,flow);}
    self.shared.stop.store(true,Ordering::Relaxed);self.shared.driver.shutdown();
    if let Some(t)=self.capture.take(){let _=t.join();}for t in self.listeners.drain(..){let _=t.join();}
    // No new workers can be created after capture and accept threads have joined.
    let jobs=std::mem::take(&mut *self.shared.workers.lock().unwrap());for t in jobs{let _=t.join();}
}}
fn capture(s:&Arc<Shared>){
    let mut bytes=vec![0u8;65575];
    while !s.stop.load(Ordering::Relaxed){
        let (n,mut address)=match s.driver.recv(&mut bytes){Ok(x)=>x,Err(e)=>{if !s.stop.load(Ordering::Relaxed){s.error(e);}break;}};
        let p=&mut bytes[..n];
        let result=process(s,p,&mut address);
        // A disappearing endpoint, ambiguous shared UDP socket or failed packet
        // injection affects this packet only. Never tear down every app's routes.
        // Drop on failure: sending the original packet would bypass its policy.
        if let Err(e)=result{s.error(format!("Packet skipped; routing remains active: {e}"));}
    }
}
fn process(s:&Arc<Shared>,p:&mut[u8],address:&mut Address)->Result<()>{
    let settings=s.settings.read().unwrap();
    let Some(meta)=Packet::parse(p)else{return s.inject(p,address);};let key=meta.key;let v4=key.local.is_ipv4();let port=s.ports[if v4{0}else{1}];
    if key.tcp&&key.local.ip()==loopback(v4)&&key.local.port()==port{
        let flow=s.tables.lock().unwrap().reverse.get(&(v4,key.remote.port())).cloned();
        if let Some(flow)=flow{
            meta.rewrite(p,flow.key.remote,flow.key.local);*address=flow.original;address.inbound();address.flags&=!(1<<18);
            return s.inject(p,address);
        }
        return Ok(()); // Never emit an unmatched internal relay response.
    }
    let (flow,upstream,pass)={let mut tables=s.tables.lock().unwrap();
        if let Some(f)=tables.flows.get(&key){
            if f.key.tcp&&!f.accepted.load(Ordering::Relaxed)&&f.created.elapsed()>Duration::from_secs(30){f.closed.store(true,Ordering::Relaxed);}
            if f.closed.load(Ordering::Relaxed)&&(!key.tcp||meta.syn){
                tables.flows.remove(&key);
            }
        }
        if tables.pass.len()>8192{tables.pass.retain(|_,t|t.elapsed()<Duration::from_secs(5));if tables.pass.len()>8192{tables.pass.clear();}}
        (tables.flows.get(&key).cloned(),tables.upstreams.get(&key).copied(),tables.pass.get(&key).is_some_and(|t|t.elapsed()<Duration::from_secs(2)))
    };
    if let Some(mark)=upstream{packet::dscp(p,mark);return s.inject(p,address);}
    if let Some(flow)=flow{
        if flow.closed.load(Ordering::Relaxed){return Ok(());}
        *flow.last.lock().unwrap()=Instant::now();
        return redirect(s,p,address,meta,&flow);
    }
    if pass{return s.inject(p,address);}
    // Only outbound unicast application traffic is eligible. Discovery and local
    // subnet broadcasts retain native Windows behavior.
    if key.remote.ip().is_multicast()||key.remote.ip().is_unspecified()||matches!(key.remote.ip(),IpAddr::V4(ip)if ip==Ipv4Addr::BROADCAST){return s.inject(p,address);}
    let pid=owner::lookup(key)?;
    let Some(path)=pid.and_then(owner::path)else{
        // System traffic with no accessible executable identity remains native.
        // Never assign such traffic to another application's rule.
        return s.inject(p,address);
    };
    let name=identity(&path);
    let Some((counter_name,policy))=policy_for(&name,&settings.policies,&settings.automatic)else{s.tables.lock().unwrap().pass.insert(key,Instant::now());return s.inject(p,address);};
    if policy.blocked{return Ok(());}
    if policy.target.as_ref().is_some_and(|n|if v4{n.v4.is_none()}else{n.v6.is_none()}){
        if key.tcp{let mut reset=packet::tcp_reset(meta,p);let mut incoming=*address;incoming.inbound();s.inject(&mut reset,&mut incoming)?;}return Ok(());
    }
    if needs_reconnect(meta){
        // An established Windows socket cannot be migrated to a different
        // source address. Reset it so the app opens a new, relayed connection.
        // Never let an assigned app continue on its previous network.
        if p[meta.transport+13]&4==0{
            let mut reset=packet::tcp_reset(meta,p);let mut incoming=*address;
            incoming.inbound();s.inject(&mut reset,&mut incoming)?;
        }
        return Ok(());
    }
    let traffic=settings.counters.get(counter_name).context("Missing application counter")?.clone();
    let flow={let mut tables=s.tables.lock().unwrap();
        for f in tables.flows.values(){if f.key.tcp&&!f.accepted.load(Ordering::Relaxed)&&f.created.elapsed()>Duration::from_secs(30){f.closed.store(true,Ordering::Relaxed);}}
        let expired:Vec<_>=tables.flows.iter().filter(|(_,f)|f.closed.load(Ordering::Relaxed)&&f.last.lock().unwrap().elapsed()>Duration::from_secs(120)).map(|(k,_)|*k).collect();
        for k in expired{if let Some(f)=tables.flows.remove(&k){tables.reverse.remove(&(k.local.is_ipv4(),f.synthetic));}}
        tables.reverse.retain(|_,f|!f.closed.load(Ordering::Relaxed)||f.last.lock().unwrap().elapsed()<Duration::from_secs(120));
        if tables.flows.len()>=8192||tables.reverse.len()>=8192||tables.flows.values().filter(|f|!f.closed.load(Ordering::Relaxed)).count()>=MAX_FLOWS{return Ok(());}
        let synthetic=loop{tables.next=if tables.next==60000{10000}else{tables.next+1};if !tables.reverse.contains_key(&(v4,tables.next))&&tables.next!=port{break tables.next;}};
        let flow=Arc::new(Flow{key,executable:name.clone(),synthetic,original:*address,policy:policy.clone(),traffic,closed:AtomicBool::new(false),last:Mutex::new(Instant::now()),udp:Mutex::new(None),accepted:AtomicBool::new(false),created:Instant::now(),reset:Mutex::new(None)});
        if key.tcp{tables.reverse.insert((v4,synthetic),flow.clone());}tables.flows.insert(key,flow.clone());flow
    };
    if !key.tcp{
        let (tx,rx)=mpsc::sync_channel(8);*flow.udp.lock().unwrap()=Some(tx);
        let worker=s.clone();let f=flow.clone();if let Err(e)=s.spawn(move||udp_relay(worker,f,rx)){flow.closed.store(true,Ordering::Relaxed);return Err(e);}
    }
    redirect(s,p,address,meta,&flow)
}
fn redirect(s:&Shared,p:&mut[u8],address:&mut Address,meta:Packet,flow:&Flow)->Result<()>{
    if meta.key.tcp{
        *flow.reset.lock().unwrap()=Some(packet::tcp_reset(meta,p));
        let v4=meta.key.local.is_ipv4();let local=loopback(v4);
        meta.rewrite(p,SocketAddr::new(local,flow.synthetic),SocketAddr::new(local,s.ports[if v4{0}else{1}]));
        address.flags|=(1<<17)|(1<<18);s.inject(p,address)
    }else{
        // A full bounded datagram queue drops packets, as a congested UDP link
        // would. It never stalls unrelated applications or grows without bound.
        if let Some(tx)=flow.udp.lock().unwrap().as_ref(){let _=tx.try_send(p[meta.payload..].to_vec());}Ok(())
    }
}
fn needs_reconnect(meta:Packet)->bool{meta.key.tcp&&!meta.syn}
fn policy_for<'a>(name:&str,policies:&'a BTreeMap<String,Policy>,automatic:&BTreeSet<String>)->Option<(&'a String,&'a Policy)>{
    if let Some(entry)=policies.get_key_value(name){return Some(entry);}
    if automatic.contains(name){return None;}
    // Steam's store/chat browser uses a separate executable. Scope inheritance
    // to its CEF helper folder, never Steam games or arbitrary child processes.
    policies.iter().find(|(root,_)|steam_helper(root,name))
}
fn steam_helper(root:&str,name:&str)->bool{
    let Some(folder)=root.strip_suffix(r"\steam.exe")else{return false;};
    let Some(relative)=name.strip_prefix(&format!(r"{folder}\bin\cef\"))else{return false;};
    relative.ends_with(r"\steamwebhelper.exe")&&!relative.split('\\').any(|p|p==".."||p==".")
}
#[cfg(test)]mod tests{
    use super::*;
    #[test]fn existing_tcp_reconnects_but_new_syn_and_udp_do_not(){
        for (local,remote)in [("192.0.2.2:51000","1.1.1.1:443"),("[2001:db8::2]:51000","[2001:db8::3]:443")]{
            let key=Key{local:local.parse().unwrap(),remote:remote.parse().unwrap(),tcp:true};
            let mut packet=Packet{key,transport:20,payload:40,syn:false};assert!(needs_reconnect(packet));
            packet.syn=true;assert!(!needs_reconnect(packet));packet.syn=false;packet.key.tcp=false;assert!(!needs_reconnect(packet));
        }
    }
    #[test]fn steam_helper_inherits_but_games_and_explicit_overrides_do_not(){
        let root=identity(r"C:\Program Files (x86)\Steam\steam.exe");
        let helper=identity(r"C:\Program Files (x86)\Steam\bin\cef\cef.win7x64\steamwebhelper.exe");
        let policy=Policy{rule:Rule::automatic(root.clone()),target:None,blocked:true};
        let mut policies=BTreeMap::from([(root.clone(),policy.clone())]);let mut automatic=BTreeSet::new();
        assert_eq!(policy_for(&helper,&policies,&automatic).unwrap().0,&root);
        for other in [r"C:\Program Files (x86)\Steam\steamapps\common\Game\game.exe",r"C:\Other\Steam\bin\cef\v1\steamwebhelper.exe",r"C:\Program Files (x86)\Steam\bin\cef\..\other\steamwebhelper.exe"]{assert!(policy_for(&identity(other),&policies,&automatic).is_none());}
        automatic.insert(helper.clone());assert!(policy_for(&helper,&policies,&automatic).is_none());
        policies.insert(helper.clone(),policy);assert_eq!(policy_for(&helper,&policies,&automatic).unwrap().0,&helper);
    }
}
fn bound_socket(flow:&Flow)->Result<Socket>{
    let key=flow.key;let socket=Socket::new(if key.local.is_ipv4(){Domain::IPV4}else{Domain::IPV6},if key.tcp{Type::STREAM}else{Type::DGRAM},Some(if key.tcp{Protocol::TCP}else{Protocol::UDP}))?;
    let source=if let Some(target)=&flow.policy.target{
        let ip=if key.local.is_ipv4(){target.v4}else{target.v6}.context("Selected network has no address for this IP family")?;
        let index=if key.local.is_ipv4(){target.index.to_be()}else{target.ipv6_index};
        ensure!(index!=0,"Selected adapter has no interface index for this IP family");
        let (level,option)=if key.local.is_ipv4(){(IPPROTO_IP,IP_UNICAST_IF)}else{(IPPROTO_IPV6,IPV6_UNICAST_IF)};
        ensure!(unsafe{setsockopt(socket.as_raw_socket()as usize,level,option,(&index as *const u32).cast(),4)}!=SOCKET_ERROR,"Cannot select adapter: {}",std::io::Error::last_os_error());ip
    }else{if key.local.is_ipv4(){Ipv4Addr::UNSPECIFIED.into()}else{Ipv6Addr::UNSPECIFIED.into()}};
    socket.bind(&SocketAddr::new(source,0).into())?;Ok(socket)
}
struct Upstream{shared:Arc<Shared>,key:Key}
impl Drop for Upstream{fn drop(&mut self){self.shared.tables.lock().unwrap().upstreams.remove(&self.key);}}
fn register(s:&Arc<Shared>,flow:&Flow,socket:&Socket)->Result<Upstream>{
    let mut local=socket.local_addr()?.as_socket().context("Invalid upstream socket")?;
    // Explicit routes always have a concrete source. Automatic QoS routes need
    // Windows to choose a source before capture, so they are not enabled here.
    ensure!(!local.ip().is_unspecified(),"Choose a network before setting traffic controls");
    local.set_port(socket.local_addr()?.as_socket().unwrap().port());
    let key=Key{local,remote:flow.key.remote,tcp:flow.key.tcp};s.tables.lock().unwrap().upstreams.insert(key,traffic::marking(&flow.policy.rule.priority));Ok(Upstream{shared:s.clone(),key})
}
fn tcp_relay(s:Arc<Shared>,flow:Arc<Flow>,client:TcpStream){
    let result=(||->Result<()>{
        let socket=bound_socket(&flow)?;let _registration=register(&s,&flow,&socket)?;
        socket.connect_timeout(&flow.key.remote.into(),Duration::from_secs(5))?;let upstream:TcpStream=socket.into();
        for stream in [&client,&upstream]{stream.set_read_timeout(Some(Duration::from_millis(250)))?;stream.set_write_timeout(Some(Duration::from_secs(2)))?;stream.set_nodelay(true)?;}
        let a=client.try_clone()?;let b=upstream.try_clone()?;let shared=s.clone();let f=flow.clone();
        // Scoped threads guarantee both halves exit before the registration is removed.
        thread::scope(|scope|{
            let upload=scope.spawn(||copy_stream(a,b,&shared,&f,false));
            let down=copy_stream(upstream,client,&s,&flow,true);
            let up=upload.join().map_err(|_|anyhow::anyhow!("Upload worker failed"))?;down?;up
        })
    })();
    if let Err(e)=result{if !s.stop.load(Ordering::Relaxed){s.error(format!("{}: {e}",flow.policy.rule.path));}}
    flow.closed.store(true,Ordering::Relaxed);*flow.last.lock().unwrap()=Instant::now();
}
fn copy_stream(mut from:TcpStream,mut to:TcpStream,s:&Shared,flow:&Flow,download:bool)->Result<()>{
    let mut buf=[0;16_384];let mut idle=Instant::now();
    while !s.stop.load(Ordering::Relaxed)&&!flow.closed.load(Ordering::Relaxed){
        match from.read(&mut buf){
            Ok(0)=>{let _=to.shutdown(Shutdown::Write);return Ok(());},
            Ok(n)=>{idle=Instant::now();if !flow.traffic.wait(&flow.policy.rule,download,n,&s.stop,&flow.closed){break;}
                if let Err(e)=to.write_all(&buf[..n]){flow.closed.store(true,Ordering::Relaxed);return Err(e.into());}flow.count(download,n);},
            Err(e)if matches!(e.kind(),std::io::ErrorKind::WouldBlock|std::io::ErrorKind::TimedOut)=>{if idle.elapsed()>Duration::from_secs(600){break;}},
            Err(e)=>{flow.closed.store(true,Ordering::Relaxed);return Err(e.into());}
        }
    }
    let _=from.shutdown(Shutdown::Both);let _=to.shutdown(Shutdown::Both);Ok(())
}
fn udp_relay(s:Arc<Shared>,flow:Arc<Flow>,rx:mpsc::Receiver<Vec<u8>>){
    let result=(||->Result<()>{
        let socket=bound_socket(&flow)?;let _registration=register(&s,&flow,&socket)?;socket.connect(&flow.key.remote.into())?;
        let socket:UdpSocket=socket.into();socket.set_read_timeout(Some(Duration::from_millis(100)))?;socket.set_write_timeout(Some(Duration::from_secs(1)))?;
        let send=socket.try_clone()?;let shared=s.clone();let f=flow.clone();
        thread::scope(|scope|{
            let upload=scope.spawn(move||->Result<()>{while !shared.stop.load(Ordering::Relaxed)&&!f.closed.load(Ordering::Relaxed){match rx.recv_timeout(Duration::from_millis(100)){
                Ok(data)=>{if !f.traffic.wait(&f.policy.rule,false,data.len(),&shared.stop,&f.closed){break;}let n=send.send(&data)?;f.count(false,n);},
                Err(mpsc::RecvTimeoutError::Timeout)=>{},Err(_)=>break,
            }}Ok(())});
            let download=(||->Result<()>{let mut buf=[0u8;65535];let mut address=flow.original;address.inbound();
                while !s.stop.load(Ordering::Relaxed)&&!flow.closed.load(Ordering::Relaxed){match socket.recv(&mut buf){
                    Ok(n)=>{*flow.last.lock().unwrap()=Instant::now();if !flow.traffic.wait(&flow.policy.rule,true,n,&s.stop,&flow.closed){break;}if let Some(mut p)=packet::udp_reply(flow.key,&buf[..n]){s.inject(&mut p,&mut address)?;flow.count(true,n);}},
                    Err(e)if matches!(e.kind(),std::io::ErrorKind::WouldBlock|std::io::ErrorKind::TimedOut)=>{if flow.last.lock().unwrap().elapsed()>Duration::from_secs(120){break;}},
                    Err(e)=>return Err(e.into()),
                }}Ok(())})();
            flow.closed.store(true,Ordering::Relaxed);let up=upload.join().map_err(|_|anyhow::anyhow!("Datagram upload worker failed"))?;download?;up
        })
    })();
    if let Err(e)=result{if !s.stop.load(Ordering::Relaxed){s.error(format!("{}: {e}",flow.policy.rule.path));}}
    flow.closed.store(true,Ordering::Relaxed);*flow.last.lock().unwrap()=Instant::now();
}
