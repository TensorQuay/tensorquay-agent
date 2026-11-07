"""The untrusted-content marker as the runtime writes it around a server's text (SEC-034, SEC-040), for the stack
tests: `marked` builds a block exactly as `tqa_tools::untrusted::wrap` does, and `unmarked` checks one and returns its
text."""
import hashlib


def _escape(value):
    for raw, escaped in (("&", "&amp;"), ('"', "&quot;"), ("<", "&lt;"), (">", "&gt;"), ("\n", "&#10;"), ("\r", "&#13;")):
        value = value.replace(raw, escaped)
    return value


def _block_id(source, attributes, body):
    digest = hashlib.sha256()
    for part in [source, *(text for pair in attributes for text in pair), body]:
        data = part.encode()
        digest.update(len(data).to_bytes(8, "big"))
        digest.update(data)
    return digest.hexdigest()[:16]


def marked(server, tool, body):
    """An MCP server's text as the model gets it."""
    attributes = [("server", server), ("tool", tool)]
    block = _block_id("mcp", attributes, body)
    names = "".join(f' {name}="{_escape(value)}"' for name, value in attributes)
    return f'<untrusted-content source="mcp"{names} id="{block}">\n{body}\n</untrusted-content id="{block}">'


def unmarked(content, server, tool):
    """The text inside `content`, which must be exactly one block from `server` and `tool`."""
    body = content.split("\n", 1)[-1].rsplit("\n", 1)[0]
    if content != marked(server, tool, body):
        raise AssertionError(f"not one untrusted-content block from mcp.{server}.{tool}: {content!r}")
    return body
