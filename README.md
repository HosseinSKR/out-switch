# Out-Switch 0.6.1 — Windows routing test build

<p><img src="assets/OutSwitch.png" width="96" alt="Out-Switch icon"></p>

[Download the Windows installer](https://github.com/HosseinSKR/out-switch/releases/tag/v0.6.1)

This update fixes existing connections bypassing newly assigned routes and errors
silently turning Network Control OFF. Steam's web helper follows Steam's selected
network. Settings now separates appearance, monitoring and startup preferences.
The Windows tests include real TCP reconnection and TCP/UDP replies through the selected adapter.

Native desktop app by **HosseinSKR**. Assign an internet adapter to an executable,
set shared upload/download limits, and monitor the relayed traffic locally.

**The Windows CI smoke test passed real TCP reconnection and TCP/UDP forwarding,
plus installation and service recovery. Two-adapter hardware isolation, IPv6 and
sustained bandwidth accuracy still need validation. This remains a preview.** See [validation](docs/VALIDATION.md).

## Install and use

1. Close the old app using **Exit desktop app**, then extract the complete bundle.
2. Open **Out-Switch-0.6.1-x64.msi** and approve Windows' installation prompt.
3. Open **Out-Switch** from Start. Review the connected adapters on Dashboard.
4. Applications are scanned automatically on first desktop launch. Use **Add application**
   for a fresh scan or **Browse files**. **Pin to dashboard** keeps your favorites on Dashboard.
   **More → Remove from Out-Switch** removes the app and its rules from every profile,
   without uninstalling it. Removed apps stay removed until you add them again.
5. Choose an app network and optionally set limits. Check **Network Control ON**, then
   active connections reconnect on their next activity. Network Control starts ON when the
   service starts (including upgrades); turn off this startup behavior in Settings if preferred.
   Manual OFF lasts until the service restarts. Apps that do not reconnect automatically may need reopening.
6. **Stop all routing** removes this app's capture handles and closes its relay sessions.

Requires Windows 10 2004+ or Windows 11 **Intel/AMD x64**. ARM64 and 32-bit Windows
are not supported. No separate Rust, Python, Node.js or .NET runtime is required.
The installer fixes the old SDDL error and installs the background service, driver
files, Start menu icon and dependency licenses. App/MSI code signing is still pending.
The driver is loaded on demand by the installed service; no custom driver certificate
or Microsoft hardware account is required to test this included binary.

For an administrator-run real TCP/UDP smoke test, keep Network Control OFF and run
`scripts/Test-LiveRouting.ps1` from an elevated PowerShell. It tests Cloudflare DNS
over both protocols through each selected adapter; it does not change saved rules.
Failure is reported, never turned into a simulated pass. Test details are in
[release checks](docs/RELEASE.md).

## Included

- App cards with executable icons; three columns in the normal window, persistent pins and an editable app library.
- Per-adapter inline ping tests for Cloudflare, Google and Shecan; Hide/Show preserves results for the current desktop session.
- Traffic graph hover details: sample download/upload rates and adapter byte counters.
- Per-executable IPv4/IPv6 TCP/UDP relay, explicit source/interface binding,
  new-connection fallback and app-specific rule updates.
- Per-app upload/download payload limits shared across connections and processes.
- QoS DSCP markings and gaming presets. **These are priority requests to the network;
  they are not a guaranteed local weighted scheduler.** Routers can ignore markings.
- Actual relayed payload byte counts and speeds for the current service session.
- Profiles, local SQLite settings, adapter usage history, emergency stop and tray.
- Custom app/installer/taskbar icon and Contact Us page.

## Scope and outstanding checks

Two-network isolation, live IPv6, sustained rate accuracy, games/anti-cheat, VPNs,
sleep/resume, and MSI upgrade/repair remain unverified. Clean install/uninstall passed on hosted Windows.
Raw fragments, IPsec, unknown/protected executable ownership, system-resolved DNS,
local/broadcast/multicast traffic are outside relay coverage. Existing TCP sessions belonging
to assigned apps are reset so the app can reconnect through the relay.
Shared UDP ports with ambiguous owners are rejected rather than guessed. Ordinary
unclassified Windows traffic stays native. This is not a firewall or leak-prevention tool.
Limits count application payload, not link-layer overhead. Per-app historical usage
and a local weighted QoS scheduler are not implemented. Portable monitoring works
without elevation; routing needs the installed service or an elevated helper.

## Build and prepare GitHub source

Use Rust 1.99.0, MSVC C++ build tools and Windows SDK on x64 Windows:

```powershell
$env:RUSTFLAGS='-C target-feature=+crt-static'
cargo test --workspace --locked
cargo build --workspace --release --locked
Copy-Item target/release/OutSwitch.exe,target/release/OutSwitchService.exe .
./scripts/Get-WinDivert.ps1
./scripts/Get-Wix.ps1 -Destination ./packages/wix
./scripts/Build-Installer.ps1 -WixDirectory ./packages/wix
```

The driver preparation script downloads official archives, verifies pinned SHA-256
hashes and Authenticode, and includes the matching library source and license.
The retired custom driver under `driver/` and `legacy_wfp.rs` are historical source
only; they are neither built nor installed. The prepared GitHub workflow creates
test artifacts only. Download packages are published separately on the [release page](https://github.com/HosseinSKR/out-switch/releases).

[Architecture](docs/ARCHITECTURE.md) · [Validation](docs/VALIDATION.md) · [Release checks](docs/RELEASE.md)

[Telegram @hosseinSKR](https://t.me/hosseinSKR) ·
[hosseinshakeri01@icloud.com](mailto:hosseinshakeri01@icloud.com) ·
[GitHub](https://github.com/HosseinSKR) · [Donate](https://reymit.com/mr_gixer)

### Application scanning

Scans run in the desktop user session using the built-in Windows PowerShell, without
an extra runtime installation. They read registered desktop app paths, Start menu and
desktop shortcuts, plus active network apps; they do not start scanned programs.
A blocked or timed-out scan leaves Browse available. Store apps without a discoverable
executable, game launchers and portable programs may need their actual .exe selected.
Pinning and removal use the full executable path. First-launch scanning happens once;
opening Add application always rescans, but only your chosen additions enter the library.

Scanner references: [Microsoft App Paths registration](https://learn.microsoft.com/en-us/windows/win32/shell/app-registration)
and [Windows Script Host shortcut API](https://learn.microsoft.com/en-us/troubleshoot/windows-client/admin-development/create-desktop-shortcut-with-wsh).
