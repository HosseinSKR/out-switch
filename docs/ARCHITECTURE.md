# 0.6.1 routing architecture

The normal desktop communicates with an owner/SYSTEM/administrator-authorized named
pipe. The installed LocalSystem service owns configuration and routing. Both native
executables are self-contained; data stays local. No code is injected into applications,
TLS is not decrypted, and payloads are never logged or stored.

The client validates the server's executable path. When Windows denies process-object
inspection, it verifies the pipe server PID against the installed OutSwitch own-process
service, exact configured executable and LocalSystem account through read-only SCM APIs.
The running PID is checked before and after configuration reads. This supports normal
desktop users without weakening the pipe/process ACL or granting service control rights.

The new `outswitch-routing` implementation dynamically loads the adjacent WinDivert
DLL with a restricted DLL search path. WinDivert's signed kernel component handles
packet capture and injection. It is separate LGPL software; the matching unmodified
source/build scripts and license accompany the binaries.

## Data path

1. Capture eligible outgoing TCP/UDP. Resolve the full executable path from Windows
   owner tables. TCP matches local+remote tuple; UDP requires an unambiguous owner.
2. Unconfigured/unknown owners and out-of-scope traffic stay on native Windows routes.
3. For a new selected TCP connection, reflect packets to an IPv4/IPv6 loopback-only
   listener with a unique synthetic peer port. Translate the local listener replies
   back to the original remote tuple. Sequence numbers and opaque payload are retained.
4. The relay opens an upstream TCP socket explicitly bound to both the selected source
   address and `IP_UNICAST_IF` / `IPV6_UNICAST_IF`. The IPv4 index is network-order;
   the IPv6 index is host-order. Separate adapter indices are retained for each family.
5. UDP uses a connected upstream socket per original tuple. Replies are reconstructed
   as inbound UDP to the original app. WinDivert recalculates transport/IP checksums.
6. Registered upstream tuples bypass relay recursion and receive the selected DSCP
   marking. The network-layer outbound IfIdx field is **not** used to force a route.

The loopback listener rejects peers with a real local client socket. Only a live
synthetic mapping created from captured application traffic can be accepted.
No inbound firewall rule, global route, metric, gateway or certificate is changed.

## Traffic control and lifecycle

Each executable shares one rate clock per direction across all its connections/PIDs.
Workers wait before forwarding payload. TCP applies backpressure; UDP queues are bounded
and may drop datagrams under sustained overload. DSCP values are Critical=46, High=34,
Normal=0, Low=10, Background=8. This is marking, not a guaranteed weighted scheduler.
Counters measure forwarded payload bytes, not packet overhead or pre-existing sessions.

Updates replace the policy map under a write lock and close/reset only affected
relays. Unchanged app sessions stay open. Topology changes use the same mechanism.
Stopping closes relay sockets, shuts down capture and releases WinDivert handles.
No unrelated WinDivert service is stopped/deleted: other software may share the driver.
Normal relay failures affect that connection. Individual packet failures are dropped and reported without stopping capture. Fatal capture failures close filters and retry with backoff; they never overwrite the saved Network Control choice. The dashboard shows recovery separately from the ON/OFF preference.

Resource bounds: 256 active flows, 8 queued UDP datagrams per flow, 8,192 retained
flow/reverse records, bounded TCP buffers, 120-second UDP idle timeout and 600-second
TCP idle timeout. Closed synthetic mappings linger briefly for late TCP packets.

## Explicit limitations

The hosted one-adapter TCP/UDP integration test passed; two-adapter isolation remains unverified. Fragmented raw packets,
unsupported IPv6 extension chains/IPsec, unknown ownership, local discovery, Windows
DNS Client service traffic are outside this relay. Established TCP sessions for assigned apps are reset on their next outbound packet so the app reconnects through the relay.
Failover is for reconnection, not migration of an established TCP session. App families
with multiple executables generally require separate rules. Steam CEF web helpers under its own bin/cef folder inherit the Steam rule unless an explicit helper rule overrides it; Steam games never inherit this rule. No absolute leak-prevention promise.

Primary references: [WinDivert API](https://reqrypt.org/windivert-doc.html),
[IPv4 interface option](https://learn.microsoft.com/en-us/windows/win32/winsock/ipproto-ip-socket-options),
[IPv6 interface option](https://learn.microsoft.com/en-us/windows/win32/winsock/ipproto-ipv6-socket-options).
