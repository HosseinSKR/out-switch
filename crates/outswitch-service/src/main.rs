use anyhow::{bail,Context,Result};
mod routing_test;
use outswitch_common::*;
use outswitch_core::RoutingEngine;
use outswitch_routing::WindowsRoutingEngine;
use outswitch_storage::Store;
use std::{path::PathBuf,sync::{Arc,atomic::{AtomicBool,Ordering}},time::{Duration,Instant}};
use windows_service::{define_windows_service,service::*,service_control_handler::{self,ServiceControlHandlerResult},service_dispatcher};

struct Service {
    store:Store, state:Snapshot, engine:WindowsRoutingEngine,
    networks:outswitch_network::NetworkManager, topology:String, retry_at:Option<Instant>, retry_attempt:u32,
}
impl Service {
    fn new(path:PathBuf)->Result<Self>{
        let store=Store::open(&path)?;let mut config=store.load()?;
        if config.settings.network_control_on_start{config.routing_enabled=true;store.save(&config)?;}
        Ok(Self{store,state:Snapshot{config,service_connected:true,engine_status:"Routing paused — Windows chooses connections".into(),..Default::default()},engine:WindowsRoutingEngine::default(),networks:Default::default(),topology:String::new(),retry_at:None,retry_attempt:0})
    }
    fn event(&mut self,message:String){
        if self.state.events.last().is_some_and(|e|e.message==message){return;}
        self.state.events.push(Event{time:now(),message});if self.state.events.len()>100{self.state.events.remove(0);}
    }
    fn reconcile(&mut self)->Result<()> {
        if self.state.config.routing_enabled {
            let rules:Vec<_>=self.state.config.rules().iter().map(|r|{let mut r=r.clone();if r.network.is_some(){r.priority=outswitch_core::effective_priority(&r,self.state.config.gaming_mode);}r}).collect();
            self.engine.apply(&rules,&self.state.networks)?;
            self.state.rule_status=self.engine.statuses.clone();self.state.engine_status="Network Control ON — assigned apps reconnect through their selected network".into();
        } else {self.engine.clear()?;self.state.rule_status.clear();self.state.engine_status=if self.state.routing_available{"Routing paused — Windows chooses connections".into()}else{self.state.routing_unavailable_reason.clone()};}
        self.retry_at=None;self.retry_attempt=0;self.state.routing_recovering=false;
        Ok(())
    }
    fn recover_routing(&mut self,error:impl std::fmt::Display)->Result<()> {
        self.engine.clear()?;self.state.rule_status.clear();
        // Runtime health is separate from the user's saved ON/OFF choice.
        let delay=2u64.saturating_pow(self.retry_attempt.min(4)+1).min(30);
        self.retry_attempt=self.retry_attempt.saturating_add(1);
        self.retry_at=Some(Instant::now()+Duration::from_secs(delay));
        self.state.routing_recovering=self.state.config.routing_enabled;
        self.state.engine_status=format!("Routing recovering — retry in {delay}s: {error}");
        self.event(self.state.engine_status.clone());Ok(())
    }
    fn refresh(&mut self)->Result<()> {
        if let Some(error)=self.engine.failure(){self.recover_routing(error)?;}
        if let Some(message)=self.engine.notice(){self.event(message);}
        let reason=outswitch_routing::availability_error().unwrap_or_default();
        let readiness_changed=self.state.routing_available!=reason.is_empty() || self.state.routing_unavailable_reason!=reason;
        self.state.routing_available=reason.is_empty();self.state.routing_unavailable_reason=reason;
        let mut networks=self.networks.snapshot()?;
        let mut config_changed=false;
        for n in &mut networks {
            if !self.state.config.network_names.contains_key(&n.id) {
                let name=format!("Net {}",self.state.config.network_names.len()+1);
                self.state.config.network_names.insert(n.id.clone(),name);config_changed=true;
            }
            n.label=self.state.config.network_names[&n.id].clone();
            if let Some(old)=self.state.networks.iter().find(|o|o.id==n.id) {
                if n.addresses==old.addresses&&n.connected==old.connected{n.internet=old.internet;n.ping_ms=old.ping_ms;}
                if self.state.config.settings.collect_statistics {
                    self.store.record(&Usage {day:now()/86400,network:n.id.clone(),downloaded:n.received.saturating_sub(old.received),uploaded:n.sent.saturating_sub(old.sent),disconnects:(old.connected&&!n.connected) as u32})?;
                }
            }
        }
        let transitions:Vec<_>=networks.iter().filter_map(|n|self.state.networks.iter().find(|o|o.id==n.id&&o.connected!=n.connected).map(|_|format!("{} {}",n.label,if n.connected{"connected"}else{"disconnected"}))).collect();
        for e in transitions{self.event(e);}
        if config_changed{self.store.save(&self.state.config)?;}
        let topology=networks.iter().map(|n|format!("{}:{}:{}:{}:{:?}:{:?}:{}",n.id,n.index,n.ipv6_index,n.connected,n.addresses,n.internet,n.strong_host)).collect::<Vec<_>>().join(";");
        self.state.networks=networks;
        self.update_applications(outswitch_monitor::snapshot(&self.state.networks));
        self.engine.measurements(&mut self.state.applications);
        if topology!=self.topology || readiness_changed || (self.state.config.routing_enabled&&self.retry_at.is_some_and(|at|Instant::now()>=at)) {
            self.topology=topology;
            if let Err(e)=self.reconcile(){
                // On adapter-change failures remove potentially stale source-address policies.
                self.recover_routing(e)?;
            }
        }
        self.state.statistics=self.store.usage()?;self.state.timestamp=now();Ok(())
    }
    fn update_applications(&mut self,result:Result<Vec<Application>>) {
        match result {
            Ok(applications)=>{
                self.state.applications=applications;
                if self.state.config.apps_initialized{self.state.applications.retain(|a|self.state.config.apps.contains_key(&identity(&a.path)));}
                self.state.process_monitor_available=true;
                self.state.monitor_status.clear();
            }
            Err(error)=>{
                self.state.applications.clear();
                self.state.process_monitor_available=false;
                self.state.monitor_status=format!("Application monitoring unavailable: {error}");
                self.event(self.state.monitor_status.clone());
            }
        }
        for saved in self.state.config.apps.values(){
            if let Some(app)=self.state.applications.iter_mut().find(|a|identity(&a.path)==identity(&saved.path)){app.name=saved.name.clone();}
            else{self.state.applications.push(Application{path:saved.path.clone(),name:saved.name.clone(),..Default::default()});}
        }
        for rule in self.state.config.rules(){
            if !self.state.applications.iter().any(|a|identity(&a.path)==identity(&rule.path)) {
                self.state.applications.push(Application{path:rule.path.clone(),name:rule.path.rsplit('\\').next().unwrap_or("").trim_end_matches(".exe").into(),..Default::default()});
            }
        }
    }
    fn mutate(&mut self,request:Request)->Result<String>{
        if matches!(request,Request::DriverCheck){outswitch_routing::probe()?;return Ok("Windows loaded the signed routing component successfully. This check captures no traffic; test app routing separately.".into());}
        if matches!(request,Request::EmergencyStop|Request::ResetRules){
            self.engine.clear()?;self.state.rule_status.clear();self.state.config.routing_enabled=false;
            self.retry_at=None;self.retry_attempt=0;self.state.routing_recovering=false;
            if matches!(request,Request::ResetRules){for rules in self.state.config.profiles.values_mut(){rules.clear();}}
            self.state.engine_status="All Out-Switch filters removed — Windows networking restored for new connections".into();
            self.store.save(&self.state.config)?;return Ok(self.state.engine_status.clone());
        }
        if let Request::TestNetwork(id)=request {
            let n=self.state.networks.iter().find(|n|n.id==id).context("Network no longer present")?.clone();
            let result=outswitch_network::test(&n);let current=self.state.networks.iter_mut().find(|n|n.id==id).unwrap();
            match result {Ok(ms)=>{current.internet=Some(true);current.ping_ms=Some(ms);return Ok(format!("{}: TCP connection to 1.1.1.1:443 succeeded in {ms:.0} ms",n.label));},Err(e)=>{current.internet=Some(false);current.ping_ms=None;return Ok(format!("{}: probe failed ({e}); this alone does not prove an internet outage",n.label));}}
        }
        let enabling=matches!(&request,Request::Routing(true));
        let library_change=matches!(&request,Request::InitializeApps(_)|Request::AddApp(_)|Request::PinApp{..});
        let removal=matches!(&request,Request::RemoveApp(_));
        let old=self.state.config.clone();let mut next=old.clone();
        match request {
            Request::Snapshot=>return Ok(String::new()),
            Request::InitializeApps(apps)=>{
                anyhow::ensure!(apps.len()<=1000,"Too many scanned applications");
                let valid=apps.into_iter().filter(|a|Rule::automatic(a.path.clone()).validate().is_ok()&&std::path::Path::new(&a.path).is_file()&&a.name.len()<=256).collect();
                next.initialize_apps(valid);
            },
            Request::AddApp(mut app)=>{
                app.path=app.path.replace('/',"\\");Rule::automatic(app.path.clone()).validate()?;
                anyhow::ensure!(std::path::Path::new(&app.path).is_file(),"Executable does not exist");
                anyhow::ensure!(!app.name.trim().is_empty()&&app.name.len()<=256,"Use an app name of 1-256 characters");
                anyhow::ensure!(next.apps.len()<1000||next.apps.contains_key(&identity(&app.path)),"The app library is full");
                next.add_app(app);
            },
            Request::RemoveApp(path)=>next.remove_app(&path),
            Request::PinApp{path,pinned}=>{next.pin_app(&path,pinned)?;},
            Request::SaveRule(mut rule)=>{
                rule.validate()?;
                anyhow::ensure!(std::path::Path::new(&rule.path).is_file(),"Executable does not exist");
                anyhow::ensure!(rule.network.is_some()||(rule.download_mbps.is_none()&&rule.upload_mbps.is_none()&&rule.priority==Priority::Normal),"Choose a network before enabling bandwidth limits or QoS");
                next.apps.entry(identity(&rule.path)).or_insert_with(||SavedApplication::from_path(rule.path.clone()));
                rule.path=rule.path.replace('/',"\\");let rules=next.profiles.get_mut(&next.active_profile).context("Profile missing")?;
                rules.retain(|r|identity(&r.path)!=identity(&rule.path));rules.push(rule);
            },
            Request::DeleteRule(path)=>next.profiles.get_mut(&next.active_profile).context("Profile missing")?.retain(|r|identity(&r.path)!=identity(&path)),
            Request::Routing(enabled)=>{
                anyhow::ensure!(!enabled||self.state.routing_available,"{}",self.state.routing_unavailable_reason);
                next.routing_enabled=enabled;
            },
            Request::RenameNetwork{id,name}=>{anyhow::ensure!(!name.trim().is_empty()&&name.len()<=64,"Use a network name of 1–64 characters");anyhow::ensure!(next.network_names.contains_key(&id),"Unknown network");next.network_names.insert(id,name.trim().into());},
            Request::SelectProfile(name)=>{anyhow::ensure!(next.profiles.contains_key(&name),"Unknown profile");next.active_profile=name;},
            Request::CreateProfile(name)=>{anyhow::ensure!(!name.trim().is_empty()&&name.len()<=64,"Use a profile name of 1–64 characters");anyhow::ensure!(!next.profiles.contains_key(&name),"Profile already exists");next.profiles.insert(name.clone(),next.rules().to_vec());next.active_profile=name;},
            Request::DeleteProfile(name)=>{anyhow::ensure!(name!=next.active_profile,"Switch to another profile before deleting this one");anyhow::ensure!(next.profiles.remove(&name).is_some(),"Unknown profile");},
            Request::Settings(settings)=>next.settings=settings,
            Request::Gaming(enabled)=>next.gaming_mode=enabled,
            _=>bail!("Unsupported request"),
        }
        self.state.config=next;
        if let Err(e)=if library_change{Ok(())}else{self.reconcile()}{
            if enabling{self.recover_routing(e)?;}
            else{self.state.config=old;if let Err(restore)=self.reconcile(){self.recover_routing(restore)?;}return Err(e);}
        }
        if let Err(e)=self.store.save(&self.state.config){
            self.state.config=old;if let Err(restore)=self.reconcile(){self.recover_routing(restore)?;}
            return Err(e);
        }
        let monitor_available=self.state.process_monitor_available;let monitor_status=self.state.monitor_status.clone();
        self.update_applications(Ok(self.state.applications.clone()));
        self.state.process_monitor_available=monitor_available;self.state.monitor_status=monitor_status;
        if removal{return Ok("Removed from Out-Switch and all profiles. The installed app is unchanged.".into());}
        if library_change{return Ok("App library saved.".into());}
        Ok(if self.state.config.routing_enabled{"Saved. Active connections reconnect through the selected network on their next activity."}else{"Saved locally. Routing is paused; the rule is not enforced."}.into())
    }
    fn handle(&mut self,request:Request)->Response {
        let result=self.mutate(request);let (ok,message)=match result{Ok(m)=>(true,m),Err(e)=>(false,e.to_string())};
        if !message.is_empty(){self.event(message.clone());}
        Response{ok,message,snapshot:Some(self.state.clone())}
    }
}
fn run(path:PathBuf,owner:String,stop:Arc<AtomicBool>,ready:impl FnOnce())->Result<()> {
    let mut service=Service::new(path)?;let server=outswitch_ipc::Server::new(&owner)?;
    ready();
    let mut last=Instant::now()-Duration::from_secs(10);
    while !stop.load(Ordering::Relaxed){
        if last.elapsed()>=Duration::from_secs(2){if let Err(e)=service.refresh(){service.state.engine_status=format!("Monitoring error: {e}");service.event(e.to_string());}last=Instant::now();}
        if let Err(e)=server.poll(|r|service.handle(r)){eprintln!("IPC: {e}");}
        std::thread::sleep(Duration::from_millis(35));
    }
    service.engine.clear()?;Ok(())
}
define_windows_service!(ffi_service_main,service_main);
fn installed_owner(base:&std::path::Path)->Result<String>{unsafe{
    use windows_sys::Win32::System::Registry::*;
    let key:Vec<u16>="SOFTWARE\\OutSwitch\0".encode_utf16().collect();
    let value:Vec<u16>="OwnerSid\0".encode_utf16().collect();
    let mut sid=[0u16;192];let mut bytes=std::mem::size_of_val(&sid) as u32;
    let status=RegGetValueW(HKEY_LOCAL_MACHINE,key.as_ptr(),value.as_ptr(),RRF_RT_REG_SZ|RRF_SUBKEY_WOW6464KEY,std::ptr::null_mut(),sid.as_mut_ptr().cast(),&mut bytes);
    if status==0 {return Ok(String::from_utf16_lossy(&sid[..sid.iter().position(|c|*c==0).unwrap_or(sid.len())]));}
    std::fs::read_to_string(base.join("owner.sid")).map(|s|s.trim().to_string()).context("Installation owner is missing; repair Out-Switch in Windows Settings")
}}
fn service_main(_:Vec<std::ffi::OsString>){
    let stop=Arc::new(AtomicBool::new(false));let signal=stop.clone();
    let Ok(status)=service_control_handler::register("OutSwitch",move|control|match control{ServiceControl::Stop|ServiceControl::Shutdown=>{signal.store(true,Ordering::Relaxed);ServiceControlHandlerResult::NoError},ServiceControl::Interrogate=>ServiceControlHandlerResult::NoError,_=>ServiceControlHandlerResult::NotImplemented})else{return;};
    let base=PathBuf::from(std::env::var_os("ProgramData").unwrap_or_else(||"C:\\ProgramData".into())).join("OutSwitch");
    let make_status=|state,code|ServiceStatus{service_type:ServiceType::OWN_PROCESS,current_state:state,controls_accepted:ServiceControlAccept::STOP|ServiceControlAccept::SHUTDOWN,exit_code:ServiceExitCode::Win32(code),checkpoint:0,wait_hint:Duration::ZERO,process_id:None};
    let mut pending=make_status(ServiceState::StartPending,0);pending.controls_accepted=ServiceControlAccept::empty();pending.wait_hint=Duration::from_secs(10);pending.checkpoint=1;
    let _=status.set_service_status(pending);
    let result=installed_owner(&base).and_then(|sid|run(base.join("outswitch.db"),sid,stop,||{let _=status.set_service_status(make_status(ServiceState::Running,0));}));
    if let Err(error)=&result{let _=std::fs::write(base.join("service-error.log"),format!("{error:#}\n"));}
    let _=status.set_service_status(make_status(ServiceState::Stopped,if result.is_ok(){0}else{1}));
}
fn main()->Result<()> {
    let args:Vec<_>=std::env::args().collect();
    if args.iter().any(|a|a=="--start-service"){
        let manager=windows_service::service_manager::ServiceManager::local_computer(None::<&str>,windows_service::service_manager::ServiceManagerAccess::CONNECT)?;
        let service=manager.open_service("OutSwitch",ServiceAccess::QUERY_STATUS|ServiceAccess::START)?;
        if service.query_status()?.current_state==ServiceState::Stopped{service.start::<&str>(&[])?;}return Ok(());
    }
    if args.iter().any(|a|a=="--traffic-probe"){return routing_test::child();}
    if let Some(index)=args.iter().position(|a|a=="--routing-test"){return routing_test::run(args.get(index+1).context("Missing network ID")?);}
    if args.iter().any(|a|a=="--driver-check"){outswitch_routing::probe()?;println!("PASS: Windows loaded the bundled driver and opened/closed a filter that captures no traffic.");return Ok(());}
    if args.iter().any(|a|a=="--ping-list"){
        println!("{}",serde_json::to_string_pretty(&outswitch_network::scan()?)?);return Ok(());
    }
    if let Some(index)=args.iter().position(|a|a=="--ping-network"){
        let id=args.get(index+1).context("Missing network ID")?;
        let target=args.iter().position(|a|a=="--ping-target").and_then(|i|args.get(i+1)).context("Missing --ping-target")?;
        anyhow::ensure!(outswitch_network::ping::TARGETS.iter().any(|(_,ip)|ip==target),"Choose a built-in ping target");
        let network=outswitch_network::scan()?.into_iter().find(|n|n.id==*id).context("Network not found")?;
        let source=outswitch_network::ping::source(&network)?;
        println!("Source: {source}; adapter: {}; target: {target}",network.name);
        let mut replies=Vec::new();
        for sequence in 1..=3{let reply=outswitch_network::ping::probe(&network,source,target.parse()?,sequence)?;println!("{reply:?}");replies.push(reply);}
        println!("{:?}",outswitch_network::ping::Summary::from_replies(&replies));return Ok(());
    }
    if args.iter().any(|a|a=="--ipc-snapshot"){println!("{}",serde_json::to_string_pretty(&outswitch_ipc::request(&Request::Snapshot)?)?);return Ok(());}
    if args.iter().any(|a|a=="--inspect"){
        let networks=outswitch_network::scan()?;let applications=outswitch_monitor::snapshot(&networks)?;
        println!("{}",serde_json::to_string_pretty(&Snapshot{networks,applications,engine_status:"Read-only inspection; no routing changes".into(),timestamp:now(),..Default::default()})?);return Ok(());
    }
    if args.iter().any(|a|a=="--console"){
        let path=args.iter().position(|a|a=="--data").and_then(|i|args.get(i+1)).map(PathBuf::from).unwrap_or_else(||PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_else(||".".into())).join("OutSwitch/outswitch.db"));
        let stop=Arc::new(AtomicBool::new(false));
        if let Some(index)=args.iter().position(|a|a=="--parent"){
            let pid:u32=args.get(index+1).context("Missing parent PID")?.parse()?;
            unsafe {
                use windows_sys::Win32::System::Threading::*;
                let parent=OpenProcess(0x00100000,0,pid); // SYNCHRONIZE only
                anyhow::ensure!(!parent.is_null(),"The desktop application has already exited");
                let parent=parent as usize;let signal=stop.clone();
                std::thread::spawn(move||{
                    WaitForSingleObject(parent as _,INFINITE);
                    windows_sys::Win32::Foundation::CloseHandle(parent as _);
                    signal.store(true,Ordering::Relaxed);
                });
            }
        }
        return run(path,outswitch_ipc::current_sid()?,stop,||{});
    }
    service_dispatcher::start("OutSwitch",ffi_service_main)?;Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn service()->(Service,PathBuf){
        static NEXT:std::sync::atomic::AtomicU64=std::sync::atomic::AtomicU64::new(0);
        let path=std::env::temp_dir().join(format!("outswitch-service-test-{}-{}-{}.db",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos(),NEXT.fetch_add(1,Ordering::Relaxed)));
        (Service::new(path.clone()).unwrap(),path)
    }
    fn cleanup(path:PathBuf){
        // Windows scanners can retain a short-lived handle after SQLite closes.
        // Only retry sharing/lock violations; never hide a real cleanup failure.
        for attempt in 0..20{match std::fs::remove_file(&path){
            Ok(())=>return,
            Err(e)if matches!(e.raw_os_error(),Some(32|33))&&attempt<19=>std::thread::sleep(Duration::from_millis(50)),
            Err(e)=>panic!("Could not remove test database: {e}"),
        }}
    }
    #[test]
    fn routing_failure_keeps_saved_intent_and_schedules_recovery(){
        let (mut service,path)=service();service.store.save(&service.state.config).unwrap();
        service.recover_routing("temporary capture failure").unwrap();
        assert!(service.state.config.routing_enabled);assert!(service.store.load().unwrap().routing_enabled);
        assert!(service.state.routing_recovering);assert!(service.retry_at.unwrap()>Instant::now());
        service.reconcile().unwrap();assert!(!service.state.routing_recovering);assert!(service.retry_at.is_none());
        service.mutate(Request::EmergencyStop).unwrap();assert!(!service.state.config.routing_enabled);assert!(service.retry_at.is_none());
        drop(service);cleanup(path);
    }
    #[test]
    fn startup_defaults_on_and_respects_startup_opt_out(){
        let (mut service,path)=service();service.state.config.routing_enabled=false;service.store.save(&service.state.config).unwrap();drop(service);
        let mut service=Service::new(path.clone()).unwrap();assert!(service.state.config.routing_enabled);
        service.state.config.settings.network_control_on_start=false;service.state.config.routing_enabled=false;service.store.save(&service.state.config).unwrap();drop(service);
        let service=Service::new(path.clone()).unwrap();assert!(!service.state.config.routing_enabled);drop(service);cleanup(path);
    }
    #[test]
    fn scanner_failure_keeps_saved_applications_and_routing_status(){
        let (mut service,path)=service();
        let rule=Rule::automatic(r"C:\Apps\Example.exe".into());
        service.state.config.profiles.get_mut("Normal").unwrap().push(rule.clone());
        let routing_status=service.state.engine_status.clone();
        service.update_applications(Err(anyhow::anyhow!("scanner denied")));
        assert!(!service.state.process_monitor_available);
        assert!(service.state.monitor_status.contains("scanner denied"));
        assert_eq!(service.state.applications.len(),1);
        assert_eq!(service.state.applications[0].path,rule.path);
        assert_eq!(service.state.engine_status,routing_status);
        assert!(service.state.applications[0].download_bps.is_none());
        drop(service);cleanup(path);
    }
    #[test]
    fn scanner_recovers_without_duplicate_saved_applications(){
        let (mut service,path)=service();
        let rule=Rule::automatic(r"C:\Apps\Example.exe".into());
        service.state.config.profiles.get_mut("Normal").unwrap().push(rule.clone());
        service.update_applications(Err(anyhow::anyhow!("unavailable")));
        service.update_applications(Ok(vec![Application{path:rule.path,pids:vec![42],..Default::default()}]));
        assert!(service.state.process_monitor_available);
        assert!(service.state.monitor_status.is_empty());
        assert_eq!(service.state.applications.len(),1);
        assert_eq!(service.state.applications[0].pids,vec![42]);
        drop(service);cleanup(path);
    }
    #[test]
    fn library_actions_persist_and_removed_running_apps_stay_removed() {
        let (mut service,path)=service();service.state.config.routing_enabled=false;
        let exe=std::env::current_exe().unwrap().to_string_lossy().to_string();
        let app=SavedApplication{path:exe.clone(),name:"Library test".into()};
        service.mutate(Request::InitializeApps(vec![app.clone()])).unwrap();
        service.mutate(Request::PinApp{path:exe.clone(),pinned:true}).unwrap();
        service.mutate(Request::SaveRule(Rule::automatic(exe.clone()))).unwrap();
        assert_eq!(service.state.config.apps[&identity(&exe)].name,"Library test");
        drop(service);let mut service=Service::new(path.clone()).unwrap();
        assert_eq!(service.state.config.pinned_apps,vec![identity(&exe)]);
        service.update_applications(Ok(vec![Application{path:exe.clone(),pids:vec![42],..Default::default()}]));
        assert_eq!(service.state.applications[0].pids,vec![42]);
        service.mutate(Request::RemoveApp(exe.clone())).unwrap();
        service.update_applications(Ok(vec![Application{path:exe.clone(),pids:vec![42],..Default::default()}]));
        assert!(service.state.applications.is_empty());assert!(service.state.config.rules().is_empty());
        drop(service);let mut service=Service::new(path.clone()).unwrap();
        assert!(service.state.config.apps_initialized);assert!(service.state.config.apps.is_empty());assert!(service.state.config.pinned_apps.is_empty());
        service.mutate(Request::AddApp(app)).unwrap();assert_eq!(service.state.applications.len(),1);
        drop(service);cleanup(path);
    }
}

