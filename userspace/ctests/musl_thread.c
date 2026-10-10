/* Static musl threads test for Dunit OS — the thread: port target.
 *
 * Two pthreads each do 5000 mutex-protected increments of a shared counter,
 * then main joins both and checks the total (10000) and the return values.
 * Exercises pthread_create (Dunit __clone + per-thread SetThreadPointer), a
 * contended pthread_mutex (the futex wait/wake dispatcher), pthread_join, and
 * the kernel clear-child-tid primitive (SetTidAddress) that releases musl's
 * thread-list lock on thread exit.
 */
#include <pthread.h>
#include <stdio.h>

static pthread_mutex_t m = PTHREAD_MUTEX_INITIALIZER;
static long counter = 0;

static void *worker(void *arg)
{
	long n = (long)arg;
	for (long i = 0; i < n; i++) {
		pthread_mutex_lock(&m);
		counter++;
		pthread_mutex_unlock(&m);
	}
	return (void *)n;
}

int main(void)
{
	pthread_t t1, t2;
	long n = 5000;
	void *r1 = 0, *r2 = 0;

	pthread_create(&t1, NULL, worker, (void *)n);
	pthread_create(&t2, NULL, worker, (void *)n);
	pthread_join(t1, &r1);
	pthread_join(t2, &r2);

	printf("[MUSL-THREAD] counter=%ld r1=%ld r2=%ld\n", counter, (long)r1, (long)r2);
	if (counter == 2 * n && (long)r1 == n && (long)r2 == n)
		printf("[MUSL-THREAD] OK\n");
	else
		printf("[MUSL-THREAD] FAIL\n");
	return 0;
}
