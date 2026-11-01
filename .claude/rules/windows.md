---
paths:
  - "crates/resonate-wasapi/**/*.rs"
  - "crates/resonate-core/src/os_bytes.rs"
  - "crates/resonate-core/src/file_facts.rs"
  - "crates/resonate-core/src/positioned.rs"
  - "crates/resonate-core/src/windows_uri.rs"
  - "crates/resonate-core/src/volume_mounts.rs"
  - "crates/resonate-library/src/reach.rs"
  - "crates/resonate/src/interrupts.rs"
  - "crates/resonate/src/windows_launcher.rs"
  - "crates/resonate/src/awake.rs"
  - "crates/resonate-listen/src/endpoints.rs"
  - "crates/resonate-listen/src/wasapi_*.rs"
  - "crates/resonate-discord/src/pipe.rs"
  - "crates/resonate-mpris/src/pipe.rs"
  - "crates/resonate-mpris/src/registry.rs"
  - "crates/resonate-mpris/src/apartment.rs"
  - "crates/resonate-mpris/src/cost.rs"
  - "crates/resonate-mpris/src/costing.rs"
  - "crates/resonate-mpris/src/desk_toast.rs"
  - "crates/resonate-mpris/src/toast.rs"
  - "crates/resonate-ui/src/system.rs"
  - "packaging/windows/**"
---

# Windows

The whole workspace but the phones' boundary and PipeWire builds for `x86_64-pc-windows-msvc`
(`cargo xwin check --target x86_64-pc-windows-msvc --workspace --exclude resonate-mobile --exclude
resonate-bindgen --exclude resonate-pipewire` on Linux, the `windows` CI job on a hosted runner)
and for `aarch64-pc-windows-msvc` (the same command with that target; the `windows-arm64` CI job
checks it on the hosted x64 runner). No crate has an x86-only path: the DSP's vector code is
portable Rust the compiler vectorises, so ARM64 needs no fallback. `ring`, which `rustls` brings
in, assembles its ARM64 Windows code with `clang` whatever `CC` says, so a build for that target
needs clang on `PATH` (the hosted Windows image has LLVM); under `cargo xwin` on Linux the
`/imsvc` flags xwin passes for `clang-cl` are not understood by that `clang`, so a local check
puts a wrapper that rewrites `/imsvc <dir>` to `-isystem <dir>` ahead of it on `PATH`.
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
  `same-file` on Windows), `volume_of` (device on Unix; on Windows a hash of the volume's name, below),
  `changed_at` (status change on Unix,
  modification time on Windows) and `sync_directory` (a no-op on Windows, where a directory cannot
  be opened to be synced).
- **A volume is named by its GUID where a folder mounts it.** `volume_of` canonicalises the path
  (so a symlink or junction into another drive reads as the drive it reaches), then walks the
  canonical path's ancestors, nearest first, for a reparse point (`FILE_ATTRIBUTE_REPARSE_POINT`
  off `symlink_metadata`, all `std`) whose `read_link` target carries `Volume{GUID}`: that is a
  volume mounted into a folder, and its GUID names the volume for the path and everything below
  it. A junction or symlink to an ordinary folder names no volume and is passed over. With no such
  ancestor the volume is the canonical path's prefix (`\\?\C:`, upper-cased; a `\\?\Volume{GUID}`
  prefix reads as its GUID, so one volume has one name however the path spells it). The pure part
  (`volume_mounts`: `VolumeGuid::within`, `nearest_mounted_volume` over a lookup of an ancestor's
  reparse target, `identity_of`) is compiled and tested on every host. `volumes::is_mounted_among`
  needs nothing else: a mounted folder's volume differs from its parent's, a folder below it
  has its parent's (`library.md`). Nothing here reads the volume's serial number, so two volumes
  are told apart by GUID or drive only.
- **What the tag writer carries to its copy is the mode on Windows**: owner, hard-link count and
  extended attributes are `cfg(unix)` (`carries_what_the_file_did`), so NTFS alternate streams and
  ACLs are not carried. No safe crate does it: `std::fs::copy` (`CopyFileExW`) carries streams
  but not the security descriptor and cannot replace the writer's own staged copy, and the
  security-descriptor and stream crates on crates.io are young, unreviewed wrappers; the
  attributes stay in `docs/FEATURES.md`.
- **A held file's lock is mandatory on Windows** (`LockFileEx`): `writer_of` still asks the lock
  first and a dead writer's is released by the system; with no `/proc` the pid stamp reads as
  living where the lock cannot be asked. **So nothing reads or writes a file its own run holds by
  reopening its path**: a read or write of a held file goes through the `Held`'s handle or a
  `try_clone` of it (the same file object, so the lock's owner), which is correct on Windows by
  construction and costs nothing on Unix. The tag writer's `Staged::handle` is that clone
  (rewound to the head) and every stage of a staged copy uses it: the tags read off a rechunked
  copy, the tags written into it, and `WholeCopy`, which appends its record of the track, binds a
  copy kept outside the track's folder and lays itself over the track through the handle it was
  made with, never `File::open` of its name (`a_staged_copy_is_read_and_written_through_its_held_handle_whatever_its_name_now_names`,
  `a_whole_copy_is_written_back_through_the_handle_it_was_made_from_not_by_reopening_its_path`,
  both of which take the name away or put a decoy under it, which Unix lets stand while a reopen
  would find the wrong file). A rename, a sync by name (`FlushFileBuffers` moves no byte range) and
  a removal work under the lock. The only reads by path are of what no living run holds: mending
  a dead writer's journal or whole copy, where `writer_of` has said `Gone` first (the system
  released its lock with its process), and a `.torn` copy, which its own run no longer holds once
  its `Staged` is dropped.
- **A file or directory is synced through `file_facts`**: `sync_file` opens a Windows file for
  writing (`FlushFileBuffers` refuses a read-only handle) and `sync_directory` is a no-op there (a
  directory cannot be opened as a file), so no writer calls `File::open(folder)` or syncs a
  read-only `File::open`.
- **Every root is `Naming::Portable` on Windows** (`Naming::of` reads no mount table there): the
  organiser writes the characters NTFS refuses as `-` and keeps a reserved device name apart
  (`library.md`).
- **The organiser keeps a destination's whole path within `MAX_PATH`.** `Reach::of(root)` is
  `Reach::legacy` on Windows (211 UTF-16 units: `MAX_PATH` 260 less the terminating NUL, less
  `ROOM_FOR_DERIVED_NAMES`, 48 units for the staging, parked, delivery and undo names built
  beside a file) and `Reach::UNLIMITED` elsewhere; it weighs the root (verbatim prefix and trailing
  separators dropped, UTF-16 units) and every component, and `Reach::fit` takes a letter at a time
  off the longest component (the extension, protected, never cut) until the whole fits, a
  destination that cannot fit with a letter a component is `Refusal::OutOfReach`. Why cut rather
  than write a `\\?\` path: `std` already prefixes a path past the limit itself for the calls our
  code makes (no manifest or registry switch is involved), so writing would work, but Explorer,
  the shell's recycle bin, taggers and most other programs open the library by its plain path and
  fail past 260, and the catalog stores prefix-free paths (`windows_uri` drops `\\?\`) that
  must stay openable by them. The weighing is in `reach.rs` and tested on every host. Trailing dots and spaces are trimmed from every component on every platform, and a
  case-insensitive collision is found by the probe `Claims` makes with `file_facts`.
- **Deleting goes to the recycle bin** (`trashing::RecycleBin` over `trash::delete`); the hand-written
  freedesktop trash and the portal are Unix. A catalog with no trasher keeps the file
  (`Removal::Kept`), never removes it for good. **`trash` enters COM itself, on the thread that
  calls it** (a thread-local `CoInitializeEx(COINIT_APARTMENTTHREADED)`, the crate's
  `coinit_apartmentthreaded` feature, which the workspace keeps), and it *panics* if that thread
  is already in the multithreaded apartment (`RPC_E_CHANGED_MODE`). The shell's `IFileOperation`
  also wants a single-threaded apartment. A deletion therefore never runs on a caller's thread:
  `on_a_thread_of_its_own` runs it on a fresh `resonate-recycle` thread inside a `thread::scope`,
  which no other code has entered an apartment on (the WASAPI stream and capture threads enter
  the multithreaded one on threads of their own), and a panic there is `Unthreaded::Panicked`,
  which `RecycleBin` reads as a refusal (`Removal::Kept`) rather than letting it unwind into the
  caller. The crate leaves the apartment when the thread ends. No `unsafe` and no extra COM
  call of ours is needed.
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

## The window's pipe

`Running` has a second wire here, the way Discord's `Session` has (`mpris.md` has the seam: the
`Peer` trait, `BusPeer` and `PipePeer`, `wire.rs`, `answering.rs`). With no session bus,
`Mpris::start` serves the zone on a named pipe instead, so `queue`, `sleep`, `players`, a second
launch handing its files to the window, `eq` reloads and `mcp` reach a running window on Windows
through the same `Running` calls as on Linux, and `Session::open` is `Ok` without a bus.

- **A player is named as on the bus** (`bus_name_of`, `instance_name`: `org.mpris.MediaPlayer2.resonate`,
  `.zone<N>`, `.instance<pid>`), so `numbered`, `answers_to`, `PlayerName` and `--player zone2` are
  unchanged. The name is claimed in `%TEMP%\resonate-players\<name>` by `Held::taken_over`
  (`registry.rs`): the file is the **lock file for discovery**, held for the player's life, so a
  client lists the folder and `writer_of` says which entries live (a held lock; on Windows the lock
  is mandatory, so the file's content is never read, its name says everything). A dead player's
  entry is swept by the next listing once it has aged `UNHELD_AND_UNTOUCHED_FOR`; a client that
  cannot connect to a living-looking entry skips it. A taken name falls through to the instance
  names, up to `INSTANCES`, as on the bus.
- **The pipe is `\\.\pipe\resonate-<USERNAME>-<player name>`** (`registry::pipe_name`, the user
  name cut to letters, digits, `-` and `_`), created by `interprocess` with
  `FILE_FLAG_FIRST_PIPE_INSTANCE` (what its first listener instance sets), remote clients rejected
  and the security descriptor `D:P(A;;GA;;;SY)(A;;GA;;;OW)`: only the system and the pipe's
  owner may open it, so another user's process can neither connect nor create a further instance.
  **Squatting**: a process that creates the name first makes this player's own creation fail, which is
  logged and answered with the next name, and the registry entry is written only after the pipe
  is held, so a client, which reaches only the pipes the registry names, never connects to a
  squatter's. The registry folder lives in the user's temporary folder, private by the profile's
  inheritance. Not heard on a real Windows machine (`docs/FEATURES.md`).
- **One connection is one thread each side.** The listener's accept loop is one thread
  (`resonate-mpris-pipe`), woken at shutdown by a connection to its own pipe; each client is served
  on a thread of its own up to `CONNECTIONS_AT_MOST` (32), more turned away. A pipe handle has no
  read timeout, so a client's `Link` runs the blocking I/O on a thread and waits `ANSWERS_WITHIN`
  (2 s) for each reply; a call that times out severs the link (a later reply could be read for
  another call), and dropping the client ends its thread.

## Awake while playing

`awake.rs` (`resonate-awake` thread, the binary's, since `resonate-mpris` finds no bus) looks at
every zone's player each second (`play`: its one player; the window: `Zones::listed`, through a weak
handle) and holds a `keepawake::KeepAwake` (display and idle, as the portal's `IDLE`/`SUSPEND` do on
Linux) while any is `Playing` or `Buffering`. `SetThreadExecutionState` belongs to the thread that
set it, so the hold is made and dropped on that thread alone; dropping `Awake` stops and joins it.

## Capture

`resonate-listen`'s Windows `Listener` is the Linux one's twin over the `wasapi` crate, reaching core,
`resonate-audio` and `wasapi` alone (never `resonate-wasapi` or `resonate-pipewire`; the layering job
refuses both from its Windows tree). `resonate-wasapi` is its dev-dependency, so the node names the
two agree on are held by `endpoints.rs`' tests. What the two platforms share is portable and tested
on the host: `session.rs` (the spec heard, the sitting's `until_full` loop over a `Held` capture,
`Played`, `finished`, `frames_in`), `recording.rs` (`Recording` and `Recorder`, whose `take` of the
PipeWire sink is `admit`) and `endpoints.rs` (naming, the listing, the choice of tap and output).

- **A microphone is `wasapi-capture:<endpoint id>`**, the endpoint id being the system's stable one
  (`Listener::microphones` surveys `Direction::Capture` on a short thread of its own with COM entered
  and left through `Apartment`, the system's order kept, the default endpoint marked, the friendly name
  the description, the id where the name cannot be read). `Listening::Microphone(None)` is the default
  capture endpoint; a name that is not a `wasapi-capture:` one (a PipeWire name left in the settings)
  is `Error::NoSuchMicrophone`, not a guess.
- **The desktop is loopback capture of the default render endpoint** (`Listening::Desktop`): the
  render device's client initialised with `Direction::Capture`, which the `wasapi` crate turns into
  `AUDCLNT_STREAMFLAGS_LOOPBACK`. Loopback is shared mode only; the exclusive endpoints of the output
  pane cannot be listened to, and a render endpoint taken exclusively by the engine yields nothing.
- **Both are one shared-mode, event-driven capture asked for what the clip is: 48 kHz stereo `F32`**
  with `autoconvert` on, so the system's engine converts rate, word and layout and the bytes read are
  the ones `Recording::keep` takes. A buffer flagged `AUDCLNT_BUFFERFLAGS_SILENT` is zeroed
  (`zeroed_if_silent`), as its contents are undefined.
- **A capture is a thread of its own** (`resonate-listen-capture`, as the render stream is):
  every COM object is made on it, the open is answered through a one-slot channel within five
  seconds, the loop waits on the event for 100 ms and drains every packet each turn, and `Capture::stop`
  sets the flag and joins (dropping does the same). A failure of any call ends the thread with
  `lost` set, which is `Held::is_lost`, so `Sitting::until_full` opens the same capture again behind a
  held-back `Recorder` and the gap is silence of the time lost (`audio.md`'s recording survives the
  restart); a device unplugged, or the default render endpoint changed under a loopback, is that.
- **A quiet loopback is silence of the time that passed.** A render endpoint with nothing playing
  delivers no packets, so a turn of the loop that drained none pads the clip with the frames of the
  time since the last turn (`Recorder::pad`, loopback only; a microphone never starves, its quiet
  arrives as silent buffers). The clip therefore keeps wall-clock length, and a silent desktop is
  `NothingHeard` as a silent one is on Linux.
- **The measuring sweep plays through a shared-mode render stream of its own**
  (`resonate-listen-playback`, same format and autoconvert), on the default render endpoint or the
  endpoint named by `--sink`, the settings or the output pane: a `wasapi:` or `wasapi-exclusive:`
  node names the endpoint (`render_endpoint_of`; either way is played shared, so an endpoint held
  exclusively by the engine fails as `Capture { op: Play }`), any other name is
  `NoSuchSink`, a missing default `NoOutput`. The buffer is filled before the client starts and on
  every event; `Playback`'s drop stops and joins. The sweep never passes the engine, as on Linux.
- **The desktop names no endpoint.** `Listening::Desktop` carries no sink on either platform, so
  loopback is of the default render endpoint alone.

## Toasts and the metered connection

`resonate-mpris` answers both with the Windows Runtime, through the `windows` crate's safe
projections (`Windows.UI.Notifications`, `Windows.Data.Xml.Dom`, `Windows.Networking.Connectivity`;
the features `Data_Xml_Dom`, `Foundation`, `Networking_Connectivity` and `UI_Notifications`, and
nothing else). `RoInitialize` and `CoInitializeEx` are `unsafe fn`s there, so a thread enters the
multithreaded apartment through the `wasapi` crate's safe `initialize_mta` (`apartment.rs`'s
`Apartment`, left on drop), as the render and capture threads do; a Windows Runtime call on a thread
in no apartment fails `CO_E_NOTINITIALIZED`. Errors are `Error::Windows { op: WindowsOp, source }`.

- **A toast is the song-change notification of Linux, word for word** (`mpris.md`: `Shown`,
  `Telling::of(attended, notifies)`, so the `notify` setting and the held-back-while-the-window-is-
  in-front rule are the same code). `desktop.rs`'s `Errands` run on a `Desk`: `desk_bus.rs` is
  Linux's (the notification server and the idle portal), `desk_toast.rs` Windows's. A Windows `Desk`
  enters the apartment on the `resonate-desktop` thread, makes a `ToastNotifier` for the
  AppUserModelID `org.resonate.Resonate` (`ToastNotificationManager::CreateToastNotifierWithId`),
  and shows each errand: `toast.rs` writes the XML (portable, tested on every host): `ToastGeneric`,
  the title, then the artist and the album each as a line, the cover as an `appLogoOverride` image
  at its `file:///C:/...` URI (`art::Pictures` lays it in the temporary folder), `<audio
  silent="true"/>` and, for a track change, three buttons Previous, Play/Pause and Next
  (`notify::ACTIONS`, the keys Linux uses) with `activationType="foreground"`. Text is escaped for
  XML and characters XML 1.0 refuses are left out. A toast is tagged (`track`, or `listen` for what
  Listen named, which has no buttons) in the group `resonate`, so the second telling when the cover
  lands replaces the first in the Action Center; teardown hides the track's toast. `ToastNotification::Activated` carries
  the button's key as its argument and `notify::pressed` turns it into the `Command`, settled on
  the zone, so a press reaches the transport as it does on Linux; a press of a toast left in the
  Action Center after the player has gone cannot be answered.
- **No process AppUserModelID is set.** `CreateToastNotifierWithId` names the identity, which
  needs no call of ours (`SetCurrentProcessExplicitAppUserModelID` is `unsafe`). **A toast shows in
  practice only where a Start-menu shortcut carries that identity** (the `System.AppUserModel.ID`
  property of a `.lnk` in the Start menu's `Programs`); the zip has none and the installer is a
  separate roadmap entry, so until it exists a toast is created and silently not shown. The
  installer's shortcut must name `org.resonate.Resonate`.
- **Which service raises it.** With a session bus (a dbus session on Windows is rare but possible)
  the poll that serves the bus raises toasts; with none, the pipe's service runs a thread of its
  own for them (`resonate-toasts`, `toast_what_plays`), the same `snapshot` and `tell_the_desktop`
  the bus loop uses, so the two never differ in what they say.
- **The metered connection is `NetworkInformation`** (`costing.rs`): `watch_the_meter(Metering)`,
  the same function the binary calls on Linux, spawns `resonate-metered`, which enters the
  apartment, registers `NetworkStatusChanged` and reads
  `GetInternetConnectionProfile().GetConnectionCost().NetworkCostType()` before it returns (a
  lookup made straight after is gated), and parks for the rest of the process. The handler reads
  again on every change. `cost::meter_of_network_cost` (portable, tested everywhere) maps
  `Unrestricted` to `Unmetered`, `Fixed` and `Variable` to `Metered`, `Unknown` and any other
  number to `Unknown`; no profile (null) or a failed read reports `Unknown`, so an offline machine
  never blocks anything. `Roaming`, `OverDataLimit` and `ApproachingDataLimit` of the cost are
  not read.

## Not on Windows yet

The MPRIS service and the media transport answer "no bus": `resonate-mpris` finds no session bus
and the binary carries on without it as it does on a Linux machine with none (a running window is
still reached on its pipe, the machine kept awake by `awake.rs`, toasts and the metered connection
above). The launcher's icon-follows-the-accent is a no-op. All of it is on the roadmap.

## Release

`packaging/windows/build.ps1 -Architecture x86_64|aarch64` builds one archive per architecture and
zips `resonate.exe`, the licence and a README:

- **`x86_64`** (the default) builds `-C target-cpu=x86-64-v3` for the host and names it
  `resonate-<version>-x86_64_v3-windows.zip`.
- **`aarch64`** installs the `aarch64-pc-windows-msvc` target, builds `--target` it at `-C
  target-cpu=generic` (the target's own baseline, ARMv8-A with NEON, written out because a PowerShell
  variable set to the empty string is removed rather than emptied and so could not replace the
  checkout's `target-cpu=native`; every Windows 11 ARM64 machine is above it, and a higher floor
  would buy little over NEON and shut out machines for it) and names it
  `resonate-<version>-aarch64-windows.zip`.

`release.yml`'s `windows-zip` is a matrix of the two on `windows-latest`, each its own artifact, both
in the `SHA256SUMS`. The ARM64 archive is **cross-built on the x64 runner**, not on a `windows-11-arm`
one: the hosted ARM64 runner's availability follows the repository's plan, its image carries fewer
tools, and a release is immutable, so the more predictable runner builds it; the x64 image already carries the
MSVC ARM64 tools and LLVM that `ring` needs, and a step runs `clang --version` first so a missing
compiler fails by name. A cross-built archive is never run in CI, which is why the roadmap still
lists the ARM64 build among what only hardware settles. `crates/resonate/tests/packaging.rs` holds
the script to v3, to the ARM64 triple and baseline, to the manifest's version, and the workflow to
one archive and artifact name per architecture. Neither archive is signed and neither has an installer.
