"""The long tasks (L1 and L3, docs/evals/agent-teams.md section 2, 7 Oct 2026): many files over hours. Each check is
an integration check over the whole result.

Each task lives in its own module (long_l1.py and long_l3.py); this one lists them.
"""

from long_l1 import l1
from long_l3 import l3

TASKS = (l1, l3)
