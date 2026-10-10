/* Static musl file-management test for Dunit OS. Exercises mkdir/rename/unlink/
 * rmdir (native Mkdir/Rename/Unlink) with stat() checks between each step.
 */
#include <sys/stat.h>
#include <stdio.h>
#include <unistd.h>

static int exists(const char *p) { struct stat s; return stat(p, &s) == 0; }

int main(void)
{
	const char *d = "/persist/mfs";
	const char *f1 = "/persist/mfs/a.txt";
	const char *f2 = "/persist/mfs/b.txt";

	/* best-effort cleanup from a prior run (ignore errors) */
	unlink(f1); unlink(f2); rmdir(d);

	if (mkdir(d, 0755) != 0) { printf("[MUSL-FSM] FAIL mkdir\n"); return 1; }
	FILE *w = fopen(f1, "w");
	if (!w) { printf("[MUSL-FSM] FAIL create\n"); return 1; }
	fputs("hi\n", w);
	fclose(w);

	int created = exists(f1);
	if (rename(f1, f2) != 0) { printf("[MUSL-FSM] FAIL rename\n"); return 1; }
	int oldgone = !exists(f1), newthere = exists(f2);
	if (unlink(f2) != 0) { printf("[MUSL-FSM] FAIL unlink\n"); return 1; }
	int unlinked = !exists(f2);
	int rmok = (rmdir(d) == 0);

	printf("[MUSL-FSM] created=%d rename(oldgone=%d new=%d) unlinked=%d rmdir=%d\n",
	       created, oldgone, newthere, unlinked, rmok);
	if (created && oldgone && newthere && unlinked && rmok)
		printf("[MUSL-FSM] OK\n");
	else
		printf("[MUSL-FSM] FAIL\n");
	return 0;
}
