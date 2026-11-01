Resonate for Windows

The zip archive holds resonate.exe to unzip anywhere and run from a terminal, or double-click to open
the window; nothing is installed and nothing is written outside your own profile.

The setup program installs resonate.exe for your account alone (no administrator rights), adds
Resonate to the Start menu and lists it among the apps that can open audio files. Windows keeps the
choice of the default app for you: pick Resonate under Settings > Apps > Default apps. Uninstalling
removes the program, the shortcut and every registry entry setup wrote, and leaves your settings,
catalog and equaliser profiles where they are.

Settings live in %APPDATA%\resonate\config.toml; the catalog, the vault and the equaliser profiles
live in %LOCALAPPDATA%\resonate.

Sound goes through WASAPI. Every output device is listed twice by "resonate sinks": the plain name
plays through the Windows mixer, and the same name marked "(WASAPI exclusive)" takes the device for
Resonate alone at the song's own rate and word, so nothing in between touches the signal. Choose
that one for bit-perfect playback; other programs are silent on it while it plays.

The x86_64_v3 files need a machine with AVX2 (x86-64-v3): a processor from about 2015 onward.
The aarch64 files are native to Windows 11 on ARM64.
