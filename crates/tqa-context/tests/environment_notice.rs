//! RT-I-013: replay projects each environment change once at its historical position.
use serde_json::json;
use tqa_context::{Role, project, resume_point};
use tqa_types::ServerFrame;

#[test]
fn rt_i_013_projection_preserves_environment_notices_without_creating_a_user_turn()
-> Result<(), Box<dyn std::error::Error>> {
    let events = [
        json!({"type":"message.user","text":"earlier request","attachments":[]}),
        json!({"type":"environment.changed","template":"workstation",
            "capabilities":["browser","shell","python"]}),
        json!({"type":"environment.changed","template":"minimal","capabilities":[]}),
    ];
    let frames: Vec<ServerFrame> = events
        .into_iter()
        .enumerate()
        .map(|(index, event)| {
            serde_json::from_value(json!({"type":"event","seq":index+1,"run_id":null,
            "step":null,"ts":"2000-01-01T00:00:00Z","event":event}))
        })
        .collect::<Result<_, _>>()?;
    let projected = project(&frames)?;
    assert_eq!(projected.len(), 3);
    assert_eq!(projected[0].role, Role::User);
    for message in &projected[1..] {
        assert_eq!(message.role, Role::System);
    }
    assert_eq!(
        projected[1].text,
        "Environment changed to workstation. Capabilities: shell, python, browser."
    );
    assert_eq!(
        projected[2].text,
        "Environment changed to minimal. Capabilities: none."
    );
    assert_eq!(project(&frames)?, projected);
    assert_eq!(resume_point(&frames)?, resume_point(&frames[..1])?);
    assert!(matches!(
        resume_point(&frames[1..])?,
        tqa_context::ResumePoint::Idle { .. }
    ));
    Ok(())
}
