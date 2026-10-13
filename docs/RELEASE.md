# 0.4 test build and publication gates

This package is prepared locally. Do not publish it until HosseinSKR explicitly approves.
The included driver is the official WinDivert 2.2.2-A x64 binary, not the retired unsigned
OutSwitchDriver prototype. Do not enable test-signing or disable security to run it.

## Before calling this ready

1. On a disposable clean Windows 10/11 x64 machine, run the full Rust tests and
   `scripts/Test-Installer.ps1 -MsiPath <MSI>`. This checks service/IPC, the default-on (no custom routes)
   state, driver load with a false filter, and uninstall. Upgrade/repair need separate tests.
2. On the target test PC, install the MSI normally. Keep Network Control OFF. From an
   elevated PowerShell run `scripts/Test-LiveRouting.ps1`. This sends small TCP and UDP
   DNS probes to 1.1.1.1:53 and validates replies plus relay counters. DNS blocking can
   fail this test; investigate rather than treating failure as success.
3. With two real internet adapters, route separate test executables simultaneously.
   Capture on both physical interfaces; prove TCP and UDP each exit only the intended
   adapter. Repeat for IPv6. A selected dropdown alone is not evidence.
4. Run several concurrent transfers/processes of the same executable. Measure sustained
   aggregate upload/download rates against limits; allow TCP/window overhead and warm-up.
   Capture outgoing packets to confirm DSCP; separately test router prioritization.
5. Change only one rule while both apps transfer. Verify only that app reconnects.
   Test link loss, manual failed probe, fallback approval, return-to-primary, service crash,
   emergency stop, uninstall, sleep/resume and recovery without stale capture handles.
6. Test target games, Discord, browsers/QUIC and VPN coexistence. Check CPU/RAM under
   sustained load. Verify unknown/system traffic remains operational and privacy limits.
7. Sign the desktop/service/MSI with the publisher's certificate before a public stable
   release. Complete the acceptance review and update the actual validation record.

The app does not include a weighted local QoS scheduler, per-app historical usage,
an updater, or a universal compatibility guarantee. Do not advertise those features.

## Reproducible packaging

`Get-WinDivert.ps1` verifies official ZIP hashes and Authenticode. Ship WinDivert.dll,
WinDivert64.sys, LGPL notices and the exact matching source ZIP. Keep the DLL separately
replaceable. The source archive contains the icon assets, native code, pinned Cargo.lock,
installer and CI scripts. No credentials, personal database or private signing key belongs
in it. CI only uploads test artifacts; it does not create a public release.

## 0.5 app library acceptance checks

On a fresh installation, Network Control starts ON; no custom app route is created automatically.
On upgrade, an explicitly saved OFF remains OFF. First desktop launch scans the signed-in
user's Start menu/desktop shortcuts, App Paths registrations and active network applications.
Check Add application opens a fresh scan, search and Scan again work, and Browse selects an
existing .exe (cancel leaves the library unchanged). Add two apps with the same filename in
different folders; they must stay distinct. Pin both, restart the desktop and service, then
unpin one. Remove the other; it must disappear from all profiles and stay absent while running.
Re-add from the picker and verify it starts without the removed rules or pin.

Hide ping after a completed test, then Show ping test; results should remain. While hidden,
an active test must still finish and can be stopped. Hover different traffic graph positions;
check download/upload and adapter counters change with the sample and adapter selection.
Counter totals are since Windows last reset the adapter; they are not historical daily totals.
