/* Static musl stat() test for Dunit OS. Exercises the fs: stat adapter
 * (POSIX stat -> native Stat, UserFileStat -> struct stat): stat a regular file
 * (/app/musl_stat, this binary) and a directory (/persist), checking the type
 * bits and that the file's size is nonzero.
 */
#include <sys/stat.h>
#include <stdio.h>

int main(void)
{
	struct stat sf, sd;

	if (stat("/app/musl_stat", &sf) != 0) { printf("[MUSL-STAT] FAIL stat file\n"); return 1; }
	if (stat("/persist", &sd) != 0) { printf("[MUSL-STAT] FAIL stat dir\n"); return 1; }

	printf("[MUSL-STAT] file reg=%d size=%ld | dir isdir=%d\n",
	       S_ISREG(sf.st_mode), (long)sf.st_size, S_ISDIR(sd.st_mode));

	if (S_ISREG(sf.st_mode) && sf.st_size > 0 && S_ISDIR(sd.st_mode))
		printf("[MUSL-STAT] OK\n");
	else
		printf("[MUSL-STAT] FAIL\n");
	return 0;
}
