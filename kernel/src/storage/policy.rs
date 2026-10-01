//! Dunit GPT partition policy — the single source of truth for partition type GUIDs.
//!
//! M5 item 1 defines the GPT policy: ESP + Dunit System + Dunit Data. For v1 the
//! installer creates ESP + a single DunitFS root that carries the system; that root is
//! tagged with [`DUNIT_SYSTEM_TYPE_GUID`]. The Dunit Data GUID is defined here as well and
//! recognised by [`classify`] / [`crate::fs::dunitfs::auto_mount`], so introducing a
//! separate data partition later is a pure installer/config change rather than a kernel
//! change. `/system` stays read-mostly: userspace writes are rejected by `PROTECTED_ROOTS`
//! in the syscall layer (see `kernel/src/syscall/mod.rs`).
//!
//! These constants are the ONLY place the Dunit GUIDs are defined on the kernel side.
//! Both installers consume them: the in-kernel installer (`kernel/src/storage/installer.rs`)
//! and the host image builder (`tools/install_disk.py`) MUST write the identical on-disk
//! bytes. `tools/install_disk.py` mirrors `DUNIT_SYSTEM_TYPE_GUID` and points back here.

/// EFI System Partition — standard C12A7328-F81F-11D2-BA4B-00A0C93EC93B (on-disk bytes).
pub const ESP_TYPE_GUID: [u8; 16] = [
    0x28, 0x73, 0x2a, 0xc1, 0x1f, 0xf8, 0xd2, 0x11, 0xba, 0x4b, 0x00, 0xa0, 0xc9, 0x3e, 0xc9, 0x3b,
];

/// Dunit System partition. On-disk bytes spell `DUNIT\0SYSTEM\0\0\0\x01`, so the identity is
/// recognisable in a raw hexdump and never collides with the generic Linux-data GUID
/// (0FC63DAF-8483-4772-8E79-3D69D8477DE4) that the pre-M5 installer reused by mistake. The
/// v1 single DunitFS root is tagged with this.
pub const DUNIT_SYSTEM_TYPE_GUID: [u8; 16] = [
    0x44, 0x55, 0x4e, 0x49, 0x54, 0x00, 0x53, 0x59, 0x53, 0x54, 0x45, 0x4d, 0x00, 0x00, 0x00, 0x01,
];

/// Dunit Data partition. On-disk bytes spell `DUNIT\0DATA\0\0\0\0\0\x02`. Reserved for the
/// ESP + System + Data split; already recognised by [`classify`] so adding a data partition
/// is an installer/config change, not a kernel change.
pub const DUNIT_DATA_TYPE_GUID: [u8; 16] = [
    0x44, 0x55, 0x4e, 0x49, 0x54, 0x00, 0x44, 0x41, 0x54, 0x41, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartitionKind {
    Esp,
    DunitSystem,
    DunitData,
    Other,
}

impl PartitionKind {
    /// Partitions whose contents DunitFS owns and may be auto-mounted as the root.
    pub const fn is_dunitfs(self) -> bool {
        matches!(self, Self::DunitSystem | Self::DunitData)
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Esp => "esp",
            Self::DunitSystem => "system",
            Self::DunitData => "data",
            Self::Other => "other",
        }
    }
}

/// Classify a GPT partition by its 16-byte type GUID against the Dunit policy.
pub fn classify(type_guid: &[u8; 16]) -> PartitionKind {
    if type_guid == &ESP_TYPE_GUID {
        PartitionKind::Esp
    } else if type_guid == &DUNIT_SYSTEM_TYPE_GUID {
        PartitionKind::DunitSystem
    } else if type_guid == &DUNIT_DATA_TYPE_GUID {
        PartitionKind::DunitData
    } else {
        PartitionKind::Other
    }
}
