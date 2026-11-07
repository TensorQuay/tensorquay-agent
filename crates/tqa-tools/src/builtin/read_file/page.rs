//! Page assembly reserves continuation metadata before copying source bytes.
use super::{record::Record, scan::Input};
use crate::{ToolError, ToolLimits};

pub(super) fn render(
    input: &mut Input<'_>,
    mut record: Record,
    limits: ToolLimits,
) -> Result<Option<String>, ToolError> {
    let (first, mut count) = (record.line, 0);
    if record.offset != 0 {
        return long_line(input, &mut record, limits.read_page_bytes);
    }
    if let Some(total) = input.skip(first)? {
        let notice = if total == 0 && first == 1 {
            "[end of file: 0 lines]".into()
        } else {
            format!("[end of file: {total} lines; start_line {first} is past the end]")
        };
        return Ok(Some(notice));
    }
    let mut output = Vec::with_capacity(limits.read_page_bytes);
    while count < limits.read_page_lines {
        let Some((start, end, next)) = input.span()? else {
            break;
        };
        record.line_start = start;
        record.offset = start;
        record.content_end = end;
        record.next_line = next;
        let prefix = format!("{:>6}\t", record.line);
        let continuation = if record.more() {
            notice(first, record.line, None)
        } else {
            String::new()
        };
        let length = usize::try_from(end - start).map_err(|_| ToolError::OutputTooLarge {
            max: limits.read_page_bytes,
        })?;
        let needed = prefix.len() + length + 1 + continuation.len();
        if output.len() + needed > limits.read_page_bytes {
            if output.is_empty() {
                drop(output);
                return long_line(input, &mut record, limits.read_page_bytes);
            }
            let total = (record.next_line == record.stamp.size).then_some(record.line);
            output.extend_from_slice(notice(first, record.line - 1, total).as_bytes());
            return finish(output);
        }
        output.extend_from_slice(prefix.as_bytes());
        if input.append(&mut output, start, length, false)?.is_none() {
            return Ok(None);
        }
        output.push(b'\n');
        count += 1;
        if !record.more() {
            return finish(output);
        }
        if count == limits.read_page_lines {
            output.extend_from_slice(continuation.as_bytes());
            break;
        }
        input.seek(next)?;
        record.line += 1;
    }
    finish(output)
}
fn long_line(
    input: &mut Input<'_>,
    record: &mut Record,
    budget: usize,
) -> Result<Option<String>, ToolError> {
    let prefix = format!("{:>6}\t", record.line);
    let offset = record.offset;
    let remaining = usize::try_from(record.content_end - offset)
        .map_err(|_| ToolError::OutputTooLarge { max: budget })?;
    let regular = if record.more() {
        notice(record.line, record.line, None)
    } else {
        String::new()
    };
    let mut output = Vec::with_capacity(budget);
    output.extend_from_slice(prefix.as_bytes());
    if prefix.len() + remaining + 1 + regular.len() <= budget {
        if input
            .append(&mut output, offset, remaining, false)?
            .is_none()
        {
            return Ok(None);
        }
        output.push(b'\n');
        output.extend_from_slice(regular.as_bytes());
        return finish(output);
    }
    let first_byte = offset - record.line_start + 1;
    // The final offset has at least as many digits as every actual continuation.
    record.offset = record.content_end;
    let reserved = record.notice(first_byte)?.len();
    let room = budget
        .checked_sub(prefix.len() + 1 + reserved)
        .filter(|room| *room >= 4)
        .ok_or(ToolError::OutputTooLarge { max: budget })?;
    let Some(copied) = input.append(&mut output, offset, room.min(remaining), true)? else {
        return Ok(None);
    };
    record.offset = offset + copied as u64;
    output.push(b'\n');
    output.extend_from_slice(record.notice(first_byte)?.as_bytes());
    finish(output)
}
fn notice(first: u64, last: u64, total: Option<u64>) -> String {
    let total = total.map_or_else(|| "unknown".into(), |total| total.to_string());
    format!(
        "[partial: lines {first}-{last} of {total}; continue with start_line={}]",
        last + 1
    )
}
fn finish(bytes: Vec<u8>) -> Result<Option<String>, ToolError> {
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|_| ToolError::Io("page boundary failed".into()))
}
