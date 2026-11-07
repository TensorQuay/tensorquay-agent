# Files, changes and context

<!-- Tests: WEB-U-020, WEB-U-022, WEB-U-024, WEB-U-028, WEB-U-046, WEB-E2E-021. -->
Every task works on the files in its workspace. You can add files for the agent to use, browse what is there, and
review exactly what each run changed.

## Attach files
Choose **Attach files** (the + at the bottom left of the message box; on a new task, choose **Add**, then **Attach
files**), or drag files onto the message box and drop them.
Each file appears as a chip above your message while it uploads; **Remove** takes one off. You can attach up to
25 MiB per file and 32 files per message. Files are uploaded in the order you added them; you can send the message
once every upload has finished.

Attached files are saved in the workspace, so the agent can open them and they stay for later tasks on the same
workspace. If you attach images and the task's model accepts images, the agent sees up to four of them with your message; with
a model that does not, the agent gets only their paths in the workspace.

If an upload is refused, the chip says why:

| Message | What to do |
|---|---|
| Too large: up to 25 MiB per file | Attach a smaller file, or split it |
| The workspace upload space is full | Delete files you no longer need from the workspace |
| Files are not available in this environment | Switch to another environment (see [Tasks](tasks.md)) |
| Upload failed | Remove the file and attach it again |

## Browse the workspace
Where there is room (a window 1024 px wide or more with the sidebar collapsed, about 1210 px with it open) the files
panel sits beside the transcript, which narrows; with less room it takes the whole page under the task header until you
close it. It starts closed and opens only when you ask: the **Files** icon in the task header hides or shows it, and
on a wide screen the page remembers your choice. In a task with a desktop, the panel also has a **Screen** tab (see
[The desktop](desktop.md)). The **Files** tab lists the workspace's folders and files; select a folder to open it and a file to
preview it as what it is:

- An image (PNG, JPEG, GIF, WebP or SVG) shows as a picture, up to 10 MB.
- A Markdown document shows formatted; **Show the source** shows its text, and the panel remembers your choice.
- A CSV or TSV file shows as a table of its first 200 rows, with how many there are; **Show as text** shows it as text.
- A web page (HTML) shows as the page, up to 2 MB. Its scripts run in a sandbox, as the note above it says: they can't
  reach your session, the app or the network, so a page that loads files or data from elsewhere shows without them.
  **Reload** starts the page again. **Expand** opens a web page, a PDF or a spreadsheet wide, over the task; Escape
  closes it.
- A PDF (up to 20 MB) shows its first 20 pages, with how many there are. Its text can be selected and copied, and a
  screen reader reads it, page by page. Nothing in it can be clicked, and any script it contains never runs. A
  damaged or password-protected PDF says so. To get a PDF, ask for one: on the Workstation the agent writes the
  document as a web page and prints it to PDF, with no extra software to install.
- A spreadsheet (XLSX, XLSM, XLSB or ODS, up to 10 MB) shows one sheet at a time as a table of its first 500 rows and
  50 columns, with the saved results of its formulas; choose another sheet above the table (from 8 sheets, **Sheets** opens a list you can search). A damaged or
  password-protected spreadsheet says so. An old XLS file is not previewed.
- Any other text file shows with line numbers.

Another binary file is not previewed: choose **Download** to open it on your computer. A long file shows its
beginning, with a note saying how much is shown. A symbolic link is listed as **Link, not followed**. Hidden files, whose names start with a dot (such as
`.git`), are left out until you choose "Show hidden files (…)".

While the agent works, the panel notes that files may change. Choose **Refresh files** to load the current list.

## Review what a run changed
The **Changes** tab shows the files each run changed. Pick a run in the **Run** list; the newest is marked
**Latest** and is shown by default, and the view moves to a newer run when one starts.

Each changed file is marked **Created**, **Modified**, **Updated** (a changed image) or **Deleted**. Select a file the
run created, or an image it changed, to see it as it is in the **Files** tab; select a text file it modified to see its
diff, with added and removed lines; each part says where it is in the file, such as "Lines 12–18". A binary file, or one too large to compare, is listed with a note instead of a
diff.

Some files are left out on purpose:

- Files you upload are not listed as changes, even ones you add after a run ends. If a run edits or deletes one of
  your uploaded files, that is listed like any other change.
- Caches, such as `__pycache__`, are not listed.
- In a very large workspace, some files may not be compared; the tab says so.

The summary at the end of each run also says how many files it changed.

## See what the agent loaded
<!-- Tests: WEB-U-046. -->
Once a run has loaded instruction files or skills, a **Context** tab appears next to **Files** and **Changes**. It shows
what the latest run loaded into the agent's context, and the time that run started:

| Section | What it lists |
|---|---|
| **Instructions** | The files the agent read before it started (such as `AGENTS.md`), in order. A file marked **Shortened** was too long: only its start was given to the agent |
| **Skills** | The skills the agent can use, each with its description; **For everyone** marks skills your administrator installed, **Yours** marks your own |
| **Not loaded** | Skill folders that were left out. An administrator also sees each one's reason, for example "No SKILL.md in the folder." |

A skill of your own with the same name as an installed one is used instead of it. Before the first run, and after a
run that loaded nothing, there is no **Context** tab.

The file and skill names come from the workspace and are shown as plain text.
