use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::cell::UnsafeCell;

use crate::clock;
use crate::drivers::block::{self, BlockDeviceInfo};
use crate::fs::vfs::{
    DirEntry, FileHandle, FileStat, FileSystem, FileType, OpenFlags, Result, VfsError,
    VirtualFileSystem,
};

// ====================================================================
// DunitFS v2 on-disk format
//
// Layout (all relative to the partition start, 512-byte blocks):
//   block 0            primary superblock
//   block 1            backup  superblock
//   slot 0             [allocation bitmap | node table]
//   slot 1             [allocation bitmap | node table]  (COW ping-pong)
//   data region        file extents
//
// Each superblock carries a monotonically increasing `generation` and the
// index of the metadata slot it describes, plus a CRC of that slot image.
// A commit writes the new metadata image to the *inactive* slot, then
// publishes a superblock (gen+1) that points at it — primary first, then
// backup. The active slot is never overwritten in place, so a commit that
// is interrupted mid-write leaves the previous generation fully intact.
//
// Recovery (`load`): among the superblocks with a valid magic/version/CRC,
// pick the highest generation whose referenced slot still matches its
// recorded `slot_crc`. A torn slot write fails the CRC check and the loader
// falls back to the older (backup) generation.
//
// Data blocks are written in place and are NOT crash-atomic; only metadata
// (bitmap + node table + superblock) is. Uncommitted allocations revert on
// crash because the bitmap reverts with the rest of the metadata, so any
// orphaned data blocks are simply marked free again. `fsync`/`close` commit
// the deferred metadata; structural ops commit immediately.
// ====================================================================
const MAGIC: &[u8; 8] = b"DUNITFS2";
const VERSION: u32 = 2;
const BLOCK_SIZE: usize = 512;
const MAX_NODES: usize = 64;
const NODE_SIZE: usize = 256;
const PATH_SIZE: usize = 120;
const MAX_EXTENTS: usize = 6;
const NODE_TABLE_BLOCKS: u64 = (MAX_NODES * NODE_SIZE / BLOCK_SIZE) as u64; // 32
const PRIMARY_SB: u64 = 0;
const BACKUP_SB: u64 = 1;
const FIRST_SLOT_START: u64 = 2;
const DEFAULT_FILE_MODE: u16 = 0o644;
const DEFAULT_DIR_MODE: u16 = 0o755;
const SB_CRC_OFFSET: usize = 508;
const NODE_CRC_OFFSET: usize = 252;

/// Единственная смонтированная ФС DunitFS. `UnsafeCell`-newtype без `static mut`.
/// Монтирование/размонтирование и доступ идут кооперативно на одном CPU.
struct MountedFsCell(UnsafeCell<Option<DunitFs>>);
unsafe impl Sync for MountedFsCell {}
static MOUNTED_FS: MountedFsCell = MountedFsCell(UnsafeCell::new(None));

pub fn is_mounted() -> bool {
    unsafe { (*MOUNTED_FS.0.get()).is_some() }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DunitFsError {
    InvalidBlockSize,
    PartitionTooSmall,
    InvalidSuperblock,
    CorruptMetadata,
    AlreadyMounted,
    Io,
    Vfs(VfsError),
}

impl DunitFsError {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidBlockSize => "block size must be 512 bytes",
            Self::PartitionTooSmall => "partition is too small",
            Self::InvalidSuperblock => "invalid DunitFS superblock",
            Self::CorruptMetadata => "corrupt DunitFS metadata",
            Self::AlreadyMounted => "DunitFS is already mounted",
            Self::Io => "block I/O error",
            Self::Vfs(_) => "VFS mount error",
        }
    }
}

/// Геометрия раздела: вычисляется из его размера, одна и та же в Rust и в
/// `tools/install_disk.py` (единый источник истины формата).
#[derive(Clone, Copy)]
struct Geometry {
    total_blocks: u64,
    bitmap_blocks: u64,
    slot_blocks: u64,
    slot0_start: u64,
    slot1_start: u64,
    data_start: u64,
}

impl Geometry {
    fn derive(total_blocks: u64) -> Self {
        let bitmap_bytes = total_blocks.div_ceil(8);
        let bitmap_blocks = bitmap_bytes.div_ceil(BLOCK_SIZE as u64);
        let slot_blocks = bitmap_blocks + NODE_TABLE_BLOCKS;
        let slot0_start = FIRST_SLOT_START;
        let slot1_start = slot0_start + slot_blocks;
        let data_start = slot1_start + slot_blocks;
        Self {
            total_blocks,
            bitmap_blocks,
            slot_blocks,
            slot0_start,
            slot1_start,
            data_start,
        }
    }

    fn bitmap_len(&self) -> usize {
        (self.bitmap_blocks * BLOCK_SIZE as u64) as usize
    }

    fn slot_bytes(&self) -> usize {
        (self.slot_blocks * BLOCK_SIZE as u64) as usize
    }

    fn slot_start(&self, slot: u32) -> u64 {
        if slot == 0 {
            self.slot0_start
        } else {
            self.slot1_start
        }
    }
}
/// Разобранный суперблок. CRC и magic проверены при чтении.
#[derive(Clone, Copy)]
struct SuperBlock {
    generation: u64,
    active_slot: u32,
    slot_crc: u32,
    geom: Geometry,
}

impl SuperBlock {
    fn encode(&self, block: &mut [u8; BLOCK_SIZE]) {
        block.fill(0);
        block[..8].copy_from_slice(MAGIC);
        put_u32(block, 8, VERSION);
        put_u32(block, 12, BLOCK_SIZE as u32);
        put_u64(block, 16, self.geom.total_blocks);
        put_u64(block, 24, self.generation);
        put_u32(block, 32, self.active_slot);
        put_u32(block, 36, MAX_NODES as u32);
        put_u32(block, 40, NODE_SIZE as u32);
        put_u32(block, 44, self.geom.bitmap_blocks as u32);
        put_u32(block, 48, self.geom.slot_blocks as u32);
        put_u32(block, 52, MAX_EXTENTS as u32);
        put_u32(block, 56, PATH_SIZE as u32);
        put_u64(block, 64, self.geom.slot0_start);
        put_u64(block, 72, self.geom.slot1_start);
        put_u64(block, 80, self.geom.data_start);
        put_u32(block, 88, self.slot_crc);
        let checksum = crc32(&block[..SB_CRC_OFFSET]);
        put_u32(block, SB_CRC_OFFSET, checksum);
    }

    /// Декодирует суперблок и сверяет его с ожидаемой геометрией раздела.
    /// Любое несовпадение magic/version/геометрии/CRC → `None`.
    fn decode(block: &[u8; BLOCK_SIZE], expected: &Geometry) -> Option<Self> {
        if &block[..8] != MAGIC
            || get_u32(block, 8) != VERSION
            || get_u32(block, 12) != BLOCK_SIZE as u32
            || get_u64(block, 16) != expected.total_blocks
            || get_u32(block, 36) != MAX_NODES as u32
            || get_u32(block, 40) != NODE_SIZE as u32
            || get_u32(block, 44) != expected.bitmap_blocks as u32
            || get_u32(block, 48) != expected.slot_blocks as u32
            || get_u32(block, 52) != MAX_EXTENTS as u32
            || get_u32(block, 56) != PATH_SIZE as u32
            || get_u64(block, 64) != expected.slot0_start
            || get_u64(block, 72) != expected.slot1_start
            || get_u64(block, 80) != expected.data_start
            || get_u32(block, SB_CRC_OFFSET) != crc32(&block[..SB_CRC_OFFSET])
        {
            return None;
        }
        let active_slot = get_u32(block, 32);
        if active_slot > 1 {
            return None;
        }
        Some(Self {
            generation: get_u64(block, 24),
            active_slot,
            slot_crc: get_u32(block, 88),
            geom: *expected,
        })
    }
}

#[derive(Clone)]
struct Node {
    path: String,
    file_type: FileType,
    mode: u16,
    size: u64,
    ctime_ns: u64,
    mtime_ns: u64,
    extents: [(u64, u64); MAX_EXTENTS],
    extent_count: usize,
}

impl Node {
    fn allocated_blocks(&self) -> u64 {
        self.extents[..self.extent_count]
            .iter()
            .map(|(_, len)| *len)
            .sum()
    }

    /// Переводит логический блок файла в физический (относительно раздела).
    fn physical_block(&self, logical: u64) -> Option<u64> {
        let mut acc = 0u64;
        for (start, len) in &self.extents[..self.extent_count] {
            if logical < acc + len {
                return Some(start + (logical - acc));
            }
            acc += len;
        }
        None
    }
}

struct OpenHandle {
    node: usize,
    offset: usize,
    flags: OpenFlags,
}

pub struct DunitFs {
    device: BlockDeviceInfo,
    partition_start: u64,
    geom: Geometry,
    generation: u64,
    active_slot: u32,
    bitmap: Vec<u8>,
    nodes: [Option<Node>; MAX_NODES],
    handles: Vec<(FileHandle, OpenHandle)>,
    next_handle: FileHandle,
    dirty: bool,
    /// Деградированный монтаж (fsck/recovery, M5 item 5): слот физически цел, но
    /// часть узлов отброшена как битые. Любая мутация запрещена — ФС read-only.
    read_only: bool,
}
/// Форматирует раздел под пустую DunitFS v2: обнулённая область метаданных,
/// bitmap с занятым служебным регионом `[0..data_start)`, пустая таблица узлов,
/// суперблоки gen=1/active_slot=0. Строго зеркалит `format_dunitfs` в
/// `tools/install_disk.py` (единый источник истины формата).
pub fn format(
    device: BlockDeviceInfo,
    partition_start: u64,
    partition_blocks: u64,
) -> core::result::Result<(), DunitFsError> {
    if unsafe { (*MOUNTED_FS.0.get()).is_some() } {
        return Err(DunitFsError::AlreadyMounted);
    }
    let geom = Geometry::derive(partition_blocks);
    validate_geometry(device, partition_start, partition_blocks, &geom)?;

    // Нулевой образ метаданных: обнуляем суперблоки и оба слота на диске.
    let zero = [0u8; BLOCK_SIZE];
    for relative in 0..geom.data_start {
        write_partition_block(device, partition_start, partition_blocks, relative, &zero)?;
    }

    // Пустой slot 0: bitmap со служебным регионом, нулевая таблица узлов.
    let mut slot = vec![0u8; geom.slot_bytes()];
    {
        let bitmap = &mut slot[..geom.bitmap_len()];
        for block in 0..geom.data_start {
            bit_set(bitmap, block);
        }
    }
    let slot_crc = crc32(&slot);
    write_slot(device, partition_start, partition_blocks, &geom, 0, &slot)?;

    let sb = SuperBlock {
        generation: 1,
        active_slot: 0,
        slot_crc,
        geom,
    };
    let mut block = [0u8; BLOCK_SIZE];
    sb.encode(&mut block);
    write_partition_block(device, partition_start, partition_blocks, PRIMARY_SB, &block)?;
    write_partition_block(device, partition_start, partition_blocks, BACKUP_SB, &block)?;
    Ok(())
}

pub fn mount_global(
    vfs: &mut VirtualFileSystem,
    path: &str,
    device: BlockDeviceInfo,
    partition_start: u64,
    partition_blocks: u64,
) -> core::result::Result<(), DunitFsError> {
    unsafe {
        let slot = MOUNTED_FS.0.get();
        if (*slot).is_some() {
            return Err(DunitFsError::AlreadyMounted);
        }
        *slot = Some(DunitFs::load(device, partition_start, partition_blocks)?);
        let fs = (*slot).as_mut().ok_or(DunitFsError::Io)?;
        if let Err(error) = vfs.mount(path, fs) {
            *slot = None;
            return Err(DunitFsError::Vfs(error));
        }
    }
    Ok(())
}

pub fn auto_mount(vfs: &mut VirtualFileSystem) -> bool {
    if unsafe { (*MOUNTED_FS.0.get()).is_some() } {
        return true;
    }

    let mut devices = [None; 8];
    let count = block::snapshot(&mut devices);
    for device in devices[..count].iter().flatten().copied() {
        let Ok(table) = crate::storage::gpt::read(device) else {
            continue;
        };
        for partition in table.partitions.iter().flatten() {
            // GPT policy (M5 item 1): только Dunit-разделы (System/Data) —
            // кандидаты на корень DunitFS. Чужие разделы отсеиваются по type GUID
            // ещё до чтения суперблока.
            let kind = crate::storage::policy::classify(&partition.type_guid);
            if !kind.is_dunitfs() {
                continue;
            }
            if mount_global(
                vfs,
                "/persist",
                device,
                partition.first_lba,
                partition.blocks(),
            )
            .is_ok()
            {
                crate::serial_write("[GPT-POLICY] matched kind=");
                crate::serial_write(kind.as_str());
                crate::serial_write("\r\n");
                crate::serial_write("[DUNITFS] auto-mounted ");
                crate::serial_write(device.name);
                crate::serial_write(" kind=");
                crate::serial_write(kind.as_str());
                crate::serial_write(" at /persist\r\n");
                return true;
            }
        }
    }
    false
}
impl DunitFs {
    /// Загружает ФС, восстанавливая последнее целостное поколение.
    ///
    /// Стратегия: собрать суперблоки, чей слот физически цел (slot-CRC сходится),
    /// и в порядке убывания поколения попытаться СТРОГУЮ сборку (`from_slot`).
    /// Первый успех → полноценный read/write-монтаж (это же откатывает рваное
    /// новейшее поколение к целому backup). Если строго не поднялось ни одно
    /// поколение, но хотя бы один слот физически цел — ДЕГРАДИРОВАННЫЙ
    /// read-only-монтаж (`from_slot_degraded`): битые/несогласованные узлы
    /// отбрасываются, уцелевшие файлы остаются доступны на чтение. Если целых
    /// слотов нет вовсе — `InvalidSuperblock`.
    fn load(
        device: BlockDeviceInfo,
        partition_start: u64,
        partition_blocks: u64,
    ) -> core::result::Result<Self, DunitFsError> {
        let geom = Geometry::derive(partition_blocks);
        validate_geometry(device, partition_start, partition_blocks, &geom)?;

        let mut block = [0u8; BLOCK_SIZE];
        let mut candidates: [Option<SuperBlock>; 2] = [None, None];
        for (slot, sb_block) in [PRIMARY_SB, BACKUP_SB].into_iter().enumerate() {
            if read_partition_block(device, partition_start, partition_blocks, sb_block, &mut block)
                .is_ok()
            {
                candidates[slot] = SuperBlock::decode(&block, &geom);
            }
        }

        // Суперблоки с физически целым слотом (slot-CRC сходится), от старшего
        // поколения к младшему.
        let mut valid: Vec<(SuperBlock, Vec<u8>)> = Vec::new();
        for sb in candidates.into_iter().flatten() {
            let Ok(slot_image) =
                read_slot(device, partition_start, partition_blocks, &geom, sb.active_slot)
            else {
                continue;
            };
            if crc32(&slot_image) != sb.slot_crc {
                continue;
            }
            valid.push((sb, slot_image));
        }
        valid.sort_by(|a, b| b.0.generation.cmp(&a.0.generation));

        // 1) Строго целостное поколение → полноценный read/write-монтаж. Перебор
        //    от старшего поколения: первая успешная строгая сборка и есть откат
        //    к последнему хорошему состоянию (рваный новейший слот пропускается).
        for (sb, slot_image) in &valid {
            if let Ok(fs) = Self::from_slot(device, partition_start, geom, *sb, slot_image) {
                return Ok(fs);
            }
        }

        // 2) Строго не поднялось ни одно поколение, но слот физически цел →
        //    деградированный read-only-монтаж старшего целого поколения.
        if let Some((sb, slot_image)) = valid.first() {
            let (fs, dropped) =
                Self::from_slot_degraded(device, partition_start, geom, *sb, slot_image);
            crate::serial_write(&format!(
                "[DUNITFS] degraded read-only mount gen={} dropped={} node(s)\r\n",
                fs.generation, dropped
            ));
            return Ok(fs);
        }

        Err(DunitFsError::InvalidSuperblock)
    }

    fn from_slot(
        device: BlockDeviceInfo,
        partition_start: u64,
        geom: Geometry,
        sb: SuperBlock,
        slot_image: &[u8],
    ) -> core::result::Result<Self, DunitFsError> {
        let bitmap = slot_image[..geom.bitmap_len()].to_vec();
        let table = &slot_image[geom.bitmap_len()..];
        let mut nodes: [Option<Node>; MAX_NODES] = core::array::from_fn(|_| None);
        for index in 0..MAX_NODES {
            let entry = &table[index * NODE_SIZE..(index + 1) * NODE_SIZE];
            if entry[0] == 0 {
                continue;
            }
            if get_u32(entry, NODE_CRC_OFFSET) != crc32(&entry[..NODE_CRC_OFFSET]) {
                return Err(DunitFsError::CorruptMetadata);
            }
            nodes[index] = Some(decode_node(entry, &geom)?);
        }
        let mut fs = Self {
            device,
            partition_start,
            geom,
            generation: sb.generation,
            active_slot: sb.active_slot,
            bitmap,
            nodes,
            handles: Vec::new(),
            next_handle: 1,
            dirty: false,
            read_only: false,
        };
        fs.verify_consistency()?;
        Ok(fs)
    }

    /// Деградированная сборка (M5 item 5): как `from_slot`, но вместо отказа при
    /// первом битом/несогласованном узле ОТБРАСЫВАЕТ его и продолжает. Результат —
    /// read-only-ФС только из уцелевших узлов (все мутаторы и `commit` запрещены
    /// `ensure_writable`, так что отброшенные узлы не теряются на диске).
    /// Возвращает (ФС, число отброшенных узлов).
    fn from_slot_degraded(
        device: BlockDeviceInfo,
        partition_start: u64,
        geom: Geometry,
        sb: SuperBlock,
        slot_image: &[u8],
    ) -> (Self, usize) {
        let bitmap = slot_image[..geom.bitmap_len()].to_vec();
        let table = &slot_image[geom.bitmap_len()..];
        let mut nodes: [Option<Node>; MAX_NODES] = core::array::from_fn(|_| None);
        let mut dropped = 0usize;
        for index in 0..MAX_NODES {
            let entry = &table[index * NODE_SIZE..(index + 1) * NODE_SIZE];
            if entry[0] == 0 {
                continue;
            }
            if get_u32(entry, NODE_CRC_OFFSET) != crc32(&entry[..NODE_CRC_OFFSET]) {
                dropped += 1;
                continue;
            }
            match decode_node(entry, &geom) {
                Ok(node) => nodes[index] = Some(node),
                Err(_) => dropped += 1,
            }
        }
        // Отбрасываем узлы, конфликтующие с ранее принятым (дубль пути или
        // пересечение экстентов): оставляем узел с меньшим индексом.
        for index in 0..MAX_NODES {
            if nodes[index].is_none() {
                continue;
            }
            let mut conflict = false;
            for other in 0..index {
                if let (Some(a), Some(b)) = (nodes[index].as_ref(), nodes[other].as_ref()) {
                    if a.path == b.path || extents_overlap(a, b) {
                        conflict = true;
                        break;
                    }
                }
            }
            if conflict {
                nodes[index] = None;
                dropped += 1;
            }
        }
        let fs = Self {
            device,
            partition_start,
            geom,
            generation: sb.generation,
            active_slot: sb.active_slot,
            bitmap,
            nodes,
            handles: Vec::new(),
            next_handle: 1,
            dirty: false,
            read_only: true,
        };
        (fs, dropped)
    }

    /// Проверяет отсутствие дубликатов путей и пересечений экстентов между узлами.
    fn verify_consistency(&self) -> core::result::Result<(), DunitFsError> {
        for index in 0..MAX_NODES {
            let Some(node) = &self.nodes[index] else {
                continue;
            };
            for (other_index, other) in self.nodes.iter().enumerate() {
                if other_index >= index {
                    break;
                }
                let Some(other) = other else {
                    continue;
                };
                if other.path == node.path {
                    return Err(DunitFsError::CorruptMetadata);
                }
                if extents_overlap(node, other) {
                    return Err(DunitFsError::CorruptMetadata);
                }
            }
        }
        Ok(())
    }

    /// Запрещает любую мутацию на деградированном (read-only) монтаже. Так
    /// отброшенные при деградации узлы не вытесняются с диска случайной записью.
    fn ensure_writable(&self) -> Result<()> {
        if self.read_only {
            return Err(VfsError::PermissionDenied);
        }
        Ok(())
    }

    /// Фиксирует отложенные метаданные через ping-pong слотов и смену поколения.
    /// Полный образ метаданных пишется в НЕактивный слот, затем публикуются
    /// суперблоки (gen+1) — сначала primary, потом backup. Активный слот не
    /// перезаписывается, поэтому прерывание коммита сохраняет прошлое поколение.
    fn commit(&mut self) -> Result<()> {
        if !self.dirty {
            return Ok(());
        }
        self.ensure_writable()?;
        let slot_image = self.serialize_slot();
        let slot_crc = crc32(&slot_image);
        let inactive = 1 - self.active_slot;
        write_slot(
            self.device,
            self.partition_start,
            self.geom.total_blocks,
            &self.geom,
            inactive,
            &slot_image,
        )
        .map_err(|_| VfsError::IoError)?;

        let new_generation = self.generation + 1;
        let sb = SuperBlock {
            generation: new_generation,
            active_slot: inactive,
            slot_crc,
            geom: self.geom,
        };
        let mut block = [0u8; BLOCK_SIZE];
        sb.encode(&mut block);
        self.write_raw(PRIMARY_SB, &block)?;
        self.write_raw(BACKUP_SB, &block)?;

        self.active_slot = inactive;
        self.generation = new_generation;
        self.dirty = false;
        Ok(())
    }

    fn serialize_slot(&self) -> Vec<u8> {
        let mut slot = vec![0u8; self.geom.slot_bytes()];
        slot[..self.geom.bitmap_len()].copy_from_slice(&self.bitmap);
        let table = &mut slot[self.geom.bitmap_len()..];
        for index in 0..MAX_NODES {
            if let Some(node) = &self.nodes[index] {
                let entry = &mut table[index * NODE_SIZE..(index + 1) * NODE_SIZE];
                encode_node(entry, node);
            }
        }
        slot
    }

    fn write_raw(&self, relative: u64, data: &[u8; BLOCK_SIZE]) -> Result<()> {
        write_partition_block(
            self.device,
            self.partition_start,
            self.geom.total_blocks,
            relative,
            data,
        )
        .map_err(|_| VfsError::IoError)
    }

    fn read_data_block(&self, relative: u64, data: &mut [u8; BLOCK_SIZE]) -> Result<()> {
        read_partition_block(
            self.device,
            self.partition_start,
            self.geom.total_blocks,
            relative,
            data,
        )
        .map_err(|_| VfsError::IoError)
    }

    fn clean(path: &str) -> &str {
        path.trim_matches('/')
    }

    fn node_index(&self, path: &str) -> Option<usize> {
        let clean = Self::clean(path);
        self.nodes
            .iter()
            .position(|entry| entry.as_ref().map(|node| node.path.as_str()) == Some(clean))
    }

    fn free_node_index(&self) -> Option<usize> {
        self.nodes.iter().position(Option::is_none)
    }

    fn parent_exists(&self, path: &str) -> bool {
        let Some(separator) = path.rfind('/') else {
            return true;
        };
        let parent = &path[..separator];
        self.node_index(parent)
            .and_then(|index| self.nodes[index].as_ref())
            .map(|node| node.file_type == FileType::Directory)
            .unwrap_or(false)
    }

    fn handle_index(&self, handle: FileHandle) -> Option<usize> {
        self.handles.iter().position(|entry| entry.0 == handle)
    }

    /// Освобождает все блоки узла в bitmap и обнуляет его экстенты.
    fn free_extents(&mut self, index: usize) {
        let Some(node) = self.nodes[index].as_mut() else {
            return;
        };
        let extents = node.extents;
        let count = node.extent_count;
        node.extents = [(0, 0); MAX_EXTENTS];
        node.extent_count = 0;
        for (start, len) in &extents[..count] {
            for block in *start..start + len {
                bit_clear(&mut self.bitmap, block);
            }
        }
    }

    /// Гарантирует, что узлу выделено не менее `needed` блоков, расширяя его
    /// экстенты через bitmap (first-fit). Соседние прогоны сливаются в один
    /// экстент; при исчерпании слотов экстентов возвращает ошибку (ENOSPC).
    fn ensure_blocks(&mut self, index: usize, needed: u64) -> Result<()> {
        let have = self.nodes[index].as_ref().ok_or(VfsError::NotFound)?.allocated_blocks();
        if needed <= have {
            return Ok(());
        }
        let mut remaining = needed - have;
        while remaining > 0 {
            let (start, len) =
                alloc_run(&self.bitmap, &self.geom, remaining).ok_or(VfsError::IoError)?;
            let node = self.nodes[index].as_mut().unwrap();
            if node.extent_count > 0 {
                let last = &mut node.extents[node.extent_count - 1];
                if last.0 + last.1 == start {
                    last.1 += len;
                    mark_used(&mut self.bitmap, start, len);
                    remaining -= len;
                    continue;
                }
            }
            if node.extent_count == MAX_EXTENTS {
                return Err(VfsError::IoError);
            }
            node.extents[node.extent_count] = (start, len);
            node.extent_count += 1;
            mark_used(&mut self.bitmap, start, len);
            remaining -= len;
        }
        Ok(())
    }

    fn read_node(&self, index: usize, offset: usize, output: &mut [u8]) -> Result<usize> {
        let node = self.nodes[index].as_ref().ok_or(VfsError::NotFound)?;
        if offset >= node.size as usize {
            return Ok(0);
        }
        let length = output.len().min(node.size as usize - offset);
        let mut done = 0usize;
        let mut sector = [0u8; BLOCK_SIZE];
        while done < length {
            let position = offset + done;
            let logical = (position / BLOCK_SIZE) as u64;
            let within = position % BLOCK_SIZE;
            let physical = node.physical_block(logical).ok_or(VfsError::IoError)?;
            self.read_data_block(physical, &mut sector)?;
            let count = (length - done).min(BLOCK_SIZE - within);
            output[done..done + count].copy_from_slice(&sector[within..within + count]);
            done += count;
        }
        Ok(length)
    }

    /// Пишет данные в файл. Сами блоки данных пишутся сразу; метаданные
    /// (размер/экстенты/bitmap/mtime) помечаются `dirty` и фиксируются при
    /// `fsync`/`close`.
    fn write_node(&mut self, index: usize, offset: usize, input: &[u8]) -> Result<usize> {
        self.ensure_writable()?;
        let end = offset.checked_add(input.len()).ok_or(VfsError::IoError)?;
        let needed = (end as u64).div_ceil(BLOCK_SIZE as u64);
        self.ensure_blocks(index, needed)?;
        let mut done = 0usize;
        let mut sector = [0u8; BLOCK_SIZE];
        while done < input.len() {
            let position = offset + done;
            let logical = (position / BLOCK_SIZE) as u64;
            let within = position % BLOCK_SIZE;
            let count = (input.len() - done).min(BLOCK_SIZE - within);
            let physical = self.nodes[index]
                .as_ref()
                .unwrap()
                .physical_block(logical)
                .ok_or(VfsError::IoError)?;
            if within != 0 || count != BLOCK_SIZE {
                self.read_data_block(physical, &mut sector)?;
            } else {
                sector.fill(0);
            }
            sector[within..within + count].copy_from_slice(&input[done..done + count]);
            self.write_raw(physical, &sector)?;
            done += count;
        }
        let node = self.nodes[index].as_mut().unwrap();
        node.size = node.size.max(end as u64);
        node.mtime_ns = clock::monotonic_ns();
        self.dirty = true;
        Ok(input.len())
    }

    fn create_node(&mut self, path: &str, file_type: FileType) -> Result<()> {
        self.ensure_writable()?;
        let clean = Self::clean(path);
        if clean.is_empty() || clean.len() > PATH_SIZE || !self.parent_exists(clean) {
            return Err(VfsError::InvalidPath);
        }
        if self.node_index(clean).is_some() {
            return Err(VfsError::AlreadyExists);
        }
        let index = self.free_node_index().ok_or(VfsError::IoError)?;
        let now = clock::monotonic_ns();
        let mode = if file_type == FileType::Directory {
            DEFAULT_DIR_MODE
        } else {
            DEFAULT_FILE_MODE
        };
        self.nodes[index] = Some(Node {
            path: String::from(clean),
            file_type,
            mode,
            size: 0,
            ctime_ns: now,
            mtime_ns: now,
            extents: [(0, 0); MAX_EXTENTS],
            extent_count: 0,
        });
        self.dirty = true;
        self.commit()
    }

    /// Переименование. Для каталога переписывает пути всех потомков (prefix
    /// `old/` → `new/`), предварительно проверив, что новые пути укладываются в
    /// `PATH_SIZE`. Фиксируется немедленно (структурная операция).
    fn rename_node(&mut self, old: &str, new: &str) -> Result<()> {
        self.ensure_writable()?;
        let old_clean = Self::clean(old);
        let new_clean = Self::clean(new);
        let index = self.node_index(old_clean).ok_or(VfsError::NotFound)?;
        if new_clean.is_empty() || new_clean.len() > PATH_SIZE {
            return Err(VfsError::InvalidPath);
        }
        if self.node_index(new_clean).is_some() {
            return Err(VfsError::AlreadyExists);
        }
        if !self.parent_exists(new_clean) {
            return Err(VfsError::InvalidPath);
        }
        let is_dir = self.nodes[index].as_ref().unwrap().file_type == FileType::Directory;
        if is_dir {
            let prefix = format!("{}/", old_clean);
            let mut rewrites: Vec<(usize, String)> = Vec::new();
            for (child_index, entry) in self.nodes.iter().enumerate() {
                let Some(node) = entry else {
                    continue;
                };
                if let Some(suffix) = node.path.strip_prefix(&prefix) {
                    let new_child = format!("{}/{}", new_clean, suffix);
                    if new_child.len() > PATH_SIZE {
                        return Err(VfsError::InvalidPath);
                    }
                    rewrites.push((child_index, new_child));
                }
            }
            for (child_index, new_child) in rewrites {
                self.nodes[child_index].as_mut().unwrap().path = new_child;
            }
        }
        let now = clock::monotonic_ns();
        let node = self.nodes[index].as_mut().unwrap();
        node.path = String::from(new_clean);
        node.mtime_ns = now;
        self.dirty = true;
        self.commit()
    }
}

impl FileSystem for DunitFs {
    fn open(&mut self, path: &str, flags: OpenFlags) -> Result<FileHandle> {
        if !flags.is_valid() {
            return Err(VfsError::PermissionDenied);
        }
        let clean = Self::clean(path);
        let index = match self.node_index(clean) {
            Some(index) => index,
            None if flags.create() => {
                self.create(clean)?;
                self.node_index(clean).ok_or(VfsError::IoError)?
            }
            None => return Err(VfsError::NotFound),
        };
        if self.nodes[index].as_ref().unwrap().file_type != FileType::File {
            return Err(VfsError::IsADirectory);
        }
        if flags.trunc() {
            self.truncate(clean)?;
        }
        let offset = if flags.append() {
            self.nodes[index].as_ref().unwrap().size as usize
        } else {
            0
        };
        let handle = self.next_handle;
        self.next_handle += 1;
        self.handles
            .push((handle, OpenHandle { node: index, offset, flags }));
        Ok(handle)
    }

    fn read(&mut self, handle: FileHandle, buf: &mut [u8]) -> Result<usize> {
        let handle_index = self.handle_index(handle).ok_or(VfsError::InvalidDescriptor)?;
        if !self.handles[handle_index].1.flags.can_read() {
            return Err(VfsError::PermissionDenied);
        }
        let node = self.handles[handle_index].1.node;
        let offset = self.handles[handle_index].1.offset;
        let read = self.read_node(node, offset, buf)?;
        self.handles[handle_index].1.offset += read;
        Ok(read)
    }

    fn write(&mut self, handle: FileHandle, buf: &[u8]) -> Result<usize> {
        let handle_index = self.handle_index(handle).ok_or(VfsError::InvalidDescriptor)?;
        if !self.handles[handle_index].1.flags.can_write() {
            return Err(VfsError::PermissionDenied);
        }
        let node = self.handles[handle_index].1.node;
        let offset = if self.handles[handle_index].1.flags.append() {
            self.nodes[node].as_ref().unwrap().size as usize
        } else {
            self.handles[handle_index].1.offset
        };
        let written = self.write_node(node, offset, buf)?;
        self.handles[handle_index].1.offset = offset + written;
        Ok(written)
    }

    fn close(&mut self, handle: FileHandle) -> Result<()> {
        let index = self.handle_index(handle).ok_or(VfsError::InvalidDescriptor)?;
        self.handles.remove(index);
        // Фиксируем отложенные метаданные (размер/экстенты), если они менялись.
        self.commit()
    }

    fn fsync(&mut self, handle: FileHandle) -> Result<()> {
        self.handle_index(handle).ok_or(VfsError::InvalidDescriptor)?;
        self.commit()
    }

    fn readdir(&mut self, path: &str) -> Result<Vec<DirEntry>> {
        let mut buffer = [DirEntry::empty(); 64];
        let count = self.readdir_into(path, &mut buffer)?;
        Ok(buffer[..count].to_vec())
    }

    fn readdir_into(&mut self, path: &str, entries: &mut [DirEntry]) -> Result<usize> {
        let clean = Self::clean(path);
        if !clean.is_empty() {
            let index = self.node_index(clean).ok_or(VfsError::NotFound)?;
            if self.nodes[index].as_ref().unwrap().file_type != FileType::Directory {
                return Err(VfsError::NotADirectory);
            }
        }
        let mut count = 0usize;
        for node in self.nodes.iter().flatten() {
            if is_direct_child(clean, &node.path) && count < entries.len() {
                entries[count] = DirEntry::new(basename(&node.path), node.file_type);
                count += 1;
            }
        }
        Ok(count)
    }

    fn create(&mut self, path: &str) -> Result<()> {
        self.create_node(path, FileType::File)
    }

    fn mkdir(&mut self, path: &str) -> Result<()> {
        self.create_node(path, FileType::Directory)
    }

    fn remove(&mut self, path: &str) -> Result<()> {
        self.ensure_writable()?;
        let clean = Self::clean(path);
        let index = self.node_index(clean).ok_or(VfsError::NotFound)?;
        if self.nodes[index].as_ref().unwrap().file_type != FileType::File {
            return Err(VfsError::IsADirectory);
        }
        self.free_extents(index);
        self.nodes[index] = None;
        self.handles.retain(|entry| entry.1.node != index);
        self.dirty = true;
        self.commit()
    }

    fn rename(&mut self, old: &str, new: &str) -> Result<()> {
        self.rename_node(old, new)
    }

    fn truncate(&mut self, path: &str) -> Result<()> {
        self.ensure_writable()?;
        let clean = Self::clean(path);
        let index = self.node_index(clean).ok_or(VfsError::NotFound)?;
        if self.nodes[index].as_ref().unwrap().file_type != FileType::File {
            return Err(VfsError::IsADirectory);
        }
        self.free_extents(index);
        let node = self.nodes[index].as_mut().unwrap();
        node.size = 0;
        node.mtime_ns = clock::monotonic_ns();
        self.dirty = true;
        self.commit()
    }

    fn stat(&mut self, path: &str) -> Result<FileStat> {
        let clean = Self::clean(path);
        if clean.is_empty() {
            return Ok(FileStat {
                file_type: FileType::Directory,
                size: 0,
            });
        }
        let node = self.nodes[self.node_index(clean).ok_or(VfsError::NotFound)?]
            .as_ref()
            .unwrap();
        Ok(FileStat {
            file_type: node.file_type,
            size: node.size as usize,
        })
    }
}

/// Сериализует узел в 256-байтовую запись (включая CRC в `[252..256)`).
fn encode_node(entry: &mut [u8], node: &Node) {
    entry.fill(0);
    entry[0] = 1;
    entry[1] = match node.file_type {
        FileType::File => 1,
        FileType::Directory => 2,
        FileType::Device => 1,
    };
    put_u16(entry, 2, node.mode);
    entry[4] = node.extent_count as u8;
    put_u64(entry, 8, node.size);
    put_u64(entry, 16, node.ctime_ns);
    put_u64(entry, 24, node.mtime_ns);
    for (i, (start, len)) in node.extents.iter().enumerate() {
        put_u64(entry, 32 + i * 16, *start);
        put_u64(entry, 40 + i * 16, *len);
    }
    put_u16(entry, 128, node.path.len() as u16);
    entry[132..132 + node.path.len()].copy_from_slice(node.path.as_bytes());
    let checksum = crc32(&entry[..NODE_CRC_OFFSET]);
    put_u32(entry, NODE_CRC_OFFSET, checksum);
}

/// Разбирает 256-байтовую запись узла. CRC уже проверен вызывающей стороной.
fn decode_node(entry: &[u8], geom: &Geometry) -> core::result::Result<Node, DunitFsError> {
    let file_type = match entry[1] {
        1 => FileType::File,
        2 => FileType::Directory,
        _ => return Err(DunitFsError::CorruptMetadata),
    };
    let extent_count = entry[4] as usize;
    if extent_count > MAX_EXTENTS {
        return Err(DunitFsError::CorruptMetadata);
    }
    let size = get_u64(entry, 8);
    let mut extents = [(0u64, 0u64); MAX_EXTENTS];
    for (i, slot) in extents.iter_mut().enumerate() {
        let start = get_u64(entry, 32 + i * 16);
        let len = get_u64(entry, 40 + i * 16);
        *slot = (start, len);
        if i < extent_count {
            if len == 0
                || start < geom.data_start
                || start.checked_add(len).is_none()
                || start + len > geom.total_blocks
            {
                return Err(DunitFsError::CorruptMetadata);
            }
        } else if start != 0 || len != 0 {
            return Err(DunitFsError::CorruptMetadata);
        }
    }
    let allocated: u64 = extents[..extent_count].iter().map(|(_, l)| *l).sum();
    let path_len = get_u16(entry, 128) as usize;
    if path_len == 0 || path_len > PATH_SIZE {
        return Err(DunitFsError::CorruptMetadata);
    }
    let path = core::str::from_utf8(&entry[132..132 + path_len])
        .map_err(|_| DunitFsError::CorruptMetadata)?;
    if file_type == FileType::Directory && (size != 0 || extent_count != 0) {
        return Err(DunitFsError::CorruptMetadata);
    }
    if size > allocated.saturating_mul(BLOCK_SIZE as u64) {
        return Err(DunitFsError::CorruptMetadata);
    }
    Ok(Node {
        path: String::from(path),
        file_type,
        mode: get_u16(entry, 2),
        size,
        ctime_ns: get_u64(entry, 16),
        mtime_ns: get_u64(entry, 24),
        extents,
        extent_count,
    })
}

// --- allocation bitmap (1 бит на блок раздела, LSB-first) -----------
fn bit_get(bitmap: &[u8], index: u64) -> bool {
    let byte = (index / 8) as usize;
    byte < bitmap.len() && (bitmap[byte] >> (index % 8)) & 1 == 1
}

fn bit_set(bitmap: &mut [u8], index: u64) {
    let byte = (index / 8) as usize;
    if byte < bitmap.len() {
        bitmap[byte] |= 1 << (index % 8);
    }
}

fn bit_clear(bitmap: &mut [u8], index: u64) {
    let byte = (index / 8) as usize;
    if byte < bitmap.len() {
        bitmap[byte] &= !(1 << (index % 8));
    }
}

fn mark_used(bitmap: &mut [u8], start: u64, len: u64) {
    for block in start..start + len {
        bit_set(bitmap, block);
    }
}

/// First-fit: находит прогон свободных блоков в области данных, длиной не более
/// `max`. Возвращает `(start, len)` или `None`, если свободных блоков нет.
fn alloc_run(bitmap: &[u8], geom: &Geometry, max: u64) -> Option<(u64, u64)> {
    let mut block = geom.data_start;
    while block < geom.total_blocks {
        if !bit_get(bitmap, block) {
            let mut len = 1u64;
            while len < max
                && block + len < geom.total_blocks
                && !bit_get(bitmap, block + len)
            {
                len += 1;
            }
            return Some((block, len));
        }
        block += 1;
    }
    None
}

fn write_slot(
    device: BlockDeviceInfo,
    start: u64,
    blocks: u64,
    geom: &Geometry,
    slot: u32,
    data: &[u8],
) -> core::result::Result<(), DunitFsError> {
    let base = geom.slot_start(slot);
    let mut block = [0u8; BLOCK_SIZE];
    for i in 0..geom.slot_blocks {
        let offset = (i * BLOCK_SIZE as u64) as usize;
        block.copy_from_slice(&data[offset..offset + BLOCK_SIZE]);
        write_partition_block(device, start, blocks, base + i, &block)?;
    }
    Ok(())
}

fn read_slot(
    device: BlockDeviceInfo,
    start: u64,
    blocks: u64,
    geom: &Geometry,
    slot: u32,
) -> core::result::Result<Vec<u8>, DunitFsError> {
    let base = geom.slot_start(slot);
    let mut data = vec![0u8; geom.slot_bytes()];
    let mut block = [0u8; BLOCK_SIZE];
    for i in 0..geom.slot_blocks {
        read_partition_block(device, start, blocks, base + i, &mut block)?;
        let offset = (i * BLOCK_SIZE as u64) as usize;
        data[offset..offset + BLOCK_SIZE].copy_from_slice(&block);
    }
    Ok(data)
}

fn validate_geometry(
    device: BlockDeviceInfo,
    partition_start: u64,
    partition_blocks: u64,
    geom: &Geometry,
) -> core::result::Result<(), DunitFsError> {
    if device.block_size != BLOCK_SIZE {
        return Err(DunitFsError::InvalidBlockSize);
    }
    if partition_blocks <= geom.data_start
        || partition_start
            .checked_add(partition_blocks)
            .map(|end| end > device.blocks)
            .unwrap_or(true)
    {
        return Err(DunitFsError::PartitionTooSmall);
    }
    Ok(())
}

fn read_partition_block(
    device: BlockDeviceInfo,
    start: u64,
    blocks: u64,
    relative: u64,
    data: &mut [u8; BLOCK_SIZE],
) -> core::result::Result<(), DunitFsError> {
    if relative >= blocks {
        return Err(DunitFsError::Io);
    }
    let read =
        block::read_block(device.name, start + relative, data).map_err(|_| DunitFsError::Io)?;
    if read != BLOCK_SIZE {
        return Err(DunitFsError::Io);
    }
    Ok(())
}

fn write_partition_block(
    device: BlockDeviceInfo,
    start: u64,
    blocks: u64,
    relative: u64,
    data: &[u8; BLOCK_SIZE],
) -> core::result::Result<(), DunitFsError> {
    if relative >= blocks {
        return Err(DunitFsError::Io);
    }
    let written =
        block::write_block(device.name, start + relative, data).map_err(|_| DunitFsError::Io)?;
    if written != BLOCK_SIZE {
        return Err(DunitFsError::Io);
    }
    Ok(())
}

fn extents_overlap(left: &Node, right: &Node) -> bool {
    for (lstart, llen) in &left.extents[..left.extent_count] {
        for (rstart, rlen) in &right.extents[..right.extent_count] {
            if lstart < &(rstart + rlen) && rstart < &(lstart + llen) {
                return true;
            }
        }
    }
    false
}

fn is_direct_child(parent: &str, child: &str) -> bool {
    if parent.is_empty() {
        return !child.is_empty() && !child.contains('/');
    }
    child
        .strip_prefix(parent)
        .and_then(|suffix| suffix.strip_prefix('/'))
        .map(|suffix| !suffix.is_empty() && !suffix.contains('/'))
        .unwrap_or(false)
}

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn get_u16(data: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(data[offset..offset + 2].try_into().unwrap())
}

fn get_u32(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap())
}

fn get_u64(data: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(data[offset..offset + 8].try_into().unwrap())
}

fn put_u16(data: &mut [u8], offset: usize, value: u16) {
    data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn put_u32(data: &mut [u8], offset: usize, value: u32) {
    data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn put_u64(data: &mut [u8], offset: usize, value: u64) {
    data[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for byte in data {
        crc ^= *byte as u32;
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & 0u32.wrapping_sub(crc & 1));
        }
    }
    !crc
}

// ====================================================================
// fsck.dunit (M5 item 5): проверка целостности + recovery report.
// Ядро — механизм (fsck сканирует суперблоки/слоты/узлы и возвращает отчёт);
// userspace-команда `fsck_dunit` — тонкий CLI поверх syscall. Вердикт
// согласован с load(): Clean iff найдено строго целостное поколение (его и
// монтирует load() на чтение/запись); Degraded iff целых поколений нет, но
// хотя бы один слот физически цел (load() поднимет read-only); Unrecoverable
// iff ни один слот не проходит slot-CRC (load() вернёт ошибку).
// ====================================================================

/// Находит первый Dunit-овский раздел среди известных блочных устройств.
/// Общий механизм для boot-smoke self-test и syscall `fsck`.
pub fn locate_dunit_partition() -> Option<(BlockDeviceInfo, u64, u64)> {
    let mut devices = [None; 8];
    let count = block::snapshot(&mut devices);
    for device in devices[..count].iter().flatten().copied() {
        let Ok(table) = crate::storage::gpt::read(device) else {
            continue;
        };
        for partition in table.partitions.iter().flatten() {
            if crate::storage::policy::classify(&partition.type_guid).is_dunitfs() {
                return Some((device, partition.first_lba, partition.blocks()));
            }
        }
    }
    None
}

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum FsckVerdict {
    /// Найдено строго целостное поколение — ФС монтируется на чтение/запись.
    Clean,
    /// Целых поколений нет, но слот физически цел — read-only degraded mount.
    Degraded,
    /// Ни один слот не проходит slot-CRC — ФС смонтировать нельзя.
    #[default]
    Unrecoverable,
}

impl FsckVerdict {
    pub fn as_str(self) -> &'static str {
        match self {
            FsckVerdict::Clean => "clean",
            FsckVerdict::Degraded => "degraded",
            FsckVerdict::Unrecoverable => "unrecoverable",
        }
    }

    pub fn code(self) -> u32 {
        match self {
            FsckVerdict::Clean => 0,
            FsckVerdict::Degraded => 1,
            FsckVerdict::Unrecoverable => 2,
        }
    }
}

/// Recovery report: что увидел fsck на диске. Поля `nodes_*`/`*_errors`
/// относятся к тому поколению, которое реально смонтирует `load()` (celoe при
/// Clean, старшее физически целое при Degraded).
#[derive(Clone, Copy, Default)]
pub struct FsckReport {
    pub primary_ok: bool,
    pub backup_ok: bool,
    pub slot_crc_ok: bool,
    pub generation: u64,
    pub nodes_total: u32,
    pub nodes_ok: u32,
    pub nodes_corrupt: u32,
    pub duplicate_paths: u32,
    pub extent_errors: u32,
    pub bitmap_errors: u32,
    pub verdict: FsckVerdict,
}

impl FsckReport {
    /// Одна строка в serial (единый write — иначе write_stdio порвёт её).
    pub fn emit(&self) {
        crate::serial_write(&format!(
            "[FSCK] verdict={} gen={} nodes={}/{} corrupt={} dup={} extent_err={} bitmap_err={} primary={} backup={} slot_crc={}\r\n",
            self.verdict.as_str(),
            self.nodes_ok,
            self.nodes_total,
            self.nodes_total,
            self.nodes_corrupt,
            self.duplicate_paths,
            self.extent_errors,
            self.bitmap_errors,
            self.primary_ok as u8,
            self.backup_ok as u8,
            self.slot_crc_ok as u8,
        ));
    }
}

/// Результат узлового скана одного физически целого слота.
struct SlotScan {
    nodes_total: u32,
    nodes_ok: u32,
    nodes_corrupt: u32,
    duplicate_paths: u32,
    extent_errors: u32,
    bitmap_errors: u32,
}

impl SlotScan {
    /// Поколение «строго целостно», если все занятые узлы прошли self-CRC и
    /// декодирование, нет дублей путей и пересечений экстентов. Это ровно те
    /// условия, при которых `from_slot` поднимается без ошибки, поэтому вердикт
    /// Clean ⇔ load() смонтирует это поколение на чтение/запись. `bitmap_errors`
    /// информативен и на вердикт НЕ влияет (ни `from_slot`, ни
    /// `verify_consistency` не сверяют bitmap).
    fn is_clean(&self) -> bool {
        self.nodes_corrupt == 0 && self.extent_errors == 0 && self.duplicate_paths == 0
    }
}

/// Сканирует физически целый образ слота на узловом уровне. Повторяет проверки
/// `from_slot`/`decode_node`/`verify_consistency`, но ничего не прекращает —
/// копит статистику для отчёта. `bitmap_errors` — расхождение между занятыми
/// экстентами уцелевших узлов и allocation bitmap (информативно).
fn scan_slot(slot_image: &[u8], geom: &Geometry) -> SlotScan {
    let bitmap = &slot_image[..geom.bitmap_len()];
    let table = &slot_image[geom.bitmap_len()..];
    let mut scan = SlotScan {
        nodes_total: 0,
        nodes_ok: 0,
        nodes_corrupt: 0,
        duplicate_paths: 0,
        extent_errors: 0,
        bitmap_errors: 0,
    };
    let mut good: [Option<Node>; MAX_NODES] = core::array::from_fn(|_| None);
    for index in 0..MAX_NODES {
        let entry = &table[index * NODE_SIZE..(index + 1) * NODE_SIZE];
        if entry[0] == 0 {
            continue;
        }
        scan.nodes_total += 1;
        if get_u32(entry, NODE_CRC_OFFSET) != crc32(&entry[..NODE_CRC_OFFSET]) {
            scan.nodes_corrupt += 1;
            continue;
        }
        match decode_node(entry, geom) {
            Ok(node) => {
                scan.nodes_ok += 1;
                good[index] = Some(node);
            }
            Err(_) => scan.nodes_corrupt += 1,
        }
    }
    // Дубли путей и пересечения экстентов среди декодированных узлов.
    for index in 0..MAX_NODES {
        let Some(node) = &good[index] else {
            continue;
        };
        for other in 0..index {
            if let Some(prev) = &good[other] {
                if prev.path == node.path {
                    scan.duplicate_paths += 1;
                }
                if extents_overlap(node, prev) {
                    scan.extent_errors += 1;
                }
            }
        }
    }
    // Проверка allocation bitmap против экстентов уцелевших узлов (информативно).
    let mut expected = vec![0u8; geom.bitmap_len()];
    for node in good.iter().flatten() {
        for (start, len) in &node.extents[..node.extent_count] {
            mark_used(&mut expected, *start, *len);
        }
    }
    for block in geom.data_start..geom.total_blocks {
        if bit_get(&expected, block) && !bit_get(bitmap, block) {
            scan.bitmap_errors += 1;
        }
    }
    scan
}

/// Проверяет целостность Dunit-ФС на разделе и формирует recovery report. Чистый
/// механизм: ничего не чинит и не монтирует, только диагностирует. Выбирает то
/// же поколение, что и `load()` (строго целое старшего поколения → иначе старшее
/// физически целое), и наполняет отчёт его узловой статистикой.
pub fn fsck(device: BlockDeviceInfo, partition_start: u64, partition_blocks: u64) -> FsckReport {
    let mut report = FsckReport::default();
    let geom = Geometry::derive(partition_blocks);
    if validate_geometry(device, partition_start, partition_blocks, &geom).is_err() {
        return report; // Unrecoverable (geom/раздел не годятся)
    }

    // Декодируем оба суперблока.
    let mut block = [0u8; BLOCK_SIZE];
    let mut sbs: [Option<SuperBlock>; 2] = [None, None];
    for (slot, sb_block) in [PRIMARY_SB, BACKUP_SB].into_iter().enumerate() {
        if read_partition_block(device, partition_start, partition_blocks, sb_block, &mut block)
            .is_ok()
        {
            sbs[slot] = SuperBlock::decode(&block, &geom);
        }
    }
    report.primary_ok = sbs[0].is_some();
    report.backup_ok = sbs[1].is_some();

    // Физически целые кандидаты (slot-CRC сходится), старшее поколение первым.
    let mut valid: Vec<(SuperBlock, Vec<u8>)> = Vec::new();
    for sb in sbs.into_iter().flatten() {
        let Ok(slot_image) =
            read_slot(device, partition_start, partition_blocks, &geom, sb.active_slot)
        else {
            continue;
        };
        if crc32(&slot_image) == sb.slot_crc {
            valid.push((sb, slot_image));
        }
    }
    valid.sort_by(|a, b| b.0.generation.cmp(&a.0.generation));

    if valid.is_empty() {
        report.slot_crc_ok = false;
        report.verdict = FsckVerdict::Unrecoverable;
        return report;
    }
    report.slot_crc_ok = true;

    // Первое строго целостное поколение (как в load(): строгий from_slot).
    if let Some((sb, slot_image)) = valid.iter().find(|(_, img)| scan_slot(img, &geom).is_clean()) {
        let scan = scan_slot(slot_image, &geom);
        report.generation = sb.generation;
        report.nodes_total = scan.nodes_total;
        report.nodes_ok = scan.nodes_ok;
        report.nodes_corrupt = scan.nodes_corrupt;
        report.duplicate_paths = scan.duplicate_paths;
        report.extent_errors = scan.extent_errors;
        report.bitmap_errors = scan.bitmap_errors;
        report.verdict = FsckVerdict::Clean;
        return report;
    }

    // Строго целых нет → degraded: статистика старшего физически целого слота.
    let (sb, slot_image) = valid.first().unwrap();
    let scan = scan_slot(slot_image, &geom);
    report.generation = sb.generation;
    report.nodes_total = scan.nodes_total;
    report.nodes_ok = scan.nodes_ok;
    report.nodes_corrupt = scan.nodes_corrupt;
    report.duplicate_paths = scan.duplicate_paths;
    report.extent_errors = scan.extent_errors;
    report.bitmap_errors = scan.bitmap_errors;
    report.verdict = FsckVerdict::Degraded;
    report
}

// ====================================================================
// Boot smoke self-test (feature `boot-smoke-tests`): проверяет оба
// acceptance-критерия item 4 — (1) create/write/fsync/reboot/read и
// (2) восстановление после прерванного обновления метаданных. Запускается
// из drivers::init ПЕРЕД auto_mount на отдельном экземпляре ФС, который
// полностью отбрасывается до монтирования /persist (нет двух писателей).
// Деструктивный тест и переформатирование выполняются только на первой
// загрузке (sentinel отсутствует), чтобы persist переживал перезагрузку
// при повторном использовании того же диска.
// ====================================================================
#[cfg(feature = "boot-smoke-tests")]
const SMOKE_PAYLOAD: &str = "DUNIT-PERSIST-V2";
#[cfg(feature = "boot-smoke-tests")]
const SMOKE_SENTINEL: &str = "smoke_persist";

#[cfg(feature = "boot-smoke-tests")]
fn smoke_write_flags() -> OpenFlags {
    OpenFlags::from_bits(
        OpenFlags::READ.bits()
            | OpenFlags::WRITE.bits()
            | OpenFlags::CREATE.bits()
            | OpenFlags::TRUNC.bits(),
    )
}

#[cfg(feature = "boot-smoke-tests")]
fn smoke_append_flags() -> OpenFlags {
    OpenFlags::from_bits(OpenFlags::WRITE.bits() | OpenFlags::APPEND.bits())
}

#[cfg(feature = "boot-smoke-tests")]
fn write_whole(fs: &mut DunitFs, path: &str, data: &[u8]) -> Result<()> {
    let handle = fs.open(path, smoke_write_flags())?;
    fs.write(handle, data)?;
    fs.close(handle)?; // close → commit
    Ok(())
}

#[cfg(feature = "boot-smoke-tests")]
fn read_whole(fs: &mut DunitFs, path: &str) -> Result<String> {
    let handle = fs.open(path, OpenFlags::READ)?;
    let mut out: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 128];
    loop {
        let read = fs.read(handle, &mut chunk)?;
        if read == 0 {
            break;
        }
        out.extend_from_slice(&chunk[..read]);
    }
    fs.close(handle)?;
    String::from_utf8(out).map_err(|_| VfsError::IoError)
}

#[cfg(feature = "boot-smoke-tests")]
impl DunitFs {
    /// Симулирует прерванный коммит метаданных: сериализует отложенный образ и
    /// его CRC как настоящий `commit`, пишет его в НЕактивный слот, но затем
    /// портит ПОСЛЕДНИЙ блок слота (как при потере питания на последнем секторе
    /// записи слота). После этого публикует валидный primary-суперблок (gen+1),
    /// объявляющий CRC ЦЕЛОГО образа; backup остаётся на прошлом поколении.
    /// Экземпляр затем отбрасывается без `commit` (у `DunitFs` нет `Drop`),
    /// поэтому на диске: разорванный primary + целый backup. Загрузчик обязан
    /// отвергнуть primary по несовпадению CRC и восстановить прошлое поколение
    /// из backup. Порча последнего блока гарантирует несовпадение CRC
    /// независимо от геометрии и прежнего содержимого слота.
    fn commit_torn_for_test(&mut self) -> Result<()> {
        let slot_image = self.serialize_slot();
        let slot_crc = crc32(&slot_image);
        let inactive = 1 - self.active_slot;
        write_slot(
            self.device,
            self.partition_start,
            self.geom.total_blocks,
            &self.geom,
            inactive,
            &slot_image,
        )
        .map_err(|_| VfsError::IoError)?;

        // Прерываем запись на последнем секторе слота: инвертируем его байты,
        // так что содержимое слота на диске заведомо отличается от CRC, который
        // объявит суперблок.
        let last = self.geom.slot_blocks - 1;
        let offset = (last * BLOCK_SIZE as u64) as usize;
        let mut torn = [0u8; BLOCK_SIZE];
        for (dst, src) in torn.iter_mut().zip(&slot_image[offset..offset + BLOCK_SIZE]) {
            *dst = !*src;
        }
        self.write_raw(self.geom.slot_start(inactive) + last, &torn)?;

        let sb = SuperBlock {
            generation: self.generation + 1,
            active_slot: inactive,
            slot_crc,
            geom: self.geom,
        };
        let mut block = [0u8; BLOCK_SIZE];
        sb.encode(&mut block);
        self.write_raw(PRIMARY_SB, &block)?;
        Ok(())
    }
}
#[cfg(feature = "boot-smoke-tests")]
fn run_recovery_test(device: BlockDeviceInfo, start: u64, blocks: u64) {
    if format(device, start, blocks).is_err() {
        crate::serial_write("[DUNITFS-V2] recovery format FAIL\r\n");
        return;
    }
    crate::serial_write("[DUNITFS-V2] format ok\r\n");

    let gen_before;
    {
        let Ok(mut fs) = DunitFs::load(device, start, blocks) else {
            crate::serial_write("[DUNITFS-V2] recovery load FAIL\r\n");
            return;
        };
        if write_whole(&mut fs, "keep", b"durable-A").is_err() {
            crate::serial_write("[DUNITFS-V2] recovery seed FAIL\r\n");
            return;
        }
        gen_before = fs.generation;
        // Отложенное изменение метаданных: дописываем в "keep" данные, выделяя
        // новый блок (size/extents/bitmap/mtime становятся dirty), и рвём коммит,
        // НЕ закрывая файл (close иначе зафиксировал бы метаданные).
        let staged = fs
            .open("keep", smoke_append_flags())
            .and_then(|handle| fs.write(handle, &[0x42u8; 600]).map(|_| ()))
            .and_then(|_| fs.commit_torn_for_test());
        if staged.is_err() {
            crate::serial_write("[DUNITFS-V2] recovery torn-stage FAIL\r\n");
            return;
        }
        // fs отбрасывается без commit → разорванный primary + целый backup.
    }

    match DunitFs::load(device, start, blocks) {
        Ok(mut fs) => {
            let recovered = fs.generation;
            let content = read_whole(&mut fs, "keep").unwrap_or_default();
            if recovered == gen_before && content == "durable-A" {
                crate::serial_write(&format!(
                    "[DUNITFS-V2] torn-commit recovered gen={} content={}\r\n",
                    recovered, content
                ));
            } else {
                crate::serial_write(&format!(
                    "[DUNITFS-V2] torn-commit recovery FAIL gen={} expected={} content={}\r\n",
                    recovered, gen_before, content
                ));
            }
        }
        Err(_) => crate::serial_write("[DUNITFS-V2] torn-commit recovery load FAIL\r\n"),
    }
}
/// Портит запись узла на диске так, чтобы воспроизвести DEGRADED, а не
/// Unrecoverable: ломает self-CRC целевого узла, но ПЕРЕСЧИТЫВАЕТ slot_crc под
/// порчу и переиздаёт оба суперблока (то же поколение/слот). Итог: slot-CRC
/// проходит (слот физически цел) ⇒ строгий `from_slot` падает на self-CRC узла
/// ⇒ `load()` поднимает read-only degraded mount, отбросив только битый узел.
#[cfg(feature = "boot-smoke-tests")]
fn corrupt_node_on_disk(
    device: BlockDeviceInfo,
    start: u64,
    blocks: u64,
    node_index: usize,
) -> Result<()> {
    let geom = Geometry::derive(blocks);
    let mut block = [0u8; BLOCK_SIZE];
    read_partition_block(device, start, blocks, PRIMARY_SB, &mut block)
        .map_err(|_| VfsError::IoError)?;
    let sb = SuperBlock::decode(&block, &geom).ok_or(VfsError::IoError)?;
    let mut slot_image =
        read_slot(device, start, blocks, &geom, sb.active_slot).map_err(|_| VfsError::IoError)?;

    // Байт внутри записи узла (в crc-покрытой области [..252]) → ломаем self-CRC.
    let byte = geom.bitmap_len() + node_index * NODE_SIZE + 140;
    slot_image[byte] ^= 0xff;
    write_slot(device, start, blocks, &geom, sb.active_slot, &slot_image)
        .map_err(|_| VfsError::IoError)?;

    let new_sb = SuperBlock {
        generation: sb.generation,
        active_slot: sb.active_slot,
        slot_crc: crc32(&slot_image),
        geom,
    };
    let mut sb_block = [0u8; BLOCK_SIZE];
    new_sb.encode(&mut sb_block);
    write_partition_block(device, start, blocks, PRIMARY_SB, &sb_block)
        .map_err(|_| VfsError::IoError)?;
    write_partition_block(device, start, blocks, BACKUP_SB, &sb_block)
        .map_err(|_| VfsError::IoError)?;
    Ok(())
}
/// Деструктивный тест item 5: формат → два файла → fsck ждёт Clean → порча узла
/// "bad" → fsck ждёт Degraded → load() даёт read-only degraded mount, уцелевший
/// "good" читается, битый "bad" отброшен, запись отклонена. Маркеры [FSCK-TEST].
#[cfg(feature = "boot-smoke-tests")]
fn run_fsck_test(device: BlockDeviceInfo, start: u64, blocks: u64) {
    if format(device, start, blocks).is_err() {
        crate::serial_write("[FSCK-TEST] format FAIL\r\n");
        return;
    }
    {
        let Ok(mut fs) = DunitFs::load(device, start, blocks) else {
            crate::serial_write("[FSCK-TEST] seed load FAIL\r\n");
            return;
        };
        if write_whole(&mut fs, "good", b"hello").is_err()
            || write_whole(&mut fs, "bad", b"corruptme").is_err()
        {
            crate::serial_write("[FSCK-TEST] seed FAIL\r\n");
            return;
        }
    }

    let bad_index = {
        let Ok(fs) = DunitFs::load(device, start, blocks) else {
            crate::serial_write("[FSCK-TEST] index load FAIL\r\n");
            return;
        };
        match fs.node_index("bad") {
            Some(i) => i,
            None => {
                crate::serial_write("[FSCK-TEST] seed FAIL\r\n");
                return;
            }
        }
    };

    // (1) до порчи — Clean.
    let clean = fsck(device, start, blocks);
    clean.emit();
    if clean.verdict == FsckVerdict::Clean {
        crate::serial_write("[FSCK-TEST] clean verdict ok\r\n");
    } else {
        crate::serial_write("[FSCK-TEST] clean verdict FAIL\r\n");
        return;
    }

    if corrupt_node_on_disk(device, start, blocks, bad_index).is_err() {
        crate::serial_write("[FSCK-TEST] corrupt FAIL\r\n");
        return;
    }

    // (2) после порчи — Degraded, ≥1 битый узел.
    let degraded = fsck(device, start, blocks);
    degraded.emit();
    if degraded.verdict == FsckVerdict::Degraded && degraded.nodes_corrupt >= 1 {
        crate::serial_write("[FSCK-TEST] degraded verdict ok\r\n");
    } else {
        crate::serial_write("[FSCK-TEST] degraded verdict FAIL\r\n");
        return;
    }

    // (3) load() на битой ФС → read-only degraded mount.
    let Ok(mut fs) = DunitFs::load(device, start, blocks) else {
        crate::serial_write("[FSCK-TEST] degraded mount load FAIL\r\n");
        return;
    };
    if fs.read_only {
        crate::serial_write("[FSCK-TEST] degraded mount read-only ok\r\n");
    } else {
        crate::serial_write("[FSCK-TEST] degraded mount read-only FAIL\r\n");
        return;
    }

    let survivor = read_whole(&mut fs, "good").unwrap_or_default();
    if survivor == "hello" {
        crate::serial_write("[FSCK-TEST] survivor readable ok\r\n");
    } else {
        crate::serial_write(&format!(
            "[FSCK-TEST] survivor readable FAIL content={}\r\n",
            survivor
        ));
        return;
    }

    if fs.node_index("bad").is_none() {
        crate::serial_write("[FSCK-TEST] corrupt node dropped ok\r\n");
    } else {
        crate::serial_write("[FSCK-TEST] corrupt node dropped FAIL\r\n");
        return;
    }

    // Запись на read-only монтаже отклоняется (open успешен — мутатор нет).
    let write_attempt = match fs.open("good", smoke_append_flags()) {
        Ok(handle) => fs.write(handle, b"x").map(|_| ()),
        Err(e) => Err(e),
    };
    if matches!(write_attempt, Err(VfsError::PermissionDenied)) {
        crate::serial_write("[FSCK-TEST] write rejected ok\r\n");
    } else {
        crate::serial_write("[FSCK-TEST] write rejected FAIL\r\n");
    }
}
/// Entry point вызывается из `drivers::init` ПЕРЕД `auto_mount` (единственный
/// писатель). На первой загрузке (sentinel отсутствует) гоняет деструктивный
/// recovery-тест и провиженит sentinel; на повторной — только читает sentinel,
/// подтверждая персистентность через перезагрузку, и ничего не разрушает.
#[cfg(feature = "boot-smoke-tests")]
pub fn smoke_self_test() {
    let Some((device, start, blocks)) = locate_dunit_partition() else {
        crate::serial_write("[DUNITFS-V2] smoke skipped: no Dunit partition\r\n");
        return;
    };

    let second_boot = DunitFs::load(device, start, blocks)
        .map(|fs| fs.node_index(SMOKE_SENTINEL).is_some())
        .unwrap_or(false);

    if second_boot {
        match DunitFs::load(device, start, blocks) {
            Ok(mut fs) => {
                let content = read_whole(&mut fs, SMOKE_SENTINEL).unwrap_or_default();
                if content == SMOKE_PAYLOAD {
                    crate::serial_write(&format!(
                        "[DUNITFS-V2] persist reboot verified content={}\r\n",
                        content
                    ));
                } else {
                    crate::serial_write(&format!(
                        "[DUNITFS-V2] persist reboot MISMATCH content={}\r\n",
                        content
                    ));
                }
            }
            Err(_) => crate::serial_write("[DUNITFS-V2] persist reboot load FAIL\r\n"),
        }
        return;
    }

    // Первая загрузка: деструктивный тест восстановления, затем тест fsck +
    // degraded mount, затем чистый формат и запись sentinel, который должна
    // подтвердить следующая загрузка.
    run_recovery_test(device, start, blocks);
    run_fsck_test(device, start, blocks);

    if format(device, start, blocks).is_err() {
        crate::serial_write("[DUNITFS-V2] persist reformat FAIL\r\n");
        return;
    }
    match DunitFs::load(device, start, blocks) {
        Ok(mut fs) => {
            if write_whole(&mut fs, SMOKE_SENTINEL, SMOKE_PAYLOAD.as_bytes()).is_ok() {
                crate::serial_write(&format!(
                    "[DUNITFS-V2] persist first-boot wrote content={}\r\n",
                    SMOKE_PAYLOAD
                ));
            } else {
                crate::serial_write("[DUNITFS-V2] persist first-boot write FAIL\r\n");
            }
        }
        Err(_) => crate::serial_write("[DUNITFS-V2] persist post-format load FAIL\r\n"),
    }
}
