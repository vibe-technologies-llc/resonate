from pathlib import Path
import unittest

import icons

REPOSITORY = Path(__file__).resolve().parent.parent
PACKAGED = (REPOSITORY / "packaging/resonate.svg").read_text()


class Accents(unittest.TestCase):
    def test_every_accent_is_read_out_of_the_resonate_flavour(self):
        accents = icons.resonate_accents(REPOSITORY)

        self.assertEqual(tuple(accents), icons.ACCENTS)
        self.assertTrue(all(0 <= colour <= 0xFFFFFF for colour in accents.values()))

    def test_the_lift_is_a_share_of_the_way_to_white(self):
        self.assertTrue(0 < icons.lifted_toward_white(REPOSITORY) < 1)

    def test_a_flavour_missing_an_accent_is_refused_by_name(self):
        with self.assertRaisesRegex(icons.Unreadable, "the mauve accent"):
            icons.found(r"mauve: 0x([0-9a-f]{6})", "blue: 0x000000", "the mauve accent")

    def test_lifting_toward_white_by_nothing_changes_nothing_and_by_all_gives_white(self):
        self.assertEqual(icons.lifted(0x123456, 0.0), 0x123456)
        self.assertEqual(icons.lifted(0x123456, 1.0), 0xFFFFFF)


class Recolouring(unittest.TestCase):
    def test_the_packaged_icon_holds_each_stop_once(self):
        for stop in icons.PACKAGED_STOPS:
            self.assertEqual(PACKAGED.count(stop), 1, stop)

    def test_each_stop_is_replaced_by_its_colour(self):
        drawing = icons.drawn(PACKAGED, 0x112233, 0.5)

        self.assertNotIn(icons.PACKAGED_STOPS[0], drawing)
        self.assertNotIn(icons.PACKAGED_STOPS[1], drawing)
        self.assertIn('stop-color="#112233"', drawing)
        self.assertIn(f'stop-color="#{icons.lifted(0x112233, 0.5):06x}"', drawing)

    def test_a_drawing_without_a_stop_is_refused_not_left_uncoloured(self):
        with self.assertRaisesRegex(icons.Unreadable, "0 times"):
            icons.recoloured("<svg/>", icons.PACKAGED_STOPS, (0, 0))

    def test_a_stop_held_twice_is_refused_not_half_replaced(self):
        twice = icons.PACKAGED_STOPS[0] * 2 + icons.PACKAGED_STOPS[1]

        with self.assertRaisesRegex(icons.Unreadable, "2 times"):
            icons.recoloured(twice, icons.PACKAGED_STOPS, (0, 0))


class FullBleed(unittest.TestCase):
    def test_the_ground_fills_the_square_and_the_ring_is_gone(self):
        drawing = icons.full_bleed(PACKAGED)

        self.assertIn(icons.FULL_BLEED_GROUND, drawing)
        self.assertNotIn(icons.ROUNDED_GROUND, drawing)
        self.assertNotIn(icons.BAKED_RING, drawing)
        self.assertNotIn("rx=", drawing)

    def test_the_mark_is_untouched(self):
        mark = PACKAGED[PACKAGED.index("<g transform"):]

        self.assertIn(mark, icons.full_bleed(PACKAGED))

    def test_every_accent_is_drawn_full_bleed(self):
        drawing = icons.drawn(PACKAGED, 0x112233, 0.5)

        self.assertNotIn("rx=", drawing)
        self.assertNotIn(icons.BAKED_RING, drawing)

    def test_a_drawing_whose_ground_moved_is_refused_by_name(self):
        with self.assertRaisesRegex(icons.Unreadable, "0 times"):
            icons.full_bleed(PACKAGED.replace('rx="28"', 'rx="30"'))

    def test_a_drawing_whose_ring_moved_is_refused_by_name(self):
        with self.assertRaisesRegex(icons.Unreadable, "0 times"):
            icons.full_bleed(PACKAGED.replace(icons.BAKED_RING, ""))


class Declarations(unittest.TestCase):
    def test_there_are_twenty_one_icons(self):
        self.assertEqual(len(icons.icon_names(icons.ACCENTS)), 21)

    def test_the_icons_drawn_are_the_ones_xtool_and_the_info_list_declare(self):
        icons.check_declared(REPOSITORY, icons.icon_names(icons.ACCENTS))

    def test_an_icon_not_drawn_is_refused(self):
        drawn = icons.icon_names(icons.ACCENTS) - {"IconRed@3x.png"}

        with self.assertRaisesRegex(icons.Unreadable, "IconRed@3x.png"):
            icons.check_declared(REPOSITORY, drawn)


if __name__ == "__main__":
    unittest.main()
