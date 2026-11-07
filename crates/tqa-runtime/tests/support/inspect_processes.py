"""Inspect only the runtime's children and socket descriptors inside its PID namespace."""
import os
from pathlib import Path

processes = []
for path in Path('/proc').iterdir():
    if not path.name.isdigit():
        continue
    try:
        stat = (path / 'stat').read_text().rsplit(') ', 1)[1].split()
        processes.append((int(path.name), int(stat[1]), stat[0], (path / 'comm').read_text().strip()))
    except FileNotFoundError:
        continue
runtime = [pid for pid, _, _, name in processes if name == 'tqa-runtime']
assert len(runtime) == 1, processes
assert not any(parent == runtime[0] or state == 'Z' or name == 'sleep' for _, parent, state, name in processes), processes
inodes = set()
for fd in Path(f'/proc/{runtime[0]}/fd').iterdir():
    try:
        target = os.readlink(fd)
        if target.startswith('socket:['):
            inodes.add(target[8:-1])
    except FileNotFoundError:
        continue
for family in ['tcp', 'tcp6']:
    for row in Path(f'/proc/net/{family}').read_text().splitlines()[1:]:
        fields = row.split()
        assert not (fields[3] == '0A' and fields[9] in inodes), row
print('no children, zombies or listening sockets')
