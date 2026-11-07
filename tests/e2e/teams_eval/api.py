"""The public API calls the agent-teams evaluation's runner makes, as tqa-e2e's own attempts make them (crates/
tqa-testkit/src/client.rs): workspaces, a task and its message, the event stream and log, a run's cancel, files, and
for the team side the team's record and the agent teams' limits. The token is read from a file and never printed."""

import json
import urllib.error
import urllib.parse
import urllib.request
import uuid

API_TIMEOUT = 30


class ApiError(Exception):
    """A request the gateway refused or never answered: the step, the status (or None) and the code."""

    def __init__(self, step, status, code):
        super().__init__(f"{step}: {status} {code}")
        self.step, self.status, self.code = step, status, code


class Api:
    def __init__(self, base_url, token_file, opener=None):
        self.base = base_url.rstrip("/") + "/api/v1"
        with open(token_file, encoding="utf-8") as handle:
            self._token = handle.read().strip()
        self._open = opener or urllib.request.urlopen

    def _request(self, step, method, path, body=None, query=None, timeout=API_TIMEOUT, accept="application/json"):
        url = self.base + path + ("?" + urllib.parse.urlencode(query) if query else "")
        data = None if body is None else json.dumps(body).encode()
        headers = {"Authorization": f"Bearer {self._token}", "Accept": accept}
        if data is not None:
            headers["Content-Type"] = "application/json"
        if method != "GET":
            # Every write carries a fresh key, as tqa-e2e's client sends one (the gateway requires it).
            headers["Idempotency-Key"] = uuid.uuid4().hex
        request = urllib.request.Request(url, data=data, method=method, headers=headers)
        try:
            return self._open(request, timeout=timeout)
        except urllib.error.HTTPError as error:
            try:
                code = json.loads(error.read() or b"{}").get("error", {}).get("code")
            except ValueError:
                code = None
            raise ApiError(step, error.code, code) from None
        except (urllib.error.URLError, TimeoutError, OSError):
            raise ApiError(step, None, "transport") from None

    def _json(self, step, method, path, body=None, query=None):
        with self._request(step, method, path, body, query) as response:
            raw = response.read()
        return json.loads(raw) if raw else None

    def create_workspace(self, name):
        return self._json("create the workspace", "POST", "/workspaces", {"name": name})["id"]

    def delete_workspace(self, workspace):
        self._json("delete the workspace", "DELETE", f"/workspaces/{workspace}")

    def create_conversation(self, title, route, workspace, template=None):
        body = {"title": title, "model_route": route, "workspace_id": workspace}
        if template:
            body["template"] = template
        return self._json("create the task", "POST", "/conversations", body)["id"]

    def delete_conversation(self, conversation):
        self._json("delete the task", "DELETE", f"/conversations/{conversation}")

    def send_message(self, conversation, text):
        self._json("send the task", "POST", f"/conversations/{conversation}/messages", {"text": text})

    def stream(self, conversation, after, lifetime):
        """The live event stream from `after`: an open response, read line by line by the caller."""
        return self._request("open the event stream", "GET", f"/conversations/{conversation}/events",
                             query={"stream": 1, "protocol": 1, "after": after}, timeout=lifetime,
                             accept="text/event-stream")

    def events(self, conversation, after=0, limit=1000, max_pages=1000):
        """The whole event log after `after`, across pages."""
        frames = []
        for _ in range(max_pages):
            page = self._json("replay the event log", "GET", f"/conversations/{conversation}/events",
                              query={"after": after, "limit": limit})
            frames += page["items"]
            if not page["items"] or page.get("next_after") is None:
                return frames
            after = page["next_after"]
        raise ApiError("replay the event log", None, "too many pages")

    def cancel(self, conversation, run):
        self._json("cancel the run", "POST", f"/conversations/{conversation}/runs/{run}/cancel", {})

    def team(self, conversation):
        """The task's agent team, or None when it has none: no team (404), or agent teams off on the single side
        (403 teams_disabled)."""
        try:
            return self._json("read the team", "GET", f"/conversations/{conversation}/team")
        except ApiError as error:
            if error.status == 404 or (error.status == 403 and error.code == "teams_disabled"):
                return None
            raise

    def team_messages(self, conversation):
        items, after = [], 0
        while True:
            page = self._json("read the team's messages", "GET", f"/conversations/{conversation}/team/messages",
                              query={"after": after, "limit": 200})
            items += page["items"]
            if not page["more"] or not page["items"]:
                return items
            after = page["last_seq"]

    def stop_team(self, conversation):
        """Stops the task's agent team (the person's Stop on the lead): at the attempt's deadline, as its run is
        cancelled."""
        self._json("stop the team", "POST", f"/conversations/{conversation}/team/stop")

    def set_team_limits(self, token_budget, deadline_minutes):
        """The agent teams' budget and deadline for the teams started from now (GW-I-203). A write names the version
        it read, so each limit is read first."""
        listed = {item["key"]: item for item in self._json("read the limits", "GET", "/admin/limits")["items"]}
        for key, value in (("agent_teams.token_budget", token_budget),
                           ("agent_teams.deadline_minutes", deadline_minutes)):
            if key not in listed:
                raise ApiError("set the team limits", None, f"{key} not listed (agent teams off?)")
            self._json("set the team limits", "PUT", f"/admin/limits/{key}",
                       {"value": value, "version": listed[key]["version"]})

    def files(self, conversation, path=""):
        """Every entry of one workspace directory, across pages."""
        items, after = [], None
        while True:
            query = {"path": path} if after is None else {"path": path, "after": after}
            page = self._json("list the files", "GET", f"/conversations/{conversation}/files", query=query)
            items += page["items"]
            if page.get("next_after") is None:
                return items
            after = page["next_after"]

    def download(self, conversation, path):
        with self._request("download a file", "GET", f"/conversations/{conversation}/files/download",
                           query={"path": path}, accept="*/*") as response:
            return response.read()
