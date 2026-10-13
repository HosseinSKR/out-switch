use outswitch_common::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Selection { Automatic, Network(String), Unavailable(String), ApprovalRequired(String) }

/// Only this application's rule participates. Never modifies interface metrics or routes.
pub fn select_network(rule: &Rule, networks: &[Network], previous: Option<&str>) -> Selection {
    let Some(primary) = &rule.network else { return Selection::Automatic; };
    let available = |id: &str| networks.iter().any(|n| n.id == id && n.connected && n.internet != Some(false));
    if !rule.return_to_primary && rule.fallback_mode == FallbackMode::Automatic {
        if let Some(fallback) = rule.fallback.as_deref() {
            if previous == Some(fallback) && available(fallback) { return Selection::Network(fallback.into()); }
        }
    }
    if available(primary) { return Selection::Network(primary.clone()); }
    if let Some(fallback) = rule.fallback.as_ref().filter(|id| available(id)) {
        return match rule.fallback_mode {
            FallbackMode::Automatic => Selection::Network(fallback.clone()),
            FallbackMode::Ask => Selection::ApprovalRequired(fallback.clone()),
            FallbackMode::None => Selection::Unavailable(primary.clone()),
        };
    }
    Selection::Unavailable(primary.clone())
}
pub fn effective_priority(rule: &Rule, gaming: bool) -> Priority {
    if !gaming { return rule.priority.clone(); }
    if rule.game { return Priority::Critical; }
    let exe = rule.path.rsplit('\\').next().unwrap_or("").to_lowercase();
    match exe.as_str() { "discord.exe" => Priority::High, "steam.exe" | "epicgameslauncher.exe" => Priority::Low,
        "onedrive.exe" => Priority::Background, _ => rule.priority.clone() }
}
pub trait RoutingEngine { fn apply(&mut self, rules: &[Rule], networks: &[Network]) -> anyhow::Result<()>; fn clear(&mut self) -> anyhow::Result<()>; }

#[cfg(test)]
mod tests {
    use super::*;
    fn nets() -> Vec<Network> { ["wifi", "ethernet"].map(|id| Network { id: id.into(), connected: true, ..Default::default() }).to_vec() }
    fn rule(path: &str, net: &str) -> Rule { Rule { network: Some(net.into()), ..Rule::automatic(path.into()) } }
    #[test] fn independent_routes_and_changes() {
        let n = nets(); let mut discord = rule(r"C:\Discord.exe", "wifi"); let mut game = rule(r"C:\Game.exe", "ethernet");
        assert_eq!(select_network(&discord, &n, None), Selection::Network("wifi".into()));
        assert_eq!(select_network(&game, &n, None), Selection::Network("ethernet".into()));
        discord.network = Some("ethernet".into());
        assert_eq!(select_network(&discord, &n, None), Selection::Network("ethernet".into()));
        assert_eq!(select_network(&game, &n, None), Selection::Network("ethernet".into()));
        game.network = Some("wifi".into());
        assert_eq!(select_network(&discord, &n, None), Selection::Network("ethernet".into()));
        assert_eq!(select_network(&game, &n, None), Selection::Network("wifi".into()));
    }
    #[test] fn fallback_and_sticky_return() {
        let mut r = rule(r"C:\Discord.exe", "wifi"); r.fallback = Some("ethernet".into()); r.fallback_mode = FallbackMode::Automatic;
        let mut n = nets(); n[0].internet = Some(false);
        assert_eq!(select_network(&r, &n, None), Selection::Network("ethernet".into()));
        n[0].internet = Some(true); assert_eq!(select_network(&r, &n, Some("ethernet")), Selection::Network("wifi".into()));
        r.return_to_primary = false; assert_eq!(select_network(&r, &n, Some("ethernet")), Selection::Network("ethernet".into()));
    }
    #[test] fn ask_never_silently_switches() {
        let mut r = rule(r"C:\App.exe", "wifi"); r.fallback = Some("ethernet".into()); r.fallback_mode = FallbackMode::Ask;
        let mut n = nets(); n[0].connected = false;
        assert_eq!(select_network(&r, &n, None), Selection::ApprovalRequired("ethernet".into()));
    }
    #[test] fn gaming_does_not_change_route() {
        let mut r = rule(r"C:\Game.exe", "ethernet"); r.game = true;
        assert_eq!(effective_priority(&r, true), Priority::Critical); assert_eq!(r.network.as_deref(), Some("ethernet"));
    }
    #[test] fn identity_is_path_not_pid_or_basename() {
        assert_eq!(identity(r"C:\Apps\Discord.EXE"), identity("c:/apps/discord.exe"));
        assert_ne!(identity(r"C:\Apps\App.exe"), identity(r"C:\Other\App.exe"));
    }
    #[test] fn rejects_bad_limits_and_paths() {
        let mut r = rule(r"C:\App.exe", "wifi"); r.upload_mbps = Some(f64::NAN); assert!(r.validate().is_err());
        r.upload_mbps = None; r.path = "app.exe".into(); assert!(r.validate().is_err());
    }
}

