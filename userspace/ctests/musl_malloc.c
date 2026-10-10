/* Static musl malloc test for Dunit OS. Exercises the mallocng allocator, which
 * gets anonymous memory from the native Dunit Mmap syscall (the vm: adapter).
 * Covers a small alloc, a large alloc that forces a fresh mmap, calloc zeroing,
 * writes across the mapped pages, and free. Proves the heap works end to end.
 */
#include <stdlib.h>
#include <stdio.h>
#include <string.h>

int main(void)
{
	char *a = malloc(64);
	if (!a) { printf("[MUSL-MALLOC] FAIL malloc(64)\n"); return 1; }
	for (int i = 0; i < 64; i++) a[i] = (char)i;

	size_t n = 1u << 20;            /* 1 MiB forces a dedicated mmap */
	unsigned char *big = malloc(n);
	if (!big) { printf("[MUSL-MALLOC] FAIL malloc(1MiB)\n"); return 1; }
	memset(big, 0xAB, n);
	unsigned sum = 0;
	for (size_t i = 0; i < n; i += 4096) sum += big[i];

	printf("[MUSL-MALLOC] a[13]=%d big[0]=0x%x pages_sum=%u\n", a[13], big[0], sum);
	free(a);
	free(big);

	int *z = calloc(4096, sizeof(int));
	if (!z) { printf("[MUSL-MALLOC] FAIL calloc\n"); return 1; }
	if (z[0] != 0 || z[4095] != 0) { printf("[MUSL-MALLOC] FAIL calloc not zeroed\n"); return 1; }
	z[4095] = 7;
	printf("[MUSL-MALLOC] calloc z[0]=%d z[4095]=%d\n", z[0], z[4095]);
	free(z);

	printf("[MUSL-MALLOC] OK\n");
	return 0;
}
