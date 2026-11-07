//! HTML parsing runs off the async executor with bounded input and cooperative reads.
use super::{TEXT_BYTES, file_io, transport::Page};
use crate::{ToolError, ToolOutput};
use std::io::{self, Read};
use tokio_util::sync::CancellationToken;

struct Input<'a> {
    bytes: &'a [u8],
    cancel: &'a CancellationToken,
}
impl Read for Input<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.cancel.is_cancelled() {
            return Err(io::Error::other("conversion cancelled"));
        }
        let bound = buffer.len().min(8192);
        self.bytes.read(&mut buffer[..bound])
    }
}
pub(super) fn render(page: &Page, cancel: &CancellationToken) -> Result<ToolOutput, ToolError> {
    file_io::cancelled(cancel)?;
    let source = std::str::from_utf8(&page.bytes)
        .map_err(|_| ToolError::Unsupported("response is not UTF-8 text".into()))?;
    if source.contains('\0') {
        return Err(ToolError::Unsupported("binary response body".into()));
    }
    let text = if page.html {
        let input = Input {
            bytes: &page.bytes,
            cancel,
        };
        let converter = html2text::config::plain();
        let parsed = converter.parse_html(input);
        file_io::cancelled(cancel)?;
        let document = parsed.map_err(|_| ToolError::Io("HTML parsing failed".into()))?;
        bounded_document(&document, cancel)?;
        let result = converter
            .dom_to_render_tree(&document)
            .and_then(|tree| converter.render_to_string(tree, 100));
        file_io::cancelled(cancel)?;
        result.map_err(|_| ToolError::Io("HTML text conversion failed".into()))?
    } else {
        source.to_owned()
    };
    file_io::cancelled(cancel)?;
    if text.len() > TEXT_BYTES {
        return Err(ToolError::OutputTooLarge { max: TEXT_BYTES });
    }
    Ok(ToolOutput::text(text))
}

fn bounded_document(
    document: &html2text::RcDom,
    cancel: &CancellationToken,
) -> Result<(), ToolError> {
    let error = || ToolError::Unsupported("HTML complexity exceeds the conversion budget".into());
    let mut pending = vec![(document.document.clone(), 0)];
    let mut nodes = 0;
    while let Some((node, depth)) = pending.pop() {
        file_io::cancelled(cancel)?;
        nodes += 1;
        if nodes > 65_536 || depth > 128 {
            return Err(error());
        }
        if let html2text::Element { attrs, .. } = &node.data
            && attrs.borrow().iter().any(|attribute| {
                matches!(attribute.name.local.as_ref(), "colspan" | "rowspan")
                    && attribute.value.parse::<u64>().is_ok_and(|span| span > 1000)
            })
        {
            return Err(error());
        }
        let children = node.children.borrow();
        if nodes + pending.len() + children.len() > 65_536 {
            return Err(error());
        }
        pending.extend(children.iter().map(|child| (child.clone(), depth + 1)));
    }
    Ok(())
}
