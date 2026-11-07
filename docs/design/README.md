# Design notes

Short notes on open product questions: each sets out the options, recommends one and names the tests it would need.
They are proposals for the Product Owner and the technical lead to decide, not the contract. An accepted note becomes an
ADR amendment (or a new ADR) and TEST-PLAN rows before any code (DEV-GUIDELINES §1); a declined one is closed here with
the reason.

| Note | Question | State |
|---|---|---|
| [Take over and the agent's last page](take-over-last-page.md) | Should Take over bring back the page the agent stopped on, signed out? | Proposed, 18 Oct 2026 |
| [Turns on the lead's files](my-files-turns.md) | When one `my_files` helper holds the lead's files for a long run, should it yield, or should the lead be told? | Proposed, 18 Oct 2026 |
| [Refs from an earlier run](stale-refs-across-runs.md) | How should a run stop the model reusing element refs from an earlier run's snapshots? | Proposed, 18 Oct 2026 |
