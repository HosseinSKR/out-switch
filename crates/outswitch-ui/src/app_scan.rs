use outswitch_common::{identity, Application, SavedApplication};
use std::{collections::BTreeMap, io::Read, os::windows::process::CommandExt, process::{Command,Stdio}, sync::mpsc::{self,Receiver}, time::{Duration,Instant}};

pub struct ScanResult { pub apps:Vec<SavedApplication>, pub warning:String }
pub fn start(ctx:eframe::egui::Context, known:Vec<Application>)->Receiver<ScanResult> {
    let (tx,rx)=mpsc::channel();
    std::thread::spawn(move||{
        let mut found:BTreeMap<String,SavedApplication>=known.into_iter().map(|a|(identity(&a.path),SavedApplication{path:a.path,name:a.name})).collect();
        let mut warning=String::new();
        match installed(){Ok(apps)=>for app in apps{found.insert(identity(&app.path),app);},Err(e)=>warning=format!("Installed-app scan unavailable: {e}. Browse for an .exe or scan again.")}
        // Include newly active processes even when no service snapshot was available.
        if let Ok(apps)=outswitch_monitor::snapshot(&[]){for app in apps{found.entry(identity(&app.path)).or_insert(SavedApplication{path:app.path,name:app.name});}}
        let mut apps:Vec<_>=found.into_values().filter(|a|outswitch_common::Rule::automatic(a.path.clone()).validate().is_ok()&&std::path::Path::new(&a.path).is_file()).collect();
        apps.sort_by_key(|a|a.name.to_lowercase());apps.truncate(1000);
        let _=tx.send(ScanResult{apps,warning});ctx.request_repaint();
    });rx
}
fn installed()->anyhow::Result<Vec<SavedApplication>> {
    let shell=std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap_or_else(||"C:\\Windows".into())).join("System32/WindowsPowerShell/v1.0/powershell.exe");
    // Fixed bundled script. No app name, executable path, or user input is evaluated as code.
    let mut child=Command::new(shell).args(["-NoLogo","-NoProfile","-NonInteractive","-Command",include_str!("scan-apps.ps1")])
        .creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW)
        .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn()?;
    let mut stdout=child.stdout.take().expect("piped output");
    let reader=std::thread::spawn(move||{let mut bytes=Vec::new();stdout.read_to_end(&mut bytes).map(|_|bytes)});
    let started=Instant::now();
    let status=loop {
        if let Some(status)=child.try_wait()?{break status;}
        if started.elapsed()>Duration::from_secs(35){let _=child.kill();let _=child.wait();anyhow::bail!("scan timed out");}
        std::thread::sleep(Duration::from_millis(100));
    };
    anyhow::ensure!(status.success(),"Windows could not complete the scan");
    let bytes=reader.join().map_err(|_|anyhow::anyhow!("scanner output unavailable"))??;
    Ok(serde_json::from_slice(&bytes)?)
}
#[cfg(test)]
mod tests {
    #[test] fn installed_scan_returns_existing_local_executables() {
        let apps=super::installed().expect("local app scan");
        assert!(!apps.is_empty(),"Expected at least one registered desktop app");
        for app in apps{assert!(std::path::Path::new(&app.path).is_file());outswitch_common::Rule::automatic(app.path).validate().unwrap();}
    }
}
