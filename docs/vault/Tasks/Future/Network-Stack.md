# Network Stack / `netd`

**Status:** DISCOVERY ONLY -> PLANNED IMPLEMENTATION
**Roadmap:** [[../../ROADMAP|ROADMAP]]
**Depends on:** [[../InProgress/Drivers|Drivers]] · [[../InProgress/Kernel-Runtime-Prerequisites|Kernel Runtime]] · [[Libc-Musl|Dunit musl]]

## Current state

`kernel/src/drivers/net.rs` распознаёт PCI network controllers. E1000 probe временно отображает MMIO, читает status/MAC и пишет `packet-io=not-implemented`; RX/TX rings и protocol stack отсутствуют. `net0` означает discovery, не рабочую сеть.

## Target architecture

```text
native apps / musl apps / browser-network service
              -> socket/resolver protocol + event handles
              -> userspace netd
                 Ethernet, ARP/NDP, IP, ICMP, UDP, TCP, DHCP, DNS
              -> bounded shared packet rings
              -> kernel NIC driver: PCI/MMIO/IRQ/DMA/reset
              -> E1000 first, VirtIO-net later
```

Kernel не содержит TCP/DNS и не выдаёт MMIO/DMA приложениям. `netd` перезапускается как supervised service; raw packets требуют отдельного права.

## Ordered milestones

### N0 — E1000 packet I/O

- [ ] Reset/config, DMA-safe RX/TX descriptor rings, interrupts and barriers.
- [ ] Bounded buffer ownership and recycling.
- [ ] Link changes, queue exhaustion, reset recovery and counters.
- [ ] QEMU E1000 bidirectional frame test and flood soak.

### N1 — Ethernet, ARP, IPv4, ICMP

- [ ] Checked Ethernet parser/build path.
- [ ] Bounded ARP cache with retry/expiry.
- [ ] IPv4 checksum/length/TTL validation and routing table.
- [ ] Explicit MTU/fragmentation policy and bounded reassembly if enabled.
- [ ] ICMP echo/errors/rate limits and logical loopback.

### N2 — UDP, DHCP and DNS

- [ ] Datagram endpoints, ports, checksums, queues and readiness.
- [ ] DHCP state machine and atomic configuration apply/rollback.
- [ ] DNS A/AAAA/CNAME, bounded compression parser/cache, timeout/retry and TCP fallback.

### N3 — TCP

- [ ] Full connection state machine, sequence/window validation and teardown.
- [ ] Retransmission, RTT/RTO, out-of-order bounds and FIN/RST.
- [ ] Congestion control, listen/accept backlog and resource quotas.
- [ ] Loss/reorder/duplicate/timeout tests with multi-megabyte stream.

### N4 — native sockets/events

- [ ] Versioned open/bind/connect/listen/accept/send/receive/shutdown protocol.
- [ ] Typed IPv4/IPv6 endpoints, partial I/O and explicit errors.
- [ ] Common Dunit event wait for sockets, IPC, pipes and timers.
- [ ] Rights: connect, low-port bind, listen, raw, interface admin and observation.

### N5 — musl integration

- [ ] Map POSIX socket calls/`sockaddr`/flags/errors to Dunit protocol.
- [ ] Blocking/nonblocking and `poll` semantics over Dunit events.
- [ ] `getaddrinfo/getnameinfo` over resolver service.
- [ ] Native Rust and static-musl C clients pass the same conformance cases.

### N6 — TLS/HTTP/browser path

- [ ] Secure entropy, valid realtime clock and updateable certificate store.
- [ ] Port a proven TLS/crypto library; do not invent crypto.
- [ ] HTTP/1.1 framing/redirects/connection reuse with strict limits; HTTP/2 later.
- [ ] Sandboxed browser-network process owns network policy/cache/cookies.
- [ ] Headless HTTPS fetch, then minimal GUI response viewer through GUI Server.
- [ ] Full HTML/CSS/JS browser engine remains a separate later project.

### N7 — IPv6 and multi-interface

- [ ] IPv6/ICMPv6/NDP/DAD/SLAAC and dual-stack sockets.
- [ ] Multiple links, routes, DNS sources and metrics.
- [ ] Happy Eyeballs in resolver/client layer after stable dual-stack.

## Security and configuration

- System defaults: `/system/share/network/defaults.toml`.
- Service manifest/capabilities: `/system/services/netd.toml`.
- Persistent leases/state: bounded `/var/lib/netd`.
- Volatile endpoints/status: `/run/netd`.
- All packet/DNS/TCP parsers need fuzz/property tests and strict memory/time budgets.
- Default deny for raw sockets, interface configuration and ambient browser renderer access.

## Acceptance for networking v1

E1000 performs real I/O; `netd` obtains DHCP configuration, resolves DNS and maintains TCP; native and static-musl clients work; a sandboxed browser-network service loads a local deterministic HTTPS resource; malformed traffic or `netd` crash does not take down kernel/GUI.

Все автоматические network tests идут через `tools/qemu_test.py` в изолированной deterministic test network, без обязательной зависимости от public Internet.
