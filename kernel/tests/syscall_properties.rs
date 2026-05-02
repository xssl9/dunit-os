use proptest::prelude::*;

const EFAULT: i64 = -14;
const EINVAL: i64 = -22;
const EBADF: i64 = -9;
const ENOSYS: i64 = -38;

const USER_SPACE_START: u64 = 0x0000_0000_0000_0000;
const USER_SPACE_END: u64 = 0x0000_7FFF_FFFF_FFFF;
const MAX_FD: u32 = 1024;

fn is_valid_user_pointer(ptr: u64, size: usize) -> bool {
    if ptr == 0 {
        return false;
    }
    
    let end = ptr.saturating_add(size as u64);
    
    if end < ptr {
        return false;
    }
    
    ptr >= USER_SPACE_START && end <= USER_SPACE_END
}

fn is_valid_fd(fd: u32) -> bool {
    fd < MAX_FD
}

fn validate_read_syscall(fd: u32, buf: u64, count: usize) -> i64 {
    if !is_valid_fd(fd) {
        return EBADF;
    }
    
    if !is_valid_user_pointer(buf, count) {
        return EFAULT;
    }
    
    ENOSYS
}

fn validate_write_syscall(fd: u32, buf: u64, count: usize) -> i64 {
    if !is_valid_fd(fd) {
        return EBADF;
    }
    
    if !is_valid_user_pointer(buf, count) {
        return EFAULT;
    }
    
    ENOSYS
}

fn validate_close_syscall(fd: u32) -> i64 {
    if !is_valid_fd(fd) {
        return EBADF;
    }
    
    ENOSYS
}

fn validate_mmap_syscall(addr: usize, length: usize) -> i64 {
    if length == 0 {
        return EINVAL;
    }
    
    if addr != 0 && !is_valid_user_pointer(addr as u64, length) {
        return EINVAL;
    }
    
    ENOSYS
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]
    
    #[test]
    fn prop_null_pointer_returns_efault(count in 1usize..4096) {
        let result = validate_read_syscall(0, 0, count);
        assert_eq!(result, EFAULT);
    }
    
    #[test]
    fn prop_invalid_fd_returns_ebadf(fd in MAX_FD..u32::MAX, buf in 0x1000u64..USER_SPACE_END, count in 1usize..4096) {
        let result = validate_read_syscall(fd, buf, count);
        assert_eq!(result, EBADF);
    }
    
    #[test]
    fn prop_out_of_bounds_pointer_returns_efault(
        fd in 0u32..MAX_FD,
        buf in (USER_SPACE_END + 1)..u64::MAX,
        count in 1usize..4096
    ) {
        let result = validate_read_syscall(fd, buf, count);
        assert_eq!(result, EFAULT);
    }
    
    #[test]
    fn prop_valid_params_pass_validation(
        fd in 0u32..MAX_FD,
        buf in 0x1000u64..(USER_SPACE_END - 0x10000),
        count in 1usize..4096
    ) {
        let result = validate_read_syscall(fd, buf, count);
        assert_eq!(result, ENOSYS);
    }
    
    #[test]
    fn prop_write_null_pointer_returns_efault(count in 1usize..4096) {
        let result = validate_write_syscall(0, 0, count);
        assert_eq!(result, EFAULT);
    }
    
    #[test]
    fn prop_write_invalid_fd_returns_ebadf(
        fd in MAX_FD..u32::MAX,
        buf in 0x1000u64..USER_SPACE_END,
        count in 1usize..4096
    ) {
        let result = validate_write_syscall(fd, buf, count);
        assert_eq!(result, EBADF);
    }
    
    #[test]
    fn prop_close_invalid_fd_returns_ebadf(fd in MAX_FD..u32::MAX) {
        let result = validate_close_syscall(fd);
        assert_eq!(result, EBADF);
    }
    
    #[test]
    fn prop_close_valid_fd_passes(fd in 0u32..MAX_FD) {
        let result = validate_close_syscall(fd);
        assert_eq!(result, ENOSYS);
    }
    
    #[test]
    fn prop_mmap_zero_length_returns_einval(addr in 0usize..0x1000000) {
        let result = validate_mmap_syscall(addr, 0);
        assert_eq!(result, EINVAL);
    }
    
    #[test]
    fn prop_mmap_invalid_addr_returns_einval(
        addr in (USER_SPACE_END as usize + 1)..usize::MAX,
        length in 1usize..0x100000
    ) {
        let result = validate_mmap_syscall(addr, length);
        assert_eq!(result, EINVAL);
    }
    
    #[test]
    fn prop_mmap_valid_params_pass(
        addr in 0x1000usize..(USER_SPACE_END as usize - 0x100000),
        length in 1usize..0x10000
    ) {
        let result = validate_mmap_syscall(addr, length);
        assert_eq!(result, ENOSYS);
    }
}
