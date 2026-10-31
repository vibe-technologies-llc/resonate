---
paths:
  - "crates/resonate-wasapi/**/*.rs"
  - "crates/resonate-core/src/os_bytes.rs"
  - "crates/resonate-core/src/file_facts.rs"
  - "crates/resonate-core/src/positioned.rs"
  - "crates/resonate-core/src/windows_uri.rs"
  - "crates/resonate/src/interrupts.rs"
  - "crates/resonate/src/windows_launcher.rs"
  - "crates/resonate-listen/src/unavailable.rs"
  - "crates/resonate-discord/src/unreached.rs"
  - "packaging/windows/**"
---

# Windows

The whole workspace but the phones' boundary and PipeWire builds for `x86_64-pc-windows-msvc`
(`cargo xwin check --target x86_64-pc-windows-msvc --workspace --exclude resonate-mobile --exclude
resonate-bindgen --exclude resonate-pipewire` on Linux, the `windows` CI job on a hosted runner).
Nothing here has been heard on a real Windows machine: this tree was cross-checked and the portable
parts tested on the host, and `docs/FEATURES.md`'s *Windows parity* lists what only hardware can
settle. `unsafe_code = "forbid"` holds: every Win32 and COM call goes through a safe wrapper crate
(`dependencies.md`).

## The seam is what Linux already had

- **`engine::desktop_backend(app_name)` is the one place a platform's backend is named**: PipeWire
  on Linux, `resonate_wasapi::Wasapi` on Windows. `Player::with_sources` builds through it, and the
  binary's `surveyed::sinks` (`sinks`, `explain`, `eq --suggest`) and `rendering::Rendering`
  (`Beside` over the backend) use it, so the binary names no PipeWire and its Windows tree holds
  none of PipeWire, Wayland, `rustix` or `signal-hook` (the layering job).
- **`resonate-wasapi` is `resonate-audio`'s `Backend` over the `wasapi` crate** and reaches core and
  audio alone. Its portable halves compile and are tested everywhere (`ways`, `formats`, `changes`,
  `fill`); `device`, `stream`, `render` and `backend` are `cfg(windows)`.
- **An endpoint is two sinks.** `wasapi:<endpoint id>` plays through the Windows mixer in shared mode
  (event-driven, `autoconvert` on, offered at the mix format alone) and is the default sink when the
  endpoint is; `wasapi-exclusive:<endpoint id>` ("<name> (WASAPI exclusive)") takes the device in
  exclusive event-driven mode at the format the engine plans, offered for every rate, word and layout
  `IsFormatSupported` accepts out of a fixed grid (`PROBED_RATES` x `SampleFormat::ALL` x
  `PROBED_LAYOUTS`, each combination its own `SinkFormats` entry so a rate is never claimed for a
  layout it was not probed with). Bit-perfect on Windows is therefore a choice of device the
  listener makes in the output pane, and following the default never takes a device away from other
  programs. `SinkId` is an FNV-1a hash of the node name (`sink_id_of`), so the two ways of one
  endpoint have two ids.
- **The twenty-four bit word is left-aligned for the device.** The ring's `S24` is a sign-extended
  low 24 bits of an `i32`; WASAPI's 24-in-32 container carries the 24 against the top, so
  `fill_buffer` shifts each word left by eight (`wire_of`: S16 is 16/16, S24 32/24, S32 32/32, F32
  32/32 float).
- **A stream is a thread of its own** (`resonate-wasapi-stream`): every COM object is made on it
  (`initialize_mta` first), opens are answered through a one-slot channel within five seconds, and
  commands arrive on a channel the loop reads between events. `SetActive(true)` fills the buffer
  once and starts the client, `SetActive(false)` stops and resets it, `Drain` is told once the
  device position (frames written less the buffer's padding) has passed the last frame music
  reached, and a failure of any call is `StateChanged` to `Failed` so the engine waits for the
  device as for any sink gone. `Reshaper` is not given (`Unreshapable::Unsupported`): a rebind
  closes and opens.
- **Devices are watched by polling.** The backend thread surveys every two seconds and tells the
  difference (`changes_between`: added, removed, default moved, formats or mix rate moved); the
  crate's endpoint notifications are not used. Volume and mute of the device are `Unsupported`.
- **A stamp is the wall clock plus the buffer's padding**, not the audio clock's position against
  the performance counter: the stream's `StreamClock` is noted each cycle with the frames queued
  (`Playing::cycle`) and `fill_stamped` takes `StreamClock::stamp`. Handing a song between zones
  therefore lands within a period of the other, not within frames (`audio.md`).

## The filesystem under the shared crates

- **A path is bytes through `resonate_core::os_bytes`**: Unix keeps `OsStrExt`; Windows writes WTF-8
  by hand over `encode_wide`/`from_wide` (a lone surrogate is three bytes), so `path_text`, `naming`
  and `MediaLocation` keep their byte designs and a stored locator is the same text on either.
- **A `file://` URI is `file:///C:/Music/a.flac`, a share `file://host/share/a.flac`**
  (`windows_uri`, tested on every host); `\\?\` prefixes are dropped; `from_uri` takes the rest of
  the URI back to a `\` path. On Unix a URI still has to start at the root.
- **Positioned I/O is one trait** (`positioned::FileExt`): the standard Unix one, or `seek_read` and
  `seek_write` on Windows (which move the cursor, so no caller reads a file by position and by
  cursor at once).
- **`file_facts` is every metadata question the Unix tree answered with `MetadataExt`**: `FileStamp`
  (device, inode, length, mtime on Unix; length and mtime on Windows) for the tag writer's
  "unchanged since taken" test, `is_the_same_entry`/`is_the_same_folder` (inode on Unix,
  `same-file` on Windows), `volume_of` (device on Unix, a hash of the drive prefix on Windows: a
  folder mounted into another drive's tree is not noticed), `changed_at` (status change on Unix,
  modification time on Windows) and `sync_directory` (a no-op on Windows, where a directory cannot
  be opened to be synced).
- **What the tag writer carries to its copy is the mode on Windows**: owner, hard-link count and
  extended attributes are `cfg(unix)` (`carries_what_the_file_did`), so NTFS alternate streams and
  ACLs are not carried.
- **A held file's lock is mandatory on Windows** (`LockFileEx`): `writer_of` still asks the lock
  first and a dead writer's is released by the system; with no `/proc` the pid stamp reads as
  living where the lock cannot be asked.
- **Deleting goes to the recycle bin** (`trashing::RecycleBin` over `trash::delete`); the hand-written
  freedesktop trash and the portal are Unix. A catalog with no trasher keeps the file
  (`Removal::Kept`), never removes it for good.
- **Settings are `%APPDATA%\resonate\config.toml`; data and cache are `%LOCALAPPDATA%\resonate`**
  (`config::known_folder`), unless an `XDG_*` variable is set; `~` is `%USERPROFILE%`. A config is
  not made owner-only (the profile folder is private by inheritance).
- **The spool is the system temporary folder** and its anonymous file is
  `FILE_FLAG_DELETE_ON_CLOSE`; free space on the volume is not read.

## The terminal and interrupts

- **Interrupts are `ctrlc`'s one handler** (`interrupts.rs`, `signals` on Windows): a process-wide
  reaction replaced by whoever watches (`quit_when_told`, `cancel_when_told`, `leave_when_told`) and
  cleared when its `Interrupting` drops; the first interrupt asks to quit or cancels the pass, the
  second leaves with 130.
- **A key at a time is `crossterm`'s raw mode**, not termios: key events become the bytes the Unix
  readers parse (arrows as `ESC [ A`), so `Keyed` and `Struck` are one state machine on both. Raw
  mode swallows Ctrl-C as a key, which `interrupted()` turns back into the interrupt. A piped stdin
  is read as bytes. `Readout` draws only where `crossterm::ansi_support::supports_ansi()` holds.

## Not on Windows yet

The MPRIS service, notifications, idle inhibit, network metering and `Running` (so `queue`, `sleep`
and `players` against a running window) answer "no bus": `resonate-mpris` compiles but finds no
session bus, and the binary carries on without it as it does on a Linux machine with none. Capture
(`resonate-listen`) is a `Listener` that lists no microphone and refuses to record
(`Error::Unavailable`). Discord's IPC is a stub that never finds Discord (`unreached.rs`). The
launcher's icon-follows-the-accent is a no-op. All of it is on the roadmap.

## Release

`packaging/windows/build.ps1` builds `-C target-cpu=x86-64-v3` and zips `resonate.exe`, the licence
and a README as `resonate-<version>-x86_64_v3-windows.zip` (`release.yml`'s `windows-zip`, in the
`SHA256SUMS`); `crates/resonate/tests/packaging.rs` holds it to v3 and to the manifest's version. It
is not signed and has no installer.
