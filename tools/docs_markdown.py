"""Offline Markdown link extraction and GitHub heading anchors for G-09."""

import html
import re
import unicodedata
from urllib.parse import unquote, urlsplit

MAX_LINK_NESTING = 32


def blank(text):
    """Mask syntax without moving the line numbers or character offsets."""
    return re.sub(r"[^\n]", " ", text)


def visible_markdown(source):
    """Remove comments and fenced or indented examples, preserving source positions."""
    source = re.sub(r"<!--.*?(?:-->|\Z)", lambda m: blank(m[0]), source, flags=re.S)
    lines, fence, in_list = [], None, False
    for line in source.splitlines(keepends=True):
        content = re.sub(r"^ {0,3}(?:> ?)+", "", line)
        marker = re.match(r" {0,3}(`{3,}|~{3,})", content)
        if fence:
            lines.append(blank(line))
            if re.fullmatch(r" {0,3}" + re.escape(fence[0]) + "{" + str(len(fence)) + r",}\s*", content):
                fence = None
        elif marker:
            fence = marker[1]
            lines.append(blank(line))
        else:
            if re.match(r" {0,3}(?:[-+*]|\d+[.)])\s", content):
                in_list = True
            elif content.strip() and not content.startswith((" ", "\t")):
                in_list = False
            lines.append(blank(line) if content.startswith(("    ", "\t")) and not in_list else line)
    return "".join(lines)


def paired_end(text, start, opening, closing):
    """Find a balanced delimiter while respecting backslash escapes."""
    depth, index = 1, start + 1
    while index < len(text):
        if text[index] == "\\":
            index += 2
            continue
        if text[index] == opening:
            depth += 1
        elif text[index] == closing:
            depth -= 1
            if depth == 0:
                return index
        index += 1
    return None


def destination(text, start):
    """Read an angle-wrapped or balanced bare Markdown destination."""
    start += len(text[start:]) - len(text[start:].lstrip())
    if text[start:start + 1] == "<":
        end = text.find(">", start + 1)
        return (text[start + 1:end], end + 1) if end >= 0 else (None, start)
    index, depth = start, 0
    while index < len(text):
        char = text[index]
        if char == "\\" and index + 1 < len(text):
            index += 2
            continue
        if char.isspace() or (char == ")" and depth == 0):
            break
        if char == "(":
            depth += 1
        elif char == ")":
            depth -= 1
        index += 1
    return (text[start:index], index) if depth == 0 else (None, start)


def label_key(label):
    """Reference labels are whitespace-normalised and case-insensitive."""
    return " ".join(label.split()).casefold()


def bracket_links(text, definitions, depth=0):
    """Yield offset, destination and end offset for inline and reference links."""
    if depth > MAX_LINK_NESTING:
        raise ValueError("Markdown link nesting exceeds {} levels".format(MAX_LINK_NESTING))
    index = 0
    while index < len(text):
        if text[index] == "\\":
            index += 2
            continue
        if text[index] != "[":
            index += 1
            continue
        end = paired_end(text, index, "[", "]")
        if end is None:
            index += 1
            continue
        label, after = text[index + 1:end], end + 1
        target, finish = None, after
        if text[after:after + 1] == "(":
            target, finish = destination(text, after + 1)
            tail = re.match(r'''\s*(?:"[^"\n]*"|'[^'\n]*'|\([^\n)]*\))?\s*\)''', text[finish:])
            if tail:
                finish += tail.end()
            else:
                target = None
        elif text[after:after + 1] == "[":
            close = text.find("]", after + 1)
            if close >= 0:
                key = text[after + 1:close] or label
                target, finish = definitions.get(label_key(key)), close + 1
        else:
            target = definitions.get(label_key(label))
        if target is not None:
            yield index, target, finish
            if "![" in label:
                for inner, image, image_end in bracket_links(label, definitions, depth + 1):
                    yield index + 1 + inner, image, index + 1 + image_end
        index = finish


def markdown_links(source):
    """Return rendered inline, image, reference, HTML and external links with line numbers."""
    text = visible_markdown(source)
    text = re.sub(r"(?<!`)(`+)(?!`)(.*?)\1(?!`)", lambda m: blank(m[0]), text, flags=re.S)
    links, definitions = [], {}
    for match in re.finditer(r"(?m)^ {0,3}\[([^]\n]+)\]:[ \t]*", text):
        if match[1].startswith("^"):
            continue
        target, end = destination(text, match.end())
        if target is not None:
            definitions.setdefault(label_key(match[1]), target)
            links.append((match.start(), target, end))
    for start, _, end in links:
        text = text[:start] + blank(text[start:end]) + text[end:]
    inline = list(bracket_links(text, definitions))
    links.extend(inline)
    for start, _, end in inline:
        text = text[:start] + blank(text[start:end]) + text[end:]
    for match in re.finditer(r'''\b(?:href|src)\s*=\s*(["'])(.*?)\1''', text, re.I | re.S):
        links.append((match.start(), match[2], match.end()))
        text = text[:match.start()] + blank(text[match.start():match.end()]) + text[match.end():]
    for match in re.finditer(r"https?://[^\s<>]+", text):
        target = match[0].rstrip(".,;:*~\"'")
        while target.endswith(")") and target.count(")") > target.count("("):
            target = target[:-1]
        links.append((match.start(), target, match.end()))
    return [(source.count("\n", 0, start) + 1, html.unescape(re.sub(r"\\([!-~])", r"\1", target)))
            for start, target, _ in sorted(links)]


def heading_text(text):
    """Reduce heading markup to visible text before generating its anchor."""
    literals = []

    def protect(match):
        literals.append(match[1] if match[1] is not None else match[3].replace("\n", " "))
        return "\0{}\0".format(len(literals) - 1)

    text = re.sub(r"\\([!-/:-@\[-`{-~])|(?<!`)(`+)(?!`)(.*?)\2(?!`)", protect, text)
    text = re.sub(r"!?\[([^]]*)\]\([^)]*\)", r"\1", text)
    text = re.sub(r"<[^>]*>", "", text)
    text = re.sub(r"(?<!\w)(\*{1,3}|_{1,3}|~~)(?=\S)(.*?)\1(?!\w)", r"\2", text)
    text = re.sub(r"\x00(\d+)\x00", lambda m: literals[int(m[1])], html.unescape(text))
    return text.strip().lower()


def heading_slugs(source):
    """Generate GitHub-style ATX/setext heading anchors with collision-aware suffixes."""
    used, previous = set(), ""
    for line in visible_markdown(source).splitlines():
        line = re.sub(r"^ {0,3}(?:> ?)+", "", line)
        match = re.match(r"^ {0,3}#{1,6}(?:\s+|$)(.*)", line)
        if match:
            title = re.sub(r"\s+#+\s*$", "", match[1])
        elif previous.strip() and re.fullmatch(r" {0,3}(?:=+|-+)\s*", line):
            title = previous
        else:
            previous = line
            continue
        text = heading_text(title)
        slug = "".join(c for c in text if c in " -_" or unicodedata.category(c)[0] in "LNM").replace(" ", "-")
        candidate, suffix = slug, 0
        while candidate in used:
            suffix += 1
            candidate = "{}-{}".format(slug, suffix)
        used.add(candidate)
        previous = ""
    return used


def check_markdown(root, name, source):
    """Check file existence and exact fragments, refusing paths outside the repository."""
    problems, anchors = [], {name: heading_slugs(source)}
    for line, target in markdown_links(source):
        try:
            url = urlsplit(target)
            if url.scheme or url.netloc:
                continue
            path = unquote(url.path)
            full = root / name if not path else (root / name).parent / path
            if path.startswith("/"):
                full = root / path.lstrip("/")
            relative = full.resolve().relative_to(root.resolve()).as_posix()
            if relative != name and not full.is_file():
                problems.append((line, "link is not an existing file: " + target))
            elif url.fragment:
                if relative not in anchors:
                    anchors[relative] = heading_slugs(full.read_text(encoding="utf-8"))
                if unquote(url.fragment) not in anchors[relative]:
                    problems.append((line, "missing heading anchor: " + target))
        except (OSError, UnicodeError, ValueError):
            problems.append((line, "link cannot be resolved inside the repository: " + target))
    return problems
