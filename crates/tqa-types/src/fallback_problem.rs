//! What exactly made a text-protocol reply unreadable (ADR-0007), carried on `provider.retry` beside its stable
//! `fallback_parse: <code>` reason, so the retry sentence can name the problem. Content-free by construction: names
//! only (tool and property names, JSON types), positions as numbers, never a value from the reply.
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};

/// The most names a list holds (an allowed-parameter list, a reply's extra keys).
pub const MAX_PROBLEM_NAMES: usize = 8;
/// The longest name kept, in bytes: our own tool and property names.
pub const MAX_PROBLEM_NAME_BYTES: usize = 64;
/// The longest name kept that a model wrote (an unknown tool, an unexpected property, an extra key), in bytes: longer
/// identifier-shaped text is often a credential written where a name belongs (ADR-0007, amended 22 Oct 2026).
pub const MAX_MODEL_NAME_BYTES: usize = 32;
/// The most segments a problem's place has.
pub const MAX_PROBLEM_PATH_SEGMENTS: usize = 4;

/// A name in a problem: an identifier-like tool, property or key name, never a value.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(transparent)]
pub struct ProblemName(
    #[schemars(
        length(min = 1, max = 64),
        regex(pattern = r"^[A-Za-z_][A-Za-z0-9_.-]{0,63}$")
    )]
    String,
);

impl ProblemName {
    /// The name, when it looks like an identifier (`^[A-Za-z_][A-Za-z0-9_.-]*$`, at most 64 bytes); anything else,
    /// which may be a value a model wrote where a name belongs, is refused.
    #[must_use]
    pub fn new(name: &str) -> Option<Self> {
        valid_name(name).then(|| Self(name.to_owned()))
    }

    /// A name the model wrote: as [`Self::new`], but at most [`MAX_MODEL_NAME_BYTES`]. A reader can't tell whose name
    /// it was, so reading allows 64.
    #[must_use]
    pub fn model(name: &str) -> Option<Self> {
        (name.len() <= MAX_MODEL_NAME_BYTES)
            .then(|| Self::new(name))
            .flatten()
    }

    /// The name as written.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for ProblemName {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = String::deserialize(deserializer)?;
        Self::new(&name)
            .ok_or_else(|| D::Error::custom("expected an identifier of at most 64 bytes"))
    }
}

fn valid_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    name.len() <= MAX_PROBLEM_NAME_BYTES
        && bytes
            .next()
            .is_some_and(|first| first.is_ascii_alphabetic() || first == b'_')
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-'))
}

/// Where in the arguments a nested problem is, such as `edits[0]`: property names and array indices only, at most
/// [`MAX_PROBLEM_PATH_SEGMENTS`] segments, the first a name.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(transparent)]
pub struct ProblemPath(
    #[schemars(
        length(min = 1, max = 280),
        regex(
            pattern = r"^[A-Za-z_][A-Za-z0-9_-]{0,63}(\[[0-9]{1,6}\]|\.[A-Za-z_][A-Za-z0-9_-]{0,63}){0,3}$"
        )
    )]
    String,
);

impl ProblemPath {
    /// The place a JSON pointer into the arguments names (`/edits/0` is `edits[0]`); `None` for the top level, or a
    /// pointer with a segment that is neither a plain name nor an index, or with too many.
    #[must_use]
    pub fn from_pointer(pointer: &str) -> Option<Self> {
        let segments: Vec<String> = pointer
            .split('/')
            .skip(1)
            .map(|segment| segment.replace("~1", "/").replace("~0", "~"))
            .collect();
        if segments.is_empty() || segments.len() > MAX_PROBLEM_PATH_SEGMENTS {
            return None;
        }
        let mut path = String::new();
        for (index, segment) in segments.iter().enumerate() {
            let (open, close) = match (index, is_index(segment), path_name(segment)) {
                (1.., true, _) => ("[", "]"),
                (0, _, true) => ("", ""),
                (_, _, true) => (".", ""),
                _ => return None,
            };
            path.push_str(open);
            path.push_str(segment);
            path.push_str(close);
        }
        Some(Self(path))
    }

    /// The place as written, such as `edits[0]`.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for ProblemPath {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let path = String::deserialize(deserializer)?;
        if valid_path(&path) {
            Ok(Self(path))
        } else {
            Err(D::Error::custom(
                "expected a place of names and indices, such as edits[0]",
            ))
        }
    }
}

fn is_index(segment: &str) -> bool {
    (1..=6).contains(&segment.len()) && segment.bytes().all(|byte| byte.is_ascii_digit())
}

/// A name in a place: as a problem's name, but without `.`, which separates segments.
fn path_name(segment: &str) -> bool {
    valid_name(segment) && !segment.contains('.')
}

/// Whether `path` is one [`ProblemPath::from_pointer`] could write.
fn valid_path(path: &str) -> bool {
    let mut pointer = String::new();
    let mut rest = path;
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix('[') {
            let Some((index, tail)) = after.split_once(']') else {
                return false;
            };
            if pointer.is_empty() || !is_index(index) {
                return false;
            }
            pointer.push('/');
            pointer.push_str(index);
            rest = tail;
        } else {
            let body = if pointer.is_empty() {
                rest
            } else if let Some(body) = rest.strip_prefix('.') {
                body
            } else {
                return false;
            };
            let end = body.find(['.', '[']).unwrap_or(body.len());
            if !path_name(&body[..end]) {
                return false;
            }
            pointer.push('/');
            pointer.push_str(&body[..end]);
            rest = &body[end..];
        }
    }
    ProblemPath::from_pointer(&pointer).is_some_and(|written| written.0 == path)
}

/// A JSON type a schema expects.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum JsonType {
    /// A string.
    String,
    /// A number.
    Number,
    /// An integer.
    Integer,
    /// `true` or `false`.
    Boolean,
    /// An object.
    Object,
    /// An array.
    Array,
    /// `null`.
    Null,
}

/// What was wrong with a text-protocol reply. An unknown `kind` from a newer runtime reads as [`Self::Other`].
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FallbackProblem {
    /// The call block is a JSON object without a `name` key.
    NoName,
    /// The call block is a JSON object with a `name` but without an `arguments` key.
    NoArguments,
    /// The call block is not JSON: cut off before the object closed, or a syntax error, at a line and column.
    NotJson {
        /// The text ended before the object closed.
        cut_off: bool,
        /// The line of the error, from 1.
        line: u32,
        /// The column of the error, from 1.
        column: u32,
    },
    /// The call block holds JSON that is not an object.
    NotObject,
    /// The call object has keys besides `name` and `arguments`; only identifier-like keys of at most
    /// [`MAX_MODEL_NAME_BYTES`] are kept.
    ExtraKeys {
        /// The extra keys, at most [`MAX_PROBLEM_NAMES`].
        #[serde(deserialize_with = "names")]
        #[schemars(length(max = 8))]
        keys: Vec<ProblemName>,
    },
    /// The call's `name` is not a string.
    NameNotString,
    /// The call names no tool that exists.
    UnknownTool {
        /// The name written, when it looks like an identifier of at most [`MAX_MODEL_NAME_BYTES`].
        #[serde(default, skip_serializing_if = "Option::is_none")]
        name: Option<ProblemName>,
    },
    /// The arguments have a property the tool doesn't take.
    UnexpectedProperty {
        /// The tool.
        tool: ProblemName,
        /// Where the property belongs, when it's inside the arguments rather than at their top level.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        at: Option<ProblemPath>,
        /// The property written, when it looks like an identifier of at most [`MAX_MODEL_NAME_BYTES`].
        #[serde(default, skip_serializing_if = "Option::is_none")]
        property: Option<ProblemName>,
        /// The properties taken there (the tool's parameters, or the nested object's), at most [`MAX_PROBLEM_NAMES`].
        #[serde(deserialize_with = "names")]
        #[schemars(length(max = 8))]
        allowed: Vec<ProblemName>,
    },
    /// The arguments leave out a property the tool requires.
    MissingProperty {
        /// The tool.
        tool: ProblemName,
        /// Where the property belongs, when it's inside the arguments rather than at their top level.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        at: Option<ProblemPath>,
        /// The property.
        property: ProblemName,
        /// The types it takes.
        #[serde(deserialize_with = "types")]
        #[schemars(length(max = 7))]
        expected: Vec<JsonType>,
    },
    /// A property has a value of the wrong type.
    WrongType {
        /// The tool.
        tool: ProblemName,
        /// Where the property belongs, when it's inside the arguments rather than at their top level.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        at: Option<ProblemPath>,
        /// The property.
        property: ProblemName,
        /// The types it takes.
        #[serde(deserialize_with = "types")]
        #[schemars(length(max = 7))]
        expected: Vec<JsonType>,
    },
    /// The arguments don't match the tool's schema in another way.
    OtherSchema {
        /// The tool.
        tool: ProblemName,
    },
    /// More calls than the route takes in one reply.
    TooMany {
        /// The calls the reply had.
        count: u32,
        /// The most the route takes.
        limit: u32,
    },
    /// A problem this reader doesn't know (written by a newer runtime).
    #[serde(other)]
    Other,
}

fn names<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<ProblemName>, D::Error> {
    let names = Vec::<ProblemName>::deserialize(deserializer)?;
    if names.len() > MAX_PROBLEM_NAMES {
        return Err(D::Error::custom("expected at most 8 names"));
    }
    Ok(names)
}

fn types<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<JsonType>, D::Error> {
    let types = Vec::<JsonType>::deserialize(deserializer)?;
    if types.len() > 7 {
        return Err(D::Error::custom("expected at most 7 types"));
    }
    Ok(types)
}
