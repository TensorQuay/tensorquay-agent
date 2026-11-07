//! The accessibility reader on zbus 6 without a desktop (TOOL-U-068): handles, which errors mean an element is gone,
//! and a bus that isn't there. The tree on a real desktop is RT-I-046's and RT-I-047's.
use super::atspi_tree::{AtspiTree, gone_or_unavailable, objects, split, unknown};
use crate::builtin::computer::driver::DriverError;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn unavailable() -> DriverError {
    DriverError::Unavailable("the accessibility bus isn't answering".into())
}

#[test]
fn tool_u_068_a_handle_splits_into_its_bus_name_and_path() {
    // TEST: TOOL-U-068
    assert_eq!(
        split(":1.23/org/a11y/atspi/accessible/42"),
        Ok((":1.23", "/org/a11y/atspi/accessible/42"))
    );
    assert_eq!(split(":1.23"), Err(DriverError::Gone));
}

#[test]
fn tool_u_068_an_unknown_object_or_method_is_gone_and_anything_else_is_the_bus() {
    // TEST: TOOL-U-068
    for standard in [
        zbus::fdo::Error::UnknownObject("no such object".into()),
        zbus::fdo::Error::UnknownMethod("no such method".into()),
    ] {
        assert_eq!(
            gone_or_unavailable(zbus::Error::FDO(Box::new(standard))),
            DriverError::Gone
        );
    }
    assert_eq!(
        gone_or_unavailable(zbus::Error::FDO(Box::new(zbus::fdo::Error::Failed(
            "busy".into()
        )))),
        unavailable()
    );
    assert_eq!(
        gone_or_unavailable(zbus::Error::InvalidReply),
        unavailable()
    );
    // A non-standard error is a `MethodError` by name: a toolkit's own unknown-object error is gone too.
    assert!(unknown("org.freedesktop.DBus.Error.UnknownObject"));
    assert!(unknown("org.freedesktop.DBus.Error.UnknownMethod"));
    assert!(unknown("org.gtk.GDBus.UnknownObject"));
    assert!(!unknown("org.freedesktop.DBus.Error.Failed"));
    assert!(!unknown("org.freedesktop.DBus.Error.UnknownInterface"));
}

#[tokio::test]
async fn tool_u_068_a_bus_that_isnt_there_is_unavailable() {
    // TEST: TOOL-U-068
    let connected = AtspiTree::connect_to("unix:path=/nonexistent/tqa-tool-u-068/bus").await;
    assert!(matches!(connected, Err(error) if error == unavailable()));
}

#[test]
fn tool_u_068_a_named_null_child_is_skipped_and_the_frame_found() -> TestResult {
    // TEST: TOOL-U-068
    // An application's children as GTK's bridge sends them: a null child with the application's own unique name
    // ahead of its frame. It is no object, so nothing asks it for its state and the frame after it is found.
    let child = |name: &str, path: &str| -> Result<(String, zbus::OwnedObjectPath), zbus::Error> {
        Ok((name.to_owned(), zbus::OwnedObjectPath::try_from(path)?))
    };
    let children = [
        child(":1.5", "/org/a11y/atspi/null")?,
        child(":1.5", "/org/a11y/atspi/accessible/2")?,
        child("", "/org/a11y/atspi/null")?,
        child(":1.5", "/org/a11y/atspi/accessible/9")?,
    ];
    assert_eq!(
        objects(&children),
        vec![
            (":1.5", "/org/a11y/atspi/accessible/2"),
            (":1.5", "/org/a11y/atspi/accessible/9")
        ]
    );
    assert_eq!(objects(&[]), Vec::<(&str, &str)>::new());
    Ok(())
}
