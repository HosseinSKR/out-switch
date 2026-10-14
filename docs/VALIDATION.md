# 0.6.0 validation — 14 October 2026

- 32 local tests passed. Two restricted Windows integration tests are deferred to hosted CI.
- Real tray tests passed: hide/open, restored visibility, keyboard activation, exit from a hidden window.
- Live GitHub update discovery and an MSI download passed, including streamed progress and SHA-256 verification.
- The Settings layout was captured and inspected; the release build excludes debug fixtures.
- Hosted Windows CI runs all tests, MSI ICE validation, installation, driver smoke check, forced service termination/recovery, IPC reconnection and uninstall.
- Live multi-adapter routing, upgrade/repair across all Windows versions, bandwidth accuracy and VPN/game compatibility still require testing. App and installer publisher signing remains pending.
- The prior 0.5.0 hosted run passed all Rust tests and compilation but failed Start menu shortcut ICE checks; this version changes that shortcut to an advertised file shortcut.

## Previous routing validation notes

# 0.5.0 validation — 13 October 2026

The user confirmed 0.4.1 reports Service connected after installation. This update retains
that authentication fallback. New automated checks cover the default-on migration,
persistent library/pins/removal/re-add across a service restart, hidden running apps,
real local installed-app discovery, and traffic aggregation by actual timestamps.

This is a routing **test build**, not a completed production certification.

- Workspace compilation and native x64 packaging are checked locally. See the bundle's
  BUILD-STATUS.json and SHA256SUMS.txt for the delivered artifacts.
- Twenty-nine focused tests cover policy/fallback/identity, persistence, pipe input validation,
  ping statistics, real executable icon extraction, WinDivert ABI size/layout, IPv4/IPv6
  UDP reconstruction, TCP tuple/sequence/payload preservation, reset generation,
  malformed/fragment rejection, DSCP/ECN preservation, shared rate scheduling and cancel.
- Two existing Windows integration tests (named pipe round trip and endpoint ownership)
  are excluded locally because this execution environment previously denied those APIs.
  They remain in source and the full CI test command. Exclusion is not a pass.
- Official driver ZIP and source ZIP SHA-256 hashes were verified. Authenticode reports
  the driver Valid. `signtool /pa /all` validates both signatures, including Microsoft
  Windows Hardware Compatibility Publisher. This does not substitute for a driver-load test.
- The actual `--driver-check` attempt failed with **Access is denied (Windows error 5)**.
  No successful kernel load or real TCP/UDP relay is claimed. The new administrator-run
  probe validates actual replies and relayed counters, but has not passed here.
- Only Ethernet was connected in the observed environment; Wi-Fi was disconnected.
  Simultaneous Wi-Fi/Ethernet routing, live IPv6, real rate accuracy, failover, games,
  anti-cheat and VPN interaction have not been verified.
- The old installer SDDL issue is fixed with a static protected SYSTEM/administrators
  descriptor. Packaging checks validate the descriptor with Windows and compare embedded
  payloads. Full MSI installation/upgrade/repair/uninstall and ICE validation remain pending.
- The custom app icon is embedded in the desktop/service executables, window, tray,
  Start menu shortcut and installed-program entry. Existing grid and ping UI is retained.
- No Windows security configuration, default route, adapter metric or unrelated firewall
  rule was changed. No driver/service installation succeeded in this session.
- Nothing has been published to GitHub.

Known coverage and implementation gaps are listed in README.md and ARCHITECTURE.md.
Use RELEASE.md before promoting this build. Earlier 0.3 packages preserve historical
validation records for that version.
