import re

STYLE = re.compile(r"\x1b\[[0-9;]*m|\x1b\]8;;[^\x1b]*\x1b\\")
WARNING = re.compile(r"^(?:\S.*?:\d+:\d+: |ld: )?warning: ")
ERROR = re.compile(r"^(?:\S.*?:\d+:\d+: |ld: )?error: ")
COMPLETE = re.compile(r"^Build complete!", re.MULTILINE)


def widget_info(app):
    return {
        "CFBundleShortVersionString": app["CFBundleShortVersionString"],
        "CFBundleVersion": app["CFBundleVersion"],
        "CFBundleDisplayName": app["CFBundleDisplayName"],
        "CFBundleLocalizations": app["CFBundleLocalizations"],
        "CFBundleSupportedPlatforms": ["iPhoneOS"],
        "UIDeviceFamily": app["UIDeviceFamily"],
        "UIRequiredDeviceCapabilities": ["arm64"],
        "NSExtension": {"NSExtensionPointIdentifier": "com.apple.widgetkit-extension"},
    }


def plain(output):
    return STYLE.sub("", output)


def warnings_in(output):
    return [line for line in plain(output).splitlines() if WARNING.search(line)]


def errors_in(output):
    return [line for line in plain(output).splitlines() if ERROR.search(line)]


def completed(output):
    return COMPLETE.search(plain(output)) is not None
