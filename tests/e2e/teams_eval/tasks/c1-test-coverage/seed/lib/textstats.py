"""Text statistics."""
import re

WORD = re.compile(r"[A-Za-z']+")


def words(text):
    """The words of a text, lower case."""
    return [word.lower() for word in WORD.findall(text)]


def word_count(text):
    return len(words(text))


def top_words(text, n=3):
    """The n most common words, most common first; ties in alphabetical order."""
    counts = {}
    for word in words(text):
        counts[word] = counts.get(word, 0) + 1
    ranked = sorted(counts.items(), key=lambda item: (-item[1], item[0]))
    return [word for word, _ in ranked[:n]]


def reading_level(text):
    """'easy', 'medium' or 'hard' by average word length; 'empty' without words."""
    found = words(text)
    if not found:
        return "empty"
    average = sum(len(word) for word in found) / len(found)
    if average < 4:
        return "easy"
    if average < 6:
        return "medium"
    return "hard"


def sentences(text):
    parts = [part.strip() for part in re.split(r"[.!?]+", text)]
    return [part for part in parts if part]
