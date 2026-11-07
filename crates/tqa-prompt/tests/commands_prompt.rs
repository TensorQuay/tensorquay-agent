//! PROMPT-U-015: the system prompt names the commands the environment has, as the runtime found them on its PATH, so the
//! model uses python3 (or python) where it exists and doesn't try a tool that isn't there. `python` next to `python3` is
//! the same program, said once. With none of the usual commands, it says the shell's own tools are what there is.
use tqa_prompt::{PROMPT_VERSION, WorkspaceFacts, system_prompt};
use tqa_types::{Environment, NetworkSnapshot};

fn render(commands: &[&str]) -> String {
    system_prompt(&WorkspaceFacts {
        root_display: "/workspace",
        os: "Linux",
        environment: &Environment::default(),
        network: &NetworkSnapshot::default(),
        commands,
        host_keys: &[],
    })
}

// TEST: PROMPT-U-015
#[test]
fn prompt_u_015_the_prompt_names_the_commands_the_environment_has() {
    let prompt = render(&["python3", "python", "node", "npm", "go", "git"]);
    assert!(
        prompt.contains("\nCommands: python3 (also as python), node, npm, go, git\n"),
        "{prompt}"
    );
    assert!(render(&["python"]).contains("\nCommands: python\n"));
    assert!(
        render(&[]).contains(
            "\nCommands: only the shell's own; none of the usual language tools are installed\n"
        ),
        "{}",
        render(&[])
    );
    assert_eq!(PROMPT_VERSION, 14, "the prompt contract changed");
}
