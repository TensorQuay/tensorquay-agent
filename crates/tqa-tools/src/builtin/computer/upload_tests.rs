//! TOOL-U-062 (real-world use before 17.0, 17 Oct 2026): a page that asks for a file gets one of the workspace's files
//! through the `computer` tool, never through a native dialog; a path that resolves anywhere but inside the workspace
//! is refused before the browser sees it, so an upload never hands a site the runtime's token or the browser's profile.
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex as StdMutex},
};

use async_trait::async_trait;
use serde_json::json;

use super::{call, context, element, page, refusal, state};
use crate::{
    Tool,
    builtin::computer::{
        Computer, chosen_files,
        driver::{DriverError, ElementAct, FileInput, RawSnapshot, ScreenDriver, ScreenState},
    },
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

/// A page with a plain file input (e1), a styled Upload button that opens its chooser (e2), and a text field (e3).
#[derive(Default)]
struct Form {
    /// The chooser a click on the styled button opened, not yet taken.
    asked: StdMutex<Option<FileInput>>,
    done: StdMutex<Vec<String>>,
    multiple: bool,
}

impl Form {
    fn input(&self) -> FileInput {
        FileInput {
            handle: "node-1".into(),
            multiple: self.multiple,
        }
    }

    fn record(&self, what: String) {
        if let Ok(mut done) = self.done.lock() {
            done.push(what);
        }
    }

    fn done(&self) -> Vec<String> {
        self.done
            .lock()
            .map(|done| done.clone())
            .unwrap_or_default()
    }
}

#[async_trait]
impl ScreenDriver for Form {
    async fn snapshot(&self) -> Result<RawSnapshot, DriverError> {
        Ok(page(
            "w1",
            vec![
                element("button", "Choose File", "node-1"),
                element("button", "Upload", "node-2"),
                element("textbox", "Note", "node-3"),
            ],
        ))
    }

    async fn state(&self) -> Result<ScreenState, DriverError> {
        Ok(state("w1", 1))
    }

    async fn act(&self, handle: &str, act: ElementAct) -> Result<(), DriverError> {
        if handle == "node-2"
            && act == ElementAct::Click
            && let Ok(mut asked) = self.asked.lock()
        {
            *asked = Some(self.input());
        }
        self.record(format!("{handle} {act:?}"));
        Ok(())
    }

    async fn type_focused(&self, text: &str) -> Result<(), DriverError> {
        self.record(format!("focus {text}"));
        Ok(())
    }

    async fn key(&self, keys: &[String]) -> Result<(), DriverError> {
        self.record(format!("key {}", keys.join("+")));
        Ok(())
    }

    async fn take_file_chooser(&self) -> Result<Option<FileInput>, DriverError> {
        Ok(self.asked.lock().ok().and_then(|mut asked| asked.take()))
    }

    async fn file_input(&self, handle: &str) -> Result<Option<FileInput>, DriverError> {
        Ok((handle == "node-1").then(|| self.input()))
    }

    async fn choose_files(&self, input: &FileInput, files: &[PathBuf]) -> Result<(), DriverError> {
        let names: Vec<String> = files
            .iter()
            .map(|file| file.display().to_string())
            .collect();
        self.record(format!("choose {} {}", input.handle, names.join(",")));
        Ok(())
    }
}

/// A workspace with report.txt, notes/a.md, a folder, a link to report.txt and links out; beside it, a secret.
fn workspace() -> Result<(tempfile::TempDir, PathBuf), Box<dyn std::error::Error>> {
    let outer = tempfile::tempdir()?;
    let root = outer.path().join("workspace");
    std::fs::create_dir_all(root.join("notes"))?;
    std::fs::create_dir_all(root.join("folder"))?;
    std::fs::write(root.join("report.txt"), "Q3\n")?;
    std::fs::write(root.join("notes/a.md"), "# a\n")?;
    std::fs::write(outer.path().join("secret.token"), "not for a website")?;
    std::os::unix::fs::symlink(root.join("report.txt"), root.join("same.txt"))?;
    std::os::unix::fs::symlink(outer.path().join("secret.token"), root.join("escape.txt"))?;
    std::os::unix::fs::symlink(outer.path(), root.join("up"))?;
    let root = root.canonicalize()?;
    Ok((outer, root))
}

async fn tool_over(
    form: &Arc<Form>,
    root: &Path,
) -> Result<(Computer, crate::ToolContext), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let (mut cx, _) = context(&directory)?;
    cx.workspace = crate::WorkspaceRoot::open(root)?;
    let tool = Computer::new(form.clone());
    tool.call(call(&json!({"action": "observe"})), &cx).await?;
    Ok((tool, cx))
}

// TEST: TOOL-U-062
#[tokio::test]
async fn tool_u_062_a_page_that_asks_for_a_file_gets_a_workspace_file() -> TestResult {
    let (_outer, root) = workspace()?;
    let form = Arc::new(Form::default());
    let (tool, cx) = tool_over(&form, &root).await?;
    // The styled button opens the chooser: the click says the page asks for a file.
    let said = tool
        .call(call(&json!({"action": "click", "ref": "e2"})), &cx)
        .await?
        .text;
    assert!(
        said.starts_with("Clicked e2")
            && said.contains(
                "The page asks for a file: type the path of a file in /workspace, with no ref, to choose it."
            ),
        "{said}"
    );
    // A type with no ref then chooses the file, by its real path.
    let said = tool
        .call(call(&json!({"action": "type", "text": "report.txt"})), &cx)
        .await?
        .text;
    assert!(
        said.starts_with("Chose report.txt for the page's file input"),
        "{said}"
    );
    // By ref, the plain input takes a file with no chooser; a link inside the workspace is its target.
    let said = tool
        .call(
            call(&json!({"action": "type", "ref": "e1", "text": format!("{}/same.txt", root.display())})),
            &cx,
        )
        .await?
        .text;
    assert!(
        said.starts_with("Chose report.txt for the page's file input"),
        "{said}"
    );
    let report = root.join("report.txt").display().to_string();
    assert_eq!(
        form.done(),
        [
            "node-2 Click".to_owned(),
            format!("choose node-1 {report}"),
            format!("choose node-1 {report}"),
        ]
    );
    // The chooser was used once: the next type with no ref goes to the focus again.
    tool.call(call(&json!({"action": "type", "text": "hello"})), &cx)
        .await?;
    assert_eq!(form.done().last().map(String::as_str), Some("focus hello"));
    Ok(())
}

// TEST: TOOL-U-062
#[tokio::test]
async fn tool_u_062_another_action_puts_the_chooser_away() -> TestResult {
    let (_outer, root) = workspace()?;
    let form = Arc::new(Form::default());
    let (tool, cx) = tool_over(&form, &root).await?;
    tool.call(call(&json!({"action": "click", "ref": "e2"})), &cx)
        .await?;
    // Looking again keeps it; a key in between puts it away, so later text is typed, not taken as a path.
    tool.call(call(&json!({"action": "observe"})), &cx).await?;
    tool.call(call(&json!({"action": "key", "keys": "Escape"})), &cx)
        .await?;
    let said = tool
        .call(call(&json!({"action": "type", "text": "report.txt"})), &cx)
        .await?
        .text;
    assert!(
        said.starts_with("Typed 10 characters into the focus"),
        "{said}"
    );
    assert!(
        form.done().iter().all(|done| !done.starts_with("choose")),
        "{:?}",
        form.done()
    );
    // A text field by ref is typed into as before.
    tool.call(call(&json!({"action": "observe"})), &cx).await?;
    tool.call(
        call(&json!({"action": "type", "ref": "e3", "text": "hi"})),
        &cx,
    )
    .await?;
    assert_eq!(
        form.done().last().map(String::as_str),
        Some("node-3 Type(\"hi\")")
    );
    Ok(())
}

// TEST: TOOL-U-062
#[tokio::test]
async fn tool_u_062_only_a_file_inside_the_workspace_is_ever_chosen() -> TestResult {
    let (outer, root) = workspace()?;
    let form = Arc::new(Form::default());
    let (tool, cx) = tool_over(&form, &root).await?;
    let secret = outer.path().join("secret.token").display().to_string();
    let only = "invalid_args: /text: only a file in /workspace can be chosen";
    for (path, said) in [
        // Outside, by any spelling, and the runtime's own directories, whatever the workspace is.
        ("/etc/passwd", only.to_owned()),
        (secret.as_str(), only.to_owned()),
        ("../secret.token", format!("{only}, named without '..'")),
        ("notes/../report.txt", format!("{only}, named without '..'")),
        ("escape.txt", only.to_owned()),
        ("up/secret.token", only.to_owned()),
        ("/run/tqa/token", only.to_owned()),
        ("/run/tqa-browser/profile/Default/Cookies", only.to_owned()),
        // Inside, but not a file there.
        (
            "folder",
            "invalid_args: /text: folder is a folder, not a file".to_owned(),
        ),
        (
            "missing.txt",
            "invalid_args: /text: there's no file missing.txt in /workspace".to_owned(),
        ),
        // The input takes one.
        (
            "report.txt\nnotes/a.md",
            "invalid_args: /text: this input takes one file".to_owned(),
        ),
    ] {
        assert_eq!(
            refusal(
                &tool,
                &cx,
                json!({"action": "type", "ref": "e1", "text": path})
            )
            .await,
            said,
            "{path:?}"
        );
    }
    assert!(
        form.done().is_empty(),
        "nothing reached the page: {:?}",
        form.done()
    );
    Ok(())
}

// TEST: TOOL-U-062
#[tokio::test]
async fn tool_u_062_an_input_for_several_files_takes_one_per_line() -> TestResult {
    let (_outer, root) = workspace()?;
    let form = Arc::new(Form {
        multiple: true,
        ..Form::default()
    });
    let (tool, cx) = tool_over(&form, &root).await?;
    let said = tool
        .call(
            call(&json!({"action": "type", "ref": "e1", "text": "report.txt\n\nnotes/a.md\n"})),
            &cx,
        )
        .await?
        .text;
    assert!(
        said.starts_with("Chose report.txt and a.md for the page's file input"),
        "{said}"
    );
    assert_eq!(
        form.done(),
        [format!(
            "choose node-1 {},{}",
            root.join("report.txt").display(),
            root.join("notes/a.md").display()
        )]
    );
    let eleven = ["report.txt"; 11].join("\n");
    assert_eq!(
        refusal(
            &tool,
            &cx,
            json!({"action": "type", "ref": "e1", "text": eleven})
        )
        .await,
        "invalid_args: /text: at most 10 files, one per line"
    );
    Ok(())
}

// TEST: TOOL-U-062
#[test]
fn tool_u_062_the_rule_alone() -> TestResult {
    // The check the tool makes before any driver call, on its own: real paths, inside, files only.
    let (_outer, root) = workspace()?;
    let workspace = crate::WorkspaceRoot::open(&root)?;
    assert_eq!(
        chosen_files(&workspace, "./report.txt", false)?,
        [root.join("report.txt")]
    );
    assert!(chosen_files(&workspace, "escape.txt", false).is_err());
    assert!(chosen_files(&workspace, "", false).is_err(), "no path");
    assert!(chosen_files(&workspace, "report.txt\0", false).is_err());
    Ok(())
}
