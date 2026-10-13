# Dashboard ping tests

Open Dashboard, find the connected network and choose a target:

| Target | IPv4 address |
|---|---|
| Cloudflare | 1.1.1.1 |
| Google | 8.8.8.8 |
| Shecan | 178.22.122.100 |

Press **Test ping**. The card updates in place and keeps the result after the test. Each network can run its own test concurrently. A test sends ten ICMP echo requests, with 32 bytes of diagnostic payload, at intervals of at least one second. A reply may take up to 1.5 seconds. Stop ends the test after any current request returns; the UI remains responsive. Closing to tray keeps an ongoing test running; exiting the desktop cancels it. No periodic background ping runs without a button press.

The card shows the test's actual source address and target, latest latency, minimum/average/maximum latency, sent/received/lost counts, packet loss percentage and a ten-sample graph. Expand Reply details in the card to see each reply address, latency, byte count and TTL, or its timeout/error. Jitter is the mean absolute change between consecutive successful replies; a missing reply breaks that sequence. Timeouts are not treated as zero-millisecond replies. Windows reports latency with millisecond resolution. The results are kept in memory, not uploaded or persisted to the database.

Changing the target selector does not relabel a previous result: its original target remains shown below. Starting a new test clears that network's previous results. A disconnected adapter retains its last result while Windows still reports the adapter; a removed adapter is no longer displayed. A test stops if the adapter disappears, disconnects or changes its source address.

These targets are IPv4 addresses. An IPv6-only network receives an explicit message. Some targets, VPNs, firewalls or networks block ICMP. A timeout is not proof that browsing is offline. An error opening Windows ICMP is reported as an error rather than a latency result. The existing TCP connection test is separate and never substitutes a TCP measurement for ICMP ping.

## Adapter selection

The implementation uses [IcmpSendEcho2Ex](https://learn.microsoft.com/en-us/windows/win32/api/icmpapi/nf-icmpapi-icmpsendecho2ex) with the selected adapter's explicit source IPv4 address. Before every probe it rechecks adapter identity, index, address and link state, verifies IPv4 strong-host sending and uses GetBestRoute2 with the selected interface index. Ambiguous duplicate source addresses and weak-host adapters are rejected. It does not change system routes, default gateways, interface metrics or firewall rules.

The UI dispatches network work to one background worker per running card. Each test has a bounded ten-probe lifetime and a cancellation channel. Ping remains available if the Out-Switch monitoring service is offline.

## Verification status

Statistics tests cover partial loss, all loss, no samples, zero-ms replies and the jitter gap rule. Adapter enumeration and source/route preflight ran against the connected Ethernet adapter. Opening ICMP is denied by this execution environment, even with network permission granted, so a successful live echo and simultaneous independent Wi-Fi/Ethernet ping have not been verified here. Wi-Fi was disconnected during this check. The shipped binary must be exercised on normal Windows with both networks connected before claiming those scenarios passed.

For a developer console check, `OutSwitchService.exe --ping-list` lists adapters. `OutSwitchService.exe --ping-network "{adapter-id}" --ping-target 1.1.1.1` performs three probes using the same implementation as the GUI. It neither starts a service nor changes routing.
