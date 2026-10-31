Resonate for Windows

Unzip this folder anywhere and run resonate.exe from a terminal, or double-click it to open the
window. Nothing is installed and nothing is written outside your own profile.

Settings live in %APPDATA%\resonate\config.toml; the catalog, the vault and the equaliser profiles
live in %LOCALAPPDATA%\resonate.

Sound goes through WASAPI. Every output device is listed twice by "resonate sinks": the plain name
plays through the Windows mixer, and the same name marked "(WASAPI exclusive)" takes the device for
Resonate alone at the song's own rate and word, so nothing in between touches the signal. Choose
that one for bit-perfect playback; other programs are silent on it while it plays.

Needs a machine with AVX2 (x86-64-v3): a processor from about 2015 onward.
