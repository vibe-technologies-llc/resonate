from pathlib import Path
import plistlib
import unittest

IPHONE = 1
IPAD = 2
UPSIDE_DOWN = "UIInterfaceOrientationPortraitUpsideDown"
EVERY_ORIENTATION = {
    "UIInterfaceOrientationPortrait",
    UPSIDE_DOWN,
    "UIInterfaceOrientationLandscapeLeft",
    "UIInterfaceOrientationLandscapeRight",
}


def info():
    with (Path(__file__).resolve().parent / "Resonate/Info.plist").open("rb") as plist:
        return plistlib.load(plist)


class Orientation(unittest.TestCase):
    def test_the_app_runs_on_an_iphone_and_an_ipad(self):
        self.assertEqual(sorted(info()["UIDeviceFamily"]), [IPHONE, IPAD])

    def test_an_ipad_turns_every_way_so_multitasking_is_offered(self):
        plist = info()

        self.assertEqual(set(plist["UISupportedInterfaceOrientations~ipad"]), EVERY_ORIENTATION)
        self.assertNotIn("UIRequiresFullScreen", plist)

    def test_an_iphone_turns_upright_and_sideways_but_not_upside_down(self):
        orientations = set(info()["UISupportedInterfaceOrientations"])

        self.assertEqual(orientations, EVERY_ORIENTATION - {UPSIDE_DOWN})


if __name__ == "__main__":
    unittest.main()
