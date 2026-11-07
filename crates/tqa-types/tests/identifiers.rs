//! Identifier boundary and wire-format contract tests.

use std::error::Error;

use serde::{Serialize, de::DeserializeOwned};
use tqa_types::{ConversationId, RunId, ToolCallId, WorkspaceId};

fn assert_identifier<T>() -> Result<(), Box<dyn Error>>
where
    T: TryFrom<String> + Serialize + DeserializeOwned + std::fmt::Display,
    T::Error: Error + 'static,
{
    for valid in ["a".to_owned(), "A-z_09".to_owned(), "x".repeat(64)] {
        let id = T::try_from(valid.clone())?;
        assert_eq!(id.to_string(), valid);
        let encoded = serde_json::to_string(&id)?;
        assert_eq!(encoded, serde_json::to_string(&valid)?);
        assert_eq!(serde_json::from_str::<T>(&encoded)?.to_string(), valid);
    }
    for invalid in [
        String::new(),
        "x".repeat(65),
        "two words".to_owned(),
        "文".to_owned(),
        "a/b".to_owned(),
        "a\\b".to_owned(),
        "a.b".to_owned(),
        "a\n".to_owned(),
    ] {
        assert!(
            T::try_from(invalid.clone()).is_err(),
            "accepted {invalid:?}"
        );
        assert!(serde_json::from_str::<T>(&serde_json::to_string(&invalid)?).is_err());
    }
    Ok(())
}

#[test]
fn proto_c_001_identifier_boundaries_and_transparent_encoding() -> Result<(), Box<dyn Error>> {
    assert_identifier::<ConversationId>()?;
    assert_identifier::<RunId>()?;
    assert_identifier::<ToolCallId>()?;
    assert_identifier::<WorkspaceId>()?;
    let id: ConversationId = "conversation_1".parse()?;
    assert_eq!(id.as_str(), "conversation_1");
    Ok(())
}
