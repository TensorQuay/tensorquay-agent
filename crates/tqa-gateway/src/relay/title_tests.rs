//! GW-U-085: the short-title call's request is tiny and bounded, and its answer is taken only as a clean 1–8 word line.
use super::{Script, clean, fits, request, script};
use tqa_llm::{ContentPart, Role};

fn route(extra: &str) -> Result<tqa_llm::Route, Box<dyn std::error::Error>> {
    let registry = tqa_llm::Registry::from_toml(&format!(
        "[[routes]]\nname='r'\nprotocol='openai_chat'\nbase_url='https://models.example/v1'\nmodel='m'\nauth={{kind='none'}}\n{extra}"
    ))?;
    Ok(registry.get("r").ok_or("route")?.clone())
}

fn text(message: &tqa_llm::Message) -> String {
    message
        .content
        .iter()
        .map(|part| match part {
            ContentPart::Text(text) => text.clone(),
            _ => String::new(),
        })
        .collect()
}

// TEST: GW-U-085
#[test]
fn gw_u_085_the_request_names_the_task_in_a_few_words_and_is_bounded()
-> Result<(), Box<dyn std::error::Error>> {
    let long = "Clean up the attached sales CSV. ".repeat(100);
    let asked = request(&long, &route("")?);
    assert!(asked.tools.is_empty(), "no tools");
    assert_eq!(asked.messages.len(), 2);
    assert_eq!(asked.messages[0].role, Role::System);
    assert!(text(&asked.messages[0]).contains("3 to 6 words"));
    assert_eq!(asked.messages[1].role, Role::User);
    assert!(
        text(&asked.messages[1]).chars().count() <= 1_000,
        "the task's text is cut"
    );
    assert!(asked.max_output.is_some_and(|tokens| tokens <= 256));
    // A route's own, smaller output limit wins.
    assert_eq!(request("x", &route("max_output=16")?).max_output, Some(16));
    Ok(())
}

// TEST: GW-U-085
#[test]
fn gw_u_085_an_answer_is_one_clean_short_line_or_nothing() {
    for (answer, title) in [
        ("Sales CSV cleanup", Some("Sales CSV cleanup")),
        ("\"Sales CSV cleanup.\"", Some("Sales CSV cleanup")),
        ("Title: Sales CSV cleanup", Some("Sales CSV cleanup")),
        ("**Sales CSV cleanup**", Some("Sales CSV cleanup")),
        (
            "  Sales   CSV cleanup  \nThis names the task.",
            Some("Sales CSV cleanup"),
        ),
        ("\n\nQuarterly report draft", Some("Quarterly report draft")),
        (
            "Réviser le rapport trimestriel",
            Some("Réviser le rapport trimestriel"),
        ),
        ("", None),
        ("   \n  ", None),
        ("one two three four five six seven eight nine", None),
        (&"x".repeat(81), None),
        ("Sales\u{0007} cleanup", Some("Sales cleanup")),
        // A local model that reasons in its text: the reasoning block goes, and the name after it stays.
        (
            "<think>The user wants a name.\nMaybe about CSV.</think>\nSales CSV cleanup",
            Some("Sales CSV cleanup"),
        ),
        (
            "<think></think>Sales CSV cleanup",
            Some("Sales CSV cleanup"),
        ),
        (
            "<thinking>\nshort</thinking>\n\nSales CSV cleanup",
            Some("Sales CSV cleanup"),
        ),
        (
            "<reasoning>names are short</reasoning> Sales CSV cleanup",
            Some("Sales CSV cleanup"),
        ),
        (
            "<THINK>x</THINK>Sales CSV cleanup",
            Some("Sales CSV cleanup"),
        ),
        // Cut off while reasoning, or any other markup: no title.
        ("<think>\nThe user wants a name for", None),
        ("<answer>Sales</answer>", None),
        ("<think>", None),
        // A Markdown label, and a full stop after a closing quote.
        ("**Title:** Sales CSV cleanup", Some("Sales CSV cleanup")),
        ("__Title:__ Sales CSV cleanup", Some("Sales CSV cleanup")),
        ("\"Sales CSV cleanup\".", Some("Sales CSV cleanup")),
    ] {
        assert_eq!(clean(answer).as_deref(), title, "{answer:?}");
    }
}

// TEST: GW-U-096
/// F23 (candidate 12, qwen2.5): an English request got a Chinese title. The call names the writing system the task's
/// words are in, and a title in another one is refused, so the task keeps its first words.
#[test]
fn gw_u_096_the_writing_system_of_a_task_is_its_letters_majority() {
    for (text, expected) in [
        ("Create a CSV of 2025 regional sales", Some(Script::Latin)),
        ("Réviser le rapport trimestriel", Some(Script::Latin)),
        ("Tạo báo cáo doanh số", Some(Script::Latin)),
        ("创建2025年区域销售CSV", Some(Script::Han)),
        ("売上のCSVを作成して", Some(Script::Japanese)),
        ("東京の天気", Some(Script::Japanese)),
        ("2025년 지역 매출 CSV 만들기", Some(Script::Hangul)),
        ("สร้างรายงานยอดขาย", Some(Script::Thai)),
        ("விற்பனை அறிக்கை", Some(Script::Tamil)),
        ("Создать отчёт о продажах", Some(Script::Cyrillic)),
        ("2025 / 42 %", None),
        ("", None),
    ] {
        assert_eq!(script(text), expected, "{text:?}");
    }
}

// TEST: GW-U-096
#[test]
fn gw_u_096_the_request_names_the_tasks_writing_system() -> Result<(), Box<dyn std::error::Error>> {
    for (task, named) in [
        (
            "Create a CSV of 2025 regional sales",
            "written in the Latin alphabet",
        ),
        (
            "创建一个2025年区域销售的CSV文件",
            "written in Chinese characters",
        ),
        ("売上のCSVを作成して", "written in Japanese"),
        ("2025년 지역 매출 CSV 만들기", "written in Korean Hangul"),
    ] {
        let system = text(&request(task, &route("")?).messages[0]);
        assert!(system.contains("same language as the task"), "{system}");
        assert!(system.contains(named), "{task}: {system}");
    }
    // Words with no letters name no writing system, and ask for the task's own language as before.
    let system = text(&request("2025 / 42", &route("")?).messages[0]);
    assert!(!system.contains("written in"), "{system}");
    Ok(())
}

// TEST: GW-U-096
#[test]
fn gw_u_096_a_title_in_another_writing_system_is_refused() {
    for (task, title, fits_task) in [
        // F23's case: the task keeps its first words.
        (
            "Create a CSV of 2025 regional sales",
            "创建2025年区域销售CSV",
            false,
        ),
        (
            "Create a CSV of 2025 regional sales",
            "Regional sales CSV",
            true,
        ),
        (
            "创建一个2025年区域销售的CSV文件",
            "创建2025年区域销售CSV",
            true,
        ),
        (
            "创建一个2025年区域销售的CSV文件",
            "Regional sales CSV",
            false,
        ),
        ("创建一个2025年区域销售的CSV文件", "売上のCSVを作成", false),
        // A Japanese title may be all kanji.
        ("売上のCSVを作成して", "売上CSV作成", true),
        ("売上のCSVを作成して", "Sales CSV", false),
        ("2025년 지역 매출 CSV 만들기", "지역 매출 CSV", true),
        ("Создать отчёт о продажах", "Отчёт о продажах", true),
        ("Создать отчёт о продажах", "Sales report", false),
        // With no letters on one side, there is nothing to compare.
        ("2025 / 42", "Quarterly numbers", true),
        ("Tally the 2025 numbers", "2025", true),
    ] {
        assert_eq!(fits(task, title), fits_task, "{task:?} → {title:?}");
    }
}
