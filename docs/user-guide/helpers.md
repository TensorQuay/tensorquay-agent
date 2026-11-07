# Helpers: ask a task to lead
<!-- Tests: WEB-E2E-024, E2E-AGENTS-020, E2E-AGENTS-021, E2E-AGENTS-022, E2E-AGENTS-023, E2E-AGENTS-024,
     E2E-AGENTS-025 (v1.1). -->
From v1.1. Your administrator turns helpers on for your installation.

Some work splits into parts that can be done side by side: a report from several sources, a review of several
documents, a few ideas to try. A task can lead helpers for that. It plans the work, starts a helper for each part,
gives each one a brief, reads what they report and puts the result together. You watch, and step in when you want.

Helpers cost more than one task: each one does its own work with the model. For work that doesn't split, one task is usually
as good and much cheaper.

## Start
There's no button for it: start an ordinary task and ask in words, for example "Write the Q3 report, with a helper
for each region's figures, then combine them" The task decides how many helpers it needs, up to the most your
administrator allows, and writes each one's brief. There is nothing else to set up.

A helper can start from one of your agent profiles, such as Legal reviewer, when you ask for it ("have the Legal
reviewer check the contract") or when the lead chooses it. It then works with that profile's instructions, model and
environment. It never gets more access than the lead task has: tools or connectors the lead task can't use are left
out.

In the sidebar, the lead task's row gets an arrow at its start and the number of helpers at its end. Choose the arrow
to see the helpers under it; they stay closed until you do. Each is named by its job, such as Researcher, with its
state's mark. A search finds helpers too, each saying whose helper it is. While the team works, when a helper is
stuck or has failed, the number's place says so, for example "1 stuck"; once the team has ended, the row shows the
number again.

## Follow the work
- The lead task's page has a strip under its title: each helper with its state, how much of the team's budget is
  used, and the time left; hold the pointer on the budget for the exact figures. Choose a helper to open it. With
  more than four helpers, the strip ends in "+2" (or however many more), which opens **All helpers**: every helper
  with its state and its last message, and its menu (the three dots) with **Open** and **Stop helper**. On a phone the
  strip is one line, such as "3 helpers · Budget 20 % used", which opens the same list. Once the team has ended, the
  strip says how, for example "Finished · 20 % of the budget used", in place of the budget and the time left.
- Team messages show in the lead task's conversation as quiet lines: what the lead asked, and what each helper
  answered or reported. Choose a line to read all of it.
- The team's steps read as what was done, like any task's steps: on the lead task, "Started a helper: researcher",
  "Gave writer more work", "Asked every helper", "Told every helper a decision", "Asked you a question", "Read what
  researcher has done", "Took the files a helper handed back", "Stopped writer", "Checked for reports" and, when it
  finishes, "Wrapped up the team's work"; on a helper, "Reported to the lead" (or "Answered the lead", "Asked the
  lead") and "Checked for messages from the lead".
- When new team messages wait for a lead task or a helper whose turn had ended, it starts again by itself, and its
  page shows a quiet line, "New team messages", at the start of that turn. It isn't something you wrote; who wrote is
  in the team's messages. While that turn waits for a free slot, or for its sandbox to start, the same line shows
  where a waiting message would, with no Cancel: the team needs it to read what waits.
- A helper's page is a normal task page. Under its title, a line such as "Helper of Report on Q3 · researcher" links
  back to the lead task; then its **Brief from the lead**, and its work, live. While its first turn waits (for a free
  slot, or for its turn on the lead's files), a quiet "Brief from the lead" line shows where a waiting message would,
  with no Cancel; the brief itself shows once it starts.

A helper's state is one of **Working…**, **Waiting for the lead**, **Waiting for a slot**, **Stuck**, **Done**,
**Stopped** or **Failed**. The lead task is told when a helper stops without reporting, gets stuck or fails, and decides
what to do next.

A helper shows **Failed** when its work stopped with an error, or when its environment stopped again and again (five
times within ten minutes) and the installation stopped replacing it; the lead's row in the sidebar then ends, for
example, "1 failed". The helper keeps its task and its place: the lead's next message to it, or yours on its page,
starts it again, and it shows **Working…**. A helper that couldn't start at all also shows **Failed**, but it can't be
started again, and its place is free.

## Step in
- Write to the lead task as you would to any task.
- Write to a helper on its own page, as to any task. While the team works, the line under the message box,
  "The lead task sees what you send here.", is there because the lead is told what you wrote. Once the team has
  ended, the helper is an ordinary task, and the line goes.
- To disagree with a decision, tell the lead task, for example "No, use format Y". It reads your message and decides
  again.
- To answer the lead: when the team needs you, the lead task asks. Its status says **Waiting for you**, and its row in
  the sidebar gets an amber dot where it is. The question is in the conversation; answer it in the message box, and
  the question then says **Answered**. The lead may keep working while it waits.

Helpers never ask you directly: they ask their lead task, and the lead asks you if it needs to.

## Files
The lead gives each helper either its own copy of the files, so that helpers can work side by side, or the lead's own files, which they then take turns on. A helper with its own copy sends the files it made with its report,
and the lead task puts them in its workspace, by default in a folder named after the helper, such as
`helpers/researcher/`. Nothing in the workspace is replaced unless the lead says so.

## Stop
- **Stop** in a lead task's message box stops only the lead's own step: its helpers keep working, and a helper's
  report can wake the lead again. The button says so while helpers work, and once the lead has stopped, the line
  under its title says "Helpers are still working." with **Stop team**.
- **Stop team** in the lead task's menu (the three dots), or beside "Helpers are still working.", stops every helper
  at once, after asking you; their tasks and files stay.
- **Stop helper** in a helper's menu (the three dots) stops that one, and the lead is told not to give its part to
  a new helper unless you ask. A helper that is **Done** or **Failed** can be stopped too: until then it keeps its
  place among the helpers the lead task may have, in case the lead gives it more work, and stopping it frees the place.
- A team also stops when the lead says it's done, when it has used its budget or its time, or after a while with
  nothing happening.

When a team stops, the lead task's conversation gets a summary: why it stopped, each one's share of the work
(the helpers and the lead task), their reports and the files they handed back. Every task stays: you can carry on with the lead task or with any helper, as with any task.

## Delete
Deleting a helper removes that helper, its messages and any files it sent that the lead hadn't taken yet. The lead is
told. Deleting the lead task removes the team: its messages, every helper's task and the helpers' own copies of the
files. Helpers still working are stopped first. The lead task's own workspace stays, with the files placed in it. The
confirmation names each helper that goes; if a helper started after you opened it, it says so, and you check the names
and delete again.

## How many helpers
A lead task has at most as many helpers at once as your administrator allows (3 unless they changed it). A helper that
is **Done** or **Failed** still counts until it is stopped. At the limit, the lead isn't given another helper: it is
told why, and carries on with the helpers it has, or stops one to make room.
