//! GW-I-110's rules, unit by unit: which files a take places, and their names.
use super::{placed_name, wanted};
use crate::{GatewayError, error::FileRefusal};
use tqa_types::HandedFile;

fn files() -> Vec<HandedFile> {
    ["src/report.md", "data.csv", "src/report.md"]
        .map(|name| HandedFile {
            name: name.to_owned(),
            size: 1,
            sha256: "0".repeat(64),
        })
        .to_vec()
}

fn not_found(path: &str) -> GatewayError {
    GatewayError::ForFile {
        path: path.to_owned(),
        refusal: FileRefusal::NotFound,
    }
}

// TEST: GW-I-110
#[test]
fn gw_i_110_a_take_places_all_files_or_those_named_by_their_report_path() {
    let files = files();
    assert_eq!(
        wanted(&files, true, None),
        Ok(vec![0, 1, 2]),
        "all, in the report's order"
    );
    let names = ["data.csv".to_owned(), "src/report.md".to_owned()];
    assert_eq!(
        wanted(&files, true, Some(&names)),
        Ok(vec![1, 0]),
        "named, in the request's order; a twice-listed path is its first"
    );
    assert_eq!(
        wanted(&files, true, Some(&[])),
        Ok(vec![]),
        "an empty list places nothing"
    );
    assert_eq!(
        wanted(&[], true, None),
        Ok(vec![]),
        "a report without files"
    );
    let unknown = ["data.csv".to_owned(), "notes.txt".to_owned()];
    assert_eq!(
        wanted(&files, true, Some(&unknown)),
        Err(not_found("notes.txt")),
        "a path not in the report"
    );
}

// TEST: GW-I-110
#[test]
fn gw_i_110_a_deleted_members_files_are_not_found() {
    let files = files();
    assert_eq!(
        wanted(&files, false, None),
        Err(not_found("src/report.md")),
        "the first wanted names the refusal"
    );
    assert_eq!(
        wanted(&files, false, Some(&["data.csv".to_owned()])),
        Err(not_found("data.csv"))
    );
    assert_eq!(
        wanted(&files, false, Some(&[])),
        Ok(vec![]),
        "nothing wanted, nothing refused"
    );
    assert_eq!(wanted(&[], false, None), Ok(vec![]));
}

// TEST: GW-I-110
#[test]
fn gw_i_110_a_placed_file_is_named_member_dash_path() {
    assert_eq!(
        placed_name("researcher", "src/report.md"),
        "researcher-src-report.md"
    );
    assert_eq!(placed_name("researcher", "data.csv"), "researcher-data.csv");
    assert_eq!(
        placed_name("reviewer-2", "a/b/c.txt"),
        "reviewer-2-a-b-c.txt"
    );
}
