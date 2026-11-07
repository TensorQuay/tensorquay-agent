//! TOOL-U-050 (ADR-0039 section 2): an action's evidence. The screen before an action its checks let through comes from
//! the pre-start step, and the screen after it on the output; both unmarked and at the display's size. `observe`,
//! `screenshot` and a refused action take none. A child of the computer tool's tests, sharing their fakes.
use super::*;

const PNG_MAGIC: &[u8] = b"\x89PNG\r\n\x1a\n";

fn assert_unmarked_screen(image: Option<&crate::ToolImage>, what: &str) -> TestResult {
    let image = image.ok_or_else(|| format!("{what}: no capture"))?;
    assert_eq!(
        (image.width, image.height),
        (400, 300),
        "{what}: the display's size"
    );
    assert!(!image.marked && image.plain.is_none(), "{what}: unmarked");
    assert!(image.png.starts_with(PNG_MAGIC), "{what}: a PNG");
    Ok(())
}

// TEST: TOOL-U-050
#[tokio::test]
async fn tool_u_050_an_action_takes_the_screen_before_and_after_it_unmarked() -> TestResult {
    let directory = tempfile::tempdir()?;
    let (cx, _) = context(&directory)?;
    // Evidence is the person's record, not the model's view: a route without images still takes it.
    let tool = Computer::new(marked_screen());
    tool.call(call(&json!({"action": "observe"})), &cx).await?;
    let click = call(&json!({"action": "click", "ref": "e2"}));

    let before = tool.before(&click, &cx).await;
    let output = tool.call(click, &cx).await?;

    assert_unmarked_screen(before.as_ref(), "before")?;
    let evidence = output.evidence.ok_or("the action took evidence")?;
    assert_unmarked_screen(evidence.after.as_ref(), "after")?;
    Ok(())
}

// TEST: TOOL-U-050
#[tokio::test]
async fn tool_u_050_observe_and_screenshot_take_no_evidence() -> TestResult {
    let directory = tempfile::tempdir()?;
    let (cx, _) = context(&directory)?;
    let tool = Computer::with_view(marked_screen(), vision(1280));
    for action in ["observe", "screenshot"] {
        let args = call(&json!({"action": action}));
        assert!(
            tool.before(&args, &cx).await.is_none(),
            "{action}: no before"
        );
        let output = tool.call(args, &cx).await?;
        assert!(output.evidence.is_none(), "{action}: no evidence");
    }
    Ok(())
}

// TEST: TOOL-U-050
#[tokio::test]
async fn tool_u_050_an_action_its_checks_will_refuse_takes_no_before() -> TestResult {
    let directory = tempfile::tempdir()?;
    let (cx, _) = context(&directory)?;
    // A point before any screenshot has nothing to map from.
    let grounded = View {
        vision: true,
        pixel_grounding: true,
        max_image_side: 1280,
        limits: tqa_types::ComputerLimits::new(2, 1)?,
        ..View::default()
    };
    let tool = Computer::with_view(marked_screen(), grounded);
    let point = json!({"action": "click", "x": 10, "y": 10, "ref": null, "to": null});
    assert!(
        tool.before(&call(&point), &cx).await.is_none(),
        "no screenshot yet"
    );
    // A ref with no snapshot yet, then one the latest snapshot doesn't have: both `stale_ref`, so no lone before.
    let click_ref = |element: &str| {
        call(&json!({"action": "click", "ref": element, "x": null, "y": null, "to": null}))
    };
    assert!(
        tool.before(&click_ref("e2"), &cx).await.is_none(),
        "no snapshot yet"
    );
    // The one action the cap allows, then one past it.
    tool.call(
        call(&json!({"action": "observe", "x": null, "y": null, "to": null})),
        &cx,
    )
    .await?;
    assert!(
        tool.before(&click_ref("e9"), &cx).await.is_none(),
        "not in the snapshot"
    );
    let click = click_ref("e2");
    assert!(tool.before(&click, &cx).await.is_some(), "within the cap");
    tool.call(click.clone(), &cx).await?;
    assert!(tool.before(&click, &cx).await.is_none(), "the cap is spent");
    let refused = refusal(
        &tool,
        &cx,
        json!({"action": "click", "ref": "e2", "x": null, "y": null, "to": null}),
    )
    .await;
    assert!(refused.starts_with("action_limit"), "{refused}");
    Ok(())
}

// TEST: TOOL-U-050
#[tokio::test]
async fn tool_u_050_a_screen_without_pixels_still_acts_and_says_nothing_was_captured() -> TestResult
{
    let directory = tempfile::tempdir()?;
    let (cx, _) = context(&directory)?;
    let tool = Computer::new(Screen::with(vec![form()], vec![state("w1", 1)]));
    tool.call(call(&json!({"action": "observe"})), &cx).await?;
    let click = call(&json!({"action": "click", "ref": "e2"}));
    assert!(tool.before(&click, &cx).await.is_none());
    let output = tool.call(click, &cx).await?;
    assert_eq!(output.text, "Clicked e2; no visible change.");
    let evidence = output.evidence.ok_or("the action still took evidence")?;
    assert!(evidence.after.is_none(), "but no picture of it");
    Ok(())
}
