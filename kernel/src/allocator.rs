use core::alloc::{GlobalAlloc, Layout};
use core::ptr;
use core::sync::atomic::{AtomicUsize, Ordering};

struct FreeBlock {
    size: usize,
    next: *mut FreeBlock,
}

#[repr(C)]
struct AllocationHeader {
    block_start: usize,
    block_size: usize,
}

pub struct KernelAllocator {
    heap_start: AtomicUsize,
    heap_size: AtomicUsize,
    free_list: AtomicUsize,
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
            heap_start: AtomicUsize::new(0),
            heap_size: AtomicUsize::new(0),
            free_list: AtomicUsize::new(0),
        }
    }

    pub fn init(&self, heap_start: usize, heap_size: usize) {
        self.heap_start.store(heap_start, Ordering::SeqCst);
        self.heap_size.store(heap_size, Ordering::SeqCst);

        let initial_block = heap_start as *mut FreeBlock;
        unsafe {
            (*initial_block).size = heap_size;
            (*initial_block).next = ptr::null_mut();
        }

        self.free_list
            .store(initial_block as usize, Ordering::SeqCst);
    }

    fn align_up(addr: usize, align: usize) -> usize {
        (addr + align - 1) & !(align - 1)
    }

    pub fn stats(&self) -> KernelHeapStats {
        let total = self.heap_size.load(Ordering::SeqCst);
        let mut free = 0usize;
        let mut blocks = 0usize;
        let mut current_ptr = self.free_list.load(Ordering::SeqCst) as *const FreeBlock;
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

unsafe impl GlobalAlloc for KernelAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let payload_size = layout.size().max(1);
        let user_align = layout
            .align()
            .max(core::mem::align_of::<AllocationHeader>());
        let header_size = core::mem::size_of::<AllocationHeader>();
        let minimum_free = core::mem::size_of::<FreeBlock>();

        let mut current_ptr = self.free_list.load(Ordering::SeqCst) as *mut FreeBlock;
        let mut prev_ptr: *mut FreeBlock = ptr::null_mut();

        while !current_ptr.is_null() {
            let current = &mut *current_ptr;

            let block_start = current_ptr as usize;
            let block_end = match block_start.checked_add(current.size) {
                Some(end) => end,
                None => return ptr::null_mut(),
            };
            let user_addr = Self::align_up(block_start + header_size, user_align);
            let requested_end = match user_addr.checked_add(payload_size) {
                Some(end) => Self::align_up(end, core::mem::align_of::<FreeBlock>()),
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
                    // A tail too small to hold a FreeBlock belongs to this
                    // allocation and is recovered through AllocationHeader.
                    (block_end, current.next)
                };

                if prev_ptr.is_null() {
                    self.free_list.store(replacement as usize, Ordering::SeqCst);
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

    unsafe fn dealloc(&self, ptr: *mut u8, _layout: Layout) {
        if ptr.is_null() {
            return;
        }
        let header =
            (ptr as usize - core::mem::size_of::<AllocationHeader>()) as *const AllocationHeader;
        let block_start = (*header).block_start;
        let size = (*header).block_size;
        let block = block_start as *mut FreeBlock;
        (*block).size = size;

        let mut current_ptr = self.free_list.load(Ordering::SeqCst) as *mut FreeBlock;
        let mut prev_ptr: *mut FreeBlock = ptr::null_mut();

        while !current_ptr.is_null() && (current_ptr as usize) < block_start {
            prev_ptr = current_ptr;
            current_ptr = (*current_ptr).next;
        }

        if !prev_ptr.is_null() {
            let prev_end = (prev_ptr as usize) + (*prev_ptr).size;
            if prev_end == block_start {
                (*prev_ptr).size += size;

                if !current_ptr.is_null() {
                    let block_end = (prev_ptr as usize) + (*prev_ptr).size;
                    if block_end == current_ptr as usize {
                        (*prev_ptr).size += (*current_ptr).size;
                        (*prev_ptr).next = (*current_ptr).next;
                    }
                }

                return;
            }
        }

        (*block).next = current_ptr;

        if !current_ptr.is_null() {
            let block_end = block_start + size;
            if block_end == current_ptr as usize {
                (*block).size += (*current_ptr).size;
                (*block).next = (*current_ptr).next;
            }
        }

        if prev_ptr.is_null() {
            self.free_list.store(block as usize, Ordering::SeqCst);
        } else {
            (*prev_ptr).next = block;
        }
    }
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

    crate::memory::serial_write("[HEAP] OK\r\n");
}

#[cfg(not(test))]
pub fn heap_stats() -> KernelHeapStats {
    ALLOCATOR.stats()
}

#[cfg(test)]
pub fn heap_stats() -> KernelHeapStats {
    KernelHeapStats::default()
}
