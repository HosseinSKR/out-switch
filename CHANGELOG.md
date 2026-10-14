# 0.6.0 — 14 October 2026

- Fix tray open/hide/exit, including hidden-window wake-up and keyboard activation.
- Restore tray registration after Windows Explorer restarts; group menu actions and show service status.
- Configure Windows to restart the service five seconds after unexpected termination.
- Retry portable helper failures with backoff; keep the app library and settings while reconnecting.
- Add an administrator-approved Start service button for an intentionally stopped installation.
- Check GitHub on each launch; add manual update checks, download progress/cancel, SHA-256 verification and an explicit install button.
- Clean up status details and settings cards while retaining the existing grid and dashboard.
- Correct the MSI Start menu shortcut that failed hosted installer validation.

# 0.5.0 — 13 October 2026

- Network Control defaults ON for fresh settings; saved OFF and emergency stops remain OFF.
- Persistent app library with automatic first-launch discovery, fresh Add application scans,
  search and native Browse. Removal clears all profiles and persists across scans/restarts.
- Pin/unpin any number of apps on Dashboard; the existing responsive square grid remains.
- Hide/show inline ping tests without discarding the latest results.
- Traffic graph hover shows rates and adapter counters at the selected timestamp.
- Keeps the installed service authentication fix and signed networking component from 0.4.1.
- GitHub preparation only; nothing published. Live routing validation remains outstanding.

# Changelog

## 0.4.1 — installed service connection fix

- When Windows denies access to the LocalSystem service process, verify the named-pipe
  server against Windows Service Manager's running PID, exact executable path and
  LocalSystem account. Check the PID again after reading configuration to reject restarts.
- Retain portable-helper process validation, anonymous SQOS, pipe authorization and
  exact executable matching. No security descriptor or service privileges are broadened.
- Add rejection tests for wrong PID, stopped/shared service, wrong executable, injected
  command arguments and wrong service account.
- Deliver an upgrade MSI; the UI can run normally after installation. Live installed
  IPC remains unverified in the restricted build environment.

## 0.4.0 — routing test build

- Replace the runtime dependency on the unsigned custom WFP driver with a separately
  loaded official signed WinDivert component and a local TCP/UDP relay.
- Bind upstream sockets to the selected adapter/source; preserve opaque payloads and
  update only affected application flows when rules change.
- Add aggregate per-executable upload/download limits, DSCP priority markings, gaming
  presets, current-session relay counters and a Settings driver check.
- Embed a new Out-Switch icon in the executable, window, tray and installer shortcuts.
- Bundle driver/DLL, LGPL notices and matching source; add an explicit live traffic test.
- Keep routing off on a fresh installation. Live driver/relay validation remains blocked
  by Windows access denial in this environment; this is not a stable release claim.
- Preserve the 0.3 grid, ping panel, contacts and installer SDDL fix. Nothing published.

## 0.3.0 preview — 2026-10-13

- Fix installer error 1943: remove the unsupported `[UserSID]` property from MsiLockPermissionsEx folder SDDL. Keep the service database restricted to SYSTEM and administrators; desktop access remains through authenticated IPC.
- Add a packaged-MSI permissions check that rejects the old broken descriptor and validates the new one with Windows.
- Show applications as square cards in up to three columns, with a nine-card Dashboard preview and responsive narrower layouts.
- Load native executable icons in a background worker, with a letter fallback.
- Add per-network inline ICMP ping to Cloudflare, Google and Shecan, with cancellation, latency/loss/jitter statistics, a graph and expandable reply details.
- Explicitly bind each probe to the selected source and validate adapter/route identity, disconnection, address changes, duplicate addresses and strong-host sending.
- Report a missing or stopped networking driver explicitly and disable the routing-on control in that state.

Driver signing, real routing tests, bandwidth/QoS and per-app byte accounting remain unfinished. See `docs/VALIDATION.md` for checks actually performed; live ICMP and full installation could not be verified in the restricted environment.

## 0.2.0 preview — 2026-10-13

- Move the page scrollbar to the right edge and preserve scroll positions per page.
- Add Contact Us below Settings, creator credit, Telegram, email, GitHub and donation links.
- Keep sidebar navigation accessible in smaller windows.
- Start a portable helper automatically when no system service is installed; stop it when its parent desktop process exits.
- Keep network reconciliation and saved applications available when application monitoring fails.
- Read the installed owner's identity from the protected machine registry and report service startup only after initialization.
- Add an MSI preview installer, uninstall support, dependency licenses, a source package and Windows CI preparation.
- Fix the kernel driver's NDIS include and notification callback signature; compile an unsigned x64 development driver.

Per-app routing is not enabled in the preview installer. Driver signing, live routing validation, bandwidth enforcement and QoS remain unfinished.
