//! Куча ядра.
//!
//! Изначально куча жила в фиксированном 2 MiB массиве в BSS и не могла расти —
//! при исчерпании аллокации просто отказывали, что было потолком для растущего
//! числа процессов и ФС. Теперь маленький BSS-блок используется только как
//! bootstrap до готовности PMM/VMM, а дальше куча растёт по требованию: берёт у
//! PMM непрерывный блок физических фреймов и адресует его через HHDM (все
//! физические фреймы уже линейно отображены в верхней половине, разделяемой
//! всеми адресными пространствами), поэтому правка таблиц страниц не нужна.
//!
//! Доступ к free-list защищён [`IrqSafeSpinLock`]: это убирает гонку с любым
//! возможным аллокатором из контекста прерывания и является частью единой
//! стратегии синхронизации ядра (см. `crate::sync`). Дизайн рассчитан на
//! будущий userspace поверх libc, которому нужна честная растущая куча.

use core::alloc::{GlobalAlloc, Layout};
use core::ptr;

use crate::sync::IrqSafeSpinLock;

const PAGE_SIZE: usize = 4096;

/// Гранула роста кучи: при нехватке памяти куча просит у PMM непрерывный блок
/// как минимум такого размера (или больше, если запрошенная аллокация крупнее).
const HEAP_GROW_CHUNK: usize = 1024 * 1024;

struct FreeBlock {
    size: usize,
    next: *mut FreeBlock,
}

#[repr(C)]
struct AllocationHeader {
    block_start: usize,
    block_size: usize,
}

/// Защищённое локом состояние кучи. Головной указатель free-list хранится как
/// `usize`, чтобы структура была `Send` (сырой указатель им не является).
struct HeapInner {
    free_list: usize,
    total_bytes: usize,
}

impl HeapInner {
    const fn new() -> Self {
        Self {
            free_list: 0,
            total_bytes: 0,
        }
    }

    fn add_bootstrap(&mut self, heap_start: usize, heap_size: usize) {
        let initial_block = heap_start as *mut FreeBlock;
        unsafe {
            (*initial_block).size = heap_size;
            (*initial_block).next = ptr::null_mut();
        }
        self.free_list = initial_block as usize;
        self.total_bytes = heap_size;
    }

    /// Вставляет свободный регион в отсортированный по адресу free-list и
    /// сливает его с соседями.
    unsafe fn add_free_region(&mut self, start: usize, size: usize) {
        let block = start as *mut FreeBlock;
        (*block).size = size;

        let mut current = self.free_list as *mut FreeBlock;
        let mut previous: *mut FreeBlock = ptr::null_mut();
        while !current.is_null() && (current as usize) < start {
            previous = current;
            current = (*current).next;
        }

        (*block).next = current;
        if previous.is_null() {
            self.free_list = block as usize;
        } else {
            (*previous).next = block;
        }

        // Слить с последующим соседом.
        let next = (*block).next;
        if !next.is_null() && start + size == next as usize {
            (*block).size += (*next).size;
            (*block).next = (*next).next;
        }
        // Слить с предыдущим соседом.
        if !previous.is_null() && (previous as usize) + (*previous).size == block as usize {
            (*previous).size += (*block).size;
            (*previous).next = (*block).next;
        }
    }

    unsafe fn alloc_from_free_list(&mut self, layout: Layout) -> *mut u8 {
        let payload_size = layout.size().max(1);
        let user_align = layout
            .align()
            .max(core::mem::align_of::<AllocationHeader>());
        let header_size = core::mem::size_of::<AllocationHeader>();
        let minimum_free = core::mem::size_of::<FreeBlock>();

        let mut current_ptr = self.free_list as *mut FreeBlock;
        let mut prev_ptr: *mut FreeBlock = ptr::null_mut();

        while !current_ptr.is_null() {
            let current = &mut *current_ptr;

            let block_start = current_ptr as usize;
            let block_end = match block_start.checked_add(current.size) {
                Some(end) => end,
                None => return ptr::null_mut(),
            };
            let user_addr = align_up(block_start + header_size, user_align);
            let requested_end = match user_addr.checked_add(payload_size) {
                Some(end) => align_up(end, core::mem::align_of::<FreeBlock>()),
                None => return ptr::null_mut(),
            };

            if requested_end <= block_end {
                let remaining_size = block_end - requested_end;
                let (allocated_end, replacement) = if remaining_size >= minimum_free {
                    let next = requested_end as *mut FreeBlock;
                    (*next).size = remaining_size;
                    (*next).next = current.next;
                    (requested_end, next)
                } else {
                    // Хвост, в который не помещается FreeBlock, принадлежит этой
                    // аллокации и восстанавливается через AllocationHeader.
                    (block_end, current.next)
                };

                if prev_ptr.is_null() {
                    self.free_list = replacement as usize;
                } else {
                    (*prev_ptr).next = replacement;
                }

                let header = (user_addr - header_size) as *mut AllocationHeader;
                (*header).block_start = block_start;
                (*header).block_size = allocated_end - block_start;
                return user_addr as *mut u8;
            }

            prev_ptr = current_ptr;
            current_ptr = current.next;
        }

        ptr::null_mut()
    }

    unsafe fn dealloc(&mut self, ptr: *mut u8) {
        let header =
            (ptr as usize - core::mem::size_of::<AllocationHeader>()) as *const AllocationHeader;
        let block_start = (*header).block_start;
        let size = (*header).block_size;
        self.add_free_region(block_start, size);
    }

    /// Расширяет кучу, запрашивая у PMM непрерывный блок фреймов и добавляя его
    /// в free-list через HHDM. Возвращает `false`, если PMM недоступен или в
    /// системе нет непрерывного блока нужного размера.
    unsafe fn grow(&mut self, min_bytes: usize) -> bool {
        let want = match align_up_checked(min_bytes.max(HEAP_GROW_CHUNK), PAGE_SIZE) {
            Some(value) => value,
            None => return false,
        };
        let frames = want / PAGE_SIZE;

        let pmm = match crate::memory::pmm::get_pmm() {
            Some(pmm) => pmm,
            None => return false,
        };
        let phys = match pmm.alloc_contiguous(frames) {
            Some(phys) => phys,
            None => return false,
        };
        let virt = crate::memory::vmm::phys_to_virt(phys.as_usize());
        if virt == 0 {
            pmm.free_frame(phys);
            return false;
        }

        self.add_free_region(virt, want);
        self.total_bytes += want;
        true
    }

    fn stats(&self) -> KernelHeapStats {
        let total = self.total_bytes;
        let mut free = 0usize;
        let mut blocks = 0usize;
        let mut current_ptr = self.free_list as *const FreeBlock;
        unsafe {
            while !current_ptr.is_null() {
                free = free.saturating_add((*current_ptr).size);
                blocks += 1;
                current_ptr = (*current_ptr).next as *const FreeBlock;
            }
        }
        KernelHeapStats {
            total_bytes: total as u64,
            free_bytes: free as u64,
            used_bytes: total.saturating_sub(free) as u64,
            free_blocks: blocks as u64,
        }
    }
}

pub struct KernelAllocator {
    inner: IrqSafeSpinLock<HeapInner>,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct KernelHeapStats {
    pub total_bytes: u64,
    pub free_bytes: u64,
    pub used_bytes: u64,
    pub free_blocks: u64,
}

impl KernelAllocator {
    pub const fn new() -> Self {
        Self {
            inner: IrqSafeSpinLock::new(HeapInner::new()),
        }
    }

    pub fn init(&self, heap_start: usize, heap_size: usize) {
        self.inner.lock().add_bootstrap(heap_start, heap_size);
    }

    pub fn stats(&self) -> KernelHeapStats {
        self.inner.lock().stats()
    }
}

unsafe impl GlobalAlloc for KernelAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let mut inner = self.inner.lock();
        let mut result = inner.alloc_from_free_list(layout);
        if result.is_null() {
            // Заголовок + возможное выравнивание + запас на разбиение блока.
            let required = layout
                .size()
                .saturating_add(layout.align())
                .saturating_add(core::mem::size_of::<AllocationHeader>())
                .saturating_add(core::mem::size_of::<FreeBlock>());
            if inner.grow(required) {
                result = inner.alloc_from_free_list(layout);
            }
        }
        result
    }

    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        if ptr.is_null() {
            return;
        }
        self.inner.lock().dealloc(ptr);
    }
}

fn align_up(addr: usize, align: usize) -> usize {
    (addr + align - 1) & !(align - 1)
}

fn align_up_checked(value: usize, align: usize) -> Option<usize> {
    value.checked_add(align - 1).map(|v| v & !(align - 1))
}

#[cfg(not(test))]
#[global_allocator]
static ALLOCATOR: KernelAllocator = KernelAllocator::new();

#[cfg(not(test))]
pub fn init_heap(heap_start: usize, heap_size: usize) {
    ALLOCATOR.init(heap_start, heap_size);
}

#[repr(align(4096))]
struct AlignedHeap([u8; 2 * 1024 * 1024]);

static mut KERNEL_HEAP: AlignedHeap = AlignedHeap([0; 2 * 1024 * 1024]);

pub fn init() {
    crate::memory::serial_write("[HEAP] START\r\n");

    unsafe {
        let heap_start = KERNEL_HEAP.0.as_ptr() as usize;
        let heap_size = core::mem::size_of_val(&KERNEL_HEAP.0);
        init_heap(heap_start, heap_size);
    }

    crate::memory::serial_write("[HEAP] OK (PMM-backed, growable)\r\n");
}

#[cfg(not(test))]
pub fn heap_stats() -> KernelHeapStats {
    ALLOCATOR.stats()
}

#[cfg(test)]
pub fn heap_stats() -> KernelHeapStats {
    KernelHeapStats::default()
}
