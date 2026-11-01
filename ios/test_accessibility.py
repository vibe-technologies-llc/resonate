import unittest

import accessibility


class Scanner(unittest.TestCase):
    def rules(self, text):
        return [rule for rule, _ in accessibility.findings_in(text)]

    def test_a_symbol_nobody_names_is_found(self):
        self.assertEqual(self.rules('Image(systemName: "star")\n    .font(.title)\n'), ["unlabelled symbol"])

    def test_a_symbol_hidden_or_labelled_beside_it_is_not(self):
        self.assertEqual(self.rules('Image(systemName: "star")\n    .accessibilityHidden(true)\n'), [])
        self.assertEqual(self.rules('Image(systemName: "star").accessibilityLabel("Star")\n'), [])

    def test_a_tap_with_no_action_or_trait_is_found(self):
        self.assertEqual(self.rules("Text(x)\n    .onTapGesture { go() }\n"), ["gesture without an action or a trait"])

    def test_a_tap_with_a_trait_near_it_is_not(self):
        self.assertEqual(self.rules("Text(x)\n    .onTapGesture { go() }\n    .accessibilityAddTraits(.isButton)\n"), [])

    def test_a_drag_with_an_adjustable_action_near_it_is_not(self):
        self.assertEqual(self.rules("DragGesture()\n.accessibilityAdjustableAction { _ in }\n"), [])

    def test_an_animation_with_no_reduce_motion_gate_is_found(self):
        self.assertEqual(self.rules("withAnimation(.smooth) { on.toggle() }\n"), ["animation without a reduce motion gate"])
        self.assertEqual(self.rules("content.animation(.smooth, value: on)\n"), ["animation without a reduce motion gate"])

    def test_an_animation_gated_on_reduce_motion_is_not(self):
        self.assertEqual(self.rules("withAnimation(reduceMotion ? nil : .smooth) { on.toggle() }\n"), [])
        self.assertEqual(self.rules("@Environment(\\.accessibilityReduceMotion) var reduceMotion\n\nwithAnimation(.smooth) { on.toggle() }\n"), [])

    def test_a_bare_progress_view_is_found(self):
        self.assertEqual(self.rules("ProgressView()\n"), ["progress view without a label"])
        self.assertEqual(self.rules("ProgressView(value: 0.5)\n"), ["progress view without a label"])

    def test_a_progress_view_with_a_title_label_or_hiding_is_not(self):
        self.assertEqual(self.rules('ProgressView("Erasing…")\n'), [])
        self.assertEqual(self.rules('ProgressView().accessibilityLabel("Loading")\n'), [])
        self.assertEqual(self.rules("ProgressView()\n    .accessibilityHidden(true)\n"), [])
        self.assertEqual(self.rules('ProgressView(value: p) { Text("Analysing") }\n'), [])

    def test_a_font_of_a_fixed_size_is_found_and_a_scaled_one_is_not(self):
        self.assertEqual(self.rules(".font(.system(size: 34))\n"), ["font of a fixed size"])
        self.assertEqual(self.rules(".font(.system(size: glyph))\n"), [])


class Sources(unittest.TestCase):
    def test_nothing_new_goes_unlabelled_ungated_or_fixed(self):
        added = accessibility.findings() - accessibility.baseline()

        self.assertEqual(sorted(added.elements()), [], "fix these, or name why they stand in accessibility-baseline.json")

    def test_the_baseline_holds_only_what_is_still_there(self):
        gone = accessibility.baseline() - accessibility.findings()

        self.assertEqual(sorted(gone.elements()), [], "these are fixed: run `python3 ios/accessibility.py` to shrink the baseline")


if __name__ == "__main__":
    unittest.main()
