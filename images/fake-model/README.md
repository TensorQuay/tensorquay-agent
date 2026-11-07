# Scripted model image

Build the test endpoint with `docker build -f images/fake-model/Dockerfile -t tqa-fake-model:local .`.
The default scenario is `/scenarios/demo.json`. Select the longer scenario by passing
`--scenario /scenarios/demo-long.json` after the image name.

The endpoint serves `GET /v1/models`, `POST /v1/chat/completions` and `POST /v1/responses`, and every request needs a
non-empty bearer. Both completion endpoints stream the same scenario turn. For Chat, the turn is the number of assistant
messages in the request. For Responses, it is the number of earlier model responses in `input`: each assistant message
item starts one, and so does a run of `function_call` items that does not follow an assistant message. Responses
streams always report usage in their terminal event.

The long scenario makes twelve calls across write, edit, read, search, glob and shell tools. An initial shell test
fails; an edit fixes the arithmetic function, and the next test run succeeds. Seven model responses include reasoning,
progress text and a roughly 3 KiB Markdown answer. A 65 ms chunk delay provides approximately eight seconds of streaming
for follow-mode and layout checks. It needs only the Python standard library in the runtime workspace.

Regenerate the fixture with `python3 crates/tqa-testkit/scenarios/generate-demo-long.py`. The standalone endpoint tests
validate its arguments against the real tool registry, stream its Markdown and check its delay budget. The ignored
runtime container test executes the calls and verifies the failure and recovery. Browser rendering and interaction
are tested separately by the full-stack UX suite.

Under prompt selection, a tool call's argument may be `{"seq_of": {"type": T, "from": F}}`: each request fills it with the
latest seq its history names as `seq="N" type="T" from="F"`, as an agent team's read renders a message, so a scripted lead
can name a report whose number depends on which helper answered first. A history that names no such message gets a 422
naming the placeholder, never a guessed number; a placeholder of any other shape stops the scenario from loading.
