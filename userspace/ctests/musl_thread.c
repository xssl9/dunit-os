/* Static musl threads test for Dunit OS — the thread: port target.
 *
 * WIP: pthread_create (Dunit __clone + per-thread SetThreadPointer), the mutex
 * fast path, and each thread's body all run correctly, but TWO threads exiting
 * concurrently hang. Root cause: musl's __pthread_exit holds the thread-list
 * lock (__thread_list_lock) across its final SYS_exit and relies on the Linux
 * CLONE_CHILD_CLEARTID contract — the kernel clearing *ctid and futex-waking it
 * on thread death — to release that lock. Dunit has no clear-child-tid yet, so
 * the lock is never released and the other exiting thread blocks forever.
 *
 * Fix (next): give the kernel a set_tid_address-style "clear this word and
 * futex-wake it on thread exit" and have __clone's trampoline register ctid.
 * See toolchains/dunit-musl/PORTING.md.
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
