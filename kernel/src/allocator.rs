use core::alloc::{GlobalAlloc, Layout};
use core::ptr;
use core::sync::atomic::{AtomicUsize, Ordering};

struct FreeBlock {
    size: usize,
    next: *mut FreeBlock,
}

pub struct KernelAllocator {
    heap_start: AtomicUsize,
    heap_size: AtomicUsize,
    free_list: AtomicUsize,
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
        
        self.free_list.store(initial_block as usize, Ordering::SeqCst);
    }
    
    fn align_up(addr: usize, align: usize) -> usize {
        (addr + align - 1) & !(align - 1)
    }
}

unsafe impl GlobalAlloc for KernelAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let size = layout.size().max(core::mem::size_of::<FreeBlock>());
        let size = Self::align_up(size, layout.align());
        
        let mut current_ptr = self.free_list.load(Ordering::SeqCst) as *mut FreeBlock;
        let mut prev_ptr: *mut FreeBlock = ptr::null_mut();
        
        while !current_ptr.is_null() {
            let current = &mut *current_ptr;
            
            if current.size >= size {
                let block_addr = current_ptr as usize;
                let aligned_addr = Self::align_up(block_addr, layout.align());
                let padding = aligned_addr - block_addr;
                
                if current.size >= size + padding {
                    if current.size >= size + padding + core::mem::size_of::<FreeBlock>() {
                        let remaining_size = current.size - size - padding;
                        let new_block = (aligned_addr + size) as *mut FreeBlock;
                        (*new_block).size = remaining_size;
                        (*new_block).next = current.next;
                        
                        if prev_ptr.is_null() {
                            self.free_list.store(new_block as usize, Ordering::SeqCst);
                        } else {
                            (*prev_ptr).next = new_block;
                        }
                    } else {
                        if prev_ptr.is_null() {
                            self.free_list.store(current.next as usize, Ordering::SeqCst);
                        } else {
                            (*prev_ptr).next = current.next;
                        }
                    }
                    
                    return aligned_addr as *mut u8;
                }
            }
            
            prev_ptr = current_ptr;
            current_ptr = current.next;
        }
        
        ptr::null_mut()
    }
    
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let size = layout.size().max(core::mem::size_of::<FreeBlock>());
        let size = Self::align_up(size, layout.align());
        
        let block = ptr as *mut FreeBlock;
        (*block).size = size;
        
        let mut current_ptr = self.free_list.load(Ordering::SeqCst) as *mut FreeBlock;
        let mut prev_ptr: *mut FreeBlock = ptr::null_mut();
        
        while !current_ptr.is_null() && (current_ptr as usize) < (ptr as usize) {
            prev_ptr = current_ptr;
            current_ptr = (*current_ptr).next;
        }
        
        if !prev_ptr.is_null() {
            let prev_end = (prev_ptr as usize) + (*prev_ptr).size;
            if prev_end == ptr as usize {
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
            let block_end = (ptr as usize) + size;
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

static mut KERNEL_HEAP: [u8; 2 * 1024 * 1024] = [0; 2 * 1024 * 1024];

pub fn init() {
    crate::memory::serial_write("[HEAP] START\r\n");

    unsafe {
        let heap_start = KERNEL_HEAP.as_ptr() as usize;
        let heap_size = core::mem::size_of_val(&KERNEL_HEAP);
        init_heap(heap_start, heap_size);
    }

    crate::memory::serial_write("[HEAP] OK\r\n");
}
