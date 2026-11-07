# Turns on the lead's files

**State:** proposed, 18 Oct 2026. **Decides:** the technical lead; the Product Owner if the run model changes (option
A). **Builds on:** ADR-0034 point 6 (workspaces), ARCHITECTURE §4.6 (the write lease).

## The problem

A `my_files` helper works on the lead's own workspace. One live sandbox at a time holds a workspace's write lease, and
a sandbox releases it only at its run's end, when it dies or when the lease expires (ARCHITECTURE §4.6). So a
`my_files` helper holds the lead's files for its whole run, and every other `my_files` helper queues behind it. In a
live check, one helper's 16-minute run kept two others waiting the whole time. They did no work, and nothing told the
lead why.

The product already handles one side of this. When a `my_files` helper waits on the lead's own sandbox, the lead's
`read_reports(wait)` answers at once with `hint: {"kind": "yield_files", "member"}`, so the lead ends its turn and
frees the files (ADR-0034 point 6; GW-I-111). Nothing covers a helper waiting on another helper. The hint is the
lead's alone, and the `member_queued` notice is about slots, not files.

## Options

| | What happens | For | Against |
|---|---|---|---|
| **A. The holder yields** | After N minutes holding the lease while another `my_files` member waits, the holder's run ends at its next step boundary, as "yielded". Its next turn is queued behind the waiter's, with a gateway line saying why. | Nobody waits more than N minutes. | A new run-end kind and resume path. Each switch retires one sandbox and starts another on the same files. Two agents' half-finished edits interleave in one tree, which is what `my_files` exists to avoid ("work that has to happen in place, one step after another"). N is a guess. |
| **B. The lead is told** | After a `my_files` member has waited N minutes for the files, the lead gets a notice naming the waiter, the holder and how long. It comes once per wait, like the idle and stuck notices. | Uses the reconciler and the notice path that exist. The run model doesn't change, and the lead decides with what it knows. | The waiting still happens until the lead acts. |
| **C. Fewer helpers share files** | `start_member`'s description steers independent parts to `own_copy`, and keeps `my_files` for work that must happen in place. | Removes most waits before they start. | Guidance only, so it needs measuring. |

## Recommendation: B and C now; A not now

A turns a scheduling problem into a correctness risk: interleaved edits on one tree. And when work can be interleaved,
`own_copy` is the right mode anyway. B and C keep `my_files` as ADR-0034 defines it, sequential work in place, and put
the decision with the lead, which can act on it:
- `stop_member` the holder;
- tell it to wrap up;
- or stop the waiter and start it again with `own_copy`.

**B in detail:**
- A new notice kind, `member_waiting_files`, raised by the reconciler when a `my_files` member's queued turn has waited
  more than N minutes while another member's sandbox holds the lead's workspace.
- The text names both helpers and the time, and lists the lead's three choices in one sentence. For example: "tester
  has waited 6 minutes for your files, which builder holds. Stop builder, tell it to finish, or start tester on its own
  copy."
- It is told once per wait (the reconciler's `Told`). It clears when the waiter gets the lease or stops.
- N is an `[agent_teams]` setting beside the stuck threshold, 5 minutes by default.
- A wait on the lead's own sandbox keeps the `yield_files` hint and gets no notice.

**C in detail:** `start_member`'s `workspace` description says when each mode fits: `own_copy` for parts that touch
different files, `my_files` only for steps that must see each other's changes as they happen. The agent-teams eval then
measures how often leads choose each, and how long helpers wait for files.

Revisit A only if, with B shipped, the eval still shows waits longer than N minutes that the lead had a notice about and
couldn't act on.

## Tests

| Level | Test |
|---|---|
| Reconciler unit (`decide`) | No notice before N minutes; one at N, naming the waiter and the holder; none again for the same wait; a new one after the waiter got the lease and waits again. None when the holder is the lead's own sandbox: the hint covers that. None for an `own_copy` member waiting for a slot: `member_queued` covers that. |
| Gateway integration | Two `my_files` helpers on the fake model, the first one slow: the lead's next read answers with the notice after N. The second helper runs once the first is stopped. |
| Settings | The threshold reads from `[agent_teams]`, is refused outside its bounds, and defaults to 5 minutes. |
| Evaluation (agent teams) | Before and after C, over the eval's team tasks: how often leads pick `own_copy` for independent parts, the total time helpers wait for files, and the time to finish. The bar: helpers' waiting time for files falls, and the success rate doesn't drop. |
