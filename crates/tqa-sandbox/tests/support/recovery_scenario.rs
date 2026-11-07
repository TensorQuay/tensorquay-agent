//! Five task variants keep the long demo's write/read/search/test/fix/verify sequence.
use serde_json::{Value, json};
use std::{error::Error, time::Duration};
use tqa_testkit::{Script, Scripted};

pub const START: Duration = Duration::from_secs(1);
pub const MODEL: Duration = Duration::from_secs(61);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cut {
    Model,
    Tool,
}

#[derive(Clone, Copy, Debug)]
#[allow(
    dead_code,
    reason = "The manual restart binary selects one task from the shared matrix."
)]
pub enum Task {
    Arithmetic,
    StableUnique,
    Words,
    CsvTotals,
    Bounds,
}

impl Task {
    pub fn recipe(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Self::Arithmetic => (
                "def solve(left, right):\n    return left - right\n",
                "def solve(left, right):\n    return left + right\n",
                "        self.assertEqual(solve(19, 23), 42)\n\n    def test_boundary(self):\n        self.assertEqual(solve(0, 0), 0)\n",
            ),
            Self::StableUnique => (
                "def solve(values):\n    return sorted(set(values))\n",
                "def solve(values):\n    return list(dict.fromkeys(values))\n",
                "        self.assertEqual(solve([3, 1, 3, 2]), [3, 1, 2])\n\n    def test_boundary(self):\n        self.assertEqual(solve([]), [])\n",
            ),
            Self::Words => (
                "def solve(text):\n    return len(text)\n",
                "def solve(text):\n    return len(text.split())\n",
                "        self.assertEqual(solve('one  two\\nthree'), 3)\n\n    def test_boundary(self):\n        self.assertEqual(solve(''), 0)\n",
            ),
            Self::CsvTotals => (
                "import csv\nimport io\n\ndef solve(text):\n    return len(list(csv.reader(io.StringIO(text))))\n",
                "import csv\nimport io\n\ndef solve(text):\n    return sum(int(row[1]) for row in csv.reader(io.StringIO(text)))\n",
                "        self.assertEqual(solve('a,2\\nb,3\\n'), 5)\n\n    def test_boundary(self):\n        self.assertEqual(solve(''), 0)\n",
            ),
            Self::Bounds => (
                "def solve(values, index):\n    return values[index]\n",
                "def solve(values, index):\n    return values[index] if 0 <= index < len(values) else None\n",
                "        self.assertIsNone(solve([4, 5], -1))\n\n    def test_boundary(self):\n        self.assertEqual(solve([4, 5], 0), 4)\n",
            ),
        }
    }
}

pub fn scripts(task: Task, cut: Cut) -> Result<Vec<Script>, Box<dyn Error>> {
    let document: Value = serde_json::from_str(include_str!(
        "../../../tqa-testkit/scenarios/demo-long.json"
    ))?;
    let mut scripts = Vec::new();
    for (step, response) in document["responses"]
        .as_array()
        .ok_or("responses")?
        .iter()
        .enumerate()
    {
        let mut events = vec![Scripted::Text(format!("{task:?}: step {}.\n", step + 1))];
        for event in response["events"].as_array().ok_or("events")? {
            if event["type"] == "tool_call" {
                events.push(tool(task, cut, step, event)?);
            }
        }
        if step == 0 {
            events.push(call(
                "shell",
                &json!({"command":"printf 'created\\n' >> effects.log"}),
            ));
        }
        let script = Script::new(events);
        if step == 3 && cut == Cut::Model {
            scripts.push(Script::new(vec![
                Scripted::Text("model interruption checkpoint".into()),
                Scripted::Delay(MODEL),
                Scripted::Text("must not finish before replacement".into()),
            ]));
        }
        scripts.push(script);
    }
    scripts.first_mut().ok_or("empty scenario")?.header_delay = START;
    Ok(scripts)
}

fn tool(task: Task, cut: Cut, step: usize, event: &Value) -> Result<Scripted, Box<dyn Error>> {
    let name = event["name"].as_str().ok_or("tool name")?;
    let mut arguments = event["arguments"].clone();
    if let Some(path) = arguments["path"].as_str() {
        arguments["path"] = json!(path.replace("arithmetic", "solution"));
    }
    let (broken, fixed, cases) = task.recipe();
    match (name, arguments["path"].as_str()) {
        ("write_file", Some("demo/solution.py")) => {
            arguments["content"] = json!(broken);
            arguments["create_parents"] = json!(true);
        }
        ("write_file", Some("demo/test_solution.py")) => {
            arguments["content"] = json!(format!(
                "import unittest\nfrom solution import solve\n\nclass Checks(unittest.TestCase):\n    def test_case(self):\n{cases}"
            ));
        }
        ("edit_file", Some("demo/solution.py")) => {
            arguments["edits"] = json!([{"old_text":broken,"new_text":fixed}]);
        }
        ("edit_file", Some("demo/README.md")) => {
            arguments["edits"][0]["new_text"] = json!("Verified: two tests pass.");
        }
        ("shell", _) => {
            let marker = if step == 2 { "initial" } else { "verified" };
            let barrier = if step == 2 && cut == Cut::Tool {
                "touch tool-ready; while [ ! -f release-tool ]; do sleep 0.02; done; "
            } else {
                ""
            };
            arguments["command"] = json!(format!(
                "printf '{marker}\\n' >> effects.log; {barrier}python3 -B -m unittest discover -s demo -v"
            ));
        }
        _ => {}
    }
    Ok(call(name, &arguments))
}

fn call(name: &str, arguments: &Value) -> Scripted {
    Scripted::ToolCall {
        name: name.into(),
        arguments_json: arguments.to_string(),
        split_into: 4,
    }
}
