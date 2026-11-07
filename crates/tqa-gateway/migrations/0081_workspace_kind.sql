-- H4 (next release): what a workspace is, so a client words it in the reader's language instead of the gateway's
-- fixed English suffix. `helper_copy` is an agent team member's own copy of its lead's workspace (ADR-0034 point 6);
-- every other workspace is `standard`. A rename keeps the kind: it says how the workspace was made.
ALTER TABLE workspaces ADD COLUMN kind text NOT NULL DEFAULT 'standard' CHECK (kind IN ('standard', 'helper_copy'));

-- Existing helper copies: the fork a member works on as its own copy, found through the member's record, never by its
-- name (a person's own workspace may end in "(helper copy)"). Only those lose the suffix the gateway gave them, when
-- the name still ends with it.
UPDATE workspaces w
SET kind = 'helper_copy',
    name = CASE WHEN w.name LIKE '% (helper copy)' AND char_length(w.name) > char_length(' (helper copy)')
                THEN left(w.name, char_length(w.name) - char_length(' (helper copy)'))
                ELSE w.name END
FROM conversations c
JOIN agent_team_members m ON m.conversation_id = c.id
WHERE c.workspace_id = w.id AND m.workspace = 'own_copy' AND w.forked_from IS NOT NULL;
