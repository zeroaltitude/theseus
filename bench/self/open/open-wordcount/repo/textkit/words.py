"""Small text helpers for the editor's limits and previews."""


def count_words(text: str) -> int:
    """How many words `text` holds. A word is a run of characters between whitespace."""
    if not text:
        return 0
    return len(text.split(" "))


def longest_word(text: str) -> str:
    """The longest word in `text`; the first of equals; empty when there is none."""
    best = ""
    for w in text.split():
        if len(w) > len(best):
            best = w
    return best
