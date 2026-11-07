"""The dependent tasks (D1-D5, docs/evals/agent-teams.md section 2, 7 Oct 2026): work whose parts depend on each
other, so a split must settle an interface, hand work back and merge it. Each check is an integration check: it
passes only when the parts fit together, never by counting parts found.

Each task lives in its own module (dependent_d1.py to dependent_d5.py); this one lists them.
"""

from dependent_d1 import d1
from dependent_d2 import d2
from dependent_d3 import d3
from dependent_d4 import d4
from dependent_d5 import d5

TASKS = (d1, d2, d3, d4, d5)
