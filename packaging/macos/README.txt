Resonate for macOS

Drag Resonate onto the Applications folder beside it, then open it from there. It needs macOS 13
or later. The image named aarch64 is for Apple Silicon and the one named x86_64_v3 for an Intel
Mac (every Intel Mac macOS 13 runs on has the instructions it is built for).

This build is signed ad hoc and is not notarised by Apple, so the first time it is opened macOS
says it cannot check it. Open System Settings, Privacy & Security, scroll to the message about
Resonate and choose Open Anyway; that is asked once. From a terminal the same is

    xattr -dr com.apple.quarantine /Applications/Resonate.app

macOS asks once whether Resonate may show notifications; with them allowed, a song change is told
with Previous, Play/Pause and Next beside it while the window is not in front.

macOS asks before Resonate uses the microphone (naming the song playing around you, measuring your
headphones or your room), before it records what the Mac itself plays (naming the song another
program plays; macOS 14.2 or later) and before it looks for devices on your network (your other devices in
step, streamers and amplifiers to play to). Either can be refused and changed later under Privacy
& Security.

Settings live in ~/Library/Application Support/resonate/config.toml, beside the catalog, the vault
and the equaliser profiles; analyses are kept in ~/Library/Caches/resonate.

Sound goes through Core Audio. Every output device is listed twice by "resonate sinks": the plain
name plays through the system's mixer at the rate the device is set to, and the same name marked
exclusive takes the device for Resonate alone and sets it to the song's own rate and word, so
nothing in between touches the signal. Choose that one for bit-perfect playback; other programs
are silent on it while it plays.

The same program runs from a terminal. To have "resonate" on your PATH:

    sudo ln -s /Applications/Resonate.app/Contents/MacOS/resonate /usr/local/bin/resonate

Its manual pages and shell completions are in Resonate.app/Contents/Resources.

resonate-signin beside it is the YouTube Music sign-in window: Settings, Accounts, YouTube Music,
Sign in opens it on Google's own page and keeps the account's cookies for yt-dlp alone. A window
opened from the Finder or the Dock does not read your shell's PATH, so yt-dlp is also looked for
in Homebrew's folders; where it lives anywhere else, name its full path under Settings, Accounts,
YouTube Music.
