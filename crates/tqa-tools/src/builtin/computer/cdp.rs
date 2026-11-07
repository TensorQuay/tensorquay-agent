//! The browser driver (ADR-0039 slice 1): a headed Chromium the runtime starts with a debugging port on loopback,
//! driven over the `DevTools` protocol. A snapshot is the page's accessibility tree (its actionable nodes); an element's
//! own action is its DOM `click()`, `focus()` or `scrollBy()`, with `DevTools` input events at the element's centre as
//! the fallback. AT-SPI and XTEST for other applications come in slice 2.
pub mod boxes;
mod client;
mod keys;
#[cfg(test)]
mod tests;

use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    time::Duration,
};

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::{process::Child, sync::Mutex};

pub use self::client::Client;
use super::driver::{
    Direction, DriverError, ElementAct, FileInput, RawElement, RawSnapshot, ScreenDriver,
    ScreenState, Source,
};
use super::snapshot::shown_name;

/// The roles a snapshot lists: what a person can act on, and headings to find one's way.
pub(crate) const ACTIONABLE: [&str; 18] = [
    "button",
    "link",
    "textbox",
    "searchbox",
    "checkbox",
    "radio",
    "combobox",
    "listbox",
    "option",
    "menuitem",
    "menuitemcheckbox",
    "menuitemradio",
    "tab",
    "switch",
    "slider",
    "spinbutton",
    "treeitem",
    "heading",
];
/// The accessibility states a snapshot says.
const STATES: [&str; 7] = [
    "focused", "checked", "selected", "disabled", "expanded", "pressed", "required",
];
/// Pixels per scroll notch.
const NOTCH_PIXELS: i64 = 100;
/// How long the browser has to write its port file.
const START_BOUND: Duration = Duration::from_secs(15);

/// The headed Chromium the runtime started, and the page it acts on.
pub struct Browser {
    client: Client,
    /// The page target and its attached session, chosen afresh for each call.
    page: Mutex<Option<(String, String)>>,
    process: Option<Child>,
}

impl Browser {
    /// Starts `binary` (Debian's `chromium`) on the display the environment names, with its profile in `profile`
    /// ([`profile_dir`] in a sandbox, ADR-0039 sections 6 and 10), and connects to its debugging port.
    ///
    /// # Errors
    /// `Unavailable` when the browser doesn't start or doesn't open its port in time.
    pub async fn launch(binary: &Path, profile: &Path) -> Result<Self, DriverError> {
        Self::launch_at(binary, profile, "about:blank").await
    }

    /// As [`Self::launch`], opening `url` first (RT-I-046's probe opens its own page).
    ///
    /// # Errors
    /// As [`Self::launch`].
    pub async fn launch_at(binary: &Path, profile: &Path, url: &str) -> Result<Self, DriverError> {
        let mut args = launch_args(profile);
        if let Some(first) = args.last_mut() {
            url.clone_into(first);
        }
        // The leader of a group of its own: the runtime records it so its reaper leaves the browser alone, and a
        // group of its own is what the reaper spares (ADR-0039 section 10).
        let process = tokio::process::Command::new(binary)
            .args(args)
            .process_group(0)
            .kill_on_drop(true)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|error| {
                DriverError::Unavailable(format!("the browser didn't start ({})", error.kind()))
            })?;
        let endpoint = endpoint(&profile.join("DevToolsActivePort"), START_BOUND).await?;
        let client = Client::connect(&endpoint).await?;
        Ok(Self {
            client,
            page: Mutex::new(None),
            process: Some(process),
        })
    }

    /// The browser's process: its windows carry it as `_NET_WM_PID`, which is how the desktop driver tells them from
    /// other applications' windows. `None` for a driver over a connection it didn't start.
    #[must_use]
    pub fn pid(&self) -> Option<u32> {
        self.process.as_ref().and_then(Child::id)
    }

    /// Opens `url` in the focused page, as a person's address bar would (the grounding evaluation's capture loads its
    /// pages this way; the model has no such action).
    ///
    /// # Errors
    /// `Unavailable` when the browser doesn't answer.
    pub async fn navigate(&self, url: &str) -> Result<(), DriverError> {
        let (_, session) = self.page().await?;
        self.client
            .call("Page.navigate", json!({"url": url}), Some(&session))
            .await
            .map(|_| ())
    }

    /// Whether the browser answers a person's keys: its window, asked through the browser's own process, is shown (any
    /// state but minimised: a person may have maximised it), and its page has loaded (`about:blank` counts). Both are round trips through the browser's UI
    /// and its renderer, which can lag a window that is already mapped and focused (the 16v integration's
    /// `sec_059_a`: a ready reset, plain control, and keys that reached no page). A reopened browser is ready only
    /// once this is true (RT-I-057).
    ///
    /// # Errors
    /// `Unavailable` when the browser doesn't answer.
    pub async fn answers(&self) -> Result<bool, DriverError> {
        let (target, session) = self.page().await?;
        let window = self
            .client
            .call(
                "Browser.getWindowForTarget",
                json!({"targetId": target}),
                None,
            )
            .await?;
        if window["bounds"]["windowState"] == "minimized" {
            return Ok(false);
        }
        let state = self
            .client
            .call(
                "Runtime.evaluate",
                json!({"expression": "document.readyState", "returnByValue": true}),
                Some(&session),
            )
            .await?;
        Ok(state["result"]["value"] == "complete")
    }

    /// A driver over an already open debugging connection (the tests' fake browser).
    #[must_use]
    pub fn over(client: Client) -> Self {
        Self {
            client,
            page: Mutex::new(None),
            process: None,
        }
    }

    /// The focused page and its session: the page that opened last (a new tab or window takes the focus), attached
    /// once. Which one opened last is the client's record ([`Client::pages`]), never `Target.getTargets`' order, which
    /// is no promise: Chromium 155 listed a page's new window first, so the last listed was the page that opened it
    /// (c17's integration, RT-I-046). A page not in the record yet opened after it; the pages open when the client
    /// connected come in no order, so of those, as of two pages not in the record, the later listed.
    async fn page(&self) -> Result<(String, String), DriverError> {
        let targets = self
            .client
            .call("Target.getTargets", json!({}), None)
            .await?;
        let pages = self.client.pages();
        let target = targets["targetInfos"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|info| info["type"] == "page")
            .filter_map(|info| info["targetId"].as_str())
            .enumerate()
            .max_by_key(|(listed, id)| (pages.rank(id), *listed))
            .map(|(_, id)| id.to_owned())
            .ok_or_else(|| DriverError::Unavailable("the browser has no page".into()))?;
        let mut page = self.page.lock().await;
        if let Some((known, session)) = page.as_ref()
            && *known == target
        {
            return Ok((target, session.clone()));
        }
        let attached = self
            .client
            .call(
                "Target.attachToTarget",
                json!({"targetId": target, "flatten": true}),
                None,
            )
            .await?;
        let session = attached["sessionId"]
            .as_str()
            .ok_or_else(|| DriverError::Unavailable("the page wouldn't attach".into()))?
            .to_owned();
        self.client
            .call("Page.enable", json!({}), Some(&session))
            .await?;
        // A file input's chooser is the driver's to answer, never GTK's native dialog, which the tool can't reach
        // (c16k real-world A12b, TOOL-U-062).
        self.client
            .call(
                "Page.setInterceptFileChooserDialog",
                json!({"enabled": true}),
                Some(&session),
            )
            .await?;
        *page = Some((target.clone(), session.clone()));
        Ok((target, session))
    }

    /// Every laid-out node's box on the screen, by backend node id, from one DOM snapshot and the window's metrics.
    async fn boxes(
        &self,
        session: &str,
    ) -> Result<std::collections::HashMap<i64, super::raster::Rect>, DriverError> {
        let metrics = self.evaluate(session, boxes::METRICS).await?;
        let viewport = boxes::Viewport::from_metrics(&metrics)
            .ok_or_else(|| DriverError::Failed("the window's metrics are unreadable".into()))?;
        let snapshot = self
            .client
            .call(
                "DOMSnapshot.captureSnapshot",
                json!({"computedStyles": []}),
                Some(session),
            )
            .await?;
        Ok(boxes::boxes(&snapshot, &viewport))
    }

    async fn evaluate(&self, session: &str, expression: &str) -> Result<Value, DriverError> {
        let answer = self
            .client
            .call(
                "Runtime.evaluate",
                json!({"expression": expression, "returnByValue": true}),
                Some(session),
            )
            .await?;
        Ok(answer["result"]["value"].clone())
    }

    /// The element's remote object, or `Gone` when the node has left the page.
    async fn object(&self, session: &str, handle: &str) -> Result<String, DriverError> {
        let node: i64 = handle.parse().map_err(|_| DriverError::Gone)?;
        let resolved = self
            .client
            .call(
                "DOM.resolveNode",
                json!({"backendNodeId": node}),
                Some(session),
            )
            .await
            .map_err(|_| DriverError::Gone)?;
        resolved["object"]["objectId"]
            .as_str()
            .map(str::to_owned)
            .ok_or(DriverError::Gone)
    }

    async fn call_on(
        &self,
        session: &str,
        object: &str,
        function: &str,
        args: Value,
    ) -> Result<Value, DriverError> {
        let call = json!({"objectId": object, "functionDeclaration": function, "arguments": args, "returnByValue": true});
        self.function_on(session, call).await
    }

    /// `function` called on the element as a person's input, so the page has the user activation that a file input's
    /// chooser (or a popup) needs.
    async fn call_as_person(
        &self,
        session: &str,
        object: &str,
        function: &str,
    ) -> Result<Value, DriverError> {
        let call = json!({"objectId": object, "functionDeclaration": function, "arguments": [], "returnByValue": true,
                          "userGesture": true});
        self.function_on(session, call).await
    }

    async fn function_on(&self, session: &str, call: Value) -> Result<Value, DriverError> {
        let answer = self
            .client
            .call("Runtime.callFunctionOn", call, Some(session))
            .await?;
        if answer.get("exceptionDetails").is_some() {
            return Err(DriverError::Failed("the page refused the action".into()));
        }
        Ok(answer["result"]["value"].clone())
    }

    /// The element's centre in the page's CSS pixels, after scrolling it into view.
    async fn centre(&self, session: &str, handle: &str) -> Result<(f64, f64), DriverError> {
        let node: i64 = handle.parse().map_err(|_| DriverError::Gone)?;
        let _ = self
            .client
            .call(
                "DOM.scrollIntoViewIfNeeded",
                json!({"backendNodeId": node}),
                Some(session),
            )
            .await;
        let model = self
            .client
            .call(
                "DOM.getBoxModel",
                json!({"backendNodeId": node}),
                Some(session),
            )
            .await
            .map_err(|_| DriverError::Failed("the element has no box on the page".into()))?;
        let quad: Vec<f64> = model["model"]["content"]
            .as_array()
            .map(|points| points.iter().filter_map(Value::as_f64).collect())
            .unwrap_or_default();
        if quad.len() != 8 {
            return Err(DriverError::Failed(
                "the element has no box on the page".into(),
            ));
        }
        let x = (quad[0] + quad[2] + quad[4] + quad[6]) / 4.0;
        let y = (quad[1] + quad[3] + quad[5] + quad[7]) / 4.0;
        Ok((x, y))
    }

    async fn mouse(
        &self,
        session: &str,
        (x, y): (f64, f64),
        clicks: u8,
    ) -> Result<(), DriverError> {
        for kind in ["mousePressed", "mouseReleased"] {
            let event =
                json!({"type": kind, "x": x, "y": y, "button": "left", "clickCount": clicks});
            self.client
                .call("Input.dispatchMouseEvent", event, Some(session))
                .await?;
        }
        Ok(())
    }

    async fn click(&self, session: &str, handle: &str, clicks: u8) -> Result<(), DriverError> {
        if clicks == 1 {
            let object = self.object(session, handle).await?;
            // On the page's next tick, so a click that opens a dialog doesn't hold this call while the dialog waits. A
            // person's gesture, as the model's click stands for one: Chromium opens a file input's chooser only after
            // a user activation, and a styled button's handler that clicks a hidden input opened nothing without it
            // (TOOL-U-062).
            let own = "function(){if(this.disabled){return 'disabled';}if(this.focus){this.focus();}\
                       if(typeof this.click==='function'){const el=this;setTimeout(()=>el.click(),0);\
                       return 'clicked';}return 'none';}";
            match self.call_as_person(session, &object, own).await?.as_str() {
                Some("clicked") => return Ok(()),
                Some("disabled") => {
                    return Err(DriverError::Failed("the element is disabled".into()));
                }
                _ => {}
            }
        } else {
            // Present a double click as the page sees a person's: no DOM method makes one.
            self.object(session, handle).await?;
        }
        let centre = self.centre(session, handle).await?;
        self.mouse(session, centre, clicks).await
    }

    async fn scroll(
        &self,
        session: &str,
        handle: &str,
        direction: Direction,
        notches: u8,
    ) -> Result<(), DriverError> {
        let distance = NOTCH_PIXELS * i64::from(notches);
        let (dx, dy) = match direction {
            Direction::Up => (0, -distance),
            Direction::Down => (0, distance),
            Direction::Left => (-distance, 0),
            Direction::Right => (distance, 0),
        };
        let object = self.object(session, handle).await?;
        let own = "function(dx,dy){const before=[this.scrollLeft,this.scrollTop];this.scrollBy(dx,dy);\
                   return before[0]!==this.scrollLeft||before[1]!==this.scrollTop;}";
        let args = json!([{"value": dx}, {"value": dy}]);
        if self.call_on(session, &object, own, args).await? == Value::Bool(true) {
            return Ok(());
        }
        // Not scrollable itself: the wheel over it scrolls whatever is.
        let (x, y) = self.centre(session, handle).await?;
        let wheel = json!({"type": "mouseWheel", "x": x, "y": y, "deltaX": dx, "deltaY": dy});
        self.client
            .call("Input.dispatchMouseEvent", wheel, Some(session))
            .await
            .map(|_| ())
    }

    async fn answer_dialog(&self, session: &str, accept: bool) -> Result<(), DriverError> {
        self.client
            .call(
                "Page.handleJavaScriptDialog",
                json!({"accept": accept}),
                Some(session),
            )
            .await
            .map(|_| ())
    }

    /// Types `text` into the element: focused and its text selected, then as inserted text, or as key events where
    /// inserted text can't land (TOOL-U-059: Chromium's date and time segments ignore it). Text that leaves the field
    /// showing what it showed before is `Failed`, never a quiet success.
    async fn type_into(&self, session: &str, handle: &str, text: &str) -> Result<(), DriverError> {
        let object = self.object(session, handle).await?;
        let ready_fn = format!(
            "function(text){{if(this.disabled||this.readOnly){{return null;}}this.focus();\
             if(typeof this.select==='function'){{try{{this.select();}}catch(e){{}}}}\
             const host=this.getRootNode&&this.getRootNode().host;\
             return {{segmented:{DATED}(this)||{DATED}(host),same:('value' in this)&&this.value===text,\
             shown:({SHOWN}).call(this)}};}}"
        );
        let ready = self
            .call_on(session, &object, &ready_fn, json!([{"value": text}]))
            .await?;
        let Some(segmented) = ready["segmented"].as_bool() else {
            return Err(DriverError::Failed("the element doesn't take text".into()));
        };
        let field = Field::Element(&object);
        let start = Start {
            segmented,
            same: ready["same"] == true,
            before: self
                .reading(session, &field, &ready["shown"], segmented)
                .await?,
        };
        self.land(session, &field, text, start).await
    }

    /// What a field shows now: its own reading and, for a date or time field, the focused segment's value from the
    /// accessibility tree (the input's own value stays empty until every segment is filled).
    async fn reading(
        &self,
        session: &str,
        field: &Field<'_>,
        shown: &Value,
        segmented: bool,
    ) -> Result<(Value, Option<String>), DriverError> {
        let shown = if shown.is_null() {
            match field {
                Field::Element(object) => {
                    self.call_on(session, object, SHOWN, json!([])).await?
                }
                Field::Focus => {
                    self.evaluate(session, &format!("(()=>{{const a=document.activeElement;return a?({SHOWN}).call(a):'[]';}})()"))
                        .await?
                }
            }
        } else {
            shown.clone()
        };
        let segment = if segmented {
            self.focused_value(session).await?
        } else {
            None
        };
        Ok((shown, segment))
    }

    /// The focused node's value and name in the accessibility tree.
    async fn focused_value(&self, session: &str) -> Result<Option<String>, DriverError> {
        let tree = self
            .client
            .call("Accessibility.getFullAXTree", json!({}), Some(session))
            .await?;
        Ok(tree["nodes"].as_array().and_then(|nodes| {
            nodes
                .iter()
                .find(|node| states(node).iter().any(|state| state == "focused"))
                .map(|node| format!("{}={}", node["name"]["value"], node["value"]["value"]))
        }))
    }

    /// Sends `text` to the focused field and checks that it landed: inserted text first unless the field is a date or
    /// time segment, then key events if the field still shows what it did; the same reading after both is `Failed`.
    async fn land(
        &self,
        session: &str,
        field: &Field<'_>,
        text: &str,
        start: Start,
    ) -> Result<(), DriverError> {
        let Start {
            segmented,
            same,
            before,
        } = start;
        if !segmented {
            self.client
                .call("Input.insertText", json!({"text": text}), Some(session))
                .await?;
            if text.is_empty() || same {
                return Ok(());
            }
            if self.reading(session, field, &Value::Null, false).await? != before {
                return Ok(());
            }
        }
        for event in keys::typed(text) {
            self.client
                .call("Input.dispatchKeyEvent", event, Some(session))
                .await?;
        }
        if self
            .reading(session, field, &Value::Null, segmented)
            .await?
            != before
        {
            return Ok(());
        }
        Err(DriverError::Failed(format!(
            "typed {} characters but the field didn't change: it may not take typing here; observe it, or click \
             the part to fill and use key",
            text.chars().count()
        )))
    }
}

/// A field as a `type` found it: whether it is a date or time segment, whether it already holds the text, and what it
/// showed.
struct Start {
    segmented: bool,
    same: bool,
    before: (Value, Option<String>),
}

/// The field a `type` goes to: an element by its remote object, or whatever has the focus.
enum Field<'a> {
    Element(&'a str),
    Focus,
}

/// A page function: whether an element is a date or time input (whose segments ignore inserted text).
const DATED: &str = "(e=>!!e&&e.tagName==='INPUT'&&['date','time','datetime-local','month','week'].includes(e.type))";
/// A page function on an element: what it shows, to tell whether typing landed (its value, its spoken value, its text,
/// and its date or time input's value when it is one of the input's segments).
const SHOWN: &str = "function(){const host=this.getRootNode&&this.getRootNode().host;\
                     return JSON.stringify([('value' in this)?String(this.value):null,\
                     this.getAttribute?this.getAttribute('aria-valuetext'):null,(this.textContent||'').slice(0,500),\
                     host&&('value' in host)?String(host.value):null]);}";

#[async_trait]
impl ScreenDriver for Browser {
    async fn snapshot(&self) -> Result<RawSnapshot, DriverError> {
        let (target, session) = self.page().await?;
        if let Some(message) = self.client.dialog() {
            // The page's script is paused while a dialog is open; its buttons are the only things to act on.
            return Ok(dialog_snapshot(&target, &message));
        }
        let tree = self
            .client
            .call("Accessibility.getFullAXTree", json!({}), Some(&session))
            .await?;
        // A dialog whose event came after the look above paused the page and ended the read (TOOL-U-111).
        let title = match (
            self.evaluate(&session, "document.title").await,
            self.client.dialog(),
        ) {
            (Ok(title), _) => title,
            (Err(_), Some(message)) => return Ok(dialog_snapshot(&target, &message)),
            (Err(error), None) => return Err(error),
        };
        Ok(RawSnapshot {
            source: Source::Dom,
            window: target,
            title: title.as_str().unwrap_or_default().to_owned(),
            elements: elements(&tree),
        })
    }

    async fn marked_snapshot(&self) -> Result<RawSnapshot, DriverError> {
        let mut snapshot = self.snapshot().await?;
        if self.client.dialog().is_some() {
            // A paused page can't be measured; the dialog's buttons keep their refs without marks.
            return Ok(snapshot);
        }
        let (_, session) = self.page().await?;
        let found = self.boxes(&session).await.unwrap_or_default();
        for element in &mut snapshot.elements {
            element.rect = element
                .handle
                .parse::<i64>()
                .ok()
                .and_then(|id| found.get(&id).copied());
        }
        Ok(snapshot)
    }

    async fn state(&self) -> Result<ScreenState, DriverError> {
        let (target, session) = self.page().await?;
        if let Some(message) = self.client.dialog() {
            return Ok(dialog_state(&target, message));
        }
        let read = self
            .evaluate(
                &session,
                "[document.title,location.href,scrollX,scrollY,(document.body?document.body.innerText.length:0),\
                 (document.activeElement?document.activeElement.tagName:''),document.querySelectorAll('*').length]",
            )
            .await;
        // A dialog whose event came after the look above paused the page and ended the read (TOOL-U-111).
        let shown = match (read, self.client.dialog()) {
            (Ok(shown), _) => shown,
            (Err(_), Some(message)) => return Ok(dialog_state(&target, message)),
            (Err(error), None) => return Err(error),
        };
        let title = shown[0].as_str().unwrap_or_default().to_owned();
        let mut hasher = DefaultHasher::new();
        shown.to_string().hash(&mut hasher);
        Ok(ScreenState {
            window: target,
            title,
            dialog: self.client.dialog(),
            digest: hasher.finish(),
        })
    }

    async fn act(&self, handle: &str, act: ElementAct) -> Result<(), DriverError> {
        let (_, session) = self.page().await?;
        if let Some(accept) = dialog_button(handle) {
            return self.answer_dialog(&session, accept).await;
        }
        match act {
            ElementAct::Click => self.click(&session, handle, 1).await,
            ElementAct::DoubleClick => self.click(&session, handle, 2).await,
            ElementAct::Scroll(direction, notches) => {
                self.scroll(&session, handle, direction, notches).await
            }
            ElementAct::Type(text) => self.type_into(&session, handle, &text).await,
        }
    }

    async fn type_focused(&self, text: &str) -> Result<(), DriverError> {
        let (_, session) = self.page().await?;
        let literal = serde_json::to_string(text)
            .map_err(|_| DriverError::Failed("the text can't be sent".into()))?;
        let ready = self
            .evaluate(
                &session,
                &format!(
                    "((text)=>{{const a=document.activeElement;if(!a){{return {{segmented:false,same:false,shown:'[]'}};}}\
                     return {{segmented:{DATED}(a),same:('value' in a)&&a.value===text,shown:({SHOWN}).call(a)}};}})({literal})"
                ),
            )
            .await?;
        let segmented = ready["segmented"] == true;
        let start = Start {
            segmented,
            same: ready["same"] == true,
            before: self
                .reading(&session, &Field::Focus, &ready["shown"], segmented)
                .await?,
        };
        self.land(&session, &Field::Focus, text, start).await
    }

    async fn key(&self, keys: &[String]) -> Result<(), DriverError> {
        let (_, session) = self.page().await?;
        if self.client.dialog().is_some() {
            // Return and Escape answer a dialog as a person's keys would; nothing else reaches a paused page.
            return match keys {
                [key] if key == "Return" => self.answer_dialog(&session, true).await,
                [key] if key == "Escape" => self.answer_dialog(&session, false).await,
                _ => Err(DriverError::Failed(
                    "a dialog is open: press Return or Escape, or click OK or Cancel".into(),
                )),
            };
        }
        for event in keys::events(keys)? {
            self.client
                .call("Input.dispatchKeyEvent", event, Some(&session))
                .await?;
        }
        Ok(())
    }

    async fn take_file_chooser(&self) -> Result<Option<FileInput>, DriverError> {
        Ok(self
            .client
            .take_chooser()
            .map(|(node, multiple)| FileInput {
                handle: node.to_string(),
                multiple,
            }))
    }

    async fn file_input(&self, handle: &str) -> Result<Option<FileInput>, DriverError> {
        let Ok(node) = handle.parse::<i64>() else {
            return Ok(None);
        };
        let (_, session) = self.page().await?;
        let described = self
            .client
            .call(
                "DOM.describeNode",
                json!({"backendNodeId": node}),
                Some(&session),
            )
            .await
            .map_err(|_| DriverError::Gone)?;
        let node_info = &described["node"];
        // Attributes come as one flat list: name, value, name, value, ...
        let attributes: Vec<&str> = node_info["attributes"]
            .as_array()
            .map(|list| list.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        let attribute = |wanted: &str| {
            attributes
                .chunks(2)
                .find(|pair| pair[0].eq_ignore_ascii_case(wanted))
                .map(|pair| pair.get(1).copied().unwrap_or(""))
        };
        let file = node_info["nodeName"] == "INPUT"
            && attribute("type").is_some_and(|kind| kind.eq_ignore_ascii_case("file"));
        Ok(file.then(|| FileInput {
            handle: handle.to_owned(),
            multiple: attribute("multiple").is_some(),
        }))
    }

    async fn choose_files(&self, input: &FileInput, files: &[PathBuf]) -> Result<(), DriverError> {
        let node = input.handle.parse::<i64>().map_err(|_| DriverError::Gone)?;
        let files: Vec<String> = files
            .iter()
            .map(|file| file.display().to_string())
            .collect();
        let (_, session) = self.page().await?;
        self.client
            .call(
                "DOM.setFileInputFiles",
                json!({"files": files, "backendNodeId": node}),
                Some(&session),
            )
            .await
            .map(|_| ())
    }
}

/// The flags the browser starts with: headed on the session's display, its debugging port on loopback chosen by the
/// browser (written to the profile's port file), renderer accessibility on, and nothing that phones home at start.
#[must_use]
pub fn launch_args(profile: &Path) -> Vec<String> {
    vec![
        "--remote-debugging-port=0".into(),
        "--remote-debugging-address=127.0.0.1".into(),
        format!("--user-data-dir={}", profile.display()),
        "--force-renderer-accessibility".into(),
        format!("--disk-cache-size={DISK_CACHE_BYTES}"),
        // The task's sandbox is the boundary, as for the Workstation's own browser use (images/runtime/README.md,
        // `chromiumSandbox: false`): Chromium's sandbox needs namespaces the sandbox doesn't grant.
        "--no-sandbox".into(),
        "--no-first-run".into(),
        "--no-default-browser-check".into(),
        "--disable-sync".into(),
        "--window-position=0,0".into(),
        "--window-size=1280,800".into(),
        "about:blank".into(),
    ]
}

/// The browser's debugging endpoint from its port file (`<port>\n<path>`), waiting for it to appear.
async fn endpoint(file: &Path, bound: Duration) -> Result<String, DriverError> {
    let started = std::time::Instant::now();
    loop {
        if let Ok(text) = tokio::fs::read_to_string(file).await
            && let Some(found) = parse_port_file(&text)
        {
            return Ok(found);
        }
        if started.elapsed() > bound {
            return Err(DriverError::Unavailable(
                "the browser didn't open its debugging port".into(),
            ));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// `ws://127.0.0.1:<port><path>` from a port file, when it holds a port and a `/devtools/` path.
#[must_use]
pub fn parse_port_file(text: &str) -> Option<String> {
    let mut lines = text.lines();
    let port: u16 = lines.next()?.trim().parse().ok()?;
    let path = lines.next()?.trim();
    (port > 0 && path.starts_with("/devtools/")).then(|| format!("ws://127.0.0.1:{port}{path}"))
}

/// The actionable nodes of an accessibility tree, in document order, each with its backend node as its handle.
#[must_use]
pub fn elements(tree: &Value) -> Vec<RawElement> {
    let Some(nodes) = tree["nodes"].as_array() else {
        return Vec::new();
    };
    nodes
        .iter()
        .filter(|node| node["ignored"] != true)
        .filter_map(|node| {
            let role = node["role"]["value"].as_str()?;
            if !ACTIONABLE.contains(&role) {
                return None;
            }
            let handle = node["backendDOMNodeId"].as_i64()?.to_string();
            Some(RawElement {
                role: role.to_owned(),
                name: shown_name(
                    node["name"]["value"].as_str().unwrap_or_default(),
                    node["description"]["value"].as_str().unwrap_or_default(),
                ),
                states: states(node),
                value: node["value"]["value"]
                    .as_str()
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned),
                handle,
                rect: None,
            })
        })
        .collect()
}

/// The node's states worth saying: the boolean properties among [`STATES`] that are true, and `checked` as `mixed`.
fn states(node: &Value) -> Vec<String> {
    let Some(properties) = node["properties"].as_array() else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for property in properties {
        let Some(name) = property["name"].as_str() else {
            continue;
        };
        if !STATES.contains(&name) {
            continue;
        }
        match &property["value"]["value"] {
            Value::Bool(true) => found.push(name.to_owned()),
            Value::String(text) if text == "true" => found.push(name.to_owned()),
            Value::String(text) if text == "mixed" => found.push(format!("{name} mixed")),
            _ => {}
        }
    }
    found
}

/// The screen's state while a dialog is open: its message as the title, in the page's window.
fn dialog_state(target: &str, message: String) -> ScreenState {
    let mut hasher = DefaultHasher::new();
    message.hash(&mut hasher);
    ScreenState {
        window: format!("{target}#dialog"),
        title: message.clone(),
        dialog: Some(message),
        digest: hasher.finish(),
    }
}

/// The open dialog as a snapshot: its message as the title, and its two buttons.
fn dialog_snapshot(target: &str, message: &str) -> RawSnapshot {
    let button = |name: &str, handle: &str| RawElement {
        role: "button".into(),
        name: name.into(),
        states: Vec::new(),
        value: None,
        handle: handle.into(),
        rect: None,
    };
    RawSnapshot {
        source: Source::Dom,
        window: format!("{target}#dialog"),
        title: message.to_owned(),
        elements: vec![
            button("OK", "dialog:accept"),
            button("Cancel", "dialog:dismiss"),
        ],
    }
}

/// Whether a handle is a dialog's button, and which: `Some(true)` for OK.
fn dialog_button(handle: &str) -> Option<bool> {
    match handle {
        "dialog:accept" => Some(true),
        "dialog:dismiss" => Some(false),
        _ => None,
    }
}

/// The browser's own tmpfs in a desktop sandbox (ADR-0039 section 10: 512 MiB, the sandbox user's, wiped with the
/// sandbox), never the workspace or the conversation's state (section 6). The sandbox's `/tmp` is too small for it.
pub const BROWSER_SCRATCH: &str = "/run/tqa-browser";
/// The browser's disk cache bound, so its cache stays well within [`BROWSER_SCRATCH`].
pub const DISK_CACHE_BYTES: u64 = 256 * 1024 * 1024;

/// Where the runtime keeps the browser's profile.
#[must_use]
pub fn profile_dir() -> PathBuf {
    Path::new(BROWSER_SCRATCH).join("profile")
}
