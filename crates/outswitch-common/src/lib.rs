use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const PIPE_NAME: &str = r"\\.\pipe\OutSwitch.v1";
pub const MAX_MESSAGE: usize = 2 * 1024 * 1024;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum Priority { Critical, High, #[default] Normal, Low, Background }
impl Priority { pub fn all() -> [Self; 5] { [Self::Critical, Self::High, Self::Normal, Self::Low, Self::Background] } }

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum FallbackMode { #[default] None, Automatic, Ask }

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Rule {
    pub path: String,
    pub network: Option<String>,
    pub fallback: Option<String>,
    pub fallback_mode: FallbackMode,
    pub return_to_primary: bool,
    pub priority: Priority,
    pub download_mbps: Option<f64>,
    pub upload_mbps: Option<f64>,
    pub game: bool,
}
impl Rule {
    pub fn automatic(path: String) -> Self { Self { path, return_to_primary: true, ..Default::default() } }
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(self.path.len() <= 32767 && self.path.to_ascii_lowercase().ends_with(".exe"), "Choose an executable path ending in .exe");
        anyhow::ensure!(self.path.as_bytes().get(1) == Some(&b':') && self.path.as_bytes().get(2) == Some(&b'\\'), "An absolute local executable path is required");
        anyhow::ensure!(!self.path.contains('\0') && !self.path[2..].contains(':'), "Invalid executable path");
        anyhow::ensure!(self.network.is_none() || self.network != self.fallback, "Primary and fallback must be different");
        for limit in [self.download_mbps, self.upload_mbps].into_iter().flatten() {
            anyhow::ensure!(limit.is_finite() && limit >= 0.001 && limit <= 100_000.0, "Limits must be between 0.001 and 100,000 Mbps");
        }
        Ok(())
    }
}

pub fn identity(path: &str) -> String { path.replace('/', "\\").to_lowercase() }

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Network {
    pub id: String, pub index: u32, pub name: String, pub label: String, pub description: String,
    #[serde(default)] pub ipv6_index:u32,
    pub kind: String, pub connected: bool, pub addresses: Vec<String>, pub gateways: Vec<String>,
    pub dns: Vec<String>, pub link_bps: u64, pub received: u64, pub sent: u64,
    pub down_bps: f64, pub up_bps: f64, pub internet: Option<bool>, pub ping_ms: Option<f64>,
    pub strong_host: bool,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Application {
    pub path: String, pub name: String, pub pids: Vec<u32>, pub tcp_connections: u32, pub udp_endpoints: u32,
    pub observed_networks: Vec<String>, pub local_addresses: Vec<String>,
    pub download_bps: Option<f64>, pub upload_bps: Option<f64>, pub downloaded: Option<u64>, pub uploaded: Option<u64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Event { pub time: u64, pub message: String }
fn default_on()->bool{true}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    pub light: bool, pub accent: [u8; 3], pub show_system: bool, pub collect_statistics: bool,
    #[serde(default)] pub welcome_done: bool,
    #[serde(default="default_on")] pub network_control_on_start: bool,
}
impl Default for Settings{fn default()->Self{Self{light:false,accent:[63,179,235],show_system:false,collect_statistics:true,welcome_done:false,network_control_on_start:true}}}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SavedApplication { pub path: String, pub name: String }
impl SavedApplication {
    pub fn from_path(path:String)->Self {
        let name=path.rsplit('\\').next().unwrap_or(&path).trim_end_matches(".exe").to_string();
        Self {path,name}
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    pub profiles: BTreeMap<String, Vec<Rule>>, pub active_profile: String,
    pub network_names: BTreeMap<String, String>, pub settings: Settings,
    pub routing_enabled: bool, pub gaming_mode: bool,
    #[serde(default)] pub apps_initialized: bool,
    #[serde(default)] pub apps: BTreeMap<String,SavedApplication>,
    #[serde(default)] pub pinned_apps: Vec<String>,
}
impl Default for Config {
    fn default() -> Self {
        let mut profiles = BTreeMap::new();
        for name in ["Normal", "Gaming", "Streaming"] { profiles.insert(name.into(), vec![]); }
        Self { profiles, active_profile: "Normal".into(), network_names: BTreeMap::new(),
            settings: Settings { accent: [63, 179, 235], collect_statistics: true, ..Default::default() }, routing_enabled: true, gaming_mode: false, apps_initialized:false, apps:BTreeMap::new(), pinned_apps:vec![] }
    }
}
impl Config {
    pub fn add_app(&mut self,app:SavedApplication) { self.apps.insert(identity(&app.path),app); }
    pub fn remove_app(&mut self,path:&str) {
        let key=identity(path); self.apps.remove(&key); self.pinned_apps.retain(|p|p!=&key);
        for rules in self.profiles.values_mut(){rules.retain(|r|identity(&r.path)!=key);}
    }
    pub fn pin_app(&mut self,path:&str,pinned:bool)->anyhow::Result<()> {
        let key=identity(path);anyhow::ensure!(self.apps.contains_key(&key),"Add this app before pinning it");
        self.pinned_apps.retain(|p|p!=&key);if pinned{self.pinned_apps.push(key);}Ok(())
    }
    pub fn initialize_apps(&mut self,apps:Vec<SavedApplication>) {
        if self.apps_initialized{return;}
        for app in apps {self.apps.entry(identity(&app.path)).or_insert(app);}
        let paths:Vec<_>=self.profiles.values().flatten().map(|r|r.path.clone()).collect();
        for path in paths{self.apps.entry(identity(&path)).or_insert_with(||SavedApplication::from_path(path));}
        self.apps_initialized=true;
    }
    pub fn rules(&self) -> &[Rule] { self.profiles.get(&self.active_profile).map(Vec::as_slice).unwrap_or_default() }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Snapshot {
    #[serde(default)] pub routing_recovering: bool,
    #[serde(default)] pub routing_available: bool,
    #[serde(default)] pub routing_unavailable_reason: String,
    pub networks: Vec<Network>, pub applications: Vec<Application>, pub config: Config,
    pub service_connected: bool, pub process_monitor_available: bool, pub engine_status: String, pub rule_status: BTreeMap<String, String>,
    #[serde(default)] pub monitor_status: String,
    pub events: Vec<Event>, pub statistics: Vec<Usage>, pub timestamp: u64,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Usage { pub day: u64, pub network: String, pub downloaded: u64, pub uploaded: u64, pub disconnects: u32 }
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Request {
    InitializeApps(Vec<SavedApplication>), AddApp(SavedApplication), RemoveApp(String), PinApp {path:String,pinned:bool},
    Snapshot, SaveRule(Rule), DeleteRule(String), Routing(bool), EmergencyStop, ResetRules,
    RenameNetwork { id: String, name: String }, SelectProfile(String), CreateProfile(String), DeleteProfile(String),
    Gaming(bool), Settings(Settings), TestNetwork(String), DriverCheck,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Response { pub ok: bool, pub message: String, pub snapshot: Option<Snapshot> }
pub fn now() -> u64 { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs() }


#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn fresh_default_on_but_upgrade_preserves_pause() {
        assert!(Config::default().routing_enabled);
        let mut old=serde_json::to_value(Config::default()).unwrap();
        old["routing_enabled"]=false.into();for key in ["apps","apps_initialized","pinned_apps"]{old.as_object_mut().unwrap().remove(key);}
        let restored:Config=serde_json::from_value(old).unwrap();assert!(!restored.routing_enabled);assert!(!restored.apps_initialized);
    }
    #[test] fn library_pins_removal_and_readding_survive_restart() {
        let path=r"C:\Apps\Game.exe";let mut c=Config::default();c.initialize_apps(vec![SavedApplication::from_path(path.into())]);
        c.pin_app(&path.to_uppercase(),true).unwrap();c.pin_app(path,true).unwrap();assert_eq!(c.pinned_apps.len(),1);
        for rules in c.profiles.values_mut(){rules.push(Rule::automatic(path.into()));}
        let mut c:Config=serde_json::from_str(&serde_json::to_string(&c).unwrap()).unwrap();
        assert_eq!(c.pinned_apps.len(),1);c.remove_app(path);assert!(c.apps.is_empty());assert!(c.pinned_apps.is_empty());assert!(c.profiles.values().all(Vec::is_empty));
        c.initialize_apps(vec![SavedApplication::from_path(path.into())]);assert!(c.apps.is_empty());
        c.add_app(SavedApplication::from_path(path.into()));assert_eq!(c.apps.len(),1);assert!(c.pinned_apps.is_empty());
    }
    #[test] fn migration_retains_rules_from_every_profile() {
        let mut c=Config::default();c.profiles.get_mut("Gaming").unwrap().push(Rule::automatic(r"C:\Games\old.exe".into()));
        c.initialize_apps(vec![]);assert_eq!(c.apps.len(),1);assert_eq!(c.profiles["Gaming"].len(),1);
    }
}
