"""PERF-012's workload, standard library only, so the same file runs on the host and inside the runtime image:
sequential write and read of one file in 1 MiB blocks, then random aligned 4 KiB writes and reads within it.
Prints one JSON object: MB/s for the sequential measures and operations per second for the random ones, and each
measure's seconds.

`--phase all` (the default) runs the four in order and removes the file. A single phase (`seq-write`, `rand-write`,
`seq-read`, `rand-read`, in that order, the last removing the file) lets a microVM's driver act on the host between
them: inside a guest, an fsync and an eviction stop at the guest, so the driver flushes and evicts the image on the host
and charges the flush to the write (deploy/firecracker/perf_io_vm.py)."""
import argparse
import json
import os
import platform
import random
import time

BLOCK, PAGE = 1024 * 1024, 4096


def drop_cache(descriptor):
    """Writes back and evicts the file's pages, since there is no root for drop_caches."""
    os.fsync(descriptor)
    os.posix_fadvise(descriptor, 0, 0, os.POSIX_FADV_DONTNEED)


def sequential_write(path, size):
    block = os.urandom(BLOCK)
    started = time.monotonic()
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
    try:
        for _ in range(size // BLOCK):
            os.write(descriptor, block)
        os.fsync(descriptor)
    finally:
        os.close(descriptor)
    return time.monotonic() - started


def sequential_read(path, size):
    descriptor = os.open(path, os.O_RDONLY)
    try:
        drop_cache(descriptor)
        started = time.monotonic()
        total = 0
        while chunk := os.read(descriptor, BLOCK):
            total += len(chunk)
        elapsed = time.monotonic() - started
    finally:
        os.close(descriptor)
    if total != size:
        raise RuntimeError("short read")
    return elapsed


def random_writes(path, size, count, seed):
    offsets = random.Random(seed).sample(range(size // PAGE), count)
    page = os.urandom(PAGE)
    descriptor = os.open(path, os.O_WRONLY)
    try:
        started = time.monotonic()
        for index, offset in enumerate(offsets, 1):
            os.pwrite(descriptor, page, offset * PAGE)
            if index % 256 == 0:
                os.fsync(descriptor)
        os.fsync(descriptor)
        return time.monotonic() - started
    finally:
        os.close(descriptor)


def random_reads(path, size, count, seed):
    offsets = random.Random(seed + 1).sample(range(size // PAGE), count)
    descriptor = os.open(path, os.O_RDONLY)
    try:
        drop_cache(descriptor)
        os.posix_fadvise(descriptor, 0, 0, os.POSIX_FADV_RANDOM)
        started = time.monotonic()
        for offset in offsets:
            if len(os.pread(descriptor, PAGE, offset * PAGE)) != PAGE:
                raise RuntimeError("short read")
        return time.monotonic() - started
    finally:
        os.close(descriptor)


PHASES = ("seq-write", "rand-write", "seq-read", "rand-read")


def run_phase(phase, path, size, args):
    """One phase's measure and its seconds."""
    if phase == "seq-write":
        seconds = sequential_write(path, size)
        return {"seq_write_mbps": size / 1e6 / seconds, "seq_write_seconds": seconds}
    if phase == "seq-read":
        seconds = sequential_read(path, size)
        return {"seq_read_mbps": size / 1e6 / seconds, "seq_read_seconds": seconds}
    if phase == "rand-write":
        seconds = random_writes(path, size, args.random_ops, args.seed)
        return {"rand_write_ops": args.random_ops / seconds, "rand_write_seconds": seconds}
    seconds = random_reads(path, size, args.random_ops, args.seed)
    return {"rand_read_ops": args.random_ops / seconds, "rand_read_seconds": seconds}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory")
    parser.add_argument("--size-mib", type=int, default=256)
    parser.add_argument("--random-ops", type=int, default=4096)
    parser.add_argument("--seed", type=int, default=0)
    parser.add_argument("--phase", choices=("all",) + PHASES, default="all")
    args = parser.parse_args()
    size, path = args.size_mib * BLOCK, os.path.join(args.directory, "perf-io.bin")
    phases = PHASES if args.phase == "all" else (args.phase,)
    result, done = {}, False
    try:
        for phase in phases:
            result.update(run_phase(phase, path, size, args))
        done = True
    finally:
        # The file stays between single phases; the last phase, a whole run, or a failure removes it.
        if (not done or phases[-1] == "rand-read") and os.path.exists(path):
            os.unlink(path)
    report = {name: round(value, 4 if name.endswith("_seconds") else 1) for name, value in result.items()}
    report["python"] = platform.python_version()  # both sides must run the same interpreter
    print(json.dumps(report))


if __name__ == "__main__":
    main()
