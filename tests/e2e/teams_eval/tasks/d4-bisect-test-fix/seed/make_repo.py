"""Creates project/ with textkit's history (60 commits, fixed dates): python3 make_repo.py"""
import json
import os
import subprocess
from pathlib import Path

HERE = Path(__file__).resolve().parent


def make(target):
    target = Path(target)
    commits = json.loads((HERE / "history.json").read_text(encoding="utf-8"))
    env = dict(os.environ, GIT_AUTHOR_NAME="textkit", GIT_AUTHOR_EMAIL="dev@example.invalid",
               GIT_COMMITTER_NAME="textkit", GIT_COMMITTER_EMAIL="dev@example.invalid")
    subprocess.run(["git", "init", "-q", "-b", "main", str(target)], check=True)
    previous = set()
    for index, (message, tree) in enumerate(commits):
        for path in previous - set(tree):
            (target / path).unlink()
        for path, content in tree.items():
            (target / path).parent.mkdir(parents=True, exist_ok=True)
            (target / path).write_text(content, encoding="utf-8")
        previous = set(tree)
        date = f"2026-0{1 + index // 20}-{1 + index % 20:02d}T10:00:00+00:00"
        subprocess.run(["git", "-C", str(target), "add", "-A"], check=True)
        subprocess.run(["git", "-C", str(target), "commit", "-q", "-m", message],
                       check=True, env=dict(env, GIT_AUTHOR_DATE=date, GIT_COMMITTER_DATE=date))


if __name__ == "__main__":
    make(HERE / "project")
