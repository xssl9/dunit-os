# Block Storage v1

**Status:** COMPLETED  
**Roadmap:** [[../../ROADMAP|ROADMAP]]  
**Related:** [[../InProgress/Drivers|Drivers]] · [[../Future/Filesystem|Persistent dunitFS]]

---

## What Was Added

- Block layer supports external device registration through `register_device`.
- QEMU legacy virtio-blk PCI driver registers disk-backed `vd0`.
- `build_and_run_multipass.py --disk virtio` creates and attaches a 1 MiB raw
  disk image for autonomous tests.
- Terminal block diagnostics now cover:
  - `blk`
  - `blkread <device> <lba>`
  - `blkwrite <device> <lba>`
- `blkwrite` writes a deterministic `DUNIT-BLOCK-STORAGE-V1` sector pattern for
  read-after-write verification.

---

## Verified Contract

- Boot succeeds in terminal mode.
- `blk` lists both `ramblk0` and `vd0`.
- `blkread vd0 0` reads the seeded disk banner.
- `blkwrite vd0 3` writes 512 bytes.
- `blkread vd0 3` shows the deterministic storage pattern.

---

## Canonical Test

```bash
python3 build_and_run_multipass.py \
  --mode test-terminal \
  --disk virtio \
  --qemu-timeout 60 \
  --qemu-log qemu_block_v1.log \
  --qemu-test-commands "blk;blkread vd0 0;blkwrite vd0 3;blkread vd0 3" \
  --expect-log "vd0" \
  --expect-log "virtio-blk" \
  --expect-log "vd0 lba=3 written=512" \
  --expect-log "DUNIT-BLOCK-STOR" \
  --expect-log "AGE-V1"
```

The full marker is split by the 16-byte ASCII column in `blkread`, so the
automated expectations intentionally check both halves.

---

## Follow-up

Next milestone: build a minimal mountable dunitFS prototype on `vd0`.
