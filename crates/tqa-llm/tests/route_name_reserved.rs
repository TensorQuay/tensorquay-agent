//! GW-U-098: `new` is the address of the web's Add route page (`/settings/models/new`), so no route may take it as its
//! name: the file refuses it at start in plain words, and a record (the admin API's path) with its own reason code.

use std::error::Error;
use tqa_llm::{Registry, RouteInvalid};

type TestResult = Result<(), Box<dyn Error>>;

fn file(name: &str) -> String {
    format!(
        "[[routes]]\nname = \"{name}\"\nprotocol = \"openai_chat\"\nbase_url = \"http://host.docker.internal:11434/v1\"\n\
         model = \"qwen3:4b\"\nauth = {{ kind = \"none\" }}\n"
    )
}

#[test]
fn gw_u_098_the_route_name_new_is_refused_from_the_file_in_plain_words() -> TestResult {
    let error = Registry::from_toml(&file("new"))
        .err()
        .ok_or("the file was accepted")?;
    assert_eq!(error.reason(), Some(&RouteInvalid::RouteNameReserved));
    assert_eq!(
        error.reason().map(RouteInvalid::code),
        Some("name_reserved")
    );
    let message = error.to_string();
    assert!(
        message.contains("the route name new is reserved"),
        "{message}"
    );
    Ok(())
}

#[test]
fn gw_u_098_names_that_only_contain_new_stay_fine() -> TestResult {
    for name in ["new-model", "newer", "renew"] {
        Registry::from_toml(&file(name))?;
    }
    Ok(())
}
