import unittest

import building

COMPLETED = "Building for debugging...\n[147 / 148] Resonate-App-product\nBuild complete! (13.01 secs)\nWrote to x\n"


class Output(unittest.TestCase):
    def test_a_clean_build_has_no_warnings_and_completes(self):
        self.assertEqual(building.warnings_in(COMPLETED), [])
        self.assertTrue(building.completed(COMPLETED))

    def test_a_swift_warning_is_found_by_its_line(self):
        warned = "/ios/Sources/Resonate/Library.swift:12:5: warning: variable 'x' was never used\n" + COMPLETED

        self.assertEqual(building.warnings_in(warned), [warned.splitlines()[0]])

    def test_a_warning_the_compiler_styles_is_found_once_not_twice_for_its_excerpt(self):
        styled = (
            "/ios/Sources/Resonate/Work.swift:135:27: \x1b[1;33mwarning: \x1b[1;39minitialization of variable 'never' was never used\x1b[0;0m"
            " [#\x1b]8;;https://docs.swift.org/x\x1b\\NoUsage\x1b]8;;\x1b\\]\n"
            "\x1b[0;36m135 |\x1b[0;0m func probeWarning() { var never = 1 }\n"
            "    \x1b[0;36m|\x1b[0;0m                           `- \x1b[1;33mwarning: \x1b[1;39minitialization of variable 'never' was never used\x1b[0;0m\n"
        )

        self.assertEqual(len(building.warnings_in(styled + COMPLETED)), 1)

    def test_a_linker_warning_is_found(self):
        warned = "ld: warning: object file was built for newer version\n" + COMPLETED

        self.assertEqual(len(building.warnings_in(warned)), 1)

    def test_a_word_that_only_contains_warning_is_not_one(self):
        self.assertEqual(building.warnings_in("Compiling Warnings.swift\nnote: a warning-free file\n"), [])

    def test_a_compiler_error_is_found_once_by_its_header_and_not_by_its_excerpt(self):
        failed = (
            "/ios/Sources/Resonate/Library.swift:12:5: \x1b[1;31merror: \x1b[1;39mcannot find 'x' in scope\x1b[0;0m\n"
            "\x1b[0;36m12 |\x1b[0;0m   x\n"
            "    \x1b[0;36m|\x1b[0;0m   `- \x1b[1;31merror: \x1b[1;39mcannot find 'x' in scope\x1b[0;0m\n"
            "error: Build failed\n"
        )

        self.assertEqual(
            building.errors_in(failed),
            ["/ios/Sources/Resonate/Library.swift:12:5: error: cannot find 'x' in scope", "error: Build failed"],
        )

    def test_a_build_that_stopped_before_completing_is_not_complete(self):
        self.assertFalse(building.completed("Building for debugging...\nerror: something broke\n"))


if __name__ == "__main__":
    unittest.main()
