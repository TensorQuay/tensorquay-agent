//! Wire limits and explicit caller recommendations.

/// The protocol version implemented by these types.
pub const PROTOCOL_VERSION: u16 = 1;
/// The revision of protocol 1 these types implement; see [`crate::PROTOCOL_REVISIONS`] (ADR-0026).
pub const PROTOCOL_REVISION: u16 = 14;
/// The lowest runtime revision a gateway built from these types accepts; keep every check a `<` comparison against
/// it. ADR-0035 raised it to 9: a revision-8 runtime can't read a `session.config` without the policy. Revision 10
/// raised it to 10: an older runtime can't read a history that holds a `reply_cut` failure. Revision 11 leaves it at
/// 10 (guardrails §8): the gateway sends host keys and the install CA only to a runtime announcing
/// [`HOST_KEYS_REVISION`].
pub const MIN_RUNTIME_REVISION: u16 = 10;
/// The revision that brought host keys (ADR-0037): a runtime below it gets neither the run's keys nor the install CA,
/// and a turn whose sender has keys gets the `host_keys_unsupported` notice instead.
pub const HOST_KEYS_REVISION: u16 = 11;
/// The revision that brought the lead's brief on an agent team member's first turn (ADR-0034): a member's runtime
/// below it is refused at the link, so a brief never reaches a model as a person's words.
pub const LEAD_BRIEF_REVISION: u16 = 12;
/// The revision that marks the gateway's wake of an agent team reader (ADR-0034): a team's lead or member below it is
/// refused at the link, so a wake never reaches a page as the person's words.
pub const TEAM_WAKE_REVISION: u16 = 13;
/// The revision that brought the Desktop (ADR-0039): a conversation whose environment declares `desktop` is refused at
/// the link below it, so its runtime always has the `computer` tool the environment was chosen for.
pub const DESKTOP_REVISION: u16 = 14;
/// The link upgrade request header carrying the runtime's revision; absent means revision 1.
pub const PROTOCOL_REVISION_HEADER: &str = "TQA-Protocol-Revision";
/// The runtime image label carrying the revision its runtime speaks; absent means revision 1.
pub const RUNTIME_REVISION_LABEL: &str = "org.tensorquay.protocol-revision";
/// Maximum encoded JSON frame size in bytes, checked before deserializing.
pub const MAX_FRAME_BYTES: usize = 256 * 1024;
/// Maximum UTF-8 data bytes in one tool-output chunk, before JSON escaping.
pub const TOOL_OUTPUT_CHUNK_BYTES: usize = 64 * 1024;
/// Maximum UTF-8 bytes in a nonempty user message.
pub const MAX_USER_MESSAGE_BYTES: usize = 64 * 1024;
/// Maximum UTF-8 bytes in an authoritative tool result, before JSON escaping.
pub const MAX_TOOL_RESULT_BYTES: usize = 256 * 1024;
/// Maximum upload paths attached to one user message.
pub const MAX_ATTACHMENTS: usize = 32;
/// Recommended concurrent partial tool outputs; callers must pass this explicitly.
pub const RECOMMENDED_MAX_IN_FLIGHT_CALLS: usize = 32;
/// Recommended assembled bytes per call; callers must pass this explicitly.
pub const RECOMMENDED_MAX_TOOL_OUTPUT_BYTES: usize = 16 * 1024 * 1024;

// Structured compaction (ARCHITECTURE section 7, ADR-0009). These are the one source of the
// compaction constants; TEST-PLAN CTX-U-002, CTX-U-003 and CTX-U-013 quote them.
/// Compaction starts when a request needs more than this percentage of the input capacity.
pub const COMPACT_AT_PERCENT: u64 = 85;
/// What a compaction aims the need at, as a percentage of the input capacity: it keeps the most turns that bring the
/// need down to this, or the fewest. The 15 points between it and [`COMPACT_AT_PERCENT`] are room for the next steps:
/// folding only to just under the threshold, as the usual six turns did at a high density, made every following step
/// call for another compaction (23 in a 30-step run at 2.6x on an 8k window; 6 to 8 with this mark). The lead's
/// decision of 7 Oct 2026; CORE-U-073 quotes it.
pub const COMPACT_LOW_WATER_PERCENT: u64 = 70;
// The mark leaves room below the threshold, or a compaction would call for another at once.
const _: () = assert!(COMPACT_LOW_WATER_PERCENT < COMPACT_AT_PERCENT);
/// Turns kept verbatim: each is a user message, or one step with all of its tool results.
pub const COMPACT_KEEP_TURNS: usize = 6;
/// The least output the summary model call gets; it normally gets a step's output budget (CORE-U-057).
pub const COMPACTION_OUTPUT_TOKENS: u32 = 1024;
/// The summary model call's output when the route declares none, at most half the input capacity. A route can turn
/// thinking on through its request extras, which the loop can't see, and a real model's reasoning took up to 4967
/// tokens before the summary (CORE-U-057).
pub const COMPACTION_DEFAULT_OUTPUT_TOKENS: u32 = 8192;
/// Maximum items in each summary list.
pub const MAX_COMPACTION_ITEMS: usize = 32;
/// Maximum Unicode scalar values in the goal and in each list item.
pub const MAX_COMPACTION_ITEM_CHARS: usize = 512;
/// Maximum UTF-8 bytes across all summary strings.
pub const MAX_COMPACTION_SUMMARY_BYTES: usize = 16 * 1024;
/// The summary format and prompt version recorded in every compaction event.
pub const COMPACTION_VERSION: u32 = 1;

// Image input on vision routes (FR-36). These are the one source of the image constants;
// TEST-PLAN CORE-U-039, CORE-U-040 and TOOL-U-021 quote them.
/// Images sent with the current user message; further images stay paths with a notice.
pub const MAX_IMAGES_PER_MESSAGE: usize = 4;
/// Largest raw image, in bytes, that is read and sent.
pub const MAX_IMAGE_BYTES: usize = 5 * 1024 * 1024;
/// Conservative window cost of one image, in tokens; providers count images differently.
pub const IMAGE_TOKENS: u32 = 1600;
/// Request body budget when the gateway does not state its relay limit (its default).
pub const DEFAULT_MAX_REQUEST_BYTES: u32 = 1024 * 1024;
/// Smallest relay request body limit a route may state, matching the gateway's range.
pub const MIN_REQUEST_BYTES: u32 = 1024;
/// Largest relay request body limit a route may state, matching the gateway's range.
pub const MAX_REQUEST_BYTES: u32 = 16 * 1024 * 1024;

// Fallback routes (ADR-0025).
/// Fallback routes a conversation's route may declare, in order.
pub const MAX_FALLBACK_ROUTES: usize = 3;
/// Largest route name, in UTF-8 bytes, carried by a `route.fallback` event.
pub const MAX_ROUTE_NAME_BYTES: usize = 256;

// Opaque reasoning state (ADR-0027).
/// Largest provider reasoning item kept, in bytes of JSON text.
pub const MAX_REASONING_ITEM_BYTES: usize = 128 * 1024;
/// Most reasoning items kept for one step.
pub const MAX_REASONING_ITEMS_PER_STEP: usize = 16;
/// Most reasoning-item bytes kept for one step.
pub const MAX_REASONING_BYTES_PER_STEP: usize = 512 * 1024;

// MCP servers in session.config (revision 7, ADR-0011, ADR-0031).
/// Most MCP servers a session may configure, stdio servers and gateway connectors together.
pub const MAX_MCP_SERVERS: usize = 8;
/// Largest MCP server or connector name, in bytes; `mcp__<name>__<tool>` then fits a provider's 64-character tool name.
pub const MAX_MCP_NAME_BYTES: usize = 16;
/// The MCP server and connector name rule: one letter, then letters or digits, each optionally after a single `_`.
/// So there is no `__` and no trailing `_`, and `mcp__<name>__<tool>` always splits back to one server.
pub const MCP_NAME_RULE: &str = "^[a-z](_?[a-z0-9])*$";
/// Most arguments of a stdio MCP server.
pub const MAX_MCP_ARGS: usize = 32;
/// Most environment variables of a stdio MCP server.
pub const MAX_MCP_ENV: usize = 32;
/// Most tool names in an MCP server's allow or deny filter.
pub const MAX_MCP_TOOL_FILTER: usize = 64;
/// Largest MCP command, argument or filtered tool name, in bytes.
pub const MAX_MCP_STRING_BYTES: usize = 1024;
/// Largest MCP environment value, in bytes.
pub const MAX_MCP_ENV_VALUE_BYTES: usize = 4096;
/// Longest MCP tool call, in seconds.
pub const MAX_MCP_CALL_TIMEOUT_SECONDS: u32 = 600;
/// The MCP tool call timeout when none is configured, in seconds.
pub const DEFAULT_MCP_CALL_TIMEOUT_SECONDS: u32 = 60;
/// Most tools one MCP server may offer (ADR-0011).
pub const MAX_MCP_TOOLS_PER_SERVER: usize = 64;
/// Largest MCP tool part of `mcp.<server>.<tool>`, in bytes (the server's own name).
pub const MAX_MCP_TOOL_BYTES: usize = 128;

// The workspace service on the runtime link (revision 7, ADR-0030).
/// Largest decoded data in one workspace read, upload or blob chunk; base64 of it fits a frame.
pub const WORKSPACE_CHUNK_BYTES: usize = 128 * 1024;
/// Most paths in one workspace stat request.
pub const MAX_WORKSPACE_STAT_PATHS: usize = 64;
/// Most blob hashes in one workspace blobs request.
pub const MAX_WORKSPACE_BLOB_HASHES: usize = 256;
/// The serialised size budget of a listing's `items` or a manifest part's `entries` array; with the largest envelope
/// the frame stays within [`MAX_FRAME_BYTES`], however names are escaped.
pub const WORKSPACE_PART_BYTES: usize = 192 * 1024;
/// The agent profile name rule (revision 8, ADR-0012): a lowercase letter, then up to 63 lowercase letters, digits,
/// `_` or `-`.
pub const PROFILE_NAME_PATTERN: &str = "^[a-z][a-z0-9_-]{0,63}$";
/// Largest profile name, in bytes.
pub const MAX_PROFILE_NAME_BYTES: usize = 64;
/// Largest profile instructions, in UTF-8 bytes.
pub const MAX_PROFILE_INSTRUCTIONS_BYTES: usize = 16 * 1024;
/// Most built-in tools a profile lists.
pub const MAX_PROFILE_TOOLS: usize = 64;
/// Most skills a profile lists.
pub const MAX_PROFILE_SKILLS: usize = 64;
/// The skill name rule, the one the prompt layer's skill tool uses.
pub const SKILL_NAME_PATTERN: &str = "^[a-z0-9-]{1,64}$";
/// Largest skill name, in bytes.
pub const MAX_SKILL_NAME_BYTES: usize = 64;
/// Most characters (code points) of a provider's own error words on `run.failed` and `provider.retry`.
pub const MAX_PROVIDER_MESSAGE_CHARS: usize = 300;
