import unittest

from lib import textstats


class TextstatsTest(unittest.TestCase):
    def test_words_and_counts(self):
        self.assertEqual(textstats.words("It's a Test"), ["it's", "a", "test"])
        self.assertEqual(textstats.word_count("one two three"), 3)

    def test_top_words(self):
        self.assertEqual(textstats.top_words("b a b c a b", 2), ["b", "a"])

    def test_reading_level(self):
        self.assertEqual(textstats.reading_level(""), "empty")
        self.assertEqual(textstats.reading_level("a cat"), "easy")
        self.assertEqual(textstats.reading_level("simple words"), "medium")
        self.assertEqual(textstats.reading_level("extraordinary complexity"), "hard")

    def test_sentences(self):
        self.assertEqual(textstats.sentences("One. Two! Three?"), ["One", "Two", "Three"])
