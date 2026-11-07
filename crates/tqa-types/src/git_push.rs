//! Exact, credential-free push inputs.
pub(crate) mod relay;
use crate::{ErrorCode, FrameError};
use schemars::{JsonSchema, Schema, generate::SchemaGenerator, json_schema};
use serde::{Deserialize, Serialize};

/// One HTTPS destination, short branch name and full commit object ID.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, try_from = "TargetFields")]
pub struct GitPushTarget {
    /// Trusted clone URL, without credentials, query or fragment, within 4096 ASCII bytes.
    #[schemars(schema_with = "crate::workspace::url_schema")]
    pub remote: String,
    /// Current branch name, excluding HEAD and fully qualified refs, within 256 ASCII bytes.
    #[schemars(schema_with = "branch_schema")]
    pub branch: String,
    /// Nonzero full lowercase object ID, in the repository's 40- or 64-character format.
    #[schemars(schema_with = "commit_schema")]
    pub commit: String,
}
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct TargetFields {
    #[schemars(schema_with = "crate::workspace::url_schema")]
    remote: String,
    #[schemars(schema_with = "branch_schema")]
    branch: String,
    #[schemars(schema_with = "commit_schema")]
    commit: String,
}
impl TryFrom<TargetFields> for GitPushTarget {
    type Error = FrameError;
    fn try_from(value: TargetFields) -> Result<Self, Self::Error> {
        let target = Self {
            remote: value.remote,
            branch: value.branch,
            commit: value.commit,
        };
        target.validate()?;
        Ok(target)
    }
}
impl GitPushTarget {
    /// Returns the standalone parameter schema used by the builtin registry.
    #[must_use]
    pub fn schema() -> serde_json::Value {
        schemars::generate::SchemaSettings::draft07()
            .into_generator()
            .into_root_schema_for::<Self>()
            .to_value()
    }
    /// Revalidates programmatic input before hashing, process execution or transport.
    ///
    /// # Errors
    /// Rejects unsafe transports, ambiguous ref/revision expressions and incomplete object IDs.
    pub fn validate(&self) -> Result<(), FrameError> {
        if !crate::workspace::valid_url(&self.remote) {
            return Err(invalid(
                "remote",
                "expected a credential-free HTTPS repository URL",
            ));
        }
        if !branch(&self.branch) {
            return Err(invalid(
                "branch",
                "expected a short branch name within 256 ASCII bytes",
            ));
        }
        if !matches!(self.commit.len(), 40 | 64)
            || !self
                .commit
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            || self.commit.bytes().all(|byte| byte == b'0')
        {
            return Err(invalid(
                "commit",
                "expected a nonzero full lowercase commit object ID",
            ));
        }
        Ok(())
    }
}

fn invalid(path: &str, detail: &str) -> FrameError {
    FrameError {
        code: ErrorCode::InvalidFrame,
        path: path.into(),
        detail: detail.into(),
    }
}
fn branch(value: &str) -> bool {
    crate::workspace::valid_ref(value) && value != "HEAD" && !value.starts_with("refs/")
}
fn branch_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type":"string", "minLength":1, "maxLength":256,
    "not":{"anyOf":[
        {"pattern":"[^!-~]|[~^:?*\\[\\\\]"},
        {"pattern":"^-|^@$|^HEAD$|^refs/|@\\{|\\.\\.|\\.$|(^|/)\\.|\\.lock(/|$)|^/|/$|//"}
    ]}})
}
fn commit_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type":"string", "pattern":"^([0-9a-f]{40}|[0-9a-f]{64})$", "not":{"pattern":"^0+$"}})
}
