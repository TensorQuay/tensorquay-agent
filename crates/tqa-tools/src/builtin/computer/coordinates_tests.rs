//! TOOL-U-043 and TOOL-U-045: points mapped from the latest screenshot, and the run's caps (with one TOOL-U-047
//! case for a screen without boxed elements). A child of the computer tool's tests, sharing their fakes.
use super::*;

fn grounded(max_image_side: u32, limits: tqa_types::ComputerLimits) -> View {
    View {
        vision: true,
        pixel_grounding: true,
        max_image_side,
        limits,
        ..View::default()
    }
}

// TEST: TOOL-U-043
#[test]
fn tool_u_043_the_coordinate_forms_are_offered_only_with_images_and_good_grounding() {
    let schema = |view: View| {
        let tool = Computer::with_view(Screen::with(vec![form()], vec![state("w1", 1)]), view);
        let schema = tool.spec().parameters.clone();
        assert!(strict_compatible(&schema), "strict-valid: {schema}");
        schema
    };
    let plain = schema(vision(1280));
    assert!(plain["properties"].get("x").is_none(), "{plain}");
    let ungrounded_text = View {
        pixel_grounding: true,
        ..View::default()
    };
    assert!(
        schema(ungrounded_text)["properties"].get("x").is_none(),
        "no images, no points"
    );
    let points = schema(grounded(1280, tqa_types::ComputerLimits::default()));
    assert!(
        points["properties"]["action"]["enum"]
            .as_array()
            .is_some_and(|actions| actions.contains(&json!("drag")))
    );
    assert_eq!(
        points["required"],
        json!([
            "action",
            "ref",
            "text",
            "keys",
            "ms",
            "direction",
            "amount",
            "x",
            "y",
            "to"
        ])
    );
}

/// Every argument field, including the coordinate ones, null except the ones given.
fn pointed(fields: &Value) -> Value {
    let mut args = call(fields);
    for name in ["x", "y", "to"] {
        if fields.get(name).is_none() {
            args[name] = Value::Null;
        }
    }
    args
}

// TEST: TOOL-U-043
#[tokio::test]
async fn tool_u_043_a_point_maps_from_the_latest_screenshot_to_the_screen() -> TestResult {
    let directory = tempfile::tempdir()?;
    let (cx, _) = context(&directory)?;
    let screen = marked_screen();
    let tool = Computer::with_view(
        screen.clone(),
        grounded(200, tqa_types::ComputerLimits::default()),
    );
    let early = tool
        .call(pointed(&json!({"action": "click", "x": 5, "y": 5})), &cx)
        .await;
    assert!(
        matches!(&early, Err(error) if error.model_message().starts_with("no_screenshot_yet")),
        "{early:?}"
    );
    tool.call(pointed(&json!({"action": "screenshot"})), &cx)
        .await?;
    // The 400 × 300 screen was sent as 200 × 150: image pixel (10, 20) is screen (21, 41), its centre.
    let clicked = tool
        .call(pointed(&json!({"action": "click", "x": 10, "y": 20})), &cx)
        .await?;
    assert!(
        clicked.text.starts_with("Clicked at (10, 20)"),
        "{}",
        clicked.text
    );
    assert_eq!(clicked.target, None, "a pixel action names no element");
    tool.call(
        pointed(&json!({"action": "scroll", "x": 199, "y": 149, "direction": "up", "amount": 2})),
        &cx,
    )
    .await?;
    tool.call(
        pointed(&json!({"action": "drag", "x": 0, "y": 0, "to": {"x": 100, "y": 75}})),
        &cx,
    )
    .await?;
    assert_eq!(
        screen.done(),
        [
            "pointer (21, 41) Click(1)".to_owned(),
            format!(
                "pointer (399, 299) {:?}",
                PointerAct::Scroll(Direction::Up, 2)
            ),
            "drag (1, 1) (201, 151)".to_owned(),
        ]
    );
    Ok(())
}

// TEST: TOOL-U-043
#[tokio::test]
async fn tool_u_043_each_coordinate_rule_is_checked() -> TestResult {
    let directory = tempfile::tempdir()?;
    let (cx, _) = context(&directory)?;
    let tool = Computer::with_view(
        marked_screen(),
        grounded(1280, tqa_types::ComputerLimits::default()),
    );
    tool.call(pointed(&json!({"action": "screenshot"})), &cx)
        .await?;
    for (fields, refusal) in [
        (
            json!({"action": "click", "ref": "e1", "x": 1, "y": 1}),
            "invalid_args: /ref: give a ref or x and y, not both",
        ),
        (
            json!({"action": "click", "x": 1}),
            "invalid_args: /y: x needs y",
        ),
        (
            json!({"action": "click", "x": -1, "y": 1}),
            "invalid_args: /x: expected a pixel of the latest screenshot",
        ),
        (
            json!({"action": "click", "x": 400, "y": 1}),
            "invalid_args: /x: the point is outside the latest screenshot (400×300)",
        ),
        (
            json!({"action": "drag", "x": 1, "y": 1}),
            "invalid_args: /to: drag needs where to",
        ),
        (
            json!({"action": "drag", "x": 1, "y": 1, "to": {"x": 1, "y": 300}}),
            "invalid_args: /to: the point is outside",
        ),
        (
            json!({"action": "drag", "ref": "e1", "x": 1, "y": 1, "to": {"x": 2, "y": 2}}),
            "invalid_args: /ref: drag goes from point to point",
        ),
    ] {
        let answer = match tool.call(pointed(&fields), &cx).await {
            Ok(output) => format!("accepted: {}", output.text),
            Err(error) => error.model_message(),
        };
        assert!(answer.starts_with(refusal), "{fields}: {answer}");
    }
    let plain = Computer::with_view(marked_screen(), vision(1280));
    let refused = refusal(&plain, &cx, json!({"action": "click", "x": 1, "y": 1})).await;
    assert!(
        refused.starts_with("invalid_args: /x: coordinates aren't offered on this route"),
        "{refused}"
    );
    Ok(())
}

// TEST: TOOL-U-045
#[tokio::test]
async fn tool_u_045_the_runs_screenshot_and_action_caps_hold() -> TestResult {
    let directory = tempfile::tempdir()?;
    let (cx, _) = context(&directory)?;
    let limits = tqa_types::ComputerLimits::new(2, 3)?;
    let tool = Computer::with_view(marked_screen(), grounded(1280, limits));
    for _ in 0..2 {
        tool.call(pointed(&json!({"action": "screenshot"})), &cx)
            .await?;
    }
    let third = refusal(
        &tool,
        &cx,
        json!({"action": "screenshot", "x": null, "y": null, "to": null}),
    )
    .await;
    assert!(third.starts_with("screenshot_limit"), "{third}");
    assert!(third.contains("work from observe"), "{third}");
    tool.call(pointed(&json!({"action": "observe"})), &cx)
        .await?;
    for _ in 0..3 {
        tool.call(pointed(&json!({"action": "key", "keys": "Tab"})), &cx)
            .await?;
    }
    let fourth = refusal(
        &tool,
        &cx,
        json!({"action": "key", "keys": "Tab", "x": null, "y": null, "to": null}),
    )
    .await;
    assert!(fourth.starts_with("action_limit"), "{fourth}");
    assert!(
        fourth.ends_with("Stop using the desktop and tell the person what is left."),
        "{fourth}"
    );
    let looked = tool.call(pointed(&json!({"action": "observe"})), &cx).await;
    assert!(looked.is_ok(), "looking is never an action: {looked:?}");
    Ok(())
}

// TEST: TOOL-U-045
#[tokio::test]
async fn tool_u_045_a_screenshot_is_counted_only_once_it_is_sent() -> TestResult {
    let directory = tempfile::tempdir()?;
    let (cx, _) = context(&directory)?;
    // A display that can't be read: each screenshot fails, and none is counted, as the journal wouldn't count it.
    let dark = Arc::new(Screen {
        snapshots: StdMutex::new(vec![page("w1", vec![]), page("w1", vec![])].into()),
        pixels: None,
        ..Screen::default()
    });
    let tool = Computer::with_view(dark, grounded(1280, tqa_types::ComputerLimits::new(1, 3)?));
    for _ in 0..2 {
        let failed = refusal(
            &tool,
            &cx,
            json!({"action": "screenshot", "x": null, "y": null, "to": null}),
        )
        .await;
        assert!(failed.starts_with("display_unavailable"), "{failed}");
    }
    assert_eq!(
        tool.screenshots.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "nothing was sent"
    );
    assert!(
        tool.latest.lock().await.is_none(),
        "no snapshot was taken for a screenshot never sent"
    );
    Ok(())
}

// TEST: TOOL-U-047
#[tokio::test]
async fn tool_u_047_a_screen_without_boxed_elements_is_not_called_marked() -> TestResult {
    let directory = tempfile::tempdir()?;
    let (cx, _) = context(&directory)?;
    let bare = Arc::new(Screen {
        snapshots: StdMutex::new(vec![page("w1", vec![element("link", "Help", "node-3")])].into()),
        states: StdMutex::new(vec![state("w1", 1)].into()),
        pixels: Some(Raster::filled(400, 300, [250, 250, 250])),
        ..Screen::default()
    });
    let shot = Computer::with_view(bare, vision(1280))
        .call(call(&json!({"action": "screenshot"})), &cx)
        .await?;
    assert!(
        shot.text
            .contains("no element has a mark: reach what you see with keys."),
        "{}",
        shot.text
    );
    let image = shot.image.ok_or("no image")?;
    assert!(
        !image.marked && image.plain.is_none(),
        "nothing marked, nothing to say about marks"
    );
    Ok(())
}

// TEST: TOOL-U-045
#[tokio::test]
async fn tool_u_045_a_resumed_run_carries_on_from_its_journals_counts() -> TestResult {
    let directory = tempfile::tempdir()?;
    let (cx, _) = context(&directory)?;
    let view = View {
        limits: tqa_types::ComputerLimits::new(2, 3)?,
        spent: Spent {
            screenshots: 2,
            actions: 3,
        },
        ..grounded(1280, tqa_types::ComputerLimits::default())
    };
    let tool = Computer::with_view(marked_screen(), view);
    let screenshot = refusal(
        &tool,
        &cx,
        json!({"action": "screenshot", "x": null, "y": null, "to": null}),
    )
    .await;
    assert!(screenshot.starts_with("screenshot_limit"), "{screenshot}");
    let key = refusal(
        &tool,
        &cx,
        json!({"action": "key", "keys": "Tab", "x": null, "y": null, "to": null}),
    )
    .await;
    assert!(key.starts_with("action_limit"), "{key}");
    Ok(())
}

// TEST: TOOL-U-045
#[tokio::test]
async fn tool_u_045_a_screenshot_too_detailed_for_the_store_is_never_sent_or_counted() -> TestResult
{
    let directory = tempfile::tempdir()?;
    let (cx, _) = context(&directory)?;
    // Noise doesn't compress: its PNG stays over 512 KiB even at the smallest side the tool scales to.
    let mut state = 0x2545_f491_u32;
    let rgb = (0..1280 * 800 * 3)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            state.to_le_bytes()[0]
        })
        .collect();
    let busy = Arc::new(Screen {
        snapshots: StdMutex::new(vec![page("w1", vec![])].into()),
        pixels: Some(Raster {
            width: 1280,
            height: 800,
            rgb,
        }),
        ..Screen::default()
    });
    let tool = Computer::with_view(busy, grounded(1280, tqa_types::ComputerLimits::default()));
    let refused = refusal(
        &tool,
        &cx,
        json!({"action": "screenshot", "x": null, "y": null, "to": null}),
    )
    .await;
    assert!(refused.starts_with("screenshot_too_large"), "{refused}");
    assert!(
        refused.contains("work from observe"),
        "the model is told what to do instead: {refused}"
    );
    assert_eq!(
        tool.screenshots.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "not counted"
    );
    assert!(
        tool.latest.lock().await.is_none(),
        "and no snapshot taken for it"
    );
    Ok(())
}
