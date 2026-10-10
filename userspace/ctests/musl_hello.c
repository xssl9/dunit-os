/* Static musl "hello" for Dunit OS — the first program linked against the Dunit
 * musl port (toolchains/dunit-musl). Unlike userspace/ctests/hello.c (which has
 * its own crt0 and makes raw syscalls with no libc), this one goes through
 * musl's real crt1 + __libc_start_main + TLS setup and calls a libc function.
 * It proves the ported libc starts and runs on the Green Tea Kernel over the
 * Dunit ABI. Deliberately uses only write()/strlen() (no stdio) so the first
 * milestone needs only the native Write/Exit/SetThreadPointer path.
 */
#include <unistd.h>
#include <string.h>

static void put(const char *s)
{
	write(1, s, strlen(s));
}

int main(int argc, char **argv)
{
	put("[MUSL-HELLO] start\n");

	/* argv[0] proves the SysV initial stack reached musl's __libc_start_main
	 * (which derives argc/argv/envp from the stack, exactly like crt0.s). */
	if (argc >= 1 && argv && argv[0]) {
		put("[MUSL-HELLO] argv0=");
		put(argv[0]);
		put("\n");
	}

	/* Reaching here means crt1 + __init_tls + __set_thread_area (native
	 * SetThreadPointer) + write() over the Dunit syscall all worked. */
	put("[MUSL-HELLO] OK\n");
	return 0;
}
