/* Static musl directory-iteration test for Dunit OS. Exercises the fs: getdents
 * adapter (opendir snapshots via native Readdir; readdir serializes it): list
 * /app, count the entries, and find this program's own name.
 */
#include <dirent.h>
#include <string.h>
#include <stdio.h>

int main(void)
{
	DIR *d = opendir("/app");
	if (!d) { printf("[MUSL-DIR] FAIL opendir\n"); return 1; }

	int count = 0, found = 0;
	struct dirent *e;
	while ((e = readdir(d)) != NULL) {
		count++;
		if (strcmp(e->d_name, "musl_dir") == 0)
			found = 1;
	}
	closedir(d);

	printf("[MUSL-DIR] /app entries=%d found_self=%d\n", count, found);
	if (count > 5 && found)
		printf("[MUSL-DIR] OK\n");
	else
		printf("[MUSL-DIR] FAIL\n");
	return 0;
}
