//! RT-I-021: skills reach a run through the runtime: the index in the instruction slot, byte-identical across steps,
//! the `skill` tool only when a skill exists, and one `context.loaded` event after `run.started`. RT-I-028: a profile
//! narrows the built-in tools and the skills, and adds its instructions as their own section.
#[path = "support/fixture.rs"]
mod support;
use serde_json::Value;
use std::{fs, path::Path};
use support::*;
use tqa_testkit::{Script, Scripted};
use tqa_types::{
    ClientFrame, Event, LoadedInstruction, LoadedSkill, ServerFrame, SkillSkipReason, SkillTier,
    SkippedSkill, ToolStatus,
};

fn skill(root: &Path, name: &str, description: &str) -> std::io::Result<()> {
    fs::create_dir_all(root.join(name))?;
    fs::write(
        root.join(name).join("SKILL.md"),
        format!("---\nname: {name}\ndescription: {description}\n---\nStep one of {name}.\n"),
    )
}

/// Runs one turn and returns the stored events and the model request bodies.
async fn run(f: &Fixture) -> Result<(Vec<Event>, Vec<Value>), Box<dyn std::error::Error>> {
    f.gateway.set_bound(true);
    let (shutdown, task) = f.launch()?;
    f.gateway.wait_for_registrations(1).await?;
    f.gateway.send(user()).await?;
    f.wait_event("run.completed").await?;
    stop(shutdown, task).await?;
    let events = f
        .gateway
        .stored()
        .into_iter()
        .filter_map(|frame| match frame {
            ServerFrame::Event { event, .. } => Some(event),
            _ => None,
        })
        .collect();
    let bodies = f
        .gateway
        .model_requests()
        .into_iter()
        .map(|request| request.body)
        .collect();
    Ok((events, bodies))
}
fn tool_names(body: &Value) -> Vec<String> {
    body["tools"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|tool| tool["function"]["name"].as_str().map(str::to_owned))
        .collect()
}
fn system_text(body: &Value) -> String {
    body["messages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|message| message["role"] == "system")
        .map(|message| message["content"].to_string())
        .collect()
}

/// Install and user tiers with a valid, an invalid and a shadowed skill, and an `AGENTS.md`; returns the mount.
fn tiers(f: &Fixture) -> std::io::Result<std::path::PathBuf> {
    let skills = f.directory.path().join("skills");
    skill(
        &skills.join("install"),
        "pdf-report",
        "Turn a CSV into a PDF report.",
    )?;
    skill(&skills.join("install"), "notes", "Install copy.")?;
    fs::create_dir_all(skills.join("install/broken"))?;
    fs::write(skills.join("install/broken/SKILL.md"), "no front matter\n")?;
    skill(&skills.join("user"), "notes", "Keep meeting notes.")?;
    fs::write(
        f.directory.path().join("workspace/AGENTS.md"),
        "Be brief.\n",
    )?;
    Ok(skills)
}
fn expected_report() -> Event {
    Event::ContextLoaded {
        instructions: vec![LoadedInstruction {
            path: "AGENTS.md".into(),
            truncated: false,
        }],
        skills: vec![
            LoadedSkill {
                name: "notes".into(),
                tier: SkillTier::User,
                description: "Keep meeting notes.".into(),
            },
            LoadedSkill {
                name: "pdf-report".into(),
                tier: SkillTier::Install,
                description: "Turn a CSV into a PDF report.".into(),
            },
        ],
        skipped: vec![
            SkippedSkill {
                folder: "broken".into(),
                tier: SkillTier::Install,
                reason: SkillSkipReason::BadFrontMatter,
            },
            SkippedSkill {
                folder: "notes".into(),
                tier: SkillTier::Install,
                reason: SkillSkipReason::Shadowed,
            },
        ],
    }
}

#[tokio::test]
async fn rt_i_021_skills_are_indexed_loadable_and_reported_once() -> TestResult {
    let f = Fixture::in_memory(vec![
        Script::new(vec![Scripted::ToolCall {
            name: "skill".into(),
            arguments_json: r#"{"name":"pdf-report"}"#.into(),
            split_into: 1,
        }]),
        answer(),
    ])
    .await?;
    let skills = tiers(&f)?;
    let (events, bodies) = run(&f).await?;

    let started = events
        .iter()
        .position(|event| matches!(event, Event::RunStarted { .. }))
        .ok_or("run.started")?;
    let loaded: Vec<_> = events
        .iter()
        .filter(|event| matches!(event, Event::ContextLoaded { .. }))
        .collect();
    assert_eq!(loaded.len(), 1, "emitted once per run");
    assert!(
        matches!(events[started + 1], Event::ContextLoaded { .. }),
        "right after run.started"
    );
    assert_eq!(loaded[0], &expected_report());

    assert_eq!(bodies.len(), 2);
    let slot = system_text(&bodies[0]);
    assert!(slot.contains("Be brief."), "{slot}");
    assert!(
        slot.contains("- notes (user): Keep meeting notes.\\n- pdf-report (install): Turn a CSV"),
        "{slot}"
    );
    assert!(!slot.contains("broken"));
    assert_eq!(
        system_text(&bodies[1]),
        slot,
        "the instruction slot is byte-identical across steps"
    );
    assert!(tool_names(&bodies[0]).contains(&"skill".to_owned()));

    let result = events
        .iter()
        .find_map(|event| match event {
            Event::ToolResult { content, .. } => Some(content.clone()),
            _ => None,
        })
        .ok_or("tool.result")?;
    let folder = skills.join("install/pdf-report");
    assert!(
        result.starts_with(&format!(
            "Skill pdf-report (install) in {}\n\nStep one of pdf-report.\n",
            folder.display()
        )),
        "{result}"
    );
    Ok(())
}

#[tokio::test]
async fn rt_i_021_without_skills_there_is_no_tool_and_no_index() -> TestResult {
    let f = Fixture::in_memory(vec![answer()]).await?;
    let (events, bodies) = run(&f).await?;
    let loaded: Vec<_> = events
        .iter()
        .filter(|event| matches!(event, Event::ContextLoaded { .. }))
        .collect();
    assert_eq!(
        loaded,
        [&Event::ContextLoaded {
            instructions: vec![],
            skills: vec![],
            skipped: vec![]
        }]
    );
    assert!(!tool_names(&bodies[0]).contains(&"skill".to_owned()));
    assert!(!system_text(&bodies[0]).contains("Skills available"));
    Ok(())
}

// TEST: RT-I-028
#[tokio::test]
async fn rt_i_028_a_profile_narrows_tools_and_skills_and_adds_its_instructions() -> TestResult {
    let f = Fixture::in_memory(vec![
        Script::new(vec![Scripted::ToolCall {
            name: "write_file".into(),
            arguments_json: r#"{"path":"x.txt","content":"no"}"#.into(),
            split_into: 1,
        }]),
        Script::new(vec![Scripted::ToolCall {
            name: "skill".into(),
            arguments_json: r#"{"name":"pdf-report"}"#.into(),
            split_into: 1,
        }]),
        answer(),
    ])
    .await?;
    tiers(&f)?;
    let mut session = serde_json::to_value(session()?)?;
    session["profile"] = serde_json::json!({"name": "reviewer", "instructions": "Review only; never edit.",
        "tools": ["read_file", "skill"], "skills": ["notes"]});
    f.gateway
        .set_session(ClientFrame::parse(&serde_json::to_vec(&session)?)?)?;
    let (events, bodies) = run(&f).await?;

    // Only the allowed built-in tools are offered.
    let mut offered = tool_names(&bodies[0]);
    offered.sort();
    assert_eq!(offered, ["read_file", "skill"]);
    // The instructions are their own section, after the base text and before the repository's files.
    let slot = system_text(&bodies[0]);
    let own = slot
        .find("--- Profile instructions (reviewer) ---\\nReview only; never edit.")
        .ok_or(slot.clone())?;
    let agents = slot.find("Be brief.").ok_or(slot.clone())?;
    assert!(own < agents, "{slot}");
    // The index and the report show only the allowed skills.
    assert!(
        slot.contains("- notes (user): Keep meeting notes."),
        "{slot}"
    );
    assert!(!slot.contains("pdf-report"), "{slot}");
    let reported = events
        .iter()
        .find_map(|event| match event {
            Event::ContextLoaded { skills, .. } => Some(skills.clone()),
            _ => None,
        })
        .ok_or("context.loaded")?;
    assert_eq!(
        reported
            .iter()
            .map(|skill| skill.name.as_str())
            .collect::<Vec<_>>(),
        ["notes"]
    );
    // A tool outside the list is answered as unknown and never runs; the skill tool refuses a filtered-out skill.
    let results: Vec<(ToolStatus, String)> = events
        .iter()
        .filter_map(|event| match event {
            Event::ToolResult {
                status, content, ..
            } => Some((*status, content.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(
        results,
        [
            (
                ToolStatus::Error,
                "unsupported: unknown tool; known tools: read_file, skill".to_owned()
            ),
            (
                ToolStatus::Error,
                "invalid_args: /name: unknown skill; available: notes".to_owned()
            ),
        ]
    );
    assert!(!f.directory.path().join("workspace/x.txt").exists());
    Ok(())
}

// TEST: RT-I-028
#[tokio::test]
async fn rt_i_028_an_allow_list_without_the_skill_tool_indexes_no_skill() -> TestResult {
    let f = Fixture::in_memory(vec![answer()]).await?;
    tiers(&f)?;
    let mut session = serde_json::to_value(session()?)?;
    session["profile"] =
        serde_json::json!({"name": "reader", "tools": ["read_file"], "skills": "all"});
    f.gateway
        .set_session(ClientFrame::parse(&serde_json::to_vec(&session)?)?)?;
    let (events, bodies) = run(&f).await?;
    assert_eq!(tool_names(&bodies[0]), ["read_file"]);
    let slot = system_text(&bodies[0]);
    assert!(!slot.contains("Skills available"), "{slot}");
    assert!(
        slot.contains("Be brief."),
        "the repository's files stay: {slot}"
    );
    let reported = events
        .iter()
        .find_map(|event| match event {
            Event::ContextLoaded { skills, .. } => Some(skills.clone()),
            _ => None,
        })
        .ok_or("context.loaded")?;
    assert!(reported.is_empty(), "{reported:?}");
    Ok(())
}
