//! GW-U-232: a task digest's text (ADR-0041 section 3): the title, the date, the files and the answer's first lines,
//! cut to their bounds; whatever looks like a key is left out and said so; a new title replaces the first line only;
//! the answer is the last step's selected attempt; a woken or briefed run is not a person's task ending.
use super::*;
use tqa_types::{Event, ServerFrame};

fn frame(event: Event) -> ServerFrame {
    ServerFrame::Event {
        seq: 1.into(),
        run_id: None,
        step: None,
        ts: "2026-01-01T00:00:00Z".into(),
        event,
    }
}

/// A made-up key, generated here so the source holds nothing key-shaped for the secrets scan (G-10).
fn made_up_key() -> String {
    const BASE62: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    let mut state: u64 = 11;
    let text: String = (0..40)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            char::from(BASE62[usize::try_from(state % 62).unwrap_or(0)])
        })
        .collect();
    format!("sk-{text}")
}

// TEST: GW-U-232
#[test]
fn gw_u_232_a_digest_holds_the_title_the_date_the_files_and_the_answers_first_lines() {
    let files = vec!["report.md".to_owned(), "data/sales.csv".to_owned()];
    let text = compose(&Digest {
        title: "Summarise sales by region",
        date: "2026-11-20",
        files: Some(&files),
        answer: "The report is in report.md.\n\nNorth grew 12 %.\n",
    });
    assert_eq!(
        text,
        "# Summarise sales by region\ndate: 2026-11-20\nfiles: report.md, data/sales.csv\n\nThe report is in \
         report.md.\nNorth grew 12 %.\n"
    );
    // No comparison: no files line. Nothing changed: said so. No answer: no blank tail.
    let bare = compose(&Digest {
        title: "Ask a question",
        date: "2026-11-20",
        files: None,
        answer: "",
    });
    assert_eq!(bare, "# Ask a question\ndate: 2026-11-20\n");
    let none = compose(&Digest {
        title: "Look around",
        date: "2026-11-20",
        files: Some(&[]),
        answer: "Nothing to change.",
    });
    assert_eq!(
        none,
        "# Look around\ndate: 2026-11-20\nfiles: none changed\n\nNothing to change.\n"
    );
}

// TEST: GW-U-232
#[test]
fn gw_u_232_the_bounds_hold_and_markup_cannot_open_at_the_title() {
    let many: Vec<String> = (0..25).map(|n| format!("f{n:02}.txt")).collect();
    let long = "x".repeat(2_000);
    let lines = (0..20)
        .map(|n| format!("line {n}"))
        .collect::<Vec<_>>()
        .join("\n");
    let text = compose(&Digest {
        title: "## > - Injected\nheading",
        date: "2026-11-20",
        files: Some(&many),
        answer: &lines,
    });
    assert!(text.starts_with("# Injected heading\n"), "{text}");
    assert!(text.contains("f19.txt (and 5 more)\n"), "{text}");
    assert!(!text.contains("f20.txt"), "{text}");
    assert!(
        text.contains("line 7\n") && !text.contains("line 8"),
        "{text}"
    );
    let cut = first_lines(&long);
    assert_eq!(cut.chars().count(), MAX_ANSWER_CHARS + 1);
    assert!(cut.ends_with('…'));
    assert_eq!(
        compose(&Digest {
            title: " \n ",
            date: "2026-11-20",
            files: None,
            answer: ""
        }),
        "# (untitled task)\ndate: 2026-11-20\n"
    );
}

// TEST: GW-U-232
#[test]
fn gw_u_232_what_looks_like_a_key_is_left_out_and_said_so() {
    let key = made_up_key();
    let files = vec!["notes.md".to_owned(), format!("out/{key}.txt")];
    let text = compose(&Digest {
        title: &format!("Deploy with {key}"),
        date: "2026-11-20",
        files: Some(&files),
        answer: &format!("Done. The token is {key}."),
    });
    assert!(!text.contains(&key), "{text}");
    assert!(
        text.starts_with("# (title left out: it holds what looks like a key)\n"),
        "{text}"
    );
    assert!(text.contains("files: notes.md (and 1 more)\n"), "{text}");
    assert!(
        text.ends_with("\n(answer left out: it holds what looks like a key)\n"),
        "{text}"
    );
}

// TEST: GW-U-232
#[test]
fn gw_u_232_a_new_title_replaces_the_first_line_only() {
    let digest = "# Write the report\ndate: 2026-11-20\nfiles: report.md\n\nDone.\n";
    assert_eq!(
        retitle(digest, "Sales report by region"),
        "# Sales report by region\ndate: 2026-11-20\nfiles: report.md\n\nDone.\n"
    );
    assert_eq!(retitle("# Old", "New"), "# New\n");
}

// TEST: GW-U-232
#[test]
fn gw_u_232_the_answer_is_the_last_attempts_text_and_a_woken_run_is_no_task_end()
-> Result<(), Box<dyn std::error::Error>> {
    let mut answer = String::new();
    for event in [
        Event::StepStarted {
            step: 2,
            attempt: 1,
        },
        Event::TextDelta {
            text: "The rep".into(),
        },
        // The stream was cut and the step retried: its text starts again.
        Event::StepStarted {
            step: 2,
            attempt: 2,
        },
        Event::TextDelta {
            text: "The report ".into(),
        },
        Event::TextDelta {
            text: "is ready.".into(),
        },
    ] {
        answer_step(&mut answer, &frame(event));
    }
    assert_eq!(answer, "The report is ready.");
    let person = frame(Event::MessageUser {
        text: "Write the report".into(),
        attachments: vec![],
        lead_brief: None,
        team_wake: None,
    });
    assert!(!woken(Some(&person)));
    assert!(!woken(None), "an unknown opener counts as the person's");
    // The gateway's wake of a team reader, and a lead's brief to a helper, are not a person's words.
    for opener in [
        serde_json::json!({"type":"message.user","text":"Messages wait.","attachments":[],"team_wake":{"after":3}}),
        serde_json::json!({"type":"message.user","text":"Find hotels.","attachments":[],
            "lead_brief":{"role":"researcher","workspace":"own_copy","brief":{"objective":"Find three hotels","output":"a list","boundaries":"no bookings"}}}),
    ] {
        let event = serde_json::from_value::<Event>(opener.clone())?;
        assert!(woken(Some(&frame(event))), "{opener}");
    }
    Ok(())
}

// TEST: GW-U-232
#[test]
fn gw_u_232_a_task_keeps_one_file_named_by_its_first_day_and_whole_id()
-> Result<(), Box<dyn std::error::Error>> {
    let created = time::Date::from_calendar_date(2026, time::Month::November, 9)?;
    let task: Uuid = "0190f3a2-7c1e-7d55-9b0a-3f6c2d1e4a5b".parse()?;
    let file = name(created, task);
    assert_eq!(file, "2026-11-09-0190f3a2-7c1e-7d55-9b0a-3f6c2d1e4a5b.md");
    assert!(memory::valid_name(&file));
    Ok(())
}
