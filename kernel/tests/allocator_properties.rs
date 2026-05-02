use proptest::prelude::*;
use std::alloc::{GlobalAlloc, Layout};
use std::ptr;
use std::sync::atomic::{AtomicUsize, Ordering};

struct FreeBlock {
    size: usize,
    next: *mut FreeBlock,
}

struct KernelAllocator {
    heap_start: AtomicUsize,
    heap_size: AtomicUsize,
    free_list: AtomicUsize,
}

impl KernelAllocator {
    const fn new() -> Self {
        Self {
            heap_start: AtomicUsize::new(0),
            heap_size: AtomicUsize::new(0),
            free_list: AtomicUsize::new(0),
        }
    }
    
    fn init(&self, heap_start: usize, heap_size: usize) {
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
        let size = layout.size().max(std::mem::size_of::<FreeBlock>());
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
                    if current.size >= size + padding + std::mem::size_of::<FreeBlock>() {
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
        let size = layout.size().max(std::mem::size_of::<FreeBlock>());
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

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]
    
    #[test]
    fn prop_allocator_provides_memory(alloc_size in 8usize..4096) {
        let heap_size = 65536;
        let heap = vec![0u8; heap_size];
        let heap_start = heap.as_ptr() as usize;
        
        let allocator = KernelAllocator::new();
        allocator.init(heap_start, heap_size);
        
        let layout = Layout::from_size_align(alloc_size, 8).unwrap();
        let ptr = unsafe { allocator.alloc(layout) };
        
        if !ptr.is_null() {
            assert!(ptr as usize >= heap_start);
            assert!((ptr as usize) < heap_start + heap_size);
            assert_eq!((ptr as usize) % 8, 0);
            
            unsafe {
                ptr.write_bytes(0xAA, alloc_size);
                allocator.dealloc(ptr, layout);
            }
        }
        
        drop(heap);
    }
    
    #[test]
    fn prop_alloc_dealloc_roundtrip(num_allocs in 1usize..50) {
        let heap_size = 65536;
        let heap = vec![0u8; heap_size];
        let heap_start = heap.as_ptr() as usize;
        
        let allocator = KernelAllocator::new();
        allocator.init(heap_start, heap_size);
        
        let mut allocations = Vec::new();
        
        for i in 0..num_allocs {
            let size = 64 + (i * 16);
            let layout = Layout::from_size_align(size, 8).unwrap();
            let ptr = unsafe { allocator.alloc(layout) };
            
            if !ptr.is_null() {
                allocations.push((ptr, layout));
            } else {
                break;
            }
        }
        
        for (ptr, layout) in allocations {
            unsafe {
                allocator.dealloc(ptr, layout);
            }
        }
        
        let layout = Layout::from_size_align(1024, 8).unwrap();
        let ptr = unsafe { allocator.alloc(layout) };
        assert!(!ptr.is_null());
        
        unsafe {
            allocator.dealloc(ptr, layout);
        }
        
        drop(heap);
    }
}
