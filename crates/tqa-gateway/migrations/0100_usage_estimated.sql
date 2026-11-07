-- A relayed step cancelled mid-reply (a stop, a team's stop_member or deadline) never got the model server's usage,
-- which vLLM and most servers send only in their last chunk: its row stayed empty, so the person's usage and an agent
-- team's budget counted nothing for a step that streamed thousands of tokens (night 2 of the teams eval, 12 Oct 2026: a
-- helper stopped after 13974 tokens of thinking read 0). Such a row now holds the relay's estimate, from what streamed
-- and the request it sent, and says so. Reported counts are never estimated.
ALTER TABLE usage ADD COLUMN estimated boolean NOT NULL DEFAULT false;
