"""SOAK-002: finite tool cycles with enough paced work for the two-hour gate."""
import re

from soak_audit import require

RESPONSES = 126


def scenario(rehearsal):
    """Generate 42 three-step cycles plus a poison response, within the fixture's caps."""
    replies = []
    for index in range(RESPONSES):
        cycle, step = divmod(index, 3)
        events = [{"type": "text", "text": "SOAK_RESPONSE_{:03d}\n".format(index)}]
        for chunk in range(8 if rehearsal else 64):
            events.append({"type": "text", "text": "Cycle {} step {} fragment {}.\n".format(cycle, step, chunk)})
        if step == 0:
            command = "printf soak-{} > soak.txt; sleep {}; cat soak.txt".format(cycle, 5 if rehearsal else 20)
            events.append({"type": "tool_call", "name": "shell", "split_into": 2,
                           "arguments": {"command": command}})
        elif step == 1:
            events.append({"type": "tool_call", "name": "read_file", "split_into": 2,
                           "arguments": {"path": "soak.txt"}})
        replies.append({"events": events, "delay_ms": 100 if rehearsal else 1000,
                        "usage": {"prompt_tokens": 100 + index * 40, "completion_tokens": 40}})
    replies.append({"events": [{"type": "text", "text": "SOAK_SCRIPT_EXHAUSTED\n"}],
                    "delay_ms": 0, "usage": {"prompt_tokens": 100, "completion_tokens": 1}})
    return {"model": "fixture", "responses": replies}


class ScenarioProgress:
    """Response markers expose clamping, skipped scripts and history projection resets."""
    def __init__(self):
        self.completed, self.marker = 0, None

    def record(self, event):
        if event.get("type") == "text.delta":
            text = event["text"]
            require("SOAK_SCRIPT_EXHAUSTED" not in text, "scripted workload exhausted")
            match = re.fullmatch(r"SOAK_RESPONSE_(\d{3})\n", text)
            if match:
                index = int(match[1])
                require(index < RESPONSES, "scripted workload exhausted")
                require(index == self.completed, "script response skipped or replayed after completion")
                self.marker = index
        elif event.get("type") == "model.completed":
            require(self.marker == self.completed, "completed response has no script marker")
            self.completed += 1
            self.marker = None
