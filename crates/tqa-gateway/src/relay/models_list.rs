//! Reading an endpoint's model list for `GET /api/v1/admin/routes/{name}/models` (GW-U-070): the OpenAI-compatible
//! `{data: [{id, ...}]}` that Chat and Responses endpoints, local servers and proxies answer, and a Messages page with
//! `has_more` and `last_id`. Only each model's id and display name are kept; everything else in the body is dropped.
use serde::Serialize;
use serde_json::Value;

/// The most models one listing returns; past it the answer says `truncated`.
pub(super) const MAX_MODELS: usize = 500;
/// The longest model id (or display name) kept.
pub(super) const MAX_ID_CHARS: usize = 256;

/// One listed model.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(super) struct Listed {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
}

/// One page: its models, and the cursor for the next page when the endpoint says there is one.
#[derive(Debug, Default)]
pub(super) struct Page {
    pub models: Vec<Listed>,
    pub next: Option<String>,
}

/// The body isn't a model list.
#[derive(Debug, thiserror::Error)]
#[error("the endpoint's answer is not a model list")]
pub(super) struct Unreadable;

/// Reads one page. Entries without a usable id are skipped; a display name that isn't printable text is dropped.
///
/// # Errors
/// [`Unreadable`] when the body isn't a JSON object with a `data` array.
pub(super) fn page(body: &[u8]) -> Result<Page, Unreadable> {
    let value: Value = serde_json::from_slice(body).map_err(|_| Unreadable)?;
    let data = value
        .get("data")
        .and_then(Value::as_array)
        .ok_or(Unreadable)?;
    let models = data
        .iter()
        .filter_map(|entry| {
            let id = entry
                .get("id")
                .and_then(Value::as_str)
                .filter(|id| usable(id))?;
            let display_name = entry
                .get("display_name")
                .and_then(Value::as_str)
                .filter(|name| usable(name))
                .map(str::to_owned);
            Some(Listed {
                id: id.to_owned(),
                display_name,
            })
        })
        .collect();
    let next = (value.get("has_more").and_then(Value::as_bool) == Some(true))
        .then(|| {
            value
                .get("last_id")
                .and_then(Value::as_str)
                .filter(|id| usable(id))
                .map(str::to_owned)
        })
        .flatten();
    Ok(Page { models, next })
}

/// 1 to [`MAX_ID_CHARS`] characters, none of them a control character.
fn usable(text: &str) -> bool {
    let count = text.chars().count();
    (1..=MAX_ID_CHARS).contains(&count) && !text.chars().any(char::is_control)
}

/// The models of every page so far, up to [`MAX_MODELS`].
#[derive(Debug, Default)]
pub(super) struct Collected {
    pub models: Vec<Listed>,
    pub truncated: bool,
}

impl Collected {
    /// Adds a page's models; `false` once a model had to be left out. `truncated` says a model was left out, and the
    /// caller also sets it when the list is full while the endpoint has another page.
    pub(super) fn add(&mut self, models: Vec<Listed>) -> bool {
        for model in models {
            if self.full() {
                self.truncated = true;
                return false;
            }
            self.models.push(model);
        }
        true
    }

    /// Whether the cap is reached.
    pub(super) fn full(&self) -> bool {
        self.models.len() >= MAX_MODELS
    }
}
