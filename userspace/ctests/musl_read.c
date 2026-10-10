/* Static musl stdio-read test for Dunit OS. Exercises the fs: read path
 * (__stdio_read / readv -> native Read): write two lines with buffered stdio,
 * then reopen and read them back with fgets + hit EOF with fgetc.
 */
#include <stdio.h>
#include <string.h>

int main(void)
{
	const char *path = "/persist/musl_read.txt";
	const char *l1 = "alpha\n";
	const char *l2 = "beta gamma\n";

	FILE *w = fopen(path, "w");
	if (!w) { printf("[MUSL-READ] FAIL fopen w\n"); return 1; }
	fputs(l1, w);
	fputs(l2, w);
	fclose(w);

	FILE *r = fopen(path, "r");
	if (!r) { printf("[MUSL-READ] FAIL fopen r\n"); return 1; }
	char b1[64] = {0}, b2[64] = {0};
	char *g1 = fgets(b1, sizeof b1, r);
	char *g2 = fgets(b2, sizeof b2, r);
	int eofc = fgetc(r);
	fclose(r);

	printf("[MUSL-READ] read %zu+%zu bytes eof=%d\n",
	       strlen(b1), strlen(b2), eofc == EOF);
	if (g1 && g2 && !strcmp(b1, l1) && !strcmp(b2, l2) && eofc == EOF)
		printf("[MUSL-READ] OK\n");
	else
		printf("[MUSL-READ] FAIL\n");
	return 0;
}
