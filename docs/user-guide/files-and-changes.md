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

Under a sent message, an image shows as its picture and any other file as its name; choose one to open its preview
in the Files panel. Attached files are saved in the workspace, so the agent can open them and they stay for later
tasks on the same
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
- The agent reads a PDF in the workspace page by page with its usual file tool, and a model that reads pictures is
  shown the pages it just read; you don't need to convert a PDF first.
- A Markdown document shows formatted; **Show the source** shows its text, and the panel remembers your choice.
- A CSV or TSV file shows as a table of its first 200 rows, with how many there are; **Show as text** shows it as text.
- A web page (HTML, up to 2 MB) shows as the page, with the style sheets, scripts, pictures and fonts beside it in
  its folder (see [A web page with its folder](#a-web-page-with-its-folder)). Its scripts run in a sandbox, as the
  line above it says. **Reload** starts the page again. **Expand** opens a web page, a PDF or a spreadsheet wide, over
  the task; Escape closes it.
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

### A web page with its folder
<!-- Tests: WEB-U-470, WEB-E2E-122, GW-I-321. -->
Most pages the agent writes are not one file: an `index.html` has its style sheets, scripts, pictures, fonts and
other pages beside it, and a built app has its bundles and its own routes. The preview shows a page from its folder,
served as a small site, so all of that loads as the page expects:

- Everything the page names by a relative address (a style sheet, a script, a picture, a font, another page in the
  same folder or in one under it) is read from the workspace as it is now. A link to another page of the site opens in
  the preview; **Reload** starts the page you chose again.
- The page you choose is the one shown, whatever its name: a `report.html` beside other files opens as itself. The
  folder's `index.html` also answers for the folder and for any address that names no file, such as a built app's own
  routes (`/about`), so such an app works in the preview.
- Scripts run, in a sandbox: they can't reach your session, the app, this task or another. The line above the page
  says "Scripts are running in a sandbox". **Stop scripts** reloads the page without any script, for a page that is
  busy or won't stay still; the line then reads "Scripts are stopped", and **Run scripts** brings them back.
- What the page loads from the internet follows the installation's network setting: the sites tasks may reach, which
  your administrator sets under Settings › Internet access. The browser refuses any other request before it leaves. When
  the page names such sites, a line says so, "Blocked by the network setting: …", naming up to three of them and how
  many more. What a script asks for only while it runs is refused the same way, though the line can't name it. When
  the open web is allowed, nothing is blocked and there is no line.
- The folder is served under a pass minted when the preview opens: for one folder of one task, read-only, and only for
  someone who can open the task. The pass is in the preview's address and is sent without your session, so a copied
  address works on its own until the pass expires: don't share it. It lasts 15 minutes from its last use, the preview
  takes a new one when it needs it, and a restart of the gateway forgets them all; a task has at most 8 live passes at a
  time.

If the folder can't be served, for example at that limit, the page is shown on its own and offline, as earlier
versions showed every page: a page that loads from elsewhere then says "Previews run offline: what this page loads from
… won't show here. Download it to see it all.", and a link in it doesn't open ("Links in a preview don't open here.
Reload to see the page again.").

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
