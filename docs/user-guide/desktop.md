# The desktop

<!-- Tests: WEB-U-241, WEB-U-242, WEB-U-243, WEB-U-244, WEB-U-245, WEB-U-246, WEB-U-247, WEB-U-248, WEB-U-259, WEB-E2E-037. -->
Some work can only be done on a screen: a form that works only in a real browser window, a site you need to sign in to,
or an application with no command line. For this kind of work, a task can run on a desktop. This is a screen with a
browser, a terminal, a file manager and an office suite. The agent looks at the screen and clicks, types and scrolls on
it. You can watch it work, step in to take control, and replay what it did afterwards.

## When a task gets a desktop
A task has a desktop when it runs on an environment with the **Desktop** tool, such as the ready-made **Desktop**
environment. Choose it in the task's environment picker, like any other environment (see [Tasks](tasks.md)). An
environment without a desktop starts faster and uses less memory, so most tasks don't need one. Your administrator
sets how many desktops can run at once; when they are all in use, a task says "Waiting for a free desktop" and, with
others waiting before it, how many ("· 2 ahead").

The switch is always yours to make: the agent doesn't suggest it. The task keeps its files and history when it
switches, and your next message carries on. If no desktop environment is offered, ask an administrator: an
administrator adds one when the install's images are built (`deploy/build-images.sh --desktop`).

## Watch the screen
In a task with a desktop, the **Screen** button in the task header shows the screen in the panel beside the
transcript. The **Screen** tab sits next to **Files** and **Changes**. The first time the agent acts on the desktop, the
panel opens on the **Screen** tab by itself, once per task. If you close it, it stays closed for that task until you
open it again. While the agent is acting on the desktop, the **Screen** button shows a dot and its tip says "Screen:
the agent is using the desktop", so you can see it start with the panel closed.

The screen is live. A line above it says what is happening, such as "Watching. The agent is working." **Expand**
opens the screen wide, over the task, with the same line and buttons, so you can take over and sign in there; Escape
closes it. While you have control and the keyboard is in the screen, Escape goes to the desktop: press it twice to
give the keyboard back to the page, then once more to close. Other things the panel may say:

- "The desktop isn't running. It starts when you send a message." The desktop runs only while its environment is up.
  When the environment isn't running, the panel shows the replay instead (see below).
- "This desktop is open in four tabs. Close one to watch it here." You can watch the same desktop from up to four tabs.
- **Reconnecting to the desktop…** after the connection drops: the panel connects again by itself.
- "The screen isn't available. Trying again…" when the desktop's picture can't reach the panel; the panel keeps
  trying. After two minutes it says "The screen isn't available. Still trying. The agent's work on the desktop isn't
  affected.": the agent reads the screen inside its environment, not through this picture.
- "This task's environment has no desktop.", "Only the task's owner can see its screen." or, for a task that is gone
  or a wrong link, "This task's screen can't be opened." when the screen can't be opened at all.

Only you see your task's screen. A person you share a workspace with, a channel's members and administrators never
do.

## Take over and Hand back
While you watch, the screen is view-only: your clicks and keys don't reach it. To use the desktop yourself, for
example to sign in to a site, choose **Take over**. This stops the agent's current run, as **Stop** does, so the agent
never moves the pointer under your hand; while the agent works, the panel says so under the button: "Taking over stops
the agent. After you hand back, send a message to let it carry on." The panel says "Stopping the agent before you take over…" until the run has stopped and
the desktop's browser has been reset, which takes a few seconds at most; then you have control, and a sign-in lasts
for the agent's next run. It says "You have control. When you're done, hand back, then send a message to let the agent
carry on." If the run takes too long to stop, you get control anyway, and the panel says "You have control. The agent
may still be stopping, so a sign-in now may not last: wait until this line changes." Once the run has stopped and the
browser has been reset, the line changes to the usual one, and a sign-in lasts. If Take over gets no answer at all, the panel says "Take over
didn't go through. Try again." If the desktop couldn't clear the browser when the last run ended, the panel adds
"Earlier sign-ins may still be in this browser. Clear them before you sign in.", with **Clear sign-ins** beside it (also in the ⋯ menu); clear them first. Keys go to the desktop while the screen has focus. Press Escape twice to move focus out of the screen again.

While you have control, the agent doesn't work. A message you send waits until you hand back, as a message waits
behind a run, and you can **Cancel** it until then; it reads "Sends when you hand back the desktop". When you're done,
choose **Hand back**. Handing back doesn't start the agent by itself: your next message does (or the one waiting), and
the agent sees the screen as you left it. Until then the panel says "You handed back the desktop. Send a message to let
the agent carry on." The transcript notes each change, for example "You took over
the desktop." and how long you had control before you handed it back. It never records what you typed. Only one tab
has control at a time; the others say "Another tab of yours has control. Hand back there, or close that tab."

You can paste into the desktop while you have control, such as an address or a password from your
own password manager. Nothing is copied out of it: what you copy on the desktop stays there.

After you paste into the desktop, LibreOffice may now and then miss a double-click, for example on a sheet's tab,
and may be slow to close. This is a known issue in this version, and a fix is coming. If a double-click by the agent
doesn't open what it should, the agent sees on the screen that nothing opened and can try again; if you have
control, double-click again.

## Sign-ins
If the agent needs to work in an account, take over and sign in yourself. Never give the agent a password in a
message. A sign-in made during Take over lasts for the agent's next run only, as the panel says while you have
control. When that run ends, the browser forgets it, so every later run starts signed out. A new environment, another task on the same workspace, and anyone you share the
workspace with all start signed out too.

To end a sign-in at once, open the panel's ⋯ menu and choose **Clear the browser's sign-ins**, then **Clear sign-ins**
to confirm. This empties the desktop browser's sign-ins, cookies and site data. It doesn't sign you out of applications
on the desktop, which keep their own. It is available only when the agent isn't working, and only while a sign-in can
remain: once a run has ended since your last Take over, the menu says "The browser is already signed out: each run
ends that way." The panel says
**Clearing the browser's sign-ins…** until the desktop confirms, then "The desktop browser's sign-ins were cleared."
If the browser couldn't be cleared, or the desktop didn't confirm in time, the panel says so: "The browser's sign-ins
couldn't be cleared, so they may remain. Try again." or "Clearing the browser's sign-ins wasn't confirmed, so they may
remain. Try again." The transcript notes the outcome too; under the panel, the line goes once a run starts or at your
next Take over.

## What the agent did, step by step
Each thing the agent does on the desktop is one line in the transcript, in plain words, such as "Clicked “Save”" or
"Pressed ctrl+s", naming the element it acted on. Open the line to see the screen **Before** and **After** the action.
When the agent looked at the screen, the line shows **What the model saw**. On such a picture, "The numbers mark the
elements the agent could act on." Choose a picture to open it wide.

## Replay
The panel's **Replay** shows a run's desktop actions one picture at a time, after the run, or while the desktop runs
by choosing **Replay** instead of **Live**. It starts at the latest run's last step. **Run** picks another run, named by
its number, when it started and how many steps it has. **Previous step**, **Next step** and the **Step** slider move
through the run, and the line above the picture says which step it is, such as "Step 12 of 38", and what the agent
did. Only the desktop's actions count as steps; commands and file edits are left out. A step with no picture of its
own, such as a wait, shows "No new picture at this step: the screen as last seen."

## What is kept, and for how long
The task keeps a picture before and after each desktop action, and what the model saw when it looked. They are kept
with the task and count toward its storage, and only you can see them. Your administrator decides how many days they
are kept (30 by default). After that, a picture says "This screenshot isn't available any more." The transcript and
its lines stay. A picture the desktop couldn't keep, for example because it was too large or the task's storage was
full, says "This picture couldn't be saved." The run carries on regardless. Deleting the task deletes its pictures with it. The live screen
itself is never recorded.

## What the desktop can reach
The desktop reaches exactly the internet sites any task of yours may reach: the ones your administrator allows. An
application on the desktop gets no other way out. Files the agent saves on the desktop go to the task's workspace,
where the **Files** panel shows them, and downloads go to its `Downloads` folder.
