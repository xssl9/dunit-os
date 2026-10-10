/* Static musl time/sleep test for Dunit OS. Exercises clock_gettime (native
 * ClockGetTime) and nanosleep (native Sleep): sample the monotonic clock, sleep
 * ~50 ms, sample again, and check it advanced; also time() returns nonzero.
 */
#include <time.h>
#include <stdio.h>

int main(void)
{
	struct timespec t1, t2, req = { .tv_sec = 0, .tv_nsec = 50 * 1000000L };

	clock_gettime(CLOCK_MONOTONIC, &t1);
	nanosleep(&req, 0);
	clock_gettime(CLOCK_MONOTONIC, &t2);

	long long ns1 = (long long)t1.tv_sec * 1000000000LL + t1.tv_nsec;
	long long ns2 = (long long)t2.tv_sec * 1000000000LL + t2.tv_nsec;
	long long dms = (ns2 - ns1) / 1000000;
	time_t tt = time(0);

	printf("[MUSL-TIME] slept ~%lldms advanced=%d time=%ld\n",
	       dms, ns2 > ns1, (long)tt);
	if (ns2 > ns1 && dms >= 40 && tt > 0)
		printf("[MUSL-TIME] OK\n");
	else
		printf("[MUSL-TIME] FAIL\n");
	return 0;
}
