//! Root paths in what a site pass serves (18.1, built web apps preview as they are): a default web build names its
//! files from the server's root (`/assets/app.js`), which under a pass resolves outside it and is refused. The HTML
//! and CSS a pass serves have each root path moved under the pass's root; scripts are never changed, since a string
//! in a script can't be told from data. Each HTML page also gets a small reporter first in its head, which tells the
//! Files panel what the page asked for and couldn't have, so the panel says why instead of showing a blank frame.
use lol_html::html_content::{ContentType, Element, TextChunk};
use lol_html::{HtmlRewriter, Settings, element, text};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// The attributes holding one address, by element.
const SINGLE: &[(&str, &str)] = &[
    ("script[src]", "src"),
    ("link[href]", "href"),
    ("img[src]", "src"),
    ("source[src]", "src"),
    ("video[src]", "src"),
    ("video[poster]", "poster"),
    ("audio[src]", "src"),
    ("track[src]", "src"),
    ("a[href]", "href"),
    ("area[href]", "href"),
];

/// The attributes holding a candidate list (`srcset`).
const LISTS: &[(&str, &str)] = &[
    ("img[srcset]", "srcset"),
    ("source[srcset]", "srcset"),
    ("link[imagesrcset]", "imagesrcset"),
];

/// Whether `url` is a root path: one `/` then anything but `/` or `\`, or `/` alone. Protocol-relative (`//h`),
/// `/\h` (read as protocol-relative by the URL parser), absolute, relative, fragment and `data:` addresses are not.
#[must_use]
pub(crate) fn is_root_path(url: &str) -> bool {
    let mut chars = url.chars();
    chars.next() == Some('/') && !matches!(chars.next(), Some('/' | '\\'))
}

/// A root path moved under `root` (the pass's root, ending in `/`), its dot segments removed first so it can't
/// leave the pass; any other address unchanged (`None`). The query and fragment are kept as written.
#[must_use]
pub(crate) fn under_root(url: &str, root: &str) -> Option<String> {
    let trimmed = url.trim_matches(|c: char| c.is_ascii_whitespace());
    if !is_root_path(trimmed) {
        return None;
    }
    let split = trimmed.find(['?', '#']).unwrap_or(trimmed.len());
    let (path, rest) = trimmed.split_at(split);
    let mut parts: Vec<&str> = Vec::new();
    let segments: Vec<&str> = path[1..].split('/').collect();
    for (index, segment) in segments.iter().enumerate() {
        let last = index + 1 == segments.len();
        match *segment {
            "." => {
                if last {
                    parts.push("");
                }
            }
            ".." => {
                parts.pop();
                if last {
                    parts.push("");
                }
            }
            other => parts.push(other),
        }
    }
    Some(format!("{root}{}{rest}", parts.join("/")))
}

/// A `srcset` with each candidate's root path moved under `root`, its descriptors and separators kept; `None` when
/// none moved.
#[must_use]
pub(crate) fn srcset_under_root(list: &str, root: &str) -> Option<String> {
    let bytes = list.as_bytes();
    let mut out = String::with_capacity(list.len());
    let (mut index, mut changed) = (0, false);
    while index < bytes.len() {
        // Separators and space before a candidate's address.
        let start = index;
        while index < bytes.len() && (bytes[index].is_ascii_whitespace() || bytes[index] == b',') {
            index += 1;
        }
        out.push_str(&list[start..index]);
        if index >= bytes.len() {
            break;
        }
        // The address runs to the next space; a comma ending it is the separator, not part of it.
        let begin = index;
        while index < bytes.len() && !bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        let mut end = index;
        while end > begin && bytes[end - 1] == b',' {
            end -= 1;
        }
        let address = &list[begin..end];
        match under_root(address, root) {
            Some(moved) => {
                out.push_str(&moved);
                changed = true;
            }
            None => out.push_str(address),
        }
        out.push_str(&list[end..index]);
        if end < index {
            continue; // the commas after the address ended this candidate
        }
        // Descriptors up to the next comma.
        let descriptors = index;
        while index < bytes.len() && bytes[index] != b',' {
            index += 1;
        }
        out.push_str(&list[descriptors..index]);
    }
    changed.then_some(out)
}

/// CSS with each root path in `url(...)` and `@import "..."` moved under `root`; comments, other strings and
/// escaped addresses left alone. `None` when nothing moved, so the answer stays byte-identical.
#[must_use]
pub(crate) fn css_under_root(css: &str, root: &str) -> Option<String> {
    let bytes = css.as_bytes();
    let mut out = String::with_capacity(css.len() + 64);
    let (mut index, mut copied, mut changed) = (0, 0, false);
    let mut after_import = false;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b'/' && bytes.get(index + 1) == Some(&b'*') {
            index = css[index + 2..]
                .find("*/")
                .map_or(bytes.len(), |end| index + 2 + end + 2);
            continue;
        }
        if byte == b'"' || byte == b'\'' {
            let end = string_end(bytes, index);
            // The inside of the string, between its quotes.
            let inner = (index + 1, end.saturating_sub(1).max(index + 1));
            let moved = after_import
                .then(|| &css[inner.0..inner.1])
                .filter(|text| !text.contains('\\'))
                .and_then(|text| under_root(text, root));
            if let Some(moved) = moved {
                out.push_str(&css[copied..inner.0]);
                out.push_str(&moved);
                copied = inner.1;
                changed = true;
            }
            after_import = false;
            index = end;
            continue;
        }
        if byte == b'@' && starts_with_ignore_case(&css[index..], "@import") {
            after_import = true;
            index += "@import".len();
            continue;
        }
        if (byte == b'u' || byte == b'U')
            && starts_with_ignore_case(&css[index..], "url(")
            && (index == 0 || !is_name(bytes[index - 1]))
        {
            let open = index + 4;
            let close = match css[open..].find(')') {
                Some(offset) => open + offset,
                None => break,
            };
            let inside = &css[open..close];
            let trimmed = inside.trim();
            let (address, quoted) = match trimmed.as_bytes().first() {
                Some(b'"' | b'\'') if trimmed.len() >= 2 => (&trimmed[1..trimmed.len() - 1], true),
                _ => (trimmed, false),
            };
            if !address.contains('\\')
                && let Some(moved) = under_root(address, root)
            {
                let lead = inside.find(trimmed).unwrap_or(0) + usize::from(quoted);
                let start = open + lead;
                out.push_str(&css[copied..start]);
                out.push_str(&moved);
                copied = start + address.len();
                changed = true;
            }
            after_import = false;
            index = close + 1;
            continue;
        }
        if byte == b';' || byte == b'{' {
            after_import = false;
        }
        index += 1;
    }
    if !changed {
        return None;
    }
    out.push_str(&css[copied..]);
    Some(out)
}

/// The index just after a string starting at `start` (its closing quote, or the end of the text).
fn string_end(bytes: &[u8], start: usize) -> usize {
    let quote = bytes[start];
    let mut index = start + 1;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => index += 2,
            byte if byte == quote => return index + 1,
            b'\n' => return index,
            _ => index += 1,
        }
    }
    bytes.len()
}

fn starts_with_ignore_case(text: &str, prefix: &str) -> bool {
    text.len() >= prefix.len()
        && text.as_bytes()[..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes())
}

fn is_name(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_'
}

/// The reporter put first in each page's head: it tells the parent (the Files panel, at `origin`) what the page asked
/// for and couldn't have. Root paths: a load the policy blocked on this gateway, outside the pass; missing files: a
/// load under the pass that failed; drew nothing: 2 s after load, `#root` or `#app` with no element and nothing
/// failed and no script stopped on an error (that page needs no router advice). At most one message per kind, ten
/// paths of 120 characters each; it reads nothing else, sends nothing else. Before anything else it gives the page an
/// in-memory `localStorage` and `sessionStorage` where the preview's sandbox refuses the real ones (it runs without
/// same-origin access, so reading them throws and an app that keeps its data there stopped before drawing): the page
/// runs, and what it stores lasts until the preview reloads.
#[must_use]
pub(crate) fn reporter(root: &str, origin: &str) -> String {
    // JSON strings with `<` escaped, so nothing in them can close the script.
    let literal = |text: &str| {
        serde_json::to_string(text)
            .unwrap_or_else(|_| "\"\"".into())
            .replace('<', "\\u003c")
    };
    let (root, origin) = (literal(root), literal(origin));
    format!(
        "<script>(()=>{{for(const n of[\"localStorage\",\"sessionStorage\"]){{try{{window[n].length}}catch(_){{const m=new Map(),s={{getItem:k=>m.has(String(k))?m.get(String(k)):null,setItem:(k,v)=>{{m.set(String(k),String(v))}},removeItem:k=>{{m.delete(String(k))}},clear:()=>{{m.clear()}},key:i=>[...m.keys()][i]??null,get length(){{return m.size}}}};try{{Object.defineProperty(window,n,{{value:s,configurable:true}})}}catch(_){{}}}}}}\
const B={root},O={origin},L={{root:[],missing:[]}},S={{}};let F=0,R=0,E=0;\
const P=k=>{{if(S[k])return;S[k]=1;try{{parent.postMessage({{type:\"tqa-site-report\",kind:k,paths:(L[k]||[]).slice(0,10).map(p=>String(p).slice(0,120)),more:Math.max(0,(L[k]||[]).length-10)}},O)}}catch(_){{}}}};\
const A=(k,p)=>{{F=1;const l=L[k];if(p&&!l.includes(p))l.push(p);if(R)setTimeout(()=>P(k),500)}};\
const U=s=>{{try{{return new URL(s,location.href)}}catch(_){{return null}}}};\
document.addEventListener(\"securitypolicyviolation\",e=>{{const u=U(e.blockedURI);if(u&&u.origin===O&&!u.pathname.startsWith(B))A(\"root\",u.pathname===\"/\"?\"\":u.pathname)}},true);\
addEventListener(\"unhandledrejection\",()=>{{E=1}});\
addEventListener(\"error\",e=>{{const t=e.target;if(!t||t===window){{E=1;return}}const u=U(t.src||t.href||\"\");if(u&&u.origin===O&&u.pathname.startsWith(B))A(\"missing\",decodeURIComponent(u.pathname.slice(B.length)))}},true);\
addEventListener(\"load\",()=>setTimeout(()=>{{R=1;if(L.root.length||F&&!L.missing.length)P(\"root\");if(L.missing.length)P(\"missing\");\
const r=document.getElementById(\"root\")||document.getElementById(\"app\");if(r&&!r.firstElementChild&&!F&&!E)P(\"empty\")}},2000))}})()</script>"
    )
}

/// The page with its root paths moved under `root` (the attributes of the design's table, `srcset` lists, `style`
/// attributes and `<style>` text) and `reporter` first in its head (or before its first element without one). The
/// page as it came when it can't be parsed.
#[must_use]
pub(crate) fn html_under_root(page: &[u8], root: &str, reporter: &str) -> Vec<u8> {
    rewrite_html(page, root, reporter).unwrap_or_else(|| page.to_vec())
}

fn rewrite_html(page: &[u8], root: &str, reporter: &str) -> Option<Vec<u8>> {
    let injected = Rc::new(Cell::new(false));
    let style = Rc::new(RefCell::new(String::new()));
    let mut handlers = Vec::new();
    for (selector, attribute) in SINGLE {
        handlers.push(element!(*selector, move |element: &mut Element<'_, '_>| {
            move_attribute(element, attribute, root, under_root);
            Ok(())
        }));
    }
    for (selector, attribute) in LISTS {
        handlers.push(element!(*selector, move |element: &mut Element<'_, '_>| {
            move_attribute(element, attribute, root, srcset_under_root);
            Ok(())
        }));
    }
    // SVG's `image` and `use`: `href`, or the older `xlink:href`.
    handlers.push(element!("image, use", move |element: &mut Element<
        '_,
        '_,
    >| {
        move_attribute(element, "href", root, under_root);
        move_attribute(element, "xlink:href", root, under_root);
        Ok(())
    }));
    handlers.push(element!("[style]", move |element: &mut Element<'_, '_>| {
        move_attribute(element, "style", root, css_under_root);
        Ok(())
    }));
    {
        let injected = injected.clone();
        handlers.push(element!("*", move |element: &mut Element<'_, '_>| {
            if injected.get() {
                return Ok(());
            }
            match element.tag_name().as_str() {
                // Inside `<html>`, the head (or the first element) is next.
                "html" => {}
                "head" => {
                    element.prepend(reporter, ContentType::Html);
                    injected.set(true);
                }
                _ => {
                    element.before(reporter, ContentType::Html);
                    injected.set(true);
                }
            }
            Ok(())
        }));
    }
    let style_text = text!("style", move |chunk: &mut TextChunk<'_>| {
        style.borrow_mut().push_str(chunk.as_str());
        if chunk.last_in_text_node() {
            let whole = std::mem::take(&mut *style.borrow_mut());
            let moved = css_under_root(&whole, root).unwrap_or(whole);
            chunk.replace(&moved, ContentType::Html);
        } else {
            chunk.remove();
        }
        Ok(())
    });
    let settings = handlers
        .into_iter()
        .chain(std::iter::once(style_text))
        .fold(Settings::new(), Settings::append_element_content_handler);
    let mut out = Vec::with_capacity(page.len() + reporter.len() + 256);
    let mut rewriter = HtmlRewriter::new(settings, |bytes: &[u8]| out.extend_from_slice(bytes));
    rewriter.write(page).ok()?;
    rewriter.end().ok()?;
    if !injected.get() {
        // No element at all (or only `<html>` with nothing in it): the reporter goes at the end.
        out.extend_from_slice(reporter.as_bytes());
    }
    Some(out)
}

fn move_attribute(
    element: &mut Element<'_, '_>,
    attribute: &str,
    root: &str,
    change: fn(&str, &str) -> Option<String>,
) {
    if let Some(value) = element.get_attribute(attribute)
        && let Some(moved) = change(&value, root)
    {
        let _ = element.set_attribute(attribute, &moved);
    }
}

#[cfg(test)]
#[path = "site_rewrite_tests.rs"]
mod tests;
