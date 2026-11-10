---
paths:
  - "packaging/**"
---

# Packaging

`packaging/windows/` is the Windows archives' and installers' recipe (`windows.md`): the PowerShell
`build.ps1` taking the architecture (x86-64-v3, or ARM64 at the target's baseline), which builds the
zip and compiles `resonate.iss` (an Inno Setup script, per user, with the file associations and the
Start menu shortcut), `sign.ps1` (Authenticode where secrets exist), `icon.py` (renders the
committed `resonate.ico` from `resonate.svg`), and the winget (`winget/`) and Scoop (`scoop.json`)
templates `fill-manifests.sh` fills at release (`release.md`). Not a package, since the build needs no system library; `crates/resonate/tests/installer.rs` holds
the script to `AUDIO_EXTENSIONS`, to `APP_USER_MODEL_ID`, to the build script's definitions and the
templates to the filler. PKGBUILD, RPM spec and Flatpak manifest all build the default-feature `resonate` binary plus
desktop entry, icon, metainfo, man pages, completions. The `resonate*.1` man pages and completions
come from the binary crate's build script, so a CLI change reaches every package with no second
list; `resonate-config.toml.5`, the settings file's manual, is written by hand and installed from
`packaging/` by each recipe (`every_package_installs_the_manual_of_the_settings_file`, and
`binary.md` for what holds its keys in step). Desktop
entry and metainfo `<provides>` are held to `MIME_TYPES` by tests (`binary.md`).

`packaging/macos/` is the Apple Silicon disk image's recipe (`macos.md`), not a package either:

- **`build.sh [VERSION] [OUTPUT]`** builds `resonate` and `resonate-signin` for
  `aarch64-apple-darwin` in two `cargo` runs (`RUSTFLAGS=""`, `MACOSX_DEPLOYMENT_TARGET=13.0`; the
  version read from `Cargo.toml` where none is given) and assembles `Resonate.app`:
  `Contents/MacOS` holds both programs, `Contents/Info.plist` is the template with `@VERSION@`
  filled by the version's `major.minor.patch`, and `Contents/Resources` holds `resonate.icns`, the
  licence, the man pages (`man/man1`, `man/man5`), the completions (`completions/`) and the two
  faces (`Fonts/`, where the window looks for them). It signs the application, stages it beside a
  link to `/Applications`, the README and the licence, and makes
  `resonate-<version>-aarch64-macos.dmg` with `hdiutil` (`UDZO`; tried up to three times, a hosted
  runner's `hdiutil` now and then finding the folder busy), then signs the image.
- **`sign.sh APPLICATION` and `sign.sh --image DISK_IMAGE`** sign ad hoc where no certificate is
  set (`codesign --sign -`: an Apple Silicon program runs only signed) and say so, the image being
  left as it is. With `RESONATE_MACOS_CERTIFICATE_BASE64` and `RESONATE_MACOS_CERTIFICATE_PASSWORD`
  the certificate goes into a keychain of the script's own (deleted, and the search list put back,
  on exit) and signs the sign-in window, then the bundle (hardened runtime, a secure timestamp,
  `entitlements.plist`: the microphone alone), then the image; with
  `RESONATE_MACOS_NOTARY_KEY_BASE64`, `RESONATE_MACOS_NOTARY_KEY_ID` and
  `RESONATE_MACOS_NOTARY_ISSUER` as well, the image is sent to the notary (`notarytool submit
  --wait`) and the ticket stapled to it. The bundle's main program is signed by signing the
  bundle, never apart, and nothing is signed `--deep`.
- **`Info.plist`** names the bundle `org.resonate.Resonate` (`APP_USER_MODEL_ID`), asks for macOS
  13.0, declares one *Audio* document type at rank `Alternate` (it claims no default) over
  `public.audio` and an imported `org.resonate.audio` listing every `AUDIO_EXTENSIONS` entry, takes
  the `resonate` scheme, and says why the microphone and the local network are asked for (English,
  `language.md`).
- **`icon.py` renders the committed `resonate.icns` from `resonate.svg`** with `rsvg-convert`, on
  Linux, so no runner tool draws it: the drawing inset to 824 of 1 024 px, as an icon sits on
  macOS's grid, in eight PNG entries from 32 to 1 024 px (`ic11`, `ic12`, `ic07`, `ic13`, `ic08`,
  `ic14`, `ic09`, `ic10`).
- **`crates/resonate/tests/bundle.rs`** holds the property list to `AUDIO_EXTENSIONS`,
  `APP_USER_MODEL_ID`, the desktop entry's scheme and the script's deployment target, its
  placeholder to the script, what it names to what the script copies, the icon's entries to their
  sides, the five variables to the workflow and the script, and the faces' addresses and digests to
  the Flatpak's, which are the only addresses the recipe may hold.

- **Inter and JetBrains Mono come with every package**, since the window draws in them and embeds
  neither (`ui.md`): `inter-font` and `ttf-jetbrains-mono` in both Arch recipes (and `.SRCINFO`),
  `rsms-inter-fonts` and `jetbrains-mono-fonts` required by the spec, `fonts-inter` a DEB `Depends`
  and `fonts-jetbrains-mono` only `Recommends` (Debian dropped it from testing in May 2026, so a hard
  dependency could leave the player uninstallable there), and the Flatpak builds each from its
  release archive, checksummed, into `/app/share/fonts` (the runtime holds neither); the macOS
  recipe fetches the same two archives under the same digests into the bundle's
  `Contents/Resources/Fonts`, each with its licence
  (`every_package_brings_the_reading_face_and_the_face_for_codes`).
- **`yt-dlp` and `deno` are suggested, never depended on**: `optdepends` in both Arch recipes (and
  `.SRCINFO`), `Suggests` in the spec and the player's DEB control. Only YouTube Music runs it
  (browsing the account, the poll's last-resort answer and *Keep*); recent `yt-dlp` needs a
  JavaScript runtime for YouTube's challenges, and `deno` is the one it finds by itself (`node` or
  `bun` serve where present). A Flatpak holds neither and shows the provider as unavailable, and no
  package ships them (`providers.md`).
- **`resonate-signin` is recommended, never depended on**, and is its own package: `-git`'s
  PKGBUILD is split (`resonate-signin-git`, `optdepends` of the player), `release/signin/PKGBUILD`
  is `resonate-signin` (`optdepends` of the release player), the spec's subpackage
  `resonate-signin` is `Recommends`, and the DEB script's `signin` builds `resonate-signin`, the
  player's control `Recommends` it. Each depends on WebKitGTK 4.1; each builds it with `--package
  resonate-signin` in a `cargo` run apart from the player's, which is `--package resonate --bin
  resonate`, never `--workspace` (`dependencies.md`). The Windows zip and installer carry
  `resonate-signin.exe` beside `resonate.exe` (`SourceSignIn`, signed with it), and the macOS
  bundle `resonate-signin` beside `resonate` in `Contents/MacOS`. The Flatpak, having
  no `yt-dlp`, carries none.
- **Every package sets `RUSTFLAGS` over the checkout's `target-cpu=native`** (environment
  `RUSTFLAGS` replaces `build.rustflags`). The release assets (spec, release PKGBUILDs, DEB script)
  build for `x86-64-v3` and nothing else: `ExclusiveArch: x86_64`, `arch=('x86_64')`, `amd64`, the
  requirement said in the Arch `pkgdesc` and the DEB description
  (`every_release_recipe_builds_for_x86_64_v3_alone`). The macOS recipe empties `RUSTFLAGS`, which
  leaves `aarch64-apple-darwin` at the target's own baseline. The `-git` PKGBUILDs append `native` in
  `build` and `check` (AUR builds target the building machine) and keep `aarch64`; the Flatpak
  keeps its own baseline (`x86-64` / `generic`) and is no release asset.
- **Spec: `--locked`, built from a source archive** of the release tag, prefixed
  `resonate-<Version>/`; Fedora 44; the spec runs no tests, which CI and the maintainer's checks hold,
  since a release build that also tested compiled the workspace twice. `release.yml`'s
  `fedora-rpm` job builds the player and the server RPM and the source RPMs in one container, both
  into one `CARGO_TARGET_DIR` so the crates they share compile once; the specs and the release
  PKGBUILDs install from `${CARGO_TARGET_DIR:-target}`, and the `arch-package` job shares one the
  same way. The DEB script already shares the checkout's `target/`.
  `git archive` trusts `GITHUB_WORKSPACE` via `safe.directory` for that invocation only
  (the container refuses the runner-owned checkout). Dependencies install as root but `rpmbuild`
  runs as `rpm-builder`.
  Cargo fetches while building: a source RPM recipe, not an offline distribution build.
- **Pre-release spellings are in `release.md`.** The spec tests map `-` to `~` before comparing, the
  release PKGBUILDs hold `_version` to Cargo's and `pkgver` to `${_version//-/}`, and the DEB script
  derives its version from `Cargo.toml` itself (`the_deb_script_derives_its_version_from_cargo`).
- **A release names the player `resonate` in every format and the server `resonate-server`, and
  attaches no debug package.** The RPM specs set `%global debug_package %{nil}` (no `-debuginfo` or
  `-debugsource` asset) and every PKGBUILD has `'!debug'` (no `-debug` asset); the DEBs never had one
  (`no_release_recipe_builds_a_debug_package`, `the_player_is_named_resonate_in_every_release_format`).
  The release PKGBUILD `replaces` and conflicts with 0.1.0's `resonate-player` name; the AUR's
  `-git` recipe keeps `resonate-player-git`.
- **`packaging/release/` holds the non-git Arch recipes** (`PKGBUILD` is `resonate`,
  `server/PKGBUILD` is `resonate-server`): source is the GitHub archive of tag `v$_version`
  (`sha256sums` `SKIP`, an archive's bytes being GitHub's), a local file of the same name beside
  the recipe taking its place, which is how `release.yml` builds from the tag checkout it already
  has. `resonate.install` there is a symlink to the player's. The installed layout is the `-git`
  recipe's.
- **`packaging/deb/build.sh player|server [directory]` builds the DEBs** on Ubuntu 24.04 with
  rustup stable (the distribution's cargo predates edition 2024): `cargo build --locked --release`
  under `-C target-cpu=x86-64-v3`, the spec's `%install` layout (man pages gzipped, the zsh
  completion under `vendor-completions`), `Depends` from `dpkg-shlibdeps` plus `pipewire` and
  `libvulkan1` (loaded at run time) for the player or `systemd` for the server, and `dpkg-deb
  --root-owner-group`. The server's environment file is a `conffiles` entry and its maintainer
  scripts only reload systemd and stop the unit on removal.
- **`release.yml` (`build.md`) builds these recipes in parallel jobs and `publish` attaches
  everything**, so one failing job leaves the others' assets. Asset names carry no `~` (`-` in its
  place) and the Arch packages are renamed `-x86_64_v3.pkg.tar.zst`.
- **Flatpak builds its own checkout** (`dir` source, no release tarball: metainfo names no
  `<release>`) on Freedesktop 25.08 SDK + `rust-stable`, network shared for `cargo fetch --locked`,
  no tests.
- **Sandbox = the host sockets and names the app uses**: home, `xdg-run/pipewire-0` (not
  PulseAudio), own+talk `org.mpris.MediaPlayer2.resonate` and `.resonate.*`,
  `org.freedesktop.Notifications`, system-bus `org.bluez` (headset controls, `mpris.md`),
  `discord-ipc-0`..`-9` plus Discord and Vesktop runtime directories. No `--share=ipc`, no `x11` or
  `fallback-x11`: on Linux gpui is built with `wayland` alone (`the_flatpak_grants_no_x11_to_a_wayland_only_build`).
- **The spec's `Version` is the workspace's with `-` as `~`** (`the_rpm_spec_builds_the_version_cargo_declares`);
  the `-git` PKGBUILD's `pkgver()` reads `Cargo.toml` itself.

**Every package installs one `resonate.desktop` and `resonate.svg`; the Flatpak renames them
while building.** flatpak-builder exports only app-id-named files, so the manifest has
`rename-desktop-file: resonate.desktop`, `rename-icon: resonate`, giving
`org.resonate.Resonate.desktop` and `.svg`; metainfo `<launchable type="desktop-id">` is rewritten
to match. `StartupWMClass=resonate` stays (window `APP_ID` matches). Held by
`crates/resonate/tests/packaging.rs` (`the_flatpak_exports_the_desktop_entry_and_icon_under_its_id`).
Bus `DesktopEntry` and the notification `desktop-entry` hint follow: `mpris.rs` answers
`FLATPAK_ID` where set, else `resonate`.

**`packaging/server/` deploys the sync server, not the player** (`server.md`): a hardened systemd
unit, its environment file and the two recipes that install them with the `resonate-server` binary:
`PKGBUILD` (`resonate-server-git`) and `resonate-server.spec` (`resonate-server`), beside
`release/server/PKGBUILD` and the DEB script's `server`. All build only
`--package resonate-server` (it reaches no PipeWire, GPUI or Vulkan, so the recipes name no such
dependency), set `RUSTFLAGS` as the player's recipes do, install the unit under systemd's unit
directory and the environment file as `/etc/resonate-server/resonate-server.env`, kept across an
upgrade (`backup=` / `%config(noreplace)`), and leave the certificate and key to the listener. The
spec reads the player's source archive (`resonate-<version>.tar.gz`) and is held to the workspace's
version like the player's (`the_server_spec_builds_the_version_cargo_declares`);
`every_server_package_installs_the_unit_and_its_environment` holds every recipe to the two files.
CI builds the PKGBUILD (`server-package`: `makepkg` as a build user, `source` pointed at the runner's
checkout, `check()` included); the release recipes build only in `release.yml`.

**`packaging/.SRCINFO` = `makepkg --printsrcinfo` of the player's PKGBUILD beside it**, committed
because the AUR reads it. Regenerate it when publishing to the AUR; no test holds it, since the VCS
`pkgver` moves with every commit. The server's PKGBUILD has none until it is published.
