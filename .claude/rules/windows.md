---
paths:
  - "crates/resonate-wasapi/**/*.rs"
  - "crates/resonate-core/src/os_bytes.rs"
  - "crates/resonate-core/src/file_facts.rs"
  - "crates/resonate-core/src/positioned.rs"
  - "crates/resonate-core/src/windows_uri.rs"
  - "crates/resonate/src/interrupts.rs"
  - "crates/resonate/src/windows_launcher.rs"
  - "crates/resonate/src/awake.rs"
  - "crates/resonate-listen/src/unavailable.rs"
  - "crates/resonate-discord/src/pipe.rs"
  - "crates/resonate-ui/src/system.rs"
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
  `fill`, `clock`, `level`, `reshaping`, `watching`); `device`, `stream`, `render`, `priority` and
  `backend` are `cfg(windows)`.
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
  orders arrive on a channel the loop reads between events (`Order`: a `StreamCommand`, or a
  reshape taken). `SetActive(true)` fills the buffer once and starts the client (and is nothing on a
  stream already streaming, as the engine sends it after every reshape), `SetActive(false)` stops
  and resets it, `Drain` is told once the device position has passed the last frame music reached,
  and a failure of any call is `StateChanged` to `Failed` so the engine waits for the device as for
  any sink gone.
- **A stream is reshaped where its client stays the same.** The render cycle reads `Fed::source`
  (`realtime.md`), and the stream's `Reshaper` hands a new source through `Handing` and tells the
  loop (`Order::Reshaped`), which calls off a drain and moves the thread's priority; so a rebind
  that keeps the device, the format and the buffer (a ring of another depth, a chain rebuilt, a
  priority turned) keeps the stream. `refusal_of` refuses another target (`AnotherTarget`) and
  another spec or latency (`AnotherFormat`): an `IAudioClient`'s format and buffer are fixed by its
  one `Initialize`, and an exclusive endpoint cannot hold a second client while the first is open,
  so a rate, word or layout change genuinely closes the client and opens another, and the engine's
  reopen is that. `StillHandingOver` while the last source is untaken, `NoStream` once the thread
  has ended.
- **The stream thread is in MMCSS's `Audio` class** (`priority::Priority`, over
  `audio_thread_priority`, whose safe call names the `Audio` task and not `Pro Audio`) while the
  request is `realtime`, demoted on the same thread when a reshape lowers it or the thread ends;
  where MMCSS refuses (the service stopped), `thread-priority`'s `Max` stands in.
- **Devices are told by the system and checked by a poll.** The watcher thread registers the
  `wasapi` crate's endpoint notification client (added, removed, state, default and property
  changes), whose callbacks only nudge a one-slot channel; `watching::watch` waits on the nudge or
  `Pace::ENDPOINTS`' two seconds, lets a burst settle for 100 ms, surveys once and tells the
  difference through `changes_between` (added, removed, default moved, formats or mix rate moved,
  and `Turned` where the endpoint's volume or mute moved), the one difference computation for both.
  A refused registration leaves the poll alone. The endpoint volume has no notification here, so a
  volume turned from the Windows mixer is noticed by the poll, within two seconds.
- **The endpoint's volume and mute are the device's** (`set_device_volume`, `set_device_mute`, over
  `volumecontrol-windows`, which wraps `IAudioEndpointVolume` by endpoint id; both sinks of an
  endpoint turn the one level). Every survey reads it into `SinkPort` (the endpoint's description,
  the level, the mute, and `HardwareVolume::Yes` only where `IAudioMeterInformation`'s hardware
  support says the device turns its own volume, else `No`), so `device-volume` hands the slider to
  the device only where that keeps the stream bit-perfect: in exclusive mode a software endpoint
  volume is not applied at all, and in shared mode it is the mixer's. The crate speaks the Windows
  slider's whole percent, and the slider is ours (`level::percent_of` is `Volume::heard_at` in
  percent, `gain_of` its inverse), so both sliders stand at the same place; a read-back below about
  a tenth of the way can differ from what was sent by more than `A_DEVICES_HALF_STEP_DB` and moves
  the slider by under a percent.
- **A stamp is `IAudioClock`'s position against the performance counter** (`render::Playing`): each
  cycle reads `GetPosition` (the device position, in the clock's `GetFrequency` units, and the
  counter when it was read, in 100 ns) and the counter now (`win-time-sys`'s `PerformanceClock`), and
  `clock::ClockReading::device_time` hands `StreamClock::stamp_by` both in nanoseconds with the
  frames written less those played, so a zone handed a song lands within frames of the other
  (`audio.md`). The same frames are the latency the clock notes and the device position a drain
  waits for. A stream with no audio clock, a reading naming no counter or one more than 100 ms from
  now falls back to the buffer's padding and `StreamClock::stamp`.

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
- **A file or directory is synced through `file_facts`**: `sync_file` opens a Windows file for
  writing (`FlushFileBuffers` refuses a read-only handle) and `sync_directory` is a no-op there (a
  directory cannot be opened as a file), so no writer calls `File::open(folder)` or syncs a
  read-only `File::open`.
- **Every root is `Naming::Portable` on Windows** (`Naming::of` reads no mount table there): the
  organiser writes the characters NTFS refuses as `-` and keeps a reserved device name apart
  (`library.md`). Trailing dots and spaces are trimmed from every component on every platform, and a
  case-insensitive collision is found by the probe `Claims` makes with `file_facts`.
- **Deleting goes to the recycle bin** (`trashing::RecycleBin` over `trash::delete`); the hand-written
  freedesktop trash and the portal are Unix. A catalog with no trasher keeps the file
  (`Removal::Kept`), never removes it for good.
- **Settings are `%APPDATA%\resonate\config.toml`; data and cache are `%LOCALAPPDATA%\resonate`**
  (`config::known_folder`), unless an `XDG_*` variable is set; `~` is `%USERPROFILE%`. A config is
  not made owner-only (the profile folder is private by inheritance).
- **The spool is the system temporary folder** and its anonymous file is
  `FILE_FLAG_DELETE_ON_CLOSE`; free space on the temporary folder's volume is read through `fs4` and the
  ceiling leaves `LEFT_FREE_ON_DISC` of it, as `fstatvfs` does on Unix.

## The terminal and interrupts

- **Interrupts are `ctrlc`'s one handler** (`interrupts.rs`, `signals` on Windows): a process-wide
  reaction replaced by whoever watches (`quit_when_told`, `cancel_when_told`, `leave_when_told`) and
  cleared when its `Interrupting` drops; the first interrupt asks to quit or cancels the pass, the
  second leaves with 130.
- **A key at a time is `crossterm`'s raw mode**, not termios: key events become the bytes the Unix
  readers parse (arrows as `ESC [ A`), so `Keyed` and `Struck` are one state machine on both. Raw
  mode swallows Ctrl-C as a key, which `interrupted()` turns back into the interrupt. A piped stdin
  is read as bytes. `Readout` draws only where `crossterm::ansi_support::supports_ansi()` holds.

## The settings text

The window names its sound system through `system::sound_system!` (`PipeWire`, `Windows`), which
`concat!` takes into the output pane's hints and notes, so a Windows build never says PipeWire. The
*Graph rate* and *Bluetooth* groups are PipeWire's own (a graph to switch, a node to keep awake) and
`Group::is_offered` leaves them out of the pane and the search on Windows. `sinks --help` and the
man page say "the sound system".

## Discord

- **The IPC is the named pipe `\\.\pipe\discord-ipc-0` to `-9`** (`pipe.rs`, `interprocess`'s
  overlapped `DuplexPipeStream`), the same `Session` as the Unix socket's: `session.rs` holds the
  handshake, frames and replies over a `Wire` (`receive` within a time, `poll` without waiting) and
  `socket.rs` and `pipe.rs` are the two wires. A reader thread (`resonate-discord-pipe`) feeds
  chunks to a channel, since a pipe handle has no read timeout and `PIPE_NOWAIT` reports an empty
  pipe as `ERROR_NO_DATA`, which `std` reads as a broken pipe; the thread ends when the pipe closes
  or, after the session is dropped, when Discord next sends. The overlapped handle lets the reader
  and a write run at once.
- **Any user may create a pipe of that name**, so a squatter on a shared machine could learn what
  plays; the Unix socket's owner check has no counterpart yet.

## Awake while playing

`awake.rs` (`resonate-awake` thread, the binary's, since `resonate-mpris` finds no bus) looks at
every zone's player each second (`play`: its one player; the window: `Zones::listed`, through a weak
handle) and holds a `keepawake::KeepAwake` (display and idle, as the portal's `IDLE`/`SUSPEND` do on
Linux) while any is `Playing` or `Buffering`. `SetThreadExecutionState` belongs to the thread that
set it, so the hold is made and dropped on that thread alone; dropping `Awake` stops and joins it.

## Not on Windows yet

The MPRIS service, notifications, network metering and `Running` (so `queue`, `sleep`
and `players` against a running window) answer "no bus": `resonate-mpris` compiles but finds no
session bus, and the binary carries on without it as it does on a Linux machine with none. Capture
(`resonate-listen`) is a `Listener` that lists no microphone and refuses to record
(`Error::Unavailable`). The
launcher's icon-follows-the-accent is a no-op. All of it is on the roadmap.

## Release

`packaging/windows/build.ps1` builds `-C target-cpu=x86-64-v3` and zips `resonate.exe`, the licence
and a README as `resonate-<version>-x86_64_v3-windows.zip` (`release.yml`'s `windows-zip`, in the
`SHA256SUMS`); `crates/resonate/tests/packaging.rs` holds it to v3 and to the manifest's version. It
is not signed and has no installer.
