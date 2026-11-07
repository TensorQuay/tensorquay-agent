"""Create tiny object fixtures directly; the tests only run read-only Git commands."""
import hashlib
from pathlib import Path
import sys
import zlib

root, object_format = Path(sys.argv[1]), sys.argv[2]
git = root / ".git"
def store(kind, data):
    value = kind.encode() + b" " + str(len(data)).encode() + b"\0" + data
    digest = hashlib.new(object_format, value).hexdigest()
    path = git / "objects" / digest[:2] / digest[2:]
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(zlib.compress(value))
    return digest
blob = store("blob", b"example\n")
tree = store("tree", b"100644 example.txt\0" + bytes.fromhex(blob))
identity = b"TensorQuay <admin@tensorquay.com> 0 +0000\n"
commit = store("commit", b"tree " + tree.encode() + b"\nauthor " + identity + b"committer " + identity + b"\nFixture\n")
(git / "refs/heads/feature/example").write_text(commit + "\n")
with (git / "config").open("a") as output:
    output.write("[extensions]\nobjectformat=" + object_format + "\n[include]\npath=untrusted.conf\n")
(git / "untrusted.conf").write_text("[remote \"origin\"]\nurl=https://wrong.example/repo.git\n")
print(commit)
