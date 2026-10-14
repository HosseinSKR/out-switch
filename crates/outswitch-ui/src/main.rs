#![windows_subsystem = "windows"]
mod tray;
mod updater;
mod icons;
mod ping_panel;
mod app_scan;
mod app_picker;
mod traffic;
fn diagnostic(message:&str){use std::io::Write;if let Some(path)=std::env::var_os("OUTSWITCH_DIAGNOSTICS"){if let Ok(mut file)=std::fs::OpenOptions::new().create(true).append(true).open(path){let _=writeln!(file,"{message}");}}}
unsafe extern "system" fn crash(info:*const windows_sys::Win32::System::Diagnostics::Debug::EXCEPTION_POINTERS)->i32{
    use windows_sys::Win32::System::LibraryLoader::*;
    if !info.is_null()&&!(*info).ExceptionRecord.is_null(){let record=&*(*info).ExceptionRecord;let mut module=std::ptr::null_mut();GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS|GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,record.ExceptionAddress.cast(),&mut module);let mut name=[0u16;1024];let len=GetModuleFileNameW(module,name.as_mut_ptr(),1024);diagnostic(&format!("Native exception {:X} at {:?} in {}",record.ExceptionCode,record.ExceptionAddress,String::from_utf16_lossy(&name[..len as usize])));}
    0
}
use eframe::egui::{self,Color32,RichText,Vec2};
use outswitch_common::*;
use std::{collections::{BTreeMap,VecDeque},sync::mpsc::{self,Receiver,Sender},time::Duration};

const PAGES:[&str;9]=["Dashboard","Applications","Networks","Traffic Monitor","Rules","Profiles","Statistics","Settings","Contact Us"];
fn rate(bytes:f64)->String{if bytes>=1_000_000. {format!("{:.1} MB/s",bytes/1_000_000.)}else{format!("{:.0} KB/s",bytes/1000.)}}
fn total(bytes:u64)->String{format!("{:.2} GB",bytes as f64/1_000_000_000.)}
fn label(snapshot:&Snapshot,id:&str)->String{snapshot.config.network_names.get(id).cloned().unwrap_or_else(||"Unavailable adapter".into())}
fn configured(snapshot:&Snapshot,rule:Option<&Rule>)->String{rule.and_then(|r|r.network.as_ref()).map(|id|label(snapshot,id)).unwrap_or_else(||"Automatic".into())}
enum Update{State(Snapshot),Message(String)}
fn start_local_helper()->anyhow::Result<std::process::Child> {
    use std::os::windows::process::CommandExt;
    use windows_sys::Win32::System::Services::*;
    // An installed service owns its own data and lifecycle. Never replace it
    // with a portable helper just because it is currently stopped.
    unsafe {
        let manager=OpenSCManagerW(std::ptr::null(),std::ptr::null(),SC_MANAGER_CONNECT);
        anyhow::ensure!(!manager.is_null(),"Cannot inspect the installed service");
        let name:Vec<u16>="OutSwitch\0".encode_utf16().collect();
        let service=OpenServiceW(manager,name.as_ptr(),SERVICE_QUERY_STATUS);
        let code=windows_sys::Win32::Foundation::GetLastError();
        if !service.is_null(){CloseServiceHandle(service);}
        CloseServiceHandle(manager);
        anyhow::ensure!(service.is_null()&&code==windows_sys::Win32::Foundation::ERROR_SERVICE_DOES_NOT_EXIST,"Waiting for the installed service to recover. If it stays offline, use Settings > Start service or repair the installation.");
    }
    let helper=std::env::current_exe()?.with_file_name("OutSwitchService.exe");
    anyhow::ensure!(helper.is_file(),"OutSwitchService.exe is missing. Extract the full package or repair the installation.");
    let child=std::process::Command::new(helper).args(["--console","--parent",&std::process::id().to_string()])
        .creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW)
        .stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn()?;
    Ok(child)
}
fn start_installed_service()->anyhow::Result<()>{
    use windows_sys::Win32::UI::{Shell::ShellExecuteW,WindowsAndMessaging::SW_HIDE};
    let exe=std::env::current_exe()?.with_file_name("OutSwitchService.exe");
    anyhow::ensure!(exe.is_file(),"Service application is missing. Repair the installation.");
    let wide=|s:&str|s.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let result=unsafe{ShellExecuteW(std::ptr::null_mut(),wide("runas").as_ptr(),wide(&exe.to_string_lossy()).as_ptr(),wide("--start-service").as_ptr(),std::ptr::null(),SW_HIDE)};
    anyhow::ensure!(result as isize>32,"Windows did not start the service. Approval may have been cancelled.");Ok(())
}
struct App {
    updater:updater::Updater,icons:icons::Icons,pings:ping_panel::PingPanel,
    picker:app_picker::Picker,initial_scan:Option<Receiver<app_scan::ScanResult>>,initial_scan_started:bool,
    state:Snapshot,page:usize,search:String,filter:usize,tx:Sender<Request>,rx:Receiver<Update>,
    message:String,editor:Option<Rule>,new_profile:String,rename:BTreeMap<String,String>,
    history:BTreeMap<String,VecDeque<traffic::Sample>>,last_stamp:u64,graph_network:Option<String>,days:u64,
    tray_rx:Receiver<tray::Action>,exit:bool,reset_confirm:bool,settings_draft:Option<Settings>,tray_available:bool,
    screenshot:Option<String>,capture_requested:bool,capture_page:usize,started:std::time::Instant,
}
impl App {
    fn new(cc:&eframe::CreationContext<'_>)->Self{
        diagnostic("Renderer created; initializing application");
        let args:Vec<_>=std::env::args().collect();let screenshot=args.iter().position(|a|a=="--screenshot").and_then(|i|args.get(i+1)).cloned();
        let allow_helper_start=screenshot.is_none();
        let (tx,requests)=mpsc::channel();let (updates,rx)=mpsc::channel();let repaint=cc.egui_ctx.clone();
        std::thread::spawn(move||{
            let mut manager=outswitch_network::NetworkManager::default();
            let mut helper_attempted=false;
            let mut retry_at=std::time::Instant::now();let mut retry_delay=2u64;let mut last_config=Config::default();
            let mut owned_helper:Option<std::process::Child>=None;
            loop {
                let req=match requests.recv_timeout(Duration::from_secs(2)){Ok(r)=>r,Err(mpsc::RecvTimeoutError::Timeout)=>Request::Snapshot,Err(_)=>break};
                let is_snapshot=matches!(req,Request::Snapshot);
                if owned_helper.as_mut().is_some_and(|c|c.try_wait().ok().flatten().is_some()){owned_helper=None;helper_attempted=false;}
                match outswitch_ipc::request(&req){
                    Ok(response)=>{if !response.message.is_empty(){let _=updates.send(Update::Message(response.message));}else if helper_attempted{let _=updates.send(Update::Message(String::new()));helper_attempted=false;}retry_delay=2;if let Some(s)=response.snapshot{last_config=s.config.clone();let _=updates.send(Update::State(s));}},
                    Err(e)=>{
                        if allow_helper_start&&owned_helper.is_none()&&std::time::Instant::now()>=retry_at&&matches!(e.downcast_ref::<std::io::Error>().and_then(|e|e.raw_os_error()),Some(2|109|231|233)){
                            retry_at=std::time::Instant::now()+Duration::from_secs(retry_delay);retry_delay=(retry_delay*2).min(30);
                            helper_attempted=true;
                            match start_local_helper(){
                                Ok(child)=>{owned_helper=Some(child);let _=updates.send(Update::Message("Starting the local Out-Switch helper…".into()));},
                                Err(error)=>{let _=updates.send(Update::Message(error.to_string()));}
                            }
                        }
                        if !is_snapshot{let _=updates.send(Update::Message(e.to_string()));}
                        match manager.snapshot(){Ok(mut networks)=>{
                            for (i,n) in networks.iter_mut().enumerate(){n.label=format!("Net {}",i+1);}
                            let (applications,monitor_error)=match outswitch_monitor::snapshot(&networks){Ok(apps)=>(apps,String::new()),Err(err)=>(vec![],format!(" Application scanner unavailable: {err}."))};
                            let mut config=last_config.clone();for n in &networks{config.network_names.insert(n.id.clone(),n.label.clone());}
                            let process_monitor_available=monitor_error.is_empty();
                            let _=updates.send(Update::State(Snapshot{networks,applications,config,process_monitor_available,engine_status:format!("Service offline. Read-only monitoring. {e}{monitor_error}"),timestamp:now(),..Default::default()}));
                        },Err(err)=>{let _=updates.send(Update::State(Snapshot{engine_status:format!("Service offline. Adapter scanner unavailable: {err}"),timestamp:now(),..Default::default()}));}}
                    }
                }
                repaint.request_repaint();
            }
            if let Some(mut helper)=owned_helper{let _=helper.kill();let _=helper.wait();}
        });
        let (tray_tx,tray_rx)=mpsc::channel();tray::start(tray_tx,cc.egui_ctx.clone());
        let page=args.iter().position(|a|a=="--page").and_then(|i|args.get(i+1)).and_then(|name|PAGES.iter().position(|page|page.to_lowercase().replace(' ',"-")==*name)).unwrap_or(0);
        let mut app=Self{updater:Default::default(),icons:icons::Icons::new(cc.egui_ctx.clone()),pings:Default::default(),picker:Default::default(),initial_scan:None,initial_scan_started:false,state:Snapshot::default(),page,search:String::new(),filter:0,tx,rx,message:String::new(),editor:None,new_profile:String::new(),rename:BTreeMap::new(),history:BTreeMap::new(),last_stamp:0,graph_network:None,days:1,tray_rx,exit:false,reset_confirm:false,settings_draft:None,tray_available:false,screenshot,capture_requested:false,capture_page:page,started:std::time::Instant::now()};
        if allow_helper_start{app.updater.check(&cc.egui_ctx);}
        app.send(Request::Snapshot);app
    }
    fn send(&self,request:Request){let _=self.tx.send(request);}
    fn accent(&self)->Color32{let a=self.state.config.settings.accent;Color32::from_rgb(a[0],a[1],a[2])}
    fn card(&self)->egui::Frame{egui::Frame::none().fill(if self.state.config.settings.light{Color32::WHITE}else{Color32::from_rgb(24,31,43)}).rounding(12.).inner_margin(20.).stroke(egui::Stroke::new(1_f32,if self.state.config.settings.light{Color32::from_gray(222)}else{Color32::from_rgb(40,50,66)}))}
    fn heading(ui:&mut egui::Ui,title:&str,subtitle:&str){ui.heading(RichText::new(title).size(28.));ui.add_space(5.);ui.label(RichText::new(subtitle).color(Color32::from_gray(140)));ui.add_space(22.);}
    fn network_cards(&mut self,ui:&mut egui::Ui,detail:bool){
        let nets=self.state.networks.clone();
        let width=ui.available_width();let columns=((width+12.)/310.).floor().max(1.) as usize;
        let card_width=((width-(columns-1) as f32*12.)/columns as f32-40.).max(200.);
        let shown:Vec<_>=nets.iter().filter(|n|detail||n.connected||self.pings.has_test(&n.id)).collect();
        ui.horizontal_wrapped(|ui|{for n in shown{
            self.card().show(ui,|ui|{ui.vertical(|ui|{
                ui.set_width(card_width);ui.set_max_width(card_width);
                ui.horizontal(|ui|{ui.label(RichText::new(&n.label).color(self.accent()).strong());ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui|{ui.label(if n.connected{"Connected"}else{"Disconnected"});});});
                ui.add_space(12.);ui.label(RichText::new(&n.name).size(23.).strong());ui.label(RichText::new(&n.description).small().color(Color32::from_gray(145)));ui.add_space(18.);
                ui.horizontal(|ui|{ui.vertical(|ui|{ui.label("DOWNLOAD");ui.label(RichText::new(rate(n.down_bps)).size(23.));});ui.add_space(25.);ui.vertical(|ui|{ui.label("UPLOAD");ui.label(RichText::new(rate(n.up_bps)).size(23.));});});
                ui.add_space(14.);ui.separator();ui.add_space(8.);
                ui.label(match n.internet{Some(true)=>"Internet probe: reachable",Some(false)=>"Internet probe: failed",None=>"Internet: not tested"});
                let count=self.state.applications.iter().filter(|a|a.observed_networks.contains(&n.id)).count();ui.label(if self.state.process_monitor_available{format!("{count} apps with a TCP connection on this adapter")}else{"Application activity: unavailable".into()});
                if detail {
                    ui.add_space(10.);ui.label(format!("Type: {}  ·  Link: {} Mbps",n.kind,n.link_bps/1_000_000));
                    ui.label(format!("IP: {}",n.addresses.join(", ")));ui.label(format!("Gateway: {}",n.gateways.join(", ")));ui.label(format!("DNS: {}",n.dns.join(", ")));
                    ui.label(format!("Strong-host sending: {}",if n.strong_host{"yes"}else{"unavailable / disabled"}));
                    if let Some(ms)=n.ping_ms{ui.label(format!("TCP connect latency: {ms:.0} ms"));}
                    let mut text=self.rename.entry(n.id.clone()).or_insert_with(||n.label.clone()).clone();
                    ui.add_space(10.);ui.horizontal(|ui|{ui.add(egui::TextEdit::singleline(&mut text).desired_width(185.));if ui.add_enabled(self.state.service_connected,egui::Button::new("Rename")).clicked(){self.send(Request::RenameNetwork{id:n.id.clone(),name:text.clone()});}});self.rename.insert(n.id.clone(),text);
                    ui.horizontal(|ui|{
                        if ui.add_enabled(self.state.service_connected,egui::Button::new("Test connection")).on_hover_text("Opens a TCP connection from this adapter to 1.1.1.1:443. Sends no application data.").clicked(){self.send(Request::TestNetwork(n.id.clone()));}
                        if ui.button("View apps").clicked(){self.page=1;self.search.clear();self.filter=0;self.graph_network=Some(n.id.clone());}
                    });
                }
                let accent=self.accent();self.pings.show(ui,n,accent);
            });});ui.add_space(8.);
        }});
        if self.state.networks.is_empty(){ui.label("Waiting for Windows to report network adapters…");}
    }
    fn applications(&mut self,ui:&mut egui::Ui,compact:bool){
        if !compact{
            ui.horizontal(|ui|{
                ui.add(egui::TextEdit::singleline(&mut self.search).hint_text("Search applications or paths…").desired_width(300.));
                if ui.add_enabled(self.state.service_connected,egui::Button::new("+ Add application")).clicked(){self.picker.open(ui.ctx(),self.state.applications.clone());}
            });
            ui.add_space(10.);ui.horizontal(|ui|{for (i,name) in ["All","Connected / bound","Games","Custom routing"].iter().enumerate(){ui.selectable_value(&mut self.filter,i,*name);}});
            if let Some(id)=self.graph_network.clone(){ui.horizontal(|ui|{ui.label(format!("TCP connections on {}",label(&self.state,&id)));if ui.small_button("Clear filter").clicked(){self.graph_network=None;}});}
            ui.add_space(18.);
        }
        let apps=self.state.applications.clone();let query=self.search.to_lowercase();let mut visible=Vec::new();
        for app in apps {
            if !compact&&!app.path.to_lowercase().contains(&query)&&!app.name.to_lowercase().contains(&query){continue;}
            if compact&&!self.state.config.pinned_apps.contains(&identity(&app.path)){continue;}
            if !compact&&!self.state.config.settings.show_system&&app.path.to_lowercase().starts_with("c:\\windows\\"){continue;}
            let rule=self.state.config.rules().iter().find(|r|identity(&r.path)==identity(&app.path)).cloned();
            if !compact {
                if self.filter==1&&app.pids.is_empty(){continue;}if self.filter==2&&!rule.as_ref().is_some_and(|r|r.game){continue;}
                if self.filter==3&&!rule.as_ref().is_some_and(|r|r.network.is_some()){continue;}
                if self.graph_network.as_ref().is_some_and(|id|!app.observed_networks.contains(id)){continue;}
            }
            visible.push((app,rule));
        }
        if compact{visible.sort_by_key(|(a,_)|self.state.config.pinned_apps.iter().position(|p|p==&identity(&a.path)).unwrap_or(usize::MAX));}
        else{visible.sort_by_key(|(a,_)|a.name.to_lowercase());}
        let columns=(((ui.available_width()+12.)/247.).floor() as usize).clamp(1,3);
        let tile=(ui.available_width()-(columns-1) as f32*ui.spacing().item_spacing.x)/columns as f32;
        for row in visible.chunks(columns) {
            ui.horizontal_top(|ui|{for (app,rule) in row {
                let icon=self.icons.get(ui.ctx(),&app.path);
                self.card().inner_margin(14.).show(ui,|ui|{ui.vertical(|ui|{
                    ui.set_width(tile-28.);ui.set_max_width(tile-28.);ui.set_min_height(tile-28.);
                    ui.vertical_centered(|ui|{
                        ui.add_space(6.);
                        if let Some(icon)=icon{ui.image((icon.id(),Vec2::splat(44.)));}else{
                            let (rect,_)=ui.allocate_exact_size(Vec2::splat(44.),egui::Sense::hover());
                            ui.painter().rect_filled(rect,9.,self.accent().gamma_multiply(0.2));
                            ui.painter().text(rect.center(),egui::Align2::CENTER_CENTER,app.name.chars().next().unwrap_or('?').to_uppercase().to_string(),egui::FontId::proportional(21.),self.accent());
                        }
                        ui.add(egui::Label::new(RichText::new(&app.name).strong().size(17.)).truncate()).on_hover_text(&app.path);
                        ui.label(RichText::new(format!("{} TCP · {} UDP sockets",app.tcp_connections,app.udp_endpoints)).small().color(Color32::from_gray(145)));
                    });
                    ui.add_space(6.);ui.separator();
                    let mut network=rule.as_ref().and_then(|r|r.network.clone());let previous=network.clone();
                    ui.add_enabled_ui(self.state.service_connected,|ui|{
                        egui::ComboBox::from_id_salt(("route",&app.path)).width(tile-38.).selected_text(configured(&self.state,rule.as_ref())).show_ui(ui,|ui|{
                            ui.selectable_value(&mut network,None,"Automatic");for n in &self.state.networks{ui.selectable_value(&mut network,Some(n.id.clone()),&n.label);}
                        });
                    });
                    if network!=previous{let mut r=rule.clone().unwrap_or_else(||Rule::automatic(app.path.clone()));r.network=network;self.send(Request::SaveRule(r));}
                    ui.add_space(3.);ui.vertical(|ui|{
                        let observed=app.observed_networks.iter().map(|id|label(&self.state,id)).collect::<Vec<_>>().join(", ");
                        let text=if observed.is_empty(){"TCP network: unknown".into()}else{format!("TCP network: {observed}")};
                        ui.add(egui::Label::new(&text).truncate()).on_hover_text(text);
                        let mismatch=rule.as_ref().and_then(|r|r.network.as_ref()).is_some_and(|id|app.observed_networks.iter().any(|o|o!=id));
                        if mismatch{ui.label(RichText::new("Route mismatch · reconnect").small().color(Color32::from_rgb(244,184,99)));}
                        else{ui.label(RichText::new(if rule.as_ref().is_some_and(|r|r.network.is_some())&&!self.state.config.routing_enabled{"Saved · not enforced"}else{"Per-app speed: unavailable"}).small().color(Color32::from_gray(145)));}
                    });
                    ui.add_enabled_ui(self.state.service_connected&&self.state.config.apps_initialized,|ui|{ui.horizontal(|ui|{
                        let pinned=self.state.config.pinned_apps.contains(&identity(&app.path));
                        if ui.small_button(if pinned{"Unpin"}else{"Pin to dashboard"}).clicked(){self.send(Request::PinApp{path:app.path.clone(),pinned:!pinned});}
                        ui.menu_button("More",|ui|{if ui.button("Remove from Out-Switch").on_hover_text("Removes this app and its routing rules from every profile. Does not uninstall the app. Add it again at any time.").clicked(){self.send(Request::RemoveApp(app.path.clone()));ui.close_menu();}});
                    });});
                    if ui.add_sized([tile-28.,26.],egui::Button::new("App settings")).clicked(){self.editor=Some(rule.clone().unwrap_or_else(||Rule::automatic(app.path.clone())));}
                });});
            }});ui.add_space(4.);
        }
        if visible.is_empty(){ui.label(if compact{"Pin apps from Applications to keep your favorites here."}else{"No matching applications. Use Add application to scan or browse for an app."});}
    }
    fn graph(&mut self,ui:&mut egui::Ui){
        ui.horizontal(|ui|{egui::ComboBox::from_id_salt("traffic-network").selected_text(self.graph_network.as_ref().map(|s|label(&self.state,s)).unwrap_or_else(||"All adapters".into())).show_ui(ui,|ui|{ui.selectable_value(&mut self.graph_network,None,"All adapters");for n in &self.state.networks{ui.selectable_value(&mut self.graph_network,Some(n.id.clone()),&n.label);}});ui.colored_label(self.accent(),"— Download");ui.colored_label(Color32::from_rgb(164,152,230),"— Upload");});
        ui.add_space(20.);let points:Vec<traffic::Sample>=if let Some(id)=&self.graph_network{self.history.get(id).map(|h|h.iter().copied().collect()).unwrap_or_default()}else{traffic::combined(&self.history)};
        let peak=points.iter().map(|p|p.down.max(p.up)).fold(1000.,f64::max);
        let first=points.first().map(|p|p.time).unwrap_or(0);let last=points.last().map(|p|p.time).unwrap_or(first);let span=last.saturating_sub(first).max(1);
        ui.label(format!("Scale: {}  ·  {} seconds of samples · hover for details",rate(peak),last.saturating_sub(first)));
        let (rect,response)=ui.allocate_exact_size(Vec2::new(ui.available_width(),230.),egui::Sense::hover());let painter=ui.painter_at(rect);
        let x=|time:u64|rect.left()+time.saturating_sub(first) as f32/span as f32*rect.width();
        for i in 0..5{let y=rect.top()+rect.height()*i as f32/4.;painter.line_segment([egui::pos2(rect.left(),y),egui::pos2(rect.right(),y)],egui::Stroke::new(1_f32,Color32::from_gray(55)));}
        for upload in [false,true]{let line:Vec<_>=points.iter().map(|p|egui::pos2(x(p.time),rect.bottom()-((if upload{p.up}else{p.down})/peak) as f32*rect.height())).collect();if line.len()>1{painter.add(egui::Shape::line(line,egui::Stroke::new(2_f32,if upload{Color32::from_rgb(164,152,230)}else{self.accent()})));}}
        if let Some(pointer)=response.hover_pos(){
            let time=first+(((pointer.x-rect.left())/rect.width()).clamp(0.,1.)*span as f32).round() as u64;
            if let Some(sample)=points.iter().min_by_key(|p|p.time.abs_diff(time)){
                let sx=x(sample.time);painter.line_segment([egui::pos2(sx,rect.top()),egui::pos2(sx,rect.bottom())],egui::Stroke::new(1_f32,Color32::from_gray(180)));
                for (value,color) in [(sample.down,self.accent()),(sample.up,Color32::from_rgb(164,152,230))]{painter.circle_filled(egui::pos2(sx,rect.bottom()-(value/peak) as f32*rect.height()),4.,color);}
                response.on_hover_ui_at_pointer(|ui|{
                    ui.strong(format!("{} seconds ago",now().saturating_sub(sample.time)));
                    ui.label(format!("Download: {}",rate(sample.down)));ui.label(format!("Upload: {}",rate(sample.up)));
                    ui.separator();ui.small("Adapter counters at this sample");
                    ui.label(format!("Received: {:.2} MB",sample.received as f64/1_000_000.));ui.label(format!("Sent: {:.2} MB",sample.sent as f64/1_000_000.));
                });
            }
        }
        if points.is_empty(){ui.label("Waiting for traffic samples…");}
        ui.add_space(15.);ui.label("Measured adapter byte counters since Windows last reset each adapter. VPN and virtual adapters can count the same traffic twice.");
    }

    fn rules(&mut self,ui:&mut egui::Ui){
        for rule in self.state.config.rules().to_vec(){self.card().show(ui,|ui|{ui.label(RichText::new(&rule.path).strong());ui.label(format!("Configured: {}",configured(&self.state,Some(&rule))));ui.label(self.state.rule_status.get(&identity(&rule.path)).map(String::as_str).unwrap_or("Saved only — no routing filter installed"));ui.horizontal(|ui|{if ui.button("Edit").clicked(){self.editor=Some(rule.clone());}if ui.button("Remove rule").clicked(){self.send(Request::DeleteRule(rule.path.clone()));}});});ui.add_space(8.);}
        if self.state.config.rules().is_empty(){ui.label("No rules in this profile. Applications use Windows' automatic selection.");}
    }
    fn profiles(&mut self,ui:&mut egui::Ui){
        for (name,rules) in self.state.config.profiles.clone(){self.card().show(ui,|ui|{ui.horizontal(|ui|{ui.label(RichText::new(&name).size(21.));ui.label(format!("{} rules",rules.len()));if name==self.state.config.active_profile{ui.colored_label(self.accent(),"Active");}else{if ui.button("Activate").clicked(){self.send(Request::SelectProfile(name.clone()));}if ui.small_button("Delete").clicked(){self.send(Request::DeleteProfile(name.clone()));}}});});ui.add_space(10.);}
        ui.add_space(15.);ui.label("Create a copy of the active profile");ui.horizontal(|ui|{ui.text_edit_singleline(&mut self.new_profile);if ui.button("Create profile").clicked(){self.send(Request::CreateProfile(self.new_profile.clone()));self.new_profile.clear();}});
    }
    fn statistics(&mut self,ui:&mut egui::Ui){
        ui.horizontal(|ui|{for (days,name) in [(1,"Today"),(7,"7 days"),(30,"30 days")]{ui.selectable_value(&mut self.days,days,name);}});ui.add_space(16.);
        let cutoff=now()/86400+1-self.days;let mut usage:BTreeMap<String,(u64,u64,u32)>=BTreeMap::new();
        for u in &self.state.statistics{if u.day>=cutoff{let row=usage.entry(u.network.clone()).or_default();row.0+=u.downloaded;row.1+=u.uploaded;row.2+=u.disconnects;}}
        for (id,(down,up,disconnects)) in usage {self.card().show(ui,|ui|{ui.label(RichText::new(label(&self.state,&id)).size(20.));ui.label(format!("Downloaded {}    Uploaded {}    Disconnects {disconnects}",total(down),total(up)));});ui.add_space(10.);}
        ui.label("Local adapter usage collected while the service runs. Daily buckets use UTC. Application-level historical usage is not available in this build.");
    }
    fn settings(&mut self,ui:&mut egui::Ui){
        self.card().show(ui,|ui|{ui.set_min_width(ui.available_width());self.updater.ui(ui);});ui.add_space(20.);
        let mut settings=self.settings_draft.clone().unwrap_or_else(||self.state.config.settings.clone());
        self.card().show(ui,|ui|{ui.set_min_width(ui.available_width());
            ui.label(RichText::new("Appearance & monitoring").size(20.));ui.add_space(12.);ui.checkbox(&mut settings.light,"Light theme");ui.horizontal(|ui|{ui.label("Accent");ui.color_edit_button_srgb(&mut settings.accent);});ui.checkbox(&mut settings.show_system,"Show Windows system applications");ui.checkbox(&mut settings.collect_statistics,"Collect local adapter usage statistics");
            ui.add_space(10.);if ui.add_enabled(self.state.service_connected,egui::Button::new("Save settings")).clicked(){self.send(Request::Settings(settings.clone()));self.settings_draft=None;}else{self.settings_draft=Some(settings.clone());}
        });ui.add_space(20.);
        self.card().show(ui,|ui|{ui.set_min_width(ui.available_width());ui.label(RichText::new("Gaming QoS").size(20.));let mut gaming=self.state.config.gaming_mode;if ui.add_enabled(self.state.service_connected,egui::Checkbox::new(&mut gaming,"Use gaming priority markings")).changed(){self.send(Request::Gaming(gaming));}ui.label("Marks games as Critical, Discord as High, launchers as Low and OneDrive as Background. Applies to apps with a selected network; routes stay the same. Router support is required for prioritization.");});ui.add_space(20.);
        self.card().show(ui,|ui|{ui.set_min_width(ui.available_width());ui.label(RichText::new("Privacy").size(20.));ui.label("Telemetry: OFF. No account required. Update checks contact GitHub at startup.");ui.label("The local relay forwards traffic without decrypting TLS or recording payloads.");ui.label("Ping tests send 10 ICMP probes to your selected Cloudflare, Google or Shecan target only when you press Test ping. Connection tests contact 1.1.1.1:443 on request. Results remain local.");});ui.add_space(20.);
        self.card().show(ui,|ui|{ui.set_min_width(ui.available_width());
            ui.label(RichText::new("Contact & support").size(20.));
            ui.label("Get in touch with HosseinSKR, share feedback or support Out-Switch.");
            if ui.button("Contact Us").clicked(){self.page=8;}
        });ui.add_space(20.);
        self.card().show(ui,|ui|{ui.set_min_width(ui.available_width());ui.label(RichText::new("Troubleshooting").size(20.));
            ui.label("Windows restarts the installed service after an unexpected exit. Saved rules return when it reconnects.");
            if ui.add_enabled(!self.state.service_connected,egui::Button::new("Start service (administrator)")).clicked(){
                self.message=match start_installed_service(){Ok(())=>"Approve the Windows prompt to start the service. Reconnecting automatically…".into(),Err(e)=>e.to_string()};
            }ui.label("Remove only this service's session filters. Existing bound connections may need to reconnect.");
            if ui.add_enabled(self.state.service_connected,egui::Button::new("Check routing driver")).clicked(){self.send(Request::DriverCheck);}
            if ui.add(egui::Button::new("STOP ALL OUT-SWITCH ROUTING").fill(Color32::from_rgb(137,47,58))).clicked(){self.send(Request::EmergencyStop);}
            if ui.button("Reset saved network rules…").clicked(){self.reset_confirm=true;}
            if self.reset_confirm{ui.colored_label(Color32::from_rgb(244,184,99),"This deletes saved application rules in every profile.");ui.horizontal(|ui|{if ui.button("Delete rules and stop routing").clicked(){self.send(Request::ResetRules);self.reset_confirm=false;}if ui.button("Cancel").clicked(){self.reset_confirm=false;}});}
        });
    }
    fn contact(&self,ui:&mut egui::Ui){
        self.card().inner_margin(16.).show(ui,|ui|{
            ui.set_width(ui.available_width());
            ui.label(RichText::new("HosseinSKR").size(26.).strong().color(self.accent()));
            ui.label("Creator of Out-Switch");
            ui.add_space(10.);
            ui.label("Have a question, spotted a bug or want to share an idea? Get in touch using the links below.");
        });
        ui.add_space(12.);
        for (title,description,text,url) in [
            ("Telegram","Send a message or share your feedback.","@hosseinSKR","https://t.me/hosseinSKR"),
            ("Email","Questions, bug reports and other inquiries.","hosseinshakeri01@icloud.com","mailto:hosseinshakeri01@icloud.com"),
            ("GitHub","Find HosseinSKR on GitHub.","github.com/HosseinSKR","https://github.com/HosseinSKR"),
            ("Support Out-Switch","If you enjoy the app, you can support its development with a donation.","Donate at reymit.com/mr_gixer","https://reymit.com/mr_gixer"),
        ]{
            self.card().inner_margin(16.).show(ui,|ui|{
                ui.set_width(ui.available_width());
                ui.label(RichText::new(title).size(19.).strong());
                ui.label(RichText::new(description).color(Color32::from_gray(145)));
                ui.add_space(4.);
                ui.hyperlink_to(RichText::new(text).color(self.accent()),url);
            });
            ui.add_space(8.);
        }
    }
    fn editor(&mut self,ctx:&egui::Context){
        let Some(mut rule)=self.editor.take()else{return;};let mut open=true;let mut save=false;
        egui::Window::new("Application routing").open(&mut open).default_width(540.).resizable(true).show(ctx,|ui|{
            ui.label("Executable path");ui.horizontal(|ui|{ui.add(egui::TextEdit::singleline(&mut rule.path).desired_width(400.));if ui.button("Browse…").clicked(){if let Some(path)=browse(){rule.path=path;}}});
            egui::ComboBox::from_id_salt("running-app").selected_text("Select a detected application…").show_ui(ui,|ui|{for app in &self.state.applications{if ui.selectable_label(false,&app.name).on_hover_text(&app.path).clicked(){rule.path=app.path.clone();}}});
            if let Some(app)=self.state.applications.iter().find(|a|identity(&a.path)==identity(&rule.path)){
                ui.label(format!("PIDs: {:?}",app.pids));ui.label(format!("Application socket addresses: {}",app.local_addresses.join(", ")));ui.label("Traffic counters measure relayed payload bytes for this service session.");
            }
            ui.add_space(16.);ui.label("Preferred network");
            egui::ComboBox::from_id_salt("primary").selected_text(configured(&self.state,Some(&rule))).show_ui(ui,|ui|{ui.selectable_value(&mut rule.network,None,"Automatic");for n in &self.state.networks{ui.selectable_value(&mut rule.network,Some(n.id.clone()),&n.label);}});
            ui.add_space(12.);ui.label("Fallback network");egui::ComboBox::from_id_salt("fallback-net").selected_text(rule.fallback.as_ref().map(|id|label(&self.state,id)).unwrap_or_else(||"None".into())).show_ui(ui,|ui|{ui.selectable_value(&mut rule.fallback,None,"None");for n in &self.state.networks{ui.selectable_value(&mut rule.fallback,Some(n.id.clone()),&n.label);}});
            egui::ComboBox::from_id_salt("fallback-mode").selected_text(format!("{:?}",rule.fallback_mode)).show_ui(ui,|ui|{ui.selectable_value(&mut rule.fallback_mode,FallbackMode::None,"No fallback");ui.selectable_value(&mut rule.fallback_mode,FallbackMode::Automatic,"Automatically switch new sockets");ui.selectable_value(&mut rule.fallback_mode,FallbackMode::Ask,"Ask me");});
            ui.checkbox(&mut rule.return_to_primary,"Return new sockets to the preferred network");ui.checkbox(&mut rule.game,"Mark as a game");ui.add_space(12.);
            ui.add_enabled_ui(rule.network.is_some(),|ui|{
                for (title,limit)in [("Download limit",&mut rule.download_mbps),("Upload limit",&mut rule.upload_mbps)]{
                    ui.horizontal(|ui|{let mut enabled=limit.is_some();if ui.checkbox(&mut enabled,title).changed(){*limit=enabled.then_some(10.);}
                        if let Some(value)=limit{ui.add(egui::DragValue::new(value).range(0.01..=100_000.).speed(0.1).suffix(" Mbps"));}
                    });
                }
                egui::ComboBox::from_id_salt("qos-priority").selected_text(format!("QoS: {:?}",rule.priority)).show_ui(ui,|ui|{for priority in Priority::all(){let title=format!("{priority:?}");ui.selectable_value(&mut rule.priority,priority,title);}});
            });
            ui.label("Limits are shared across this app's connections. QoS marks outgoing traffic; your router decides how to prioritize it.");
            ui.colored_label(Color32::from_rgb(244,184,99),"Changing routes or limits reconnects relayed apps. Restart apps with existing connections. Fallback detects link changes; internet loss requires a connection test.");
            ui.add_space(12.);save=ui.add_enabled(self.state.service_connected,egui::Button::new("Save application rule")).clicked();
        });
        if save{self.send(Request::SaveRule(rule));}else if open{self.editor=Some(rule);}
    }
}
impl eframe::App for App {
    fn update(&mut self,ctx:&egui::Context,_frame:&mut eframe::Frame){
        self.pings.poll();self.updater.poll();
        if self.updater.exit{self.exit=true;ctx.send_viewport_cmd(egui::ViewportCommand::Close);}
        if self.screenshot.is_some(){self.page=self.capture_page;}
        if self.started.elapsed()<Duration::from_millis(500){diagnostic("Rendering application frame");}
        while let Ok(update)=self.rx.try_recv(){match update{Update::Message(m)=>self.message=m,Update::State(s)=>{
            if s.timestamp!=self.last_stamp{for n in &s.networks{let h=self.history.entry(n.id.clone()).or_default();h.push_back(traffic::Sample{time:s.timestamp,down:n.down_bps,up:n.up_bps,received:n.received,sent:n.sent});if h.len()>120{h.pop_front();}}self.last_stamp=s.timestamp;}self.state=s;
        }}}
        if self.screenshot.is_none()&&self.state.service_connected&&!self.state.config.apps_initialized&&!self.initial_scan_started{
            self.initial_scan_started=true;self.initial_scan=Some(app_scan::start(ctx.clone(),self.state.applications.clone()));
        }
        if let Some(result)=self.initial_scan.as_ref().and_then(|r|r.try_recv().ok()){
            self.initial_scan=None;if !result.warning.is_empty(){self.message=result.warning;}
            self.send(Request::InitializeApps(result.apps));
        }
        // Debug-only visual fixture. It cannot be enabled in shipped release binaries.
        #[cfg(debug_assertions)]
        if self.screenshot.is_some() && std::env::args().any(|a|a=="--layout-check") {
            let windows=std::env::var("WINDIR").unwrap_or_else(|_|"C:\\Windows".into());
            self.state.applications=[("File Explorer","explorer.exe"),("Notepad","System32\\notepad.exe"),
                ("Command Prompt","System32\\cmd.exe"),("Task Manager","System32\\Taskmgr.exe"),
                ("Registry Editor","regedit.exe"),("Control Panel","System32\\control.exe"),
                ("Windows PowerShell","System32\\WindowsPowerShell\\v1.0\\powershell.exe"),
                ("Management Console","System32\\mmc.exe"),("A long application name to check clipping","System32\\msinfo32.exe")]
                .into_iter().map(|(name,path)|Application{name:name.into(),path:format!("{windows}\\{path}"),..Default::default()}).collect();
            let picker_id=egui::Id::new("picker-layout-started");
            if std::env::args().any(|a|a=="--picker-check")&&!ctx.data(|d|d.get_temp::<bool>(picker_id).unwrap_or(false)){
                ctx.data_mut(|d|d.insert_temp(picker_id,true));self.picker.open(ctx,self.state.applications.clone());
            }
            self.state.config.settings.show_system=true;self.state.config.pinned_apps=self.state.applications.iter().map(|a|identity(&a.path)).collect();
            self.state.engine_status="Layout preview — sample app cards and ping results; no application activity measurements.".into();
            self.state.monitor_status.clear();
            if let Some(n)=self.state.networks.iter().find(|n|n.connected){self.pings.layout_example(&n.id);}
        }
        while let Ok(action)=self.tray_rx.try_recv(){match action{tray::Action::Ready=>self.tray_available=true,tray::Action::Open=>{ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));ctx.send_viewport_cmd(egui::ViewportCommand::Focus);},tray::Action::Hide=>ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false)),tray::Action::Updates=>{self.page=7;self.updater.check(ctx);},tray::Action::Pause=>self.send(Request::EmergencyStop),tray::Action::Exit=>{self.exit=true;ctx.send_viewport_cmd(egui::ViewportCommand::Close);}}}
        if self.tray_available&&self.screenshot.is_none()&&ctx.input(|i|i.viewport().close_requested())&&!self.exit{ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));}
        tray::status(self.state.service_connected,self.state.config.routing_enabled);
        if let Some(path)=self.screenshot.clone(){
            if !self.capture_requested&&self.started.elapsed()>Duration::from_secs(8){ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot);self.capture_requested=true;}
            for event in ctx.input(|i|i.events.clone()){if let egui::Event::Screenshot{image,..}=event{let rgba:Vec<u8>=image.pixels.iter().flat_map(|c|c.to_array()).collect();let _=image::save_buffer(&path,&rgba,image.size[0] as u32,image.size[1] as u32,image::ColorType::Rgba8);self.exit=true;ctx.send_viewport_cmd(egui::ViewportCommand::Close);}}
        }
        let mut visuals=if self.state.config.settings.light{egui::Visuals::light()}else{egui::Visuals::dark()};
        if !self.state.config.settings.light{visuals.panel_fill=Color32::from_rgb(15,21,31);visuals.window_fill=Color32::from_rgb(24,31,43);}
        visuals.selection.bg_fill=self.accent().gamma_multiply(0.35);visuals.widgets.active.bg_fill=self.accent().gamma_multiply(0.5);ctx.set_visuals(visuals);
        ctx.style_mut(|s|{s.spacing.item_spacing=Vec2::new(10.,9.);s.spacing.scroll=egui::style::ScrollStyle::solid();s.visuals.widgets.inactive.rounding=egui::Rounding::same(6.);});
        egui::SidePanel::left("sidebar").exact_width(205.).resizable(false).frame(egui::Frame::none().fill(if self.state.config.settings.light{Color32::from_gray(239)}else{Color32::from_rgb(19,26,37)}).inner_margin(20.)).show(ctx,|ui|{
            ui.add_space(16.);ui.label(RichText::new("↗  OUT-SWITCH").size(21.).strong().color(self.accent()));ui.label(RichText::new("Your apps. Your connections.").small().color(Color32::from_gray(140)));ui.add_space(35.);
            let navigation_height=if ui.available_height()<540.{30.}else{38.};
            egui::ScrollArea::vertical().id_salt("sidebar-navigation").max_height((ui.available_height()-100.).max(80.)).show(ui,|ui|{
                for (i,page) in PAGES.iter().enumerate(){if ui.add_sized([165.,navigation_height],egui::SelectableLabel::new(self.page==i,*page)).clicked(){self.page=i;self.graph_network=None;}}
            });
            ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT),|ui|{if ui.small_button("Exit desktop app").clicked(){self.exit=true;ctx.send_viewport_cmd(egui::ViewportCommand::Close);}ui.label(RichText::new(format!("v{VERSION} · Preview")).small());ui.label(if self.state.service_connected{"● Service connected"}else{"○ Service offline"});ui.label(RichText::new("LOCAL ONLY").small().color(self.accent()));});
        });
        egui::TopBottomPanel::top("toolbar").frame(egui::Frame::none().inner_margin(egui::Margin::symmetric(26.,16.))).show(ctx,|ui|{ui.horizontal(|ui|{
            ui.label(RichText::new("NETWORK CONTROL").small().color(Color32::from_gray(140)));
            let mut enabled=self.state.service_connected&&self.state.config.routing_enabled;
            if ui.add_enabled(self.state.service_connected&&(self.state.routing_available||enabled),egui::Checkbox::new(&mut enabled,if !self.state.service_connected{"Offline"}else if self.state.config.routing_enabled{"ON"}else{"OFF"})).on_disabled_hover_text(if self.state.service_connected{self.state.routing_unavailable_reason.as_str()}else{"The local service is offline"}).changed(){self.send(Request::Routing(enabled));}
            ui.add_space(20.);ui.label("Profile");let mut selected=self.state.config.active_profile.clone();
            egui::ComboBox::from_id_salt("profile").selected_text(&selected).show_ui(ui,|ui|{for name in self.state.config.profiles.keys(){ui.selectable_value(&mut selected,name.clone(),name);}});if selected!=self.state.config.active_profile{self.send(Request::SelectProfile(selected));}
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui|{if ui.button("Stop all routing").clicked(){self.send(Request::EmergencyStop);}ui.label(RichText::new("TCP + UDP · local relay").small().color(Color32::from_gray(145)));});
        });});
        egui::CentralPanel::default().frame(egui::Frame::none().fill(if self.state.config.settings.light{Color32::from_gray(248)}else{Color32::from_rgb(15,21,31)})).show(ctx,|ui|{
            // The scroll viewport fills the panel; padding belongs to its content.
            egui::ScrollArea::vertical().id_salt(("page",self.page)).auto_shrink([false,false]).show(ui,|ui|{
                egui::Frame::none().inner_margin(28.).show(ui,|ui|{
                ui.set_min_width(ui.available_width());
                Self::heading(ui,PAGES[self.page],match self.page{0=>"A clear view of your connections and the applications using them.",1=>"Choose a connection for each executable. Rules follow the path across restarts.",2=>"Live information from Windows network adapters.",3=>"Live bandwidth, measured locally.",4=>"Saved intentions and installed filters are shown separately.",5=>"Keep a different set of application routes for each activity.",6=>"Your usage history stays on this computer.",8=>"Questions, feedback and support for Out-Switch.",_=>"Appearance, local storage and recovery controls."});
                if self.page!=8{
                ui.horizontal_wrapped(|ui|{
                    let (status,color)=if !self.state.service_connected{("Reconnecting to the service…",Color32::from_rgb(224,176,99))}else if !self.state.config.routing_enabled{("Network Control is off",Color32::from_rgb(224,176,99))}else if !self.state.routing_available{("Routing needs attention",Color32::from_rgb(224,176,99))}else{("Network Control is ready",self.accent())};
                    ui.colored_label(color,status);
                    if self.updater.available()&&ui.small_button("Update available").clicked(){self.page=7;}
                });
                egui::CollapsingHeader::new("Connection details").show(ui,|ui|{ui.label(&self.state.engine_status);});ui.add_space(14.);
                if !self.state.monitor_status.is_empty(){ui.colored_label(Color32::from_rgb(224,176,99),&self.state.monitor_status);ui.add_space(14.);}
                if self.initial_scan.is_some(){ui.horizontal(|ui|{ui.spinner();ui.label("First run: finding your apps…");});}
                if !self.message.is_empty(){self.card().inner_margin(12.).show(ui,|ui|{ui.horizontal_wrapped(|ui|{ui.label(&self.message);if ui.small_button("Dismiss").clicked(){self.message.clear();}});});ui.add_space(14.);}
                if self.state.service_connected&&!self.state.config.settings.welcome_done{self.card().show(ui,|ui|{ui.label(RichText::new("Welcome to Out-Switch").size(24.));ui.label("Review your networks, choose a connection for each app, then restart that app. New installations start with Network Control ON. Pin favorites from Applications.");ui.label("The installer includes the signed networking component. This test build still needs live routing validation on your PC.");if ui.button("Continue with Automatic").clicked(){let mut settings=self.state.config.settings.clone();settings.welcome_done=true;self.send(Request::Settings(settings));}});ui.add_space(18.);}
                }
                match self.page{0=>{self.network_cards(ui,false);ui.add_space(26.);ui.horizontal(|ui|{ui.label(RichText::new("PINNED APPLICATIONS").strong());if ui.button("View all").clicked(){self.page=1;}});ui.add_space(12.);self.applications(ui,true);},1=>self.applications(ui,false),2=>self.network_cards(ui,true),3=>self.graph(ui),4=>self.rules(ui),5=>self.profiles(ui),6=>self.statistics(ui),8=>self.contact(ui),_=>self.settings(ui)}
                if self.page==0&&!self.state.events.is_empty(){ui.add_space(20.);ui.label(RichText::new("RECENT EVENTS").strong());for e in self.state.events.iter().rev().take(4){ui.label(&e.message);}}
                });
            });
        });self.editor(ctx);if let Some(request)=self.picker.show(ctx,&self.state,&mut self.icons){self.send(request);}ctx.request_repaint_after(Duration::from_secs(1));
    }
}
impl Drop for App{fn drop(&mut self){tray::shutdown();}}
fn browse()->Option<String>{unsafe{
    use windows_sys::Win32::UI::Controls::Dialogs::*;
    let mut file=vec![0u16;32768];let filter:Vec<u16>="Windows executable (*.exe)\0*.exe\0\0".encode_utf16().collect();let mut data:OPENFILENAMEW=std::mem::zeroed();data.lStructSize=std::mem::size_of::<OPENFILENAMEW>() as u32;data.lpstrFilter=filter.as_ptr();data.lpstrFile=file.as_mut_ptr();data.nMaxFile=file.len() as u32;data.Flags=OFN_FILEMUSTEXIST|OFN_PATHMUSTEXIST|OFN_NOCHANGEDIR;
    if GetOpenFileNameW(&mut data)==0{return None;}let len=file.iter().position(|v|*v==0).unwrap_or(file.len());Some(String::from_utf16_lossy(&file[..len]))
}}
fn main()->eframe::Result<()> {
    if std::env::var_os("OUTSWITCH_DIAGNOSTICS").is_some(){unsafe{windows_sys::Win32::System::Diagnostics::Debug::SetUnhandledExceptionFilter(Some(crash));}}
    diagnostic("Starting Out-Switch / OpenGL renderer");
    let args:Vec<_>=std::env::args().collect();
    let size=if args.iter().any(|a|a=="--screenshot")&&args.iter().any(|a|a=="--compact"){[860.,620.]}else{[1240.,820.]};
    let bitmap=image::load_from_memory(include_bytes!("../../../assets/OutSwitch.png")).expect("Bundled application icon").into_rgba8();
    let icon=egui::IconData{width:bitmap.width(),height:bitmap.height(),rgba:bitmap.into_raw()};
    let options=eframe::NativeOptions{viewport:egui::ViewportBuilder::default().with_inner_size(size).with_min_inner_size([860.,620.]).with_icon(icon),..Default::default()};
    let result=eframe::run_native("Out-Switch",options,Box::new(|cc|Ok(Box::new(App::new(cc)))));diagnostic(&format!("UI exited: {result:?}"));result
}


