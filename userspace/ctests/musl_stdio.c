/* Static musl stdio test for Dunit OS. Exercises the buffered stdio write path
 * (printf -> vfprintf -> __stdout_write -> __stdio_write), which the Dunit fs:
 * adapter routes to the native Write syscall since the kernel has no writev.
 * Proves formatted output and an explicit fflush work through the ported libc.
 */
#include <stdio.h>

int main(void)
{
	printf("[MUSL-STDIO] printf %d %s 0x%x\n", 42, "ok", 255u);
	fflush(stdout);
	printf("[MUSL-STDIO] OK\n");
	return 0;
}
