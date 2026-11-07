//! The browser driver's own parts without a browser (TOOL-U-043: the actions reach the screen as a person's would):
//! the accessibility tree read as elements, the port file, the launch flags, the key events, and the calls made over
//! a fake debugging connection.

use futures::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;

use super::{ACTIONABLE, Browser, Client, elements, keys, launch_args, parse_port_file};
use crate::builtin::computer::{
    a11y::ROLES,
    driver::{DriverError, ElementAct, FileInput, ScreenDriver, Source},
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn node(role: &str, name: &str, backend: i64, properties: &Value) -> Value {
    json!({"nodeId": backend.to_string(), "role": {"type": "role", "value": role},
           "name": {"type": "computedString", "value": name}, "backendDOMNodeId": backend, "properties": properties})
}

// TEST: TOOL-U-043
#[test]
fn tool_u_043_an_accessibility_tree_reads_as_its_actionable_elements() {
    let tree = json!({"nodes": [
        node("RootWebArea", "Form", 1, &json!([])),
        node("textbox", "Name", 5, &json!([{"name": "focused", "value": {"type": "booleanOrUndefined", "value": true}},
                                           {"name": "required", "value": {"type": "boolean", "value": false}}])),
        node("StaticText", "Your name", 6, &json!([])),
        node("checkbox", "Agree", 7, &json!([{"name": "checked", "value": {"type": "tristate", "value": "mixed"}}])),
        {"nodeId": "8", "ignored": true, "role": {"value": "button"}, "backendDOMNodeId": 8},
        node("button", "Save", 9, &json!([{"name": "disabled", "value": {"type": "boolean", "value": true}}])),
    ]});
    let mut found = elements(&tree);
    found[0].value = None;
    let summary: Vec<(String, String, Vec<String>, String)> = found
        .into_iter()
        .map(|element| (element.role, element.name, element.states, element.handle))
        .collect();
    assert_eq!(
        summary,
        vec![
            (
                "textbox".into(),
                "Name".into(),
                vec!["focused".into()],
                "5".into()
            ),
            (
                "checkbox".into(),
                "Agree".into(),
                vec!["checked mixed".into()],
                "7".into()
            ),
            (
                "button".into(),
                "Save".into(),
                vec!["disabled".into()],
                "9".into()
            ),
        ],
        "the root, static text and ignored nodes are left out"
    );
}

// TEST: TOOL-U-043
#[test]
fn tool_u_043_the_port_file_and_the_launch_flags() {
    assert_eq!(
        parse_port_file("41234\n/devtools/browser/abc-123\n").as_deref(),
        Some("ws://127.0.0.1:41234/devtools/browser/abc-123")
    );
    for bad in [
        "",
        "0\n/devtools/browser/x",
        "41234",
        "41234\n/elsewhere",
        "port\n/devtools/browser/x",
    ] {
        assert_eq!(parse_port_file(bad), None, "{bad:?}");
    }
    let flags = launch_args(&super::profile_dir());
    for flag in [
        "--remote-debugging-port=0",
        "--remote-debugging-address=127.0.0.1",
        "--user-data-dir=/run/tqa-browser/profile",
        "--force-renderer-accessibility",
        "--disk-cache-size=268435456",
    ] {
        assert!(flags.iter().any(|given| given == flag), "{flag}");
    }
    assert!(
        !flags.iter().any(|flag| flag.contains("headless")),
        "the browser is headed"
    );
}

// TEST: TOOL-U-043
#[test]
fn tool_u_043_a_combination_becomes_a_persons_key_events() -> TestResult {
    let ctrl_s = keys::events(&["ctrl".into(), "s".into()])?;
    let summary: Vec<(String, String, u64)> = ctrl_s
        .iter()
        .map(|event| {
            (
                event["type"].as_str().unwrap_or("").to_owned(),
                event["key"].as_str().unwrap_or("").to_owned(),
                event["modifiers"].as_u64().unwrap_or(99),
            )
        })
        .collect();
    assert_eq!(
        summary,
        vec![
            ("rawKeyDown".into(), "Control".into(), 2),
            ("rawKeyDown".into(), "s".into(), 2),
            ("keyUp".into(), "s".into(), 2),
            ("keyUp".into(), "Control".into(), 0),
        ],
        "with ctrl held the letter types no text"
    );
    let enter = keys::events(&["Return".into()])?;
    assert_eq!(enter[0]["text"], "\r");
    assert_eq!(enter[0]["windowsVirtualKeyCode"], 13);
    let shifted = keys::events(&["shift".into(), "a".into()])?;
    assert_eq!(shifted[1]["text"], "A");
    assert_eq!(
        keys::events(&["F5".into()])?[0]["windowsVirtualKeyCode"],
        116
    );
    assert!(
        matches!(
            keys::events(&["ctrl".into(), "shift".into()]),
            Err(DriverError::Failed(_))
        ),
        "no key"
    );
    assert!(
        matches!(
            keys::events(&["a".into(), "b".into()]),
            Err(DriverError::Failed(_))
        ),
        "two keys"
    );
    Ok(())
}

/// A fake browser: answers each call from `answers` by method, records the calls, and closes when the driver does. A
/// method listed more than once gets its answers in order, and the last one repeats. An answer of `"hang"` is never
/// sent, as a page paused by a dialog never answers a script call.
async fn fake_browser(
    answers: Vec<(&'static str, Value)>,
) -> Result<(String, tokio::task::JoinHandle<Vec<Value>>), std::io::Error> {
    fake_browser_with(answers, Vec::new()).await
}

/// As [`fake_browser`], sending each listed event just before its method's answer, as a page's event comes while the
/// call that caused it is answered.
async fn fake_browser_with(
    answers: Vec<(&'static str, Value)>,
    events: Vec<(&'static str, Value)>,
) -> Result<(String, tokio::task::JoinHandle<Vec<Value>>), std::io::Error> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let handle = tokio::spawn(async move {
        let mut seen = Vec::new();
        let mut asked: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        let Ok((stream, _)) = listener.accept().await else {
            return seen;
        };
        let Ok(mut socket) = tokio_tungstenite::accept_async(stream).await else {
            return seen;
        };
        while let Some(Ok(Message::Text(text))) = socket.next().await {
            let Ok(call) = serde_json::from_str::<Value>(text.as_str()) else {
                break;
            };
            let method = call["method"].as_str().unwrap_or("").to_owned();
            let listed: Vec<&Value> = answers
                .iter()
                .filter(|(name, _)| *name == method)
                .map(|(_, value)| value)
                .collect();
            let turn = asked.entry(method).or_insert(0);
            let result = listed
                .get((*turn).min(listed.len().saturating_sub(1)))
                .map_or(json!({}), |value| (*value).clone());
            *turn += 1;
            let answer = if result == json!("error") {
                json!({"id": call["id"], "error": {"code": -32000, "message": "No node with given id found"}})
            } else {
                json!({"id": call["id"], "result": result})
            };
            // The call's events first, then its answer.
            let outgoing: Vec<String> = events
                .iter()
                .filter(|(on, _)| *on == call["method"])
                .map(|(_, event)| event.to_string())
                .chain((result != json!("hang")).then(|| answer.to_string()))
                .collect();
            seen.push(call);
            if !send_each(&mut socket, outgoing).await {
                break;
            }
        }
        seen
    });
    Ok((
        format!("ws://127.0.0.1:{port}/devtools/browser/fake"),
        handle,
    ))
}

/// Sends each text in order; `false` once the socket refuses one.
async fn send_each(
    socket: &mut tokio_tungstenite::WebSocketStream<tokio::net::TcpStream>,
    texts: Vec<String>,
) -> bool {
    for text in texts {
        if socket.send(Message::text(text)).await.is_err() {
            return false;
        }
    }
    true
}

fn page_answers() -> Vec<(&'static str, Value)> {
    vec![
        (
            "Target.getTargets",
            json!({"targetInfos": [{"targetId": "service", "type": "service_worker"},
                                                     {"targetId": "page-1", "type": "page"}]}),
        ),
        ("Target.attachToTarget", json!({"sessionId": "session-1"})),
        (
            "Accessibility.getFullAXTree",
            json!({"nodes": [node("button", "Save", 9, &json!([]))]}),
        ),
        (
            "Runtime.evaluate",
            json!({"result": {"type": "string", "value": "Form"}}),
        ),
        (
            "DOM.resolveNode",
            json!({"object": {"objectId": "object-9"}}),
        ),
        (
            "Runtime.callFunctionOn",
            json!({"result": {"type": "string", "value": "clicked"}}),
        ),
    ]
}

// TEST: TOOL-U-043
#[tokio::test]
async fn tool_u_043_a_snapshot_and_a_click_go_to_the_focused_page() -> TestResult {
    let (endpoint, browser) = fake_browser(page_answers()).await?;
    let driver = Browser::over(Client::connect(&endpoint).await?);
    let snapshot = driver.snapshot().await?;
    assert_eq!(
        (
            snapshot.source,
            snapshot.window.as_str(),
            snapshot.title.as_str()
        ),
        (Source::Dom, "page-1", "Form")
    );
    assert_eq!(snapshot.elements[0].handle, "9");
    driver.act("9", ElementAct::Click).await?;
    drop(driver);
    let calls = browser.await?;
    let methods: Vec<&str> = calls
        .iter()
        .filter_map(|call| call["method"].as_str())
        .collect();
    assert_eq!(
        methods,
        [
            "Target.setDiscoverTargets",
            "Target.getTargets",
            "Target.attachToTarget",
            "Page.enable",
            "Page.setInterceptFileChooserDialog",
            "Accessibility.getFullAXTree",
            "Runtime.evaluate",
            "Target.getTargets",
            "DOM.resolveNode",
            "Runtime.callFunctionOn",
        ],
        "the targets followed (TOOL-U-110); attached once; the click is the element's own"
    );
    assert!(calls[5]["sessionId"] == "session-1" && calls[1].get("sessionId").is_none());
    assert_eq!(
        calls[4]["params"],
        json!({"enabled": true}),
        "the page's file chooser is the driver's, never a native dialog (TOOL-U-062)"
    );
    Ok(())
}

// TEST: TOOL-U-044
#[tokio::test]
async fn tool_u_044_a_node_the_page_no_longer_has_is_gone() -> TestResult {
    let mut answers = page_answers();
    answers.retain(|(method, _)| *method != "DOM.resolveNode");
    answers.push(("DOM.resolveNode", json!("error")));
    let (endpoint, _browser) = fake_browser(answers).await?;
    let driver = Browser::over(Client::connect(&endpoint).await?);
    let said = driver.act("9", ElementAct::Click).await;
    assert_eq!(said, Err(DriverError::Gone));
    Ok(())
}

// TEST: TOOL-U-043
#[tokio::test]
async fn tool_u_043_only_a_loopback_endpoint_is_dialled() {
    let said = Client::connect("ws://10.0.0.5:9222/devtools/browser/x").await;
    assert!(matches!(said, Err(DriverError::Unavailable(_))));
}

// TEST: TOOL-U-043
#[test]
fn tool_u_043_an_unnamed_element_is_named_by_its_description() {
    let mut icon = node("button", "", 4, &json!([]));
    icon["description"] = json!({"type": "computedString", "value": "Open settings"});
    let mut described = node("button", "Save", 5, &json!([]));
    described["description"] = json!({"type": "computedString", "value": "Saves the form"});
    let names: Vec<String> =
        elements(&json!({"nodes": [icon, described, node("link", "", 6, &json!([]))]}))
            .into_iter()
            .map(|element| element.name)
            .collect();
    assert_eq!(
        names,
        vec!["Open settings".to_owned(), "Save".to_owned(), String::new()],
        "the description only when there is no name"
    );
}

// TEST: TOOL-U-043
#[test]
fn tool_u_043_every_role_a_snapshot_lists_is_on_the_targets_list() {
    let words = ACTIONABLE.iter().chain(ROLES.iter().map(|(_, word)| word));
    for word in words {
        assert_ne!(
            tqa_types::ScreenRole::from_word(word),
            tqa_types::ScreenRole::Other,
            "{word}"
        );
    }
}

// TEST: TOOL-U-047
#[test]
fn tool_u_047_a_pages_boxes_map_to_the_screen_through_the_windows_frame() -> TestResult {
    // A window at (100, 50), 1280 × 800 outside and 1280 × 720 inside (an 80 px toolbar), scrolled 200 px down.
    let viewport =
        super::boxes::Viewport::from_metrics(&json!([100, 50, 1280, 800, 1280, 720, 0, 200, 1]))
            .ok_or("unreadable metrics")?;
    assert_eq!((viewport.left, viewport.top), (100.0, 130.0));
    let snapshot = json!({"documents": [{
        "nodes": {"backendNodeId": [1, 5, 9, 12]},
        "layout": {"nodeIndex": [1, 2, 3], "bounds": [[10, 210, 100, 20], [0, 2000, 10, 10], [40, 190, 50, 40]]},
    }]});
    let found = super::boxes::boxes(&snapshot, &viewport);
    let rect =
        |x, y, width, height| crate::builtin::computer::raster::Rect::new(x, y, width, height);
    assert_eq!(
        found.get(&5).copied(),
        rect(110, 140, 100, 20),
        "inside the page, offset by the frame and the scroll"
    );
    assert_eq!(found.get(&9), None, "scrolled out of sight: no box");
    assert_eq!(
        found.get(&12).copied(),
        rect(140, 130, 50, 30),
        "cut to the visible part"
    );
    assert!(super::boxes::Viewport::from_metrics(&json!(["x"])).is_none());
    Ok(())
}

/// The fake page for a `type`: the field's readings in order, each what `Runtime.callFunctionOn` answers (first the
/// field made ready, with whether it is a date or time segment and what it shows, then what it shows after each try).
fn typing_answers(readings: Vec<Value>) -> Vec<(&'static str, Value)> {
    let mut answers = page_answers();
    answers.retain(|(method, _)| *method != "Runtime.callFunctionOn");
    for reading in readings {
        answers.push((
            "Runtime.callFunctionOn",
            json!({"result": {"type": "object", "value": reading}}),
        ));
    }
    answers
}

/// The input the page was sent, in order: `insertText <text>` or `<keyDown|keyUp> <key>`.
fn inputs(calls: &[Value]) -> Vec<String> {
    calls
        .iter()
        .filter_map(|call| match call["method"].as_str()? {
            "Input.insertText" => Some(format!("insertText {}", call["params"]["text"].as_str()?)),
            "Input.dispatchKeyEvent" => Some(format!(
                "{} {}",
                call["params"]["type"].as_str()?,
                call["params"]["key"].as_str()?
            )),
            _ => None,
        })
        .collect()
}

// TEST: TOOL-U-059
#[tokio::test]
async fn tool_u_059_a_date_or_time_segment_is_typed_as_keys() -> TestResult {
    // A2 (17 Oct): Chromium's Hours/Minutes/AM-PM and Month/Day/Year segments ignore insertText.
    let (endpoint, browser) = fake_browser(typing_answers(vec![
        json!({"segmented": true, "same": false, "shown": "[\"--\"]"}),
        json!("[\"07\"]"),
    ]))
    .await?;
    let driver = Browser::over(Client::connect(&endpoint).await?);
    driver.act("9", ElementAct::Type("7".into())).await?;
    drop(driver);
    assert_eq!(
        inputs(&browser.await?),
        ["keyDown 7", "keyUp 7"],
        "key events, never insertText"
    );
    Ok(())
}

// TEST: TOOL-U-059
#[tokio::test]
async fn tool_u_059_a_textbox_still_takes_inserted_text() -> TestResult {
    let (endpoint, browser) = fake_browser(typing_answers(vec![
        json!({"segmented": false, "same": false, "shown": "[\"\"]"}),
        json!("[\"Ada\"]"),
    ]))
    .await?;
    let driver = Browser::over(Client::connect(&endpoint).await?);
    driver.act("9", ElementAct::Type("Ada".into())).await?;
    drop(driver);
    assert_eq!(inputs(&browser.await?), ["insertText Ada"]);
    Ok(())
}

// TEST: TOOL-U-059
#[tokio::test]
async fn tool_u_059_text_that_changes_nothing_is_tried_as_keys_then_an_error() -> TestResult {
    let (endpoint, browser) = fake_browser(typing_answers(vec![
        json!({"segmented": false, "same": false, "shown": "[\"x\"]"}),
        json!("[\"x\"]"),
        json!("[\"x\"]"),
    ]))
    .await?;
    let driver = Browser::over(Client::connect(&endpoint).await?);
    let said = driver.act("9", ElementAct::Type("ab".into())).await;
    drop(driver);
    assert!(
        matches!(&said, Err(DriverError::Failed(reason)) if reason.contains("didn't change")),
        "never ok when the field shows the same: {said:?}"
    );
    assert_eq!(
        inputs(&browser.await?),
        [
            "insertText ab",
            "keyDown a",
            "keyUp a",
            "keyDown b",
            "keyUp b"
        ]
    );
    Ok(())
}

// TEST: TOOL-U-059
#[tokio::test]
async fn tool_u_059_text_the_field_already_holds_is_not_typed_twice() -> TestResult {
    let (endpoint, browser) = fake_browser(typing_answers(vec![
        json!({"segmented": false, "same": true, "shown": "[\"Ada\"]"}),
        json!("[\"Ada\"]"),
    ]))
    .await?;
    let driver = Browser::over(Client::connect(&endpoint).await?);
    driver.act("9", ElementAct::Type("Ada".into())).await?;
    drop(driver);
    assert_eq!(inputs(&browser.await?), ["insertText Ada"]);
    Ok(())
}

// TEST: TOOL-U-059
#[tokio::test]
async fn tool_u_059_typing_into_a_focused_segment_is_keys_too() -> TestResult {
    // `type` without a ref after a click on the Hours segment: the focus is the time input; the segment's value is
    // read from the accessibility tree (the input's own value stays empty until every segment is filled).
    let segment = |value: i64| {
        json!({"nodes": [{"nodeId": "28", "role": {"type": "role", "value": "spinbutton"},
               "name": {"type": "computedString", "value": "Hours"}, "backendDOMNodeId": 28,
               "value": {"type": "number", "value": value},
               "properties": [{"name": "focused", "value": {"type": "booleanOrUndefined", "value": true}}]}]})
    };
    let mut answers = page_answers();
    answers.retain(|(method, _)| {
        *method != "Runtime.evaluate" && *method != "Accessibility.getFullAXTree"
    });
    answers.extend([
        (
            "Runtime.evaluate",
            json!({"result": {"type": "object", "value": {"segmented": true, "same": false, "shown": "[\"\"]"}}}),
        ),
        (
            "Runtime.evaluate",
            json!({"result": {"type": "string", "value": "[\"\"]"}}),
        ),
        ("Accessibility.getFullAXTree", segment(0)),
        ("Accessibility.getFullAXTree", segment(7)),
    ]);
    let (endpoint, browser) = fake_browser(answers).await?;
    let driver = Browser::over(Client::connect(&endpoint).await?);
    driver.type_focused("7").await?;
    drop(driver);
    assert_eq!(inputs(&browser.await?), ["keyDown 7", "keyUp 7"]);
    Ok(())
}

// TEST: TOOL-U-059
#[test]
fn tool_u_059_text_becomes_one_key_per_character() {
    let events = keys::typed("7P-é\t");
    let shown: Vec<(String, String, Value, Value, Value)> = events
        .iter()
        .map(|event| {
            (
                event["type"].as_str().unwrap_or("").to_owned(),
                event["key"].as_str().unwrap_or("").to_owned(),
                event["text"].clone(),
                event["code"].clone(),
                event["modifiers"].clone(),
            )
        })
        .collect();
    let row = |kind: &str, key: &str, text: Value, code: Value, held: u8| {
        (kind.to_owned(), key.to_owned(), text, code, json!(held))
    };
    assert_eq!(
        shown,
        vec![
            row("keyDown", "7", json!("7"), json!("Digit7"), 0),
            row("keyUp", "7", Value::Null, json!("Digit7"), 0),
            row("keyDown", "P", json!("P"), json!("KeyP"), 8),
            row("keyUp", "P", Value::Null, json!("KeyP"), 8),
            row("keyDown", "-", json!("-"), json!("Minus"), 0),
            row("keyUp", "-", Value::Null, json!("Minus"), 0),
            row("keyDown", "é", json!("é"), Value::Null, 0),
            row("keyUp", "é", Value::Null, Value::Null, 0),
            row("rawKeyDown", "Tab", Value::Null, json!("Tab"), 0),
            row("keyUp", "Tab", Value::Null, json!("Tab"), 0),
        ],
        "a digit, shift for a capital, punctuation by its key, other text alone, Tab with no text"
    );
    assert_eq!(events[0]["windowsVirtualKeyCode"], json!(55));
}

// TEST: TOOL-U-059
#[tokio::test]
async fn tool_u_059_text_typed_into_a_heading_is_an_error_not_ok() -> TestResult {
    // c16k real-world A5 (17 Oct 2026): the model typed a URL six times with ref=e1, the 404 page's heading; each said
    // "ok ... no visible change". A heading takes no text: it reads the same after inserted text and after keys.
    let heading = "[null,null,\"This httpbin.org page can't be found\",null]";
    let (endpoint, browser) = fake_browser(typing_answers(vec![
        json!({"segmented": false, "same": false, "shown": heading}),
        json!(heading),
        json!(heading),
    ]))
    .await?;
    let driver = Browser::over(Client::connect(&endpoint).await?);
    let said = driver
        .act("9", ElementAct::Type("https://httpbin.org/ip".into()))
        .await;
    drop(driver);
    assert!(
        matches!(&said, Err(DriverError::Failed(reason)) if reason.contains("didn't change")),
        "{said:?}"
    );
    assert_eq!(
        inputs(&browser.await?).first().map(String::as_str),
        Some("insertText https://httpbin.org/ip")
    );
    Ok(())
}

// TEST: TOOL-U-062
#[tokio::test]
async fn tool_u_062_a_pages_file_chooser_is_taken_once_and_its_input_given_files() -> TestResult {
    let mut answers = page_answers();
    answers.push((
        "DOM.describeNode",
        json!({"node": {"nodeName": "INPUT", "attributes": ["type", "file", "multiple", ""]}}),
    ));
    answers.push((
        "DOM.describeNode",
        json!({"node": {"nodeName": "BUTTON", "attributes": ["type", "button"]}}),
    ));
    let opened = json!({"method": "Page.fileChooserOpened", "sessionId": "session-1",
                        "params": {"frameId": "f", "mode": "selectSingle", "backendNodeId": 12}});
    let (endpoint, browser) =
        fake_browser_with(answers, vec![("Runtime.callFunctionOn", opened)]).await?;
    let driver = Browser::over(Client::connect(&endpoint).await?);
    assert_eq!(driver.take_file_chooser().await?, None, "nothing asked yet");
    driver.act("9", ElementAct::Click).await?;
    let asked = driver.take_file_chooser().await?;
    assert_eq!(
        asked,
        Some(FileInput {
            handle: "12".into(),
            multiple: false
        })
    );
    assert_eq!(driver.take_file_chooser().await?, None, "taken once");
    // An element's own input: a file input says whether it takes several; anything else isn't one.
    assert_eq!(
        driver.file_input("12").await?,
        Some(FileInput {
            handle: "12".into(),
            multiple: true
        })
    );
    assert_eq!(driver.file_input("9").await?, None);
    let file = std::path::PathBuf::from("/workspace/report.txt");
    driver
        .choose_files(
            &FileInput {
                handle: "12".into(),
                multiple: false,
            },
            &[file],
        )
        .await?;
    drop(driver);
    let calls = browser.await?;
    let chosen: Vec<&Value> = calls
        .iter()
        .filter(|call| call["method"] == "DOM.setFileInputFiles")
        .collect();
    assert_eq!(chosen.len(), 1);
    assert_eq!(
        (&chosen[0]["params"], &chosen[0]["sessionId"]),
        (
            &json!({"files": ["/workspace/report.txt"], "backendNodeId": 12}),
            &json!("session-1")
        )
    );
    Ok(())
}

// TEST: TOOL-U-062
#[tokio::test]
async fn tool_u_062_a_click_by_ref_is_a_persons_gesture_so_a_chooser_may_open() -> TestResult {
    // Chromium opens a file input's chooser only after a person's activation: a styled button's `input.click()` from
    // a click without one opens nothing (16n's live upload, 17 Oct 2026).
    let (endpoint, browser) = fake_browser(page_answers()).await?;
    let driver = Browser::over(Client::connect(&endpoint).await?);
    driver.act("9", ElementAct::Click).await?;
    drop(driver);
    let calls = browser.await?;
    let clicked: Vec<&Value> = calls
        .iter()
        .filter(|call| call["method"] == "Runtime.callFunctionOn")
        .collect();
    assert_eq!(clicked.len(), 1);
    assert_eq!(clicked[0]["params"]["userGesture"], json!(true));
    Ok(())
}

// TEST: RT-I-057
/// A browser answers a person's keys only once its window, asked through the browser's own process, is a normal
/// window and its page has loaded: a minimised window, then a page still loading, then both (about:blank counts once
/// it is complete).
#[tokio::test]
async fn rt_i_057_the_browser_answers_once_its_window_is_shown_and_its_page_loaded() -> TestResult {
    let (endpoint, browser) = fake_browser(vec![
        (
            "Target.getTargets",
            json!({"targetInfos": [{"targetId": "page-1", "type": "page", "url": "about:blank"}]}),
        ),
        ("Target.attachToTarget", json!({"sessionId": "session-1"})),
        (
            "Browser.getWindowForTarget",
            json!({"windowId": 1, "bounds": {"windowState": "minimized"}}),
        ),
        (
            "Browser.getWindowForTarget",
            json!({"windowId": 1, "bounds": {"windowState": "normal"}}),
        ),
        (
            "Runtime.evaluate",
            json!({"result": {"type": "string", "value": "loading"}}),
        ),
        (
            "Runtime.evaluate",
            json!({"result": {"type": "string", "value": "complete"}}),
        ),
    ])
    .await?;
    let driver = Browser::over(Client::connect(&endpoint).await?);
    assert!(!driver.answers().await?, "a minimised window");
    assert!(!driver.answers().await?, "a page still loading");
    assert!(driver.answers().await?, "shown and loaded");
    drop(driver);
    let seen = browser.await?;
    let windows: Vec<&Value> = seen
        .iter()
        .filter(|call| call["method"] == "Browser.getWindowForTarget")
        .collect();
    assert_eq!(windows.len(), 3);
    assert!(
        windows
            .iter()
            .all(|call| call["params"]["targetId"] == "page-1" && call.get("sessionId").is_none()),
        "the window is asked of the browser, for the page's target: {windows:?}"
    );
    let loads: Vec<&Value> = seen
        .iter()
        .filter(|call| call["method"] == "Runtime.evaluate")
        .collect();
    assert_eq!(loads.len(), 2, "a minimised window isn't asked further");
    assert!(
        loads.iter().all(|call| call["sessionId"] == "session-1"
            && call["params"]["expression"] == "document.readyState"),
        "the page is asked in its session: {loads:?}"
    );
    Ok(())
}

// TEST: RT-I-057
/// Any shown window takes keys: normal, maximised (a person who maximised the browser) and fullscreen answer; only a
/// minimised one doesn't, so a maximised browser never makes the next reset not ready.
#[tokio::test]
async fn rt_i_057_any_window_state_but_minimised_answers() -> TestResult {
    let state = |name: &str| json!({"windowId": 1, "bounds": {"windowState": name}});
    let (endpoint, _browser) = fake_browser(vec![
        (
            "Target.getTargets",
            json!({"targetInfos": [{"targetId": "page-1", "type": "page", "url": "about:blank"}]}),
        ),
        ("Target.attachToTarget", json!({"sessionId": "session-1"})),
        ("Browser.getWindowForTarget", state("normal")),
        ("Browser.getWindowForTarget", state("maximized")),
        ("Browser.getWindowForTarget", state("fullscreen")),
        ("Browser.getWindowForTarget", state("minimized")),
        (
            "Runtime.evaluate",
            json!({"result": {"type": "string", "value": "complete"}}),
        ),
    ])
    .await?;
    let driver = Browser::over(Client::connect(&endpoint).await?);
    for (state, answers) in [
        ("normal", true),
        ("maximized", true),
        ("fullscreen", true),
        ("minimized", false),
    ] {
        assert_eq!(driver.answers().await?, answers, "a {state} window");
    }
    Ok(())
}

// TEST: TOOL-U-110
/// The page acted on is the one that opened last by the driver's own record of the browser's targets, whatever order
/// `Target.getTargets` lists them in, which is no promise (c17's integration, RT-I-046: Chromium 155 listed a page's
/// `window.open` first, it went unseen, and the driver kept acting on the page that opened it).
#[tokio::test]
async fn tool_u_110_a_new_window_is_the_page_that_opened_last_in_any_listed_order() -> TestResult {
    let info = |id: &str, kind: &str| json!({"targetId": id, "type": kind, "url": "about:blank"});
    let created = |id: &str, kind: &str| {
        let info = info(id, kind);
        json!({"method": "Target.targetCreated", "params": {"targetInfo": info}})
    };
    let before = json!({"targetInfos": [info("popup", "browser_ui"), info("form", "page")]});
    let (opened, popup, form) = (
        info("opened", "page"),
        info("popup", "browser_ui"),
        info("form", "page"),
    );
    let newest_first = json!({"targetInfos": [opened.clone(), popup.clone(), form.clone()]});
    let oldest_first = json!({"targetInfos": [form, popup, opened]});
    // The third run's window is listed before its event comes: a page not in the record yet opened after it.
    for (order, after, reported) in [
        ("newest first", newest_first.clone(), true),
        ("oldest first", oldest_first, true),
        ("listed before it's reported", newest_first, false),
    ] {
        let mut answers = page_answers();
        answers.retain(|(method, _)| {
            !matches!(*method, "Target.getTargets" | "Target.attachToTarget")
        });
        answers.extend([
            ("Target.getTargets", before.clone()),
            ("Target.getTargets", before.clone()),
            ("Target.getTargets", after),
            (
                "Target.attachToTarget",
                json!({"sessionId": "session-form"}),
            ),
            (
                "Target.attachToTarget",
                json!({"sessionId": "session-opened"}),
            ),
        ]);
        let mut events = vec![
            ("Target.setDiscoverTargets", created("form", "page")),
            ("Target.setDiscoverTargets", created("popup", "browser_ui")),
        ];
        if reported {
            // The click's page opens a window as its handler runs.
            events.push(("Runtime.callFunctionOn", created("opened", "page")));
        }
        let (endpoint, browser) = fake_browser_with(answers, events).await?;
        let driver = Browser::over(Client::connect(&endpoint).await?);
        assert_eq!(
            driver.state().await?.window,
            "form",
            "{order}: the one page"
        );
        driver.act("9", ElementAct::Click).await?;
        assert_eq!(
            driver.state().await?.window,
            "opened",
            "{order}: the window the click opened"
        );
        drop(driver);
        let calls = browser.await?;
        let discover: Vec<&Value> = calls
            .iter()
            .filter(|call| call["method"] == "Target.setDiscoverTargets")
            .collect();
        assert!(
            discover.len() == 1
                && discover[0]["params"] == json!({"discover": true})
                && discover[0].get("sessionId").is_none(),
            "{order}: the browser's targets are followed once, on the browser: {discover:?}"
        );
        assert_eq!(
            calls[0]["method"], "Target.setDiscoverTargets",
            "{order}: before any page is chosen"
        );
    }
    Ok(())
}

// TEST: TOOL-U-110
/// The pages open when the driver connects come in no order: the browser reports them all before it answers
/// `Target.setDiscoverTargets`, in an order of its own (Chromium 155 reported four tabs opened A, B, C, D as D, A, B,
/// C, and listed them A, C, D, B). So that order counts for nothing: of those pages the later listed is acted on, as
/// before the driver kept a record.
#[tokio::test]
async fn tool_u_110_the_pages_open_at_connect_come_in_no_order() -> TestResult {
    let info = |id: &str| json!({"targetId": id, "type": "page", "url": "about:blank"});
    let created = |id: &str| {
        let info = info(id);
        json!({"method": "Target.targetCreated", "params": {"targetInfo": info}})
    };
    let mut answers = page_answers();
    answers.retain(|(method, _)| *method != "Target.getTargets");
    answers.push((
        "Target.getTargets",
        json!({"targetInfos": [info("first"), info("second")]}),
    ));
    let events = vec![
        ("Target.setDiscoverTargets", created("second")),
        ("Target.setDiscoverTargets", created("first")),
    ];
    let (endpoint, _browser) = fake_browser_with(answers, events).await?;
    let driver = Browser::over(Client::connect(&endpoint).await?);
    assert_eq!(driver.state().await?.window, "second", "the later listed");
    Ok(())
}

// TEST: TOOL-U-111
/// A page read under way when the page opens a JavaScript dialog ends at once, and the screen's state is the dialog:
/// a page paused by an alert never answers a script call, and the dialog's event can come after the driver last looked
/// for one (c17's integration, RT-I-046: a first click's alert, then `display_unavailable` at the call's 10 s bound).
#[tokio::test]
async fn tool_u_111_a_dialog_opening_during_a_page_read_is_the_screens_state() -> TestResult {
    let mut answers = page_answers();
    answers.retain(|(method, _)| *method != "Runtime.evaluate");
    answers.push(("Runtime.evaluate", json!("hang")));
    let opening = json!({"method": "Page.javascriptDialogOpening", "sessionId": "session-1",
                         "params": {"message": "Sure?", "type": "alert", "url": "about:blank"}});
    let (endpoint, _browser) =
        fake_browser_with(answers, vec![("Runtime.evaluate", opening)]).await?;
    let driver = Browser::over(Client::connect(&endpoint).await?);
    let state = tokio::time::timeout(std::time::Duration::from_secs(3), driver.state())
        .await
        .map_err(|_| "the read waited on a page the dialog paused")??;
    assert_eq!(
        (
            state.window.as_str(),
            state.title.as_str(),
            state.dialog.as_deref()
        ),
        ("page-1#dialog", "Sure?", Some("Sure?"))
    );
    Ok(())
}

// TEST: TOOL-U-111
/// The same for a snapshot: a dialog whose event comes while the page's title is read is the snapshot, its two buttons
/// to act on, never an error where the screen is simply a dialog.
#[tokio::test]
async fn tool_u_111_a_dialog_opening_during_a_snapshot_is_the_snapshot() -> TestResult {
    let mut answers = page_answers();
    answers.retain(|(method, _)| *method != "Runtime.evaluate");
    answers.push(("Runtime.evaluate", json!("hang")));
    let opening = json!({"method": "Page.javascriptDialogOpening", "sessionId": "session-1",
                         "params": {"message": "Sure?", "type": "alert", "url": "about:blank"}});
    let (endpoint, _browser) =
        fake_browser_with(answers, vec![("Runtime.evaluate", opening)]).await?;
    let driver = Browser::over(Client::connect(&endpoint).await?);
    let snapshot = tokio::time::timeout(std::time::Duration::from_secs(3), driver.snapshot())
        .await
        .map_err(|_| "the read waited on a page the dialog paused")??;
    let handles: Vec<&str> = snapshot
        .elements
        .iter()
        .map(|element| element.handle.as_str())
        .collect();
    assert_eq!(
        (snapshot.window.as_str(), snapshot.title.as_str(), handles),
        (
            "page-1#dialog",
            "Sure?",
            vec!["dialog:accept", "dialog:dismiss"]
        )
    );
    Ok(())
}
