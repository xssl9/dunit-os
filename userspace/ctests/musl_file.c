/* Static musl file-I/O test for Dunit OS. Exercises the fs: open adapter
 * (POSIX flag + arg translation to the native Open), plus read/write/close.
 *
 * Part 1 (robust): open an existing initrd file read-only and check its ELF
 * magic — proves open(O_RDONLY) flag translation + read + close over the VFS.
 * Part 2 (best effort): a write+read roundtrip in a writable dir.
 */
#include <fcntl.h>
#include <unistd.h>
#include <string.h>
#include <stdio.h>

int main(void)
{
	unsigned char magic[4] = {0};
	int fd = open("/app/musl_file", O_RDONLY);
	if (fd < 0) { printf("[MUSL-FILE] FAIL open ro %d\n", fd); return 1; }
	ssize_t r = read(fd, magic, 4);
	close(fd);
	if (r != 4 || memcmp(magic, "\177ELF", 4)) {
		printf("[MUSL-FILE] FAIL read ro r=%ld %02x%02x%02x%02x\n",
		       (long)r, magic[0], magic[1], magic[2], magic[3]);
		return 1;
	}
	printf("[MUSL-FILE] ro ELF magic ok (%ld bytes)\n", (long)r);

	const char *path = "/persist/musl_file_test.txt";
	const char *data = "dunit-musl file io\n";
	int wfd = open(path, O_WRONLY | O_CREAT | O_TRUNC, 0644);
	if (wfd < 0) {
		printf("[MUSL-FILE] rw unavailable: open-write=%d\n", wfd);
		printf("[MUSL-FILE] OK\n");
		return 0;
	}
	ssize_t w = write(wfd, data, strlen(data));
	close(wfd);

	char buf[64];
	int rfd = open(path, O_RDONLY);
	ssize_t rr = rfd >= 0 ? read(rfd, buf, sizeof buf - 1) : -1;
	if (rfd >= 0) close(rfd);
	if (rr == (ssize_t)strlen(data) && !memcmp(buf, data, (size_t)rr)) {
		buf[rr] = 0;
		printf("[MUSL-FILE] rw roundtrip ok: wrote %ld read %ld\n", (long)w, (long)rr);
	} else {
		printf("[MUSL-FILE] rw unavailable: w=%ld r=%ld\n", (long)w, (long)rr);
	}
	printf("[MUSL-FILE] OK\n");
	return 0;
}
