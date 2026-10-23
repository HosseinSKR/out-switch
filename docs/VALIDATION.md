# 0.6.1 validation — 23 October 2026

Hosted Windows [run 39962399810](https://github.com/HosseinSKR/out-switch/actions/runs/39962399810),
code commit `40453ebb47fcaca45bff751bf1715af9a8bd3016`, passed:

- All 38 tests, including real Windows pipe authorization and endpoint ownership.
- Release compilation and full MSI ICE validation.
- An actual pre-existing TCP connection reset after assigning the probe executable to a network.
- New TCP and UDP DNS requests with validated responses through the selected-adapter relay:
  60 uploaded payload bytes and 124 downloaded payload bytes, with the selected adapter recorded.
- Clean MSI installation, authenticated service IPC, Network Control ON by default, forced
  service termination/restart with restored state, signed driver load, and uninstall.

The delivered package uses locally built GNU-toolchain binaries; hosted CI uses MSVC.
Locally, 36 tests passed; two restricted Windows API tests were covered by hosted CI.
Normal and compact Settings screenshots were inspected, and the MSI's version, icon,
permissions, UI references and all four embedded binary payloads were checked.
Local driver loading returned error 5 (not elevated), and only Ethernet was connected.
The hosted result is a one-adapter smoke test, not proof of Ethernet/Wi-Fi isolation,
live IPv6, sustained bandwidth accuracy, VPN/game compatibility or all upgrade scenarios.

## Changes relevant to the reported bug

Previously, TCP connections already open when assigning a rule continued on their old
route. The service also persisted OFF after capture or topology failures. This version
resets assigned pre-existing TCP sessions on outbound activity, handles packet errors
without stopping capture, and retries fatal failures without changing saved intent.
Steam CEF helpers inherit Steam's rule unless explicitly overridden; games do not.
Relay-confirmed adapter labels and speeds replace misleading local socket observations.

Startup defaults ON for new installs and upgrades. Manual OFF lasts until the service
restarts while the startup option is enabled. Disable the startup option to retain OFF
across restarts. Fatal recovery releases capture handles temporarily; native Windows
networking may operate during recovery. This app is not a leak-prevention firewall.

# 0.6.0 validation — 14 October 2026

Hosted Windows run [38512320804](https://github.com/HosseinSKR/out-switch/actions/runs/38512320804)
passed all 34 tests, compilation, full MSI ICE validation, clean install, authenticated
service IPC, forced service termination and automatic recovery with a new process,
saved routing-on state, signed driver loading, and uninstall. This ran against code
commit `e82aeaa64a1434dce96aa78e638bcc91090fe53c` with the MSVC toolchain.
The download package uses the locally built GNU toolchain binaries, whose payloads
and desktop behavior were checked locally. Live multi-adapter routing remains unverified.

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
