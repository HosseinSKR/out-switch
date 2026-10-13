# Required acceptance gates — NOT YET PASSED

Do not describe routing as working or production-ready until evidence exists for these gates. Unit tests of selection logic do not substitute for packet-level verification.

## 0.4 driver gates

1. Verify the official WinDivert archive hashes, Authenticode signatures, accompanying licenses and matching source. The retired custom WDK project is not part of this release.
2. Load the driver with normal Windows security enabled on a dedicated machine. Run the false-filter check followed by the explicit TCP/UDP relay probe. Test repeated start/stop, BFE restart, standby/resume and shutdown.
3. Review packet parsing, executable ownership, local relay admission, bounded buffers, cancellation and handle lifetime. Exercise malformed packets and capture/relay failures.
4. Install the service and bundled component on a test computer with two independent active internet connections. Establish a clean baseline and export routes/interface metrics for comparison. Follow RELEASE.md for the current implementation scope and remaining gates.

## Exact application independence scenario

Use real Discord and a game, with each application's actual network-owning executable identified. Capture traffic independently on both physical interfaces. Use fresh sessions after each rule change. A source-IP label or installed filter alone is insufficient proof.

| Step | Discord | Game | Required evidence |
|---|---|---|---|
| 1 | Wi-Fi / Net 1 | Ethernet / Net 2 | Both transmit simultaneously on their selected interfaces |
| 2 | Ethernet / Net 2 | Ethernet / Net 2 | Discord changes; game's route remains unchanged |
| 3 | Ethernet / Net 2 | Wi-Fi / Net 1 | Game changes; Discord's route remains unchanged |

Repeat with TCP, UDP, IPv4 and IPv6. Test Chrome's same-path subprocesses, QUIC, unconnected UDP `sendto`, explicit binds, DNS resolution, app restart and Windows restart. Confirm no change to default routes or metrics. Verify fallback using fresh sockets, sticky fallback, Ask mode, missing-family blocking and primary restoration. Already-established TCP and existing UDP sockets are expected to require reconnect; this limitation must remain visible.

## Recovery and security gates

- Crash the UI: service rules remain and networking continues.
- Kill the service: its dynamic filters disappear; fresh connections use Windows behavior.
- Emergency stop and Reset remove only Out-Switch filters; unrelated firewall/WFP providers remain intact.
- Test disconnected adapters, disappeared devices, address changes, VPN coexistence and IPv6 privacy-address rotation.
- Test a standard authorized user can use the UI; an unauthorized local user and a remote pipe client cannot alter rules.
- Test oversized/truncated JSON, clients that connect without sending, pipename squatting, database write failures and concurrent profile changes.
- Test signed driver behavior with each game's official anti-cheat under its supported configuration. Do not bypass protections or inject into games.

## Outstanding product gates

The original specification additionally requires working upload/download shaping, contention priority/Gaming Mode, genuine per-app TCP/UDP byte accounting, packet-level egress evidence, automatic internet health detection, notifications, and the remaining settings. These features must be implemented and independently tested before declaring the original request complete.

