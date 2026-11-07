# Evaluation reports

One page per phase gate (DEV-PLAN §8, TEST-PLAN §17): scope, what was independently verified, measurements with
their environment, and the limits of the acceptance. The technical lead writes and accepts each report. A page marked
**draft** has been assembled from the team's gate audits and is awaiting that acceptance.

| Gate | Phase | Page | State |
|---|---|---|---|
| 0 | Foundation | [gate-0.md](gate-0.md) | **technical criteria met** (2026-10-12), on 15b's evidence: the four IDs and the CI gates pass; open on the product owner's MS0 record |
| 1 | Walking skeleton | [gate-1.md](gate-1.md) | **reached** (2026-10-12), on 15b's evidence: 36 of 36 IDs pass, PERF-001 inside its target; the demo's change step isn't scripted |
| 2 | Robustness | [gate-2.md](gate-2.md) | **reached on one condition** (2026-10-12), on 15b's evidence: 49 of 50 live IDs pass on their tests; SOAK-002 closes with c16's complete soak |
| 3 | Safety | [gate-3.md](gate-3.md) | **reached** (2026-10-12), on 15b's evidence, Docker mode: 20 of 20 live IDs pass (6 void with approvals); the demo's network half passed |
| 4 | Team | [gate-4.md](gate-4.md) | reached (2026-09-28) |
| 5 | Local-model quality | [gate-5.md](gate-5.md) | reached on 15b (2026-10-11): every local-model run passed, M6 met, the phase demo accepted |
| 6 | Extensions | [gate-6.md](gate-6.md) | reached (2026-09-27) |
| 7 | Channels | [gate-7.md](gate-7.md) | reached on c15 (2026-10-08) |
| 8 | Firecracker | [gate-8.md](gate-8.md) | reached on 15b (2026-10-11); PERF-012's microVM half (Gate 9's row) waived by the product owner; the microVM host-key test is a known gap |
| 9 | PRD metrics M1–M8 | [gate-9.md](gate-9.md) | reached on 15b (2026-10-11), with the product owner's waiver of PERF-012's microVM half |
| 10 | Agent teams | [gate-10.md](gate-10.md) | **draft** (2026-10-12): every row passed in 16d, the agents suite 12 of 12; open: the technical lead's live session, SEC-046's agent-team case, the evaluation, the soak |
| 11 | Computer use | [gate-11.md](gate-11.md) | **draft** (2026-10-21): every computer-use row passed in 16y's integration, none failed since 16u; 16k's real-world Desktop cases' defects fixed and passed live on 16o and 16x; open: a soak with Desktop tasks, the grounding evaluation on 16y's capture |

Other measurements, kept current before each `main` update:

| Measurement | Page |
|---|---|
| Live provider suite (E2E-PROVIDER-014) | [providers.md](providers.md) |
| Agent waves (E2E-WAVE-001, E2E-WAVE-002) | [waves.md](waves.md) |
| Comparison with the open reference (E2E-CMP-003, PRD M6) | [comparison.md](comparison.md) |
| Agent teams: E2E-AGENTS-020 and the v1.1 evaluation (ADR-0034 proposal J), plan | [agent-teams.md](agent-teams.md) |
| Computer use: the grounding evaluation (ADR-0039), measured 12 and 15 Oct 2026 | [computer-grounding.md](computer-grounding.md) |
