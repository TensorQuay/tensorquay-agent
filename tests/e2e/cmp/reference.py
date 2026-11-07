"""The reference product's agent server API, as E2E-CMP-003 uses it: one conversation per attempt, started with the
task and its step limit, then polled until it ends. Only this file knows the API's shapes. They match the server's own
/openapi.json for agent server 1.49.6, saved in the setup step; a run records the version it used in run.json.
"""

# Where the attempt's workspace is mounted in the reference container, and the agent's working directory.
WORKDIR = "/workspace"
# The port the agent server listens on inside its container.
SERVER_PORT = 8000
# The server accepts only requests carrying one of its session keys; one random key per attempt.
SESSION_ENV = "OH_SESSION_API_KEYS_0"
SESSION_HEADER = "X-Session-API-Key"
# Answers once the server has finished starting.
HEALTH = "/ready"
CONVERSATIONS = "/api/conversations"
# Where the server records why a conversation stopped in `error`: its conversation-level error events, each with its
# own classification (`FailureKind`).
ERROR_EVENTS = "/api/conversations/{}/events/search?kind=ConversationErrorEvent&limit=100"
# The kinds that mean the model server or the connection failed, as our harness's `harness.provider` does: only these
# are rerun. `agent_action` and `quota` are the agent's own outcome (as our `model.budget`); the rest are the product's.
PROVIDER_KINDS = ("transient", "rate_limit")
AGENT_KINDS = ("agent_action", "quota")
# Statuses after which the conversation does no more work on its own.
TERMINAL = ("finished", "error", "stuck")


def start_body(task_text, model_url, model, model_key, max_steps, max_output=None, extra_body=None):
    """The request that starts one attempt: the task statement verbatim as the only user message, run at once, with
    the task's own step limit, the reference's default tools, and the same model server our side uses, asked the same
    way our route asks it: its output cap and extra body, and sampling and reasoning effort left to the server (the
    reference would otherwise send its own reasoning effort)."""
    return {
        "agent": {
            "llm": {
                "usage_id": "agent",
                # The OpenAI-compatible provider prefix, pointed at the local server by base_url.
                "model": "openai/" + model,
                "base_url": model_url,
                "api_key": model_key,
                "max_output_tokens": max_output,
                "litellm_extra_body": extra_body or {},
                "reasoning_effort": None,
                "temperature": None,
                "top_p": None,
            },
            "tools": default_tools(),
            "kind": "Agent",
        },
        "workspace": {"working_dir": WORKDIR, "kind": "LocalWorkspace"},
        "initial_message": {"role": "user", "content": [{"type": "text", "text": task_text}], "run": True},
        "max_iterations": max_steps,
    }


def default_tools():
    """The reference's default coding tools, without a browser (our tasks don't use one): a shell, the file editor
    and its task tracker, by the names its server registers."""
    return [{"name": name, "params": {}} for name in ("terminal", "file_editor", "task_tracker")]


def terminal(status):
    return status in TERMINAL


def conversation_path(conversation_id):
    return "{}/{}".format(CONVERSATIONS, conversation_id)


def tokens(info):
    """Prompt and completion tokens the reference reports for the conversation, when it does: summed over its LLM
    usage ids (`stats.usage_to_metrics`, where agent server 1.49.6 keeps them), else the top-level `metrics`."""
    info = info or {}
    usages = [
        (entry or {}).get("accumulated_token_usage") or {}
        for entry in (((info.get("stats") or {}).get("usage_to_metrics")) or {}).values()
    ]
    usages = [usage for usage in usages if "prompt_tokens" in usage or "completion_tokens" in usage]
    if not usages:
        usage = (info.get("metrics") or {}).get("accumulated_token_usage") or {}
        if "prompt_tokens" not in usage and "completion_tokens" not in usage:
            return None
        usages = [usage]
    return {
        "prompt": sum(usage.get("prompt_tokens", 0) for usage in usages),
        "completion": sum(usage.get("completion_tokens", 0) for usage in usages),
    }


def error_kind(page):
    """The classification kind of the conversation's last error event, or None when it recorded none."""
    items = [item for item in (page or {}).get("items") or [] if item.get("kind") == "ConversationErrorEvent"]
    return ((items[-1].get("classification") or {}).get("kind")) if items else None
