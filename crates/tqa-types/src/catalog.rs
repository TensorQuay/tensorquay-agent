//! Borrowed, bounded accounting for the model-facing tool catalog.
use serde::Serialize;
use serde_json::Value;
use std::io::{self, Write};

/// Maximum number of registered or outgoing tool definitions.
pub const MAX_TOOLS: usize = 256;
/// Maximum serialized JSON bytes in one tool's parameters.
pub const MAX_TOOL_SCHEMA_BYTES: usize = 64 * 1024;
/// Maximum serialized JSON bytes in the complete outgoing tools array.
pub const MAX_TOOL_CATALOG_BYTES: usize = 1024 * 1024;

/// A catalog rejection identifies only the exceeded bound, never tool or schema contents.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ToolCatalogLimit {
    /// The catalog contains too many tools.
    #[error("tool catalog exceeds 256 tools")]
    Count,
    /// One encoded parameters value exceeds its bound.
    #[error("tool parameter schema exceeds 65536 bytes")]
    SchemaBytes,
    /// Definitions, metadata and array punctuation exceed the combined bound.
    #[error("tool catalog exceeds 1048576 bytes")]
    CatalogBytes,
}

/// Borrowed function metadata; measuring it never clones a schema or allocates an HTTP body.
#[derive(Serialize)]
pub struct ToolCatalogEntry<'a> {
    /// The model-facing tool name.
    pub name: &'a str,
    /// The model-facing description.
    pub description: &'a str,
    /// The JSON parameter schema, including every annotation.
    pub parameters: &'a Value,
    /// Whether the outgoing definition requests strict arguments.
    pub strict: bool,
}

#[derive(Serialize)]
struct Definition<'a> {
    r#type: &'static str,
    function: ToolCatalogEntry<'a>,
}

/// Incremental accounting for the same function array sent to the model.
/// A rejected candidate leaves the caller's previous budget unchanged.
#[derive(Clone, Copy, Debug)]
pub struct ToolCatalogBudget {
    tools: usize,
    bytes: usize,
}

impl Default for ToolCatalogBudget {
    fn default() -> Self {
        Self { tools: 0, bytes: 2 }
    }
}

impl ToolCatalogBudget {
    /// Returns the budget including this entry, counting encoded UTF-8 and JSON escapes.
    /// Schema size is checked before catalog metadata; count is checked before either.
    ///
    /// # Errors
    /// Returns the first exceeded limit without cloning, compiling or retaining the input.
    pub fn include(self, entry: ToolCatalogEntry<'_>) -> Result<Self, ToolCatalogLimit> {
        if self.tools == MAX_TOOLS {
            return Err(ToolCatalogLimit::Count);
        }
        encoded_size(entry.parameters, MAX_TOOL_SCHEMA_BYTES)
            .map_err(|_| ToolCatalogLimit::SchemaBytes)?;
        let bytes = self.bytes + usize::from(self.tools != 0);
        let definition = Definition {
            r#type: "function",
            function: entry,
        };
        let size = encoded_size(&definition, MAX_TOOL_CATALOG_BYTES.saturating_sub(bytes))
            .map_err(|_| ToolCatalogLimit::CatalogBytes)?;
        Ok(Self {
            tools: self.tools + 1,
            bytes: bytes + size,
        })
    }
}

fn encoded_size(value: &impl Serialize, maximum: usize) -> Result<usize, serde_json::Error> {
    let mut counter = ByteCounter { count: 0, maximum };
    serde_json::to_writer(&mut counter, value)?;
    Ok(counter.count)
}

struct ByteCounter {
    count: usize,
    maximum: usize,
}
impl Write for ByteCounter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.maximum.saturating_sub(self.count) {
            return Err(io::Error::other("tool catalog byte limit"));
        }
        self.count += bytes.len();
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
