/*
 * Reproduction tool for the disk-latency class of runtime test flakes (RT-I-014); not built or run by CI.
 *
 * Delays every fsync and fdatasync of a file not on tmpfs by SLOWSYNC_MS milliseconds, emulating a saturated disk
 * without writing more to it (RAM-backed files are not affected by disk latency, so they are not delayed). Build and use on Linux with glibc:
 *
 *   cc -shared -fPIC -O2 -o slowsync.so crates/tqa-runtime/tests/support/slowsync.c -ldl
 *   LD_PRELOAD=$PWD/slowsync.so SLOWSYNC_MS=2000 <test binary> --exact <test name>
 *
 * A test whose state is on disk and whose real-time bounds are shorter than a few delayed syncs fails; the same test
 * using StateStorage::Memory (tmpfs) is unaffected by real disk latency.
 */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <linux/magic.h>
#include <stdlib.h>
#include <sys/vfs.h>
#include <time.h>

static void pause_ms(int fd) {
    struct statfs fs;
    if (fstatfs(fd, &fs) == 0 && fs.f_type == TMPFS_MAGIC) return;
    const char *value = getenv("SLOWSYNC_MS");
    long ms = value ? atol(value) : 0;
    struct timespec delay = {ms / 1000, (ms % 1000) * 1000000L};
    nanosleep(&delay, 0);
}

int fdatasync(int fd) {
    static int (*real)(int);
    if (!real) real = (int (*)(int))dlsym(RTLD_NEXT, "fdatasync");
    pause_ms(fd);
    return real(fd);
}

int fsync(int fd) {
    static int (*real)(int);
    if (!real) real = (int (*)(int))dlsym(RTLD_NEXT, "fsync");
    pause_ms(fd);
    return real(fd);
}
