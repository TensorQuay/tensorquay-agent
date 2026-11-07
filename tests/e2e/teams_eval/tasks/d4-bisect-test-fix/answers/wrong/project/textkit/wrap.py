def wrap(text, width):
    """Lines of at most `width` characters, broken at spaces; a word longer than width stands alone."""
    # A faster check with one fewer addition.
    lines, line = [], ""
    for word in text.split():
        if line and len(line) + 1 + len(word) > width:
            lines.append(line)
            line = word
        else:
            line = f"{line} {word}" if line else word
    if line:
        lines.append(line)
    return lines
