-- A channel's wake budget is a daily allowance (ADR-0014's amendment, A5; decided on 21 Nov 2026, C-3): its window
-- opens at the first charge and lasts `[channels] budget_window_seconds` (a day); after it, spent is 0 again and the
-- next charge opens a new one. NULL: no window open.
--
-- No release has channel wakes yet, so this reaches development installations only. Every channel starts at 0 with no
-- window open: the old lifetime figure is dropped (the runs' metering keeps what was spent), so a channel the old
-- budget had stopped can be woken again. A budget someone set keeps its number; one nobody set stays NULL and follows
-- the installation's default.
ALTER TABLE channels ADD COLUMN budget_window_start timestamptz;
UPDATE channels SET spent_tokens = 0 WHERE spent_tokens <> 0;
