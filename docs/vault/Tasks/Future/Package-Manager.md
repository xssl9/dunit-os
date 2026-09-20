# Application and Package Model

**Status:** PLANNED
**Depends on:** [[Installed-System|Installed System]] · [[Filesystem|DunitFS v2]] · [[Network-Stack|Networking]] · [[Libc-Musl|SDK/libc]]

## Target

Dunit packages — native signed bundles, не `deb`/`rpm` compatibility. Один format обслуживает Rust и C applications, Minimal/DWM images и offline/online installation.

## Package manifest

- Package/app ID, version, architecture and minimum Dunit ABI.
- Executables, resources, services and GUI protocol version.
- Runtime/package dependencies with bounded solver rules.
- Requested capabilities: files, network scopes, GUI, devices, services.
- Data/config/cache directories and uninstall ownership.
- Content hashes, signature and publisher/trust metadata.

## Lifecycle

- [ ] Build reproducible bundle from SDK.
- [ ] Validate manifest/signature/ABI/capabilities before writes.
- [ ] Transactional stage -> verify -> activate -> rollback.
- [ ] Maintain local package database on persistent filesystem.
- [ ] Never remove user data silently on uninstall.
- [ ] Support offline local packages before network repository.
- [ ] Add signed repository only after trusted time, TLS, storage recovery and key rotation.

## Acceptance

Install/update/remove survives interruption without half-installed state; incompatible ABI and missing permissions fail before activation; rollback restores previous app; Minimal and DWM differ by manifests/packages, not kernel forks.

## Not yet

- Public online repository before signatures/time/TLS/transactions.
- Arbitrary maintainer scripts with ambient privileges.
- Linux package format compatibility as platform strategy.
