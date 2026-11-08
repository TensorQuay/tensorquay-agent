# Tasks

<!-- Tests: WEB-E2E-004, WEB-U-013, WEB-U-015, WEB-U-016, WEB-U-017, WEB-U-018, WEB-U-031, WEB-U-071, WEB-U-072, WEB-E2E-018, WEB-U-119. -->
A task is one piece of work with the agent: your messages, the agent's answers and everything it did in between. Your
tasks are listed in the sidebar, grouped by when they were last active, newest first.

## Find a task
The sidebar groups your tasks into **Today**, **Yesterday**, **Previous 7 days** and **Older**, eight at a time;
**Show more** shows the rest of a group. Once you have eight tasks or more, **Search** under **New task** (or `Ctrl K`,
`⌘K` on a Mac) opens **Search tasks**: it finds every task whose title contains what you type, however old. Choosing
one opens it; Escape closes the search.

A mark beside a task shows how it is doing: a turning circle while the agent works, and a cross when it failed. A
finished or stopped task has no mark.

Each group's heading says when its tasks were last active. Point at a task to see its full title, its workspace and
the exact time.

## Start a task
Choose **New task** in the sidebar. The page asks "What should the agent do?". The bar at the top and the message box
show what the task will use, and you change it right there:

- **Model** and **Environment:** the two small choices at the right of the bar at the top, such as "DeepSeek Flash" and
  "Workstation". Choosing one applies at once. On a narrow screen they show as icons; the chosen name is in each one's
  tooltip and at the top of its list. Your administrator decides which models are offered; each environment
  lists its tools, such as **Shell**, **Python**, **Node.js**, **Build tools**, a **Browser** and **Data** (Python's
  pandas, matplotlib and openpyxl, for tables, charts and spreadsheets). An environment with the **Desktop** tool gives
  the task a screen the agent works on, which you can watch and take over (see [The desktop](desktop.md)).
- **Add** (the + at the bottom left) adds more: **Attach files**, **Workspace…** (the files the task works on; by default a task gets
  a new, empty workspace, and you can pick an existing one to continue work on its files, see
  [Workspaces](workspaces.md)) and, when you have agent profiles, **Agent profile…** (a ready-made setup for the task:
  its instructions, model, environment, tools and limits; manage them under **Agent profiles** in **Settings**). What
  you pick shows above your message; its × takes it off again. A choice the agent profile makes reads **Set by the
  agent profile …** and can't be changed for this task.

Type your message and press Enter to send (Shift+Enter starts a new line). Or choose one of the suggestions under the
message box, such as **Make a sample sales spreadsheet and chart the totals by region**, to start from it; with a
workspace that holds files, they suggest work on its files, such as **Explain the files in this workspace**. The task takes its title from
your first message; after the first answer it gets a short name of a few words, such as "Sales CSV cleanup". Click the
title in the task header (or choose **Rename task** in its … menu, or in the task's menu in the sidebar) and type a new one: Enter or
clicking elsewhere saves it, Escape keeps the old one. A name you choose stays.

If the page says "No model routes are available. Ask your administrator to configure one." or "No environments are
available. Ask your administrator to configure one.", ask your administrator. An administrator is shown where to fix it
instead: "No model routes are set up yet: add one in Settings › Administration › Models.", with **Add a model route**,
or "No environments are available yet: see Settings › Administration › Environments.", with **Open Environments**. Under
the message box, "Tasks can't reach the internet: no sites are allowed yet. Ask an administrator to allow sites." means
the installation allows no website yet, so a task can't install packages or read a page. An administrator reads "Tasks
can't reach the internet: no sites are allowed yet." there instead, with **Allow sites**, which opens **Internet
access** in **Settings**.

On the Workstation, the agent can search the web when the installation's internet access allows it: ask, for example,
"find the current Node.js LTS version and give the page you found it on". It searches with the tools in its environment
(Wikipedia for facts, then a web search) and then reads the pages it picks. A search site sometimes answers with a page
asking it to prove it's a person instead of results; the agent says so rather than trying again. If it can't search, ask
an administrator to allow the search sites in **Internet access**.

## Follow the work
The run status in the header, beside the task's title, says what is happening in plain words:

| Status | Meaning |
|---|---|
| **Ready** | Nothing is running; send a message to start |
| **Sending…** | Your message is on its way |
| **Not sent. …** | Your message didn't reach the task; it says what happened and what to do, and your text stays in the message box to send again |
| **Starting…** | The environment is starting for your message, including while one of your own idle environments closes to make room |
| **Working…** | The agent is working; the elapsed time is shown |
| **Waiting for a free slot** | The installation is busy; your run starts when an environment is free, and the status says how many are ahead |
| **Waiting for the workspace** | Another task is using the workspace; your run starts when it is free |
| **Done**, **Stopped**, **Failed: …** | The run ended; a failed run says why |

While a run is under way, starting and waiting included, a small pulsing dot sits before the status, and the browser
tab's title starts with "●". Once the run has started, the task's item in the sidebar shows the same dot.

If the environment stops unexpectedly, the status line says so beside the run's status, for example "Environment
stopped 2 times; restarting in 15 s", and counts down while it is replaced. If it keeps stopping, the installation
stops replacing it; send a message to try again, and the run carries on where it was.

While the agent works, each thing it does appears as one line in plain words, such as "Read …" for a page or a file,
"Edited …" or "Ran a command: …", with a mark when a step failed or was interrupted; a command that didn't succeed
says so. A line appears as soon as its step starts, with a small turning mark while it runs, and then says how long it
took. While a command runs, its open line shows its latest output as it comes. Point at a page's line for its whole address, and select a line to see its output and **Details**. When the
run ends, its lines fold under one line such as "Worked for 2 s · read 2 pages, ran 1 command"; select it to see them
again. A line you opened yourself stays open, and so does its group. The answer itself is shown as formatted text, with code blocks and tables.
The last line says what the agent is doing now and for how long. While a step runs it names the step, such as
"Reading README.md…" or "Running the tests… · 12 s". While the model thinks before its next step, it says "Thinking…",
such as "Thinking… · 92 s"; when the model shares its thinking, it shows under that line as it comes, and selecting the
line folds it. Once the model starts writing, its thinking folds to a line such as "Thought for 12 s"; select it to
read the thinking again. A model that has not started answering after 10 seconds says "Waiting for the model to start
answering…", with its time too.

The transcript follows new output while you are at the bottom. If you scroll up to read, it stays where you are, and
**Jump to latest** takes you back.

Under the answer, a short line says what the task wrote ("No files changed" when it wrote nothing); point at it for
how long the task took, its steps, the commands it ran and the tokens it used. When it wrote one file, the line names
it, and **Open** shows it in the files panel. When it made two files the panel can show, such as a chart and a
brief, a button opens each (**Open the chart**, or the file by name); otherwise **Open changes** opens the
**Changes** tab, and never both. After a test changed, **Run all the tests** puts that request in the message box for
you to send. The status
says **Done**; point at it for how long it took. A failed task says what went wrong and what to do
next, such as sending a message to try again. If it says the model spent its whole output limit thinking and wrote no
answer, send the message again: this is often a one-off. If it keeps happening, ask your administrator to raise the
route's **Max output** above its current limit in **Settings**, **Models**.

## Stop a run
While the agent works, the send button becomes **Stop**. Choose it to end the current run at once. You can send another
message afterwards.

You can also write your next message while the agent works: choose **Send after this** (or press Enter). It waits
until the agent finishes, and the message box lists it; if several wait, they go together as one message. You, or the
task's owner, can **Cancel** it until then. If the run fails or you stop it, a waiting message isn't sent: the message
box says so, with **Send again** and **Dismiss**. Everyone working in the task sees what waits, even after a reload or
from another browser. While the environment is starting, or while the task waits for a free slot, the message box says
why it can't take a message yet. Your message shows in the task with **Cancel** until it starts, also after a reload;
cancelling it takes it back and the task stops waiting. If it had just started, the page says "It had already
started." and the agent gets it.

## Change the model or the environment
The task's **Model** and **Environment** are small pickers in the task header, right of the title and the run status.
Each opens its choices below it. A new model applies at once. A new environment asks you to confirm the switch first,
just under the header; it keeps the task's files and history, and running processes in the old environment stop.

You can change them between runs, not during one. During a run a picker is muted and says what to do, for example
"Stop the current run before changing the model." A choice the task's agent profile makes is muted too, and reads
**Set by the agent profile …**.

## Notices in a task
Some events appear as a short notice in the transcript:

| Notice | Meaning |
|---|---|
| Switched to … (… unavailable) | The model was unavailable, so the agent continued on a fallback model your administrator set up |
| Earlier steps summarised | A long task was summarised so the model can keep working on it |
| Resumed after the environment restarted | The environment was replaced; the agent continued from where it was |
| Blocked network access to … | The agent tried to reach a host that is not on the installation's allow-list |
| The workspace is full. … | Delete files you no longer need, then continue |

When the model server has a brief problem, the agent tries again by itself. While it works, its steps say what
happened in plain words, for example that the model took too long to answer and the agent is trying again. Once the
task is done, a retry that worked is only counted in the steps' summary line (such as "the model was slow, retried
twice"); open the steps for any other reasons in words, with the technical codes after "Details:" for an
administrator. A problem that stopped the task is said below it.

## When the server restarts
If the installation's server restarts while you have a task open, the page says "Reconnecting to the task…" and
carries on by itself once the server is back: nothing is lost, and you don't need to reload. A step that was waiting
for the model's answer starts over, because that answer is lost with the restart. While the step works, it says
"The connection to the server dropped for … s, so the model was asked again and this step started over." Once the task
is done, its summary line counts it ("the connection dropped, retried once"). With a model that thinks for minutes,
those minutes are spent again; an administrator can see, before restarting, how many tasks would be affected.

## Running without a GPU
A local model on a computer without a suitable GPU works, slowly: the model server runs it on the processor. Measured on
a CPU-only Linux computer with the install's default small model (qwen3:4b on Ollama), each step of a task, one model
call, took 1½ to 5½ minutes: the model thinks before it answers, at about 7 to 12 tokens a second. A short first task
(write one file) took about 7 minutes.

- What works: single tasks, slowly. Keep them small and run one at a time: the model answers one request at a time,
  so tasks wait for each other.
- What doesn't: helpers, and the desktop. A lead task and its helpers need many calls, one after another, and a
  team's work takes hours. A desktop task needs a call for every look and action; its first look took over 4 minutes.
- The check says so: **Check** on such a route ends "Verified, but slowly", with the model's speed.

For helpers or the desktop, use a computer whose GPU the model server uses (on a Mac with Apple silicon, Ollama uses
its GPU), or a hosted model.

## Delete a task
Open the menu beside a task in the sidebar (the three dots), or the … menu in the task header, and choose **Delete task**.
It asks you to confirm, saying that the task is deleted for good and the files in its workspace stay. Deleting stops
its environment; from the header, the page then goes to a new task.

Deleting a task can't be undone. It is gone at once: a page still open on it says so, and its link no longer opens it.
Within a few minutes its conversation, its outputs and its saved changes are removed from the server too.

The workspace and its files stay, under the workspace's name, even for a task that made its own workspace when it
started. Delete the workspace separately if you no longer need it ([Workspaces](workspaces.md)).

What stays after a delete: posts the task made in a channel (they belong to the channel, shown as by "a deleted task"),
and the number of model tokens it used, which still counts towards your usage.
