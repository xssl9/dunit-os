/* Static musl working-directory test for Dunit OS. Exercises getcwd() (native
 * GetCwd) and chdir() (native Chdir, pointer+length reshape).
 */
#include <unistd.h>
#include <string.h>
#include <stdio.h>

int main(void)
{
	char a[64] = {0}, b[64] = {0}, c[64] = {0};

	if (!getcwd(a, sizeof a)) { printf("[MUSL-CWD] FAIL getcwd 1\n"); return 1; }
	if (chdir("/persist") != 0) { printf("[MUSL-CWD] FAIL chdir\n"); return 1; }
	if (!getcwd(b, sizeof b)) { printf("[MUSL-CWD] FAIL getcwd 2\n"); return 1; }
	if (chdir("/") != 0) { printf("[MUSL-CWD] FAIL chdir back\n"); return 1; }
	if (!getcwd(c, sizeof c)) { printf("[MUSL-CWD] FAIL getcwd 3\n"); return 1; }

	printf("[MUSL-CWD] start=%s after=%s back=%s\n", a, b, c);
	if (!strcmp(a, "/") && !strcmp(b, "/persist") && !strcmp(c, "/"))
		printf("[MUSL-CWD] OK\n");
	else
		printf("[MUSL-CWD] FAIL\n");
	return 0;
}
