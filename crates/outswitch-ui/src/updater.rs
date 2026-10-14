use eframe::egui;
use serde_json::{Value,json};
use std::{io::{BufRead,BufReader,Write},path::PathBuf,process::{Command,Stdio},sync::{Arc,atomic::{AtomicBool,Ordering},mpsc::{self,Receiver}},time::{Duration,Instant}};
use std::os::windows::process::CommandExt;
#[derive(Clone,Debug)]
pub struct Release{version:String,url:String,hash:String,size:u64}
fn version(s:&str)->Option<[u32;3]>{let parts:Vec<_>=s.strip_prefix('v').unwrap_or(s).split('.').collect();if parts.len()!=3{return None;}Some([parts[0].parse().ok()?,parts[1].parse().ok()?,parts[2].parse().ok()?])}
fn select(data:&Value,current:&str)->Result<Option<Release>,String>{
    let current=version(current).ok_or("Invalid installed version")?;
    let releases=data.as_array().ok_or("GitHub returned an invalid update list")?;
    let mut candidates=Vec::new();
    for r in releases{
        if r["draft"].as_bool()!=Some(false){continue;}
        let Some(tag)=r["tag_name"].as_str()else{continue};let Some(v)=version(tag)else{continue};if v<=current{continue;}
        let name=format!("Out-Switch-{}-x64.msi",tag.trim_start_matches('v'));
        let Some(asset)=r["assets"].as_array().and_then(|a|a.iter().find(|a|a["name"]==name))else{continue};
        let expected=format!("https://github.com/HosseinSKR/out-switch/releases/download/{tag}/{name}");
        if asset["browser_download_url"].as_str()!=Some(&expected){return Err("The update has an unexpected download address".into());}
        let hash=asset["digest"].as_str().and_then(|s|s.strip_prefix("sha256:")).ok_or("This release has no verified checksum. Open GitHub to review it manually.")?;
        if hash.len()!=64||!hash.bytes().all(|c|c.is_ascii_hexdigit()){return Err("Invalid update checksum".into());}
        let size=asset["size"].as_u64().filter(|s|*s>0&&*s<=256*1024*1024).ok_or("Invalid installer size")?;
        candidates.push((v,Release{version:tag.into(),url:expected,hash:hash.into(),size}));
    }
    candidates.sort_by_key(|r|r.0);Ok(candidates.pop().map(|r|r.1))
}
#[derive(PartialEq)]enum Phase{Idle,Checking,Available,Downloading,Verifying,Ready,Installing,Error}
pub struct Updater{phase:Phase,message:String,release:Option<Release>,bytes:u64,path:Option<PathBuf>,rx:Option<Receiver<Value>>,cancel:Arc<AtomicBool>,pub exit:bool,checked:Option<Instant>}
impl Default for Updater{fn default()->Self{Self{phase:Phase::Idle,message:"Updates are checked each time Out-Switch opens.".into(),release:None,bytes:0,path:None,rx:None,cancel:Arc::new(AtomicBool::new(false)),exit:false,checked:None}}}
impl Updater{
    fn busy(&self)->bool{matches!(self.phase,Phase::Checking|Phase::Downloading|Phase::Verifying|Phase::Installing)}
    fn worker(&mut self,input:Value,ctx:&egui::Context){
        let(tx,rx)=mpsc::channel();self.rx=Some(rx);self.cancel=Arc::new(AtomicBool::new(false));let cancel=self.cancel.clone();let ctx=ctx.clone();
        std::thread::spawn(move||{
            let result=(||->anyhow::Result<()>{
                let ps=PathBuf::from(std::env::var_os("SystemRoot").unwrap_or_else(||"C:\\Windows".into())).join("System32/WindowsPowerShell/v1.0/powershell.exe");
                let mut child=Command::new(ps).args(["-NoLogo","-NoProfile","-NonInteractive","-Command",include_str!("update-worker.ps1")]).creation_flags(0x08000000).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn()?;
                if let Err(e)=child.stdin.take().unwrap().write_all(input.to_string().as_bytes()){let _=child.kill();let _=child.wait();return Err(e.into());}
                let stdout=child.stdout.take().unwrap();let tx_read=tx.clone();let repaint=ctx.clone();
                let reader=std::thread::spawn(move||{for line in BufReader::new(stdout).lines().map_while(Result::ok){if let Ok(value)=serde_json::from_str::<Value>(&line){let _=tx_read.send(value);repaint.request_repaint();}}});
                let start=Instant::now();loop{if child.try_wait()?.is_some(){break;}if cancel.load(Ordering::Relaxed)||start.elapsed()>Duration::from_secs(900){let _=child.kill();let _=child.wait();let _=reader.join();anyhow::bail!("Update cancelled or timed out. You can retry at any time.");}std::thread::sleep(Duration::from_millis(100));}
                let _=reader.join();Ok(())
            })();
            if let Err(e)=result{let _=tx.send(json!({"kind":"error","message":e.to_string()}));}
            let _=tx.send(json!({"kind":"finished"}));ctx.request_repaint();
        });
    }
    pub fn check(&mut self,ctx:&egui::Context){if self.busy(){return;}self.cleanup();self.release=None;self.phase=Phase::Checking;self.message="Checking GitHub for updates…".into();self.worker(json!({"action":"check"}),ctx);}
    fn cleanup(&mut self){if let Some(path)=self.path.take(){let _=std::fs::remove_file(&path);if let Some(parent)=path.parent(){let _=std::fs::remove_dir(parent);}}}
    pub fn poll(&mut self){
        while let Some(value)=self.rx.as_ref().and_then(|r|r.try_recv().ok()){
            match value["kind"].as_str().unwrap_or(""){
                "releases"=>{self.checked=Some(Instant::now());match select(&value["data"],outswitch_common::VERSION){Ok(Some(r))=>{self.message=format!("{} is available",r.version);self.release=Some(r);self.phase=Phase::Available;},Ok(None)=>{self.message="You're up to date.".into();self.phase=Phase::Idle;},Err(e)=>{self.message=e;self.phase=Phase::Error;}}},
                "progress"=>self.bytes=value["bytes"].as_u64().unwrap_or(0),
                "verifying"=>{self.phase=Phase::Verifying;self.message="Verifying installer…".into();},
                "ready"=>{self.phase=Phase::Ready;self.message="Verified and ready to install. Your settings will be kept.".into();},
                "installing"=>{self.exit=true;},
                "error"=>{self.phase=Phase::Error;self.message=value["message"].as_str().unwrap_or("Update failed. Try again.").into();},
                "finished"=>{self.rx=None;if self.busy()&&!self.exit{self.phase=Phase::Error;self.message="The update could not finish. Check your connection and try again.".into();}if self.phase==Phase::Error{self.cleanup();}},_=>{}
            }
        }
    }
    pub fn available(&self)->bool{self.release.is_some()}
    pub fn ui(&mut self,ui:&mut egui::Ui){
        ui.label(egui::RichText::new("App updates").size(20.));ui.label(format!("Installed version {} · Preview channel",outswitch_common::VERSION));ui.label(&self.message);
        if self.phase==Phase::Checking||self.phase==Phase::Verifying||self.phase==Phase::Installing{ui.spinner();}
        if self.phase==Phase::Downloading{let size=self.release.as_ref().map(|r|r.size).unwrap_or(1);ui.add(egui::ProgressBar::new(self.bytes as f32/size as f32).text(format!("{:.1} / {:.1} MB",self.bytes as f64/1e6,size as f64/1e6)));}
        ui.horizontal_wrapped(|ui|{
            if ui.add_enabled(!self.busy(),egui::Button::new("Check for updates")).clicked(){self.check(ui.ctx());}
            if self.phase==Phase::Available||self.phase==Phase::Error&&self.release.is_some(){if ui.button("Download update").clicked(){
                self.cleanup();let root=std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("OutSwitch/Updates");
                let unique=format!("{}-{}",std::process::id(),std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos());
                let dir=root.join(unique);match std::fs::create_dir_all(&dir){Ok(())=>{let path=dir.join("Out-Switch-update.msi");self.path=Some(path.clone());let r=self.release.as_ref().unwrap();let input=json!({"action":"download","url":r.url,"size":r.size,"hash":r.hash,"path":path});self.bytes=0;self.phase=Phase::Downloading;self.message="Downloading update…".into();self.worker(input,ui.ctx());},Err(e)=>{self.phase=Phase::Error;self.message=e.to_string();}}
            }}
            if self.phase==Phase::Ready&&ui.button("Install update & close app").clicked(){let input=json!({"action":"install","path":self.path,"hash":self.release.as_ref().unwrap().hash});self.phase=Phase::Installing;self.message="Opening Windows Installer…".into();self.worker(input,ui.ctx());}
            if matches!(self.phase,Phase::Checking|Phase::Downloading|Phase::Verifying)&&ui.button("Cancel").clicked(){self.cancel.store(true,Ordering::Relaxed);}
            ui.hyperlink_to("Release notes","https://github.com/HosseinSKR/out-switch/releases");
        });
        if let Some(at)=self.checked{ui.small(format!("Last checked {} minute(s) ago",at.elapsed().as_secs()/60));}
        ui.small("Checks GitHub at startup. Downloads and installation begin only when you choose them. Windows may ask for administrator approval.");
    }
}
impl Drop for Updater{fn drop(&mut self){self.cancel.store(true,Ordering::Relaxed);}}
#[cfg(test)]mod tests{
    use super::*;
    fn release(v:&str)->Value{json!({"draft":false,"prerelease":true,"tag_name":format!("v{v}"),"assets":[{"name":format!("Out-Switch-{v}-x64.msi"),"browser_download_url":format!("https://github.com/HosseinSKR/out-switch/releases/download/v{v}/Out-Switch-{v}-x64.msi"),"size":123,"digest":format!("sha256:{}","a".repeat(64))}]})}
    #[test]fn chooses_numeric_newest_preview(){let r=select(&json!([release("0.9.0"),release("0.10.0")]),"0.6.0").unwrap().unwrap();assert_eq!(r.version,"v0.10.0");assert!(select(&json!([release("0.5.0"),release("0.6.0")]),"0.6.0").unwrap().is_none());}
    #[test]fn rejects_untrusted_or_unverifiable_installer(){for field in ["browser_download_url","digest","size"]{let mut r=release("0.7.0");r["assets"][0][field]=json!("invalid");assert!(select(&json!([r]),"0.6.0").is_err());}}
    #[test]fn ignores_drafts_and_invalid_versions(){let mut r=release("0.7.0");r["draft"]=json!(true);assert!(select(&json!([r,release("oops")]),"0.6.0").unwrap().is_none());assert!(select(&json!({}),"0.6.0").is_err());}
}
