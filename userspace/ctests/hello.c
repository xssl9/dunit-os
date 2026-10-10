/* Dunit Userspace ABI v0 conformance test — freestanding C, no libc.
 *
 * The roadmap's ABI v0 "done" criterion (M6.3): an independent C program with no
 * musl receives its arguments/environment, makes raw syscalls, and exits
 * correctly. It links against the generated ABI headers (abi/include/dunit) and
 * our crt0, proving the C side of the ABI works end to end before any libc port.
 */
#include <dunit/syscall.h>
#include <dunit/errno.h>

typedef unsigned long usize;

/* Raw 3-argument Dunit syscall: rax=number, rdi/rsi/rdx=args, rax=result. */
static long dsys3(long num, long a0, long a1, long a2) {
    long ret;
    __asm__ volatile("syscall"
                     : "=a"(ret)
                     : "a"(num), "D"(a0), "S"(a1), "d"(a2)
                     : "rcx", "r11", "memory");
    return ret;
}

static long dwrite(int fd, const char *buf, usize len) {
    return dsys3(SYS_WRITE, fd, (long)buf, (long)len);
}

static usize dstrlen(const char *s) {
    usize n = 0;
    while (s[n]) n++;
    return n;
}

static void puts1(const char *s) { dwrite(1, s, dstrlen(s)); }

int main(int argc, char **argv, char **envp) {
    puts1("[C-ABI-TEST] start\n");

    if (argc < 1 || argv == 0 || argv[0] == 0) {
        puts1("[C-ABI-TEST] FAIL: argc/argv\n");
        return 1;
    }
    /* argv[argc] must be the NULL terminator the ABI promises. */
    if (argv[argc] != 0) {
        puts1("[C-ABI-TEST] FAIL: argv not NULL-terminated\n");
        return 1;
    }
    /* envp must be a valid (possibly empty) NULL-terminated vector. */
    if (envp == 0) {
        puts1("[C-ABI-TEST] FAIL: envp missing\n");
        return 1;
    }

    char line[64];
    const char *name = argv[0];
    usize nlen = dstrlen(name);
    const char *pfx = "[C-ABI-TEST] argc=";
    usize i = 0, k = 0;
    while (pfx[k]) line[i++] = pfx[k++];
    line[i++] = (char)('0' + (argc % 10));
    const char *mid = " argv0=";
    k = 0;
    while (mid[k]) line[i++] = mid[k++];
    for (usize j = 0; j < nlen && i < sizeof(line) - 2; j++) line[i++] = name[j];
    line[i++] = '\n';
    dwrite(1, line, i);

    /* A deliberately out-of-range fd must fail with EBADF, not a fake success —
     * proves the errno contract reaches C as a negative return. */
    if (dwrite(99999, "x", 1) != -DUNIT_EBADF) {
        puts1("[C-ABI-TEST] FAIL: bad-fd errno\n");
        return 1;
    }

    puts1("[C-ABI-TEST] OK\n");
    return 0;
}
