from pathlib import Path
import unittest

SOURCES = Path(__file__).resolve().parent / "Sources/Resonate"
CARDS_ON = {"CollectionViews.swift": 2, "HomeViews.swift": 2}
CARD_STYLE = ".buttonStyle(.shortcutCard)"


def source(name):
    return (SOURCES / name).read_text()


class ShortcutCards(unittest.TestCase):
    def test_every_card_on_both_screens_is_a_button_in_the_one_card_style(self):
        for name, cards in CARDS_ON.items():
            text = source(name)

            self.assertEqual(text.count(CARD_STYLE), cards, name)

    def test_the_card_draws_no_frame_of_its_own(self):
        text = source("CollectionViews.swift")
        card = text[text.index("struct ShortcutCard: View"):text.index("struct ShortcutCardStyle")]

        for modifier in (".frame(", ".padding(", ".background(", ".containerRelativeFrame("):
            self.assertNotIn(modifier, card)

    def test_the_cards_of_a_row_stretch_to_the_tallest(self):
        text = source("CollectionViews.swift")

        self.assertIn("maxHeight: .infinity", text)
        self.assertIn(".fixedSize(horizontal: false, vertical: true)", text)


if __name__ == "__main__":
    unittest.main()
