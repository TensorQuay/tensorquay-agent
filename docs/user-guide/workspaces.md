# Workspaces

<!-- Tests: WEB-U-025, WEB-U-026, WEB-U-073, WEB-E2E-011. -->
A workspace keeps a task's files. It outlives the task: you can start new tasks on the same files, make a copy to try
something else, or share it with other people. Choose **Workspaces** in the sidebar to see yours.

## The Workspaces page
The page lists your workspaces and those shared with you, the most recently used first, 25 at a time: **Show more**
adds the next 25. To find one:

- **Search workspaces** finds the names that contain what you type;
- **All**, **In use**, **Idle** and **Shared with me** narrow the list;
- **Sort** orders it by **Recently used** or by **Name**.

The page keeps your filter and sort in its address, so a reload or the browser's Back button keeps them. On a laptop
the workspaces are a table; on a phone, cards. Each workspace shows:

- its state: **In use** (with the task using it), or **Idle** with when it was last used;
- its size against its quota, marked **almost full** when it nears the limit;
- for a shared workspace, the owner and your access (**Can edit** or **Can view**);
- a **Copy** badge on a copy, naming the workspace it was copied from.

**New workspace** creates an empty workspace with the name you give it. When a task first gets its workspace's files,
it shows the progress ("Getting the workspace's files…", then "Got the workspace's files").

## Work on a workspace
Choose a workspace's row, or press Enter on it, to open it; the other actions are in its menu (the three dots). On a
phone each workspace is a card with an **Open** button.

| Action | What it does |
|---|---|
| **Open** | Shows only this workspace's tasks in the sidebar ("Workspace: …"); remove the filter to see all tasks again |
| **New task here** | Starts a new task on this workspace; not on one you can only view |
| **Make a copy** | Copies the files into a new workspace you own; tasks in the copy do not change the original |
| **Share** | Gives other people access (see [Sharing](sharing.md)); owners only |
| **Rename** | Makes the name editable where it is: type the new one (1 to 100 characters) and press Enter, or Escape to keep the old one; owners only |
| **Delete** | Deletes the workspace; owners only (see below) |

You can also pick a workspace when you start a task: **Add** (the + in the message box), then **Workspace…**, opens a
list of yours and those shared with you; a workspace you can only view, or a copy still being made, is listed but
cannot be picked. Only one task works on a workspace at a time. If you pick one that is in use, a line under its chip
says "In use: your task will wait for it.", and your run starts when the other task's run ends. Once it has started,
the task says which workspace it is in on the quiet line under its title, such as "In Team handbook".

A copy is ready once its files are copied; until then it shows **being copied** and cannot be used. After making a
copy, **Start a task in the copy** in the confirmation takes you straight to a new task on it.

## Delete a workspace
**Delete** removes the workspace's files and all your tasks on it, after you confirm. The confirmation says how many
tasks go with it. A workspace cannot be deleted while:

- a task is using it;
- other people have tasks on it;
- it is still being copied.

The page says which of these applies. Deleting a workspace cannot be undone; make a copy first if you want to keep a
copy. Its files go at once; its tasks, their conversations and the workspace's saved changes are removed from the server
within a few minutes. Copies made from it stay: each is a workspace of its own.
