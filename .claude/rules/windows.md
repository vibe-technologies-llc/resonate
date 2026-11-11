---
paths:
  - "crates/resonate-wasapi/**/*.rs"
  - "crates/resonate-win32/**"
  - "crates/resonate/tests/console.rs"
  - "crates/resonate-core/src/os_bytes.rs"
  - "crates/resonate-core/src/file_facts.rs"
  - "crates/resonate-core/src/positioned.rs"
  - "crates/resonate-core/src/windows_uri.rs"
  - "crates/resonate-core/src/volume_mounts.rs"
  - "crates/resonate-library/src/reach.rs"
  - "crates/resonate-library/tests/support/**"
  - "crates/resonate/src/interrupts.rs"
  - "crates/resonate/src/plain_launcher.rs"
  - "crates/resonate/src/awake.rs"
  - "crates/resonate/build.rs"
  - "crates/resonate/resources.rs"
  - "crates/resonate/icon_resources.rs"
  - "crates/resonate/tests/icon_resources.rs"
  - "crates/resonate/tests/installer.rs"
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
  - "crates/resonate-mpris/src/smtc.rs"
  - "crates/resonate-mpris/src/transport.rs"
  - "crates/resonate-ui/src/system.rs"
  - "packaging/windows/**"
---

# Windows

The whole workspace but the phones' boundary and PipeWire builds for `x86_64-pc-windows-msvc`
(`cargo xwin check --target x86_64-pc-windows-msvc --workspace --exclude resonate-mobile --exclude
resonate-bindgen --exclude resonate-pipewire --all-targets` on Linux, the `windows` CI job on a hosted runner)
and for `aarch64-pc-windows-msvc` (the same command with that target; the `windows-arm64` CI job
checks it on the hosted x64 runner). No crate has an x86-only path: the DSP's vector code is
portable Rust the compiler vectorises, so ARM64 needs no fallback. `ring`, which `rustls` brings
in, assembles its ARM64 Windows code with `clang` whatever `CC` says, so a build for that target
needs clang on `PATH` (the hosted Windows image has LLVM); under `cargo xwin` on Linux the
`/imsvc` flags xwin passes for `clang-cl` are not understood by that `clang`, so a local check
puts a wrapper that rewrites `/imsvc <dir>` to `-isystem <dir>` ahead of it on `PATH`.
Windows 11 under QEMU/KVM has run the 0.5.0 x64 release: the installer and uninstaller, registry
associations, scanning, paths and file URIs containing non-ASCII letters and escaped punctuation,
the window's named-pipe handover and raising, shared playback and exclusive 16-bit playback at
44.1, 48 and 96 kHz, media play/pause and next keys, settings and queue resumption. The inspector
reported bit-perfect exclusive output and no underruns; this is a virtual endpoint, not a DAC
measurement. A cross-built executable verified the corrected caption's minimise, maximise,
restore, Snap Layouts, drag and close. The headless binary's 212 tests and all five ConPTY tests
also ran there; the console tests exercised the virtual endpoint rather than skipping.
`docs/FEATURES.md`'s *Windows parity* lists what only hardware can settle.
`unsafe_code = "forbid"` holds everywhere but `resonate-win32`: a Win32 or COM call goes
through a safe wrapper crate (`dependencies.md`), or through that crate's safe API where none
exists (*The Win32 exception*, below).

## The Win32 exception

`resonate-win32` is the one crate besides `resonate-mobile` whose lints allow `unsafe_code`
(approved by the listener, `dependencies.md`). It is empty on every other target (each module is
`cfg(windows)`, or `cfg(any(windows, test))` for the sequencing the next bullet describes), reaches
no workspace crate, and hands out a safe API over the `windows` crate:

- **Each module's raw calls are a trait, so its sequencing runs on every host against a fake**, as
  `resonate-apple`'s `Toolbox` does (`macos.md`): `Scheduling` (MMCSS), `Counting`, `Naming`,
  `Attachment` (the console), `Identifying`, `PowerModes`, `Endpoints` (COM), `Securing`,
  `Streaming` and, under `console`, `Terminal`. The unit struct `system::Windows` implements each
  in a `cfg(windows)` `mod real` at the foot of its module, and that is the only place the module's
  `unsafe` blocks and `windows` imports are; the module's generic type or function over the trait
  (`Joined<T>`, `Opened<T>`, `carrying_security`, `Pseudo<T>`) holds every decision (which `Op` a
  refusal is, what is freed or closed on which path, the order of the calls, how a buffer is sized,
  how a flag is chosen), and the public type is a newtype over it instantiated with `Windows`
  (`ProAudioClass(Joined<Windows>)`), so the safe API callers see is unchanged. A handle the
  system returns is `Copy` in the trait and closed by the sequencing's `Drop` through a trait
  method (`close_handle`, `close_pseudo_console`, `delete_attribute_list`, `unregister`); a COM
  interface or a security descriptor is an owned associated type whose own `Drop` releases it, which
  the fake's `Drop` logs, so a test sees the release order. The calls cross the trait as plain
  data: `Raw<T>` is `Result<T, HResult>`, a path as a `Wide`, an event as `ConsoleEvent`, a buffer as
  a slice the sequencing sized (`attribute_list_bytes`, then `initialize_attribute_list` over
  `bytes.div_ceil(size_of::<usize>())` words).
- **A fake is written in the module's test, never shared**, logging each call into an
  `Arc<Mutex<Vec<Call>>>` and refusing the call a test names. It runs on Linux, so a name's UTF-16
  (`wide::units_of`, `text_of`, the one pair that differs by target) and the pipes and files the
  console and stream code touch through `std` are the portable ones. The few constants the real impl
  turns into the crate's own (`HResult::HANDLE_EOF`, `GAME_MODE`, the console's line-input flags) are
  checked against the `windows` crate's in a Windows-only test inside `mod real`, since a fake
  cannot. `GameModeWatch` keeps one process-wide `ACTIVE` flag, so its tests hold a lock.

- **Every `unsafe` block is one call**, its preconditions made true by the wrapper's types rather
  than said beside it: a name is a `Wide` (UTF-16 ending in its NUL, refused as `NulInName` where
  the text holds one) that lives across the call; an out-parameter is a local taken `&raw mut`; a
  handle the system returns is owned by a type that closes it on drop (`StreamSearch` and
  `FindClose`, `Descriptor` and `LocalFree`, `Attributes` and `DeleteProcThreadAttributeList`,
  `PseudoConsole` and `ClosePseudoConsole`, `Process` over `OwnedHandle`); a thread-bound handle is
  `!Send` (`ProAudioClass` holds a `PhantomData<*const ()>`, so it is reverted on the thread that
  joined).
- **What it wraps**: `ProAudioClass` (`AvSetMmThreadCharacteristicsW("Pro Audio")`, reverted on
  drop or `leave`); `PerformanceCounter::of_this_machine` and `performance_counter`
  (`QueryPerformanceFrequency`, `QueryPerformanceCounter`); `EndpointVolume` (an
  `IAudioEndpointVolume` by endpoint id through `IMMDeviceEnumerator::GetDevice`: the scalar
  level as a `Level` clamped to 0..=1, the mute, and `watched`, an `IAudioEndpointVolumeCallback`
  made with `#[implement]` that only calls the closure it is given, unregistered when its
  `VolumeWatch` drops); `name_the_process` (`SetCurrentProcessExplicitAppUserModelID`);
  `leave_a_console_of_its_own` (`GetConsoleProcessList`, then `FreeConsole` only where this process
  is the console's one, answering a `Console`);
  `carry_alternate_streams` and `carry_security` (below); and, under the `console` feature the
  binary's tests take, `PseudoConsole` (ConPTY over two `std::io::pipe`s, a process started into
  it by `CreateProcessW` with the `PSEUDOCONSOLE` attribute, its standard handles invalid so it
  takes the console's, a quoted command line and a sorted environment block), `Process`,
  `tell_the_console` (`GenerateConsoleCtrlEvent` to every process on the caller's console) and
  `console_input_mode` (`GetConsoleMode` of `CONIN$`).
- **COM is the caller's**: `EndpointVolume::of` makes its enumerator with `CoCreateInstance`, so
  the thread must already be in an apartment (the WASAPI threads enter the multithreaded one
  through `wasapi::initialize_mta`); one in none is refused `CO_E_NOTINITIALIZED` as an `Error`.
- Errors are `Error::{Windows, WindowsOnFile} { op: Op, source: HResult }` (the code the call
  returned, printed as eight hex digits with the system's message beside it on Windows), `Io` for a
  stream copied through `std`, `NulInName`, `CounterStandsStill`; the command-line quoting, the
  environment block and every `Terminal` sequence are tests on every host, the `console` module being
  compiled for `test` as well as under the feature.

## The seam is what Linux already had

- **`engine::desktop_backend(app_name)` is the one place a platform's backend is named**: PipeWire
  on Linux, `resonate_wasapi::Wasapi` on Windows. `Player::with_sources` builds through it, and the
  binary's `surveyed::sinks` (`sinks`, `explain`, `eq --suggest`) and `rendering::Rendering`
  (`Beside` over the backend) use it, so the binary names no PipeWire and its Windows tree holds
  none of PipeWire, Wayland, `rustix` or `signal-hook` (the layering job).
- **`resonate-wasapi` is `resonate-audio`'s `Backend` over the `wasapi` crate** and reaches core,
  audio and (on Windows) `resonate-win32` alone. Its portable halves compile and are tested everywhere (`ways`, `formats`,
  `fill`, `clock`); `device`, `stream`, `render`, `priority` and
  `backend` are `cfg(windows)`. **What every desktop backend past PipeWire does the same way is
  `resonate-audio`'s** and reexported here: `sink_id_of`, `changes_between`, `watch` and `Pace`,
  `refusal_of`, `scalar_of` and `gain_of`, `left_aligned` (`resonate-coreaudio` is the other user).
- **An endpoint is two sinks.** `wasapi:<endpoint id>` plays through the Windows mixer in shared mode
  (event-driven, `autoconvert` on, offered at the mix format alone) and is the default sink when the
  endpoint is; `wasapi-exclusive:<endpoint id>` ("<name> (WASAPI exclusive)") takes the device in
  exclusive event-driven mode at the format the engine plans, offered for every rate, word and layout
  `IsFormatSupported` accepts out of a fixed grid (`PROBED_RATES` x `SampleFormat::ALL` x
  `PROBED_LAYOUTS`, each combination its own `SinkFormats` entry so a rate is never claimed for a
  layout it was not probed with). **A held endpoint keeps the formats it took while free**: while a
  client holds it exclusively (this player's own stream included) every exclusive probe answers
  `AUDCLNT_E_DEVICE_IN_USE`, so `taken_while_free` answers the grid last probed for that endpoint id
  instead (nothing where it was never free this run); listing it empty made the engine read its own
  open stream as the device gone, rebind, and fail the row when the next survey dropped it again.
  Bit-perfect on Windows is therefore a choice of device the
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
- **The stream thread is in MMCSS's `Pro Audio` class** (`priority::Priority`, over
  `resonate_win32::ProAudioClass`) while the request is `realtime`, left on the same thread when a
  reshape lowers it or the thread ends; where MMCSS refuses (the service stopped),
  `thread-priority`'s `Max` stands in.
- **Devices are told by the system and checked by a poll.** The watcher thread registers the
  `wasapi` crate's endpoint notification client (added, removed, state, default and property
  changes), whose callbacks only nudge a one-slot channel; `resonate_audio::watch` waits on the nudge or
  `Pace::ENDPOINTS`' two seconds, lets a burst settle for 100 ms, surveys once and tells the
  difference through `changes_between` (added, removed, default moved, formats or mix rate moved,
  and `Turned` where the endpoint's volume or mute moved), the one difference computation for both.
  A refused registration leaves the poll alone. **The endpoint volume nudges the same channel**:
  after every survey the watcher holds one `VolumeWatch` an endpoint listed (`VolumesWatched`,
  dropped as an endpoint leaves, retried each survey where refused), so a volume or mute turned from
  the Windows mixer is surveyed within the settle, not the two-second poll.
- **The endpoint's volume and mute are the device's** (`set_device_volume`, `set_device_mute`, over
  `resonate_win32::EndpointVolume` by endpoint id; both sinks of an endpoint turn the one level). Every survey reads it into `SinkPort` (the endpoint's description,
  the level, the mute, and `HardwareVolume::Yes` only where `IAudioMeterInformation`'s hardware
  support says the device turns its own volume, else `No`), so `device-volume` hands the slider to
  the device only where that keeps the stream bit-perfect: in exclusive mode a software endpoint
  volume is not applied at all, and in shared mode it is the mixer's. The level is the endpoint's
  scalar, which is the Windows slider's position, and the slider is ours (`resonate_audio::scalar_of` is
  `Volume::heard_at`, `gain_of` its inverse), so both sliders stand at the same place and a level
  sent reads back within float rounding at any height
  (`a_level_sent_reads_back_far_within_half_a_devices_step_however_low`): the engine's echo test
  (`A_DEVICES_HALF_STEP_DB`, `audio.md`) is left only a device's own rounding to absorb.
- **A stamp is `IAudioClock`'s position against the performance counter** (`render::Playing`): each
  cycle reads `GetPosition` (the device position, in the clock's `GetFrequency` units, and the
  counter when it was read, in 100 ns) and the counter now (`resonate_win32::performance_counter`,
  its frequency read once as the stream opens; a counter that cannot be read stamps by the padding), and
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
  the URI back to a `\` path **written verbatim** (`windows_uri::verbatim`: `\\?\C:\…`,
  `\\?\UNC\host\share\…`; a `.` or `..` segment is refused, since Windows never normalises a
  verbatim path), the spelling `fs::canonicalize` gives and so the one the scan stores,
  so a song kept as a URI (the resumed queue, a delivery's offer, a playlist, `OpenUri`) finds its
  catalog row again; the binary's `from_here` writes a path that does not exist yet the same way.
  A playlist sheet's `file://` row is read through the same `from_uri` (`sheet::local_file`), so an M3U
  or XSPF naming `file:///C:/…` finds its file.
  `path_text::verbatim` and `path_text::plain` are the two directions (identity off Windows),
  `path_text::shown` is what a window or notice draws, and a call into the Windows Runtime
  (`StorageFile` for the media controls' cover) is handed the plain spelling. On Unix a URI still has to start at the root.
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
- **What the tag writer carries to its copy on Windows is the alternate streams, the security and
  the read-only attribute** (`carries_what_the_file_did`'s Windows arm): every named `:name:$DATA`
  stream `FindFirstStreamW`/`FindNextStreamW` list (the file's own `::$DATA` passed over) is copied
  into the same name on the staged copy through `std` (a stream is its own file object); the DACL
  is read with
  `GetNamedSecurityInfoW` and laid on the copy with `SetNamedSecurityInfoW`, protected where the
  file's was (`SE_DACL_PROTECTED`) and otherwise inheriting from the folder as the file did, the
  owner set only where it differs from the copy's (setting another's needs a privilege); the
  read-only attribute last, since it would refuse the stream writes. Any of the three refused
  answers `false`, and the writer then lays a whole copy over the track through its own handle, which
  keeps every stream and the descriptor because the file stays the file. The hard-link count and
  extended attributes stay `cfg(unix)`. `std::fs::copy` (`CopyFileExW`) carries streams but not the
  security descriptor and cannot replace the writer's own staged copy.
- **A held file is held by its open handle on Windows, not by a lock** (`writer::holding`): a
  `LockFileEx` lock is mandatory, so it refused every reopening of the file by path that a
  writer's own run makes (the vault's staging, the library's import and take-in, a copy read back),
  each failing with `ERROR_LOCK_VIOLATION` (33). So no lock is taken there; `writer_of` opens the
  file sharing nothing but deletion, and a sharing violation (32) means a handle is open, a living
  writer; a crashed writer's handles are closed by the system, so its file opens and is gone once
  untouched for `UNHELD_AND_UNTOUCHED_FOR`. With no `/proc` the pid stamp reads as living, so that
  age alone decides a dead writer's file there. `Held::taken_over` and `resumed` look the same way
  before opening (`WouldBlock` where a handle is open); a look racing a writer's own open by path
  can make that open fail with a sharing violation for the moment the look holds the file, which is
  the cost of the design and never a lost write (the writer's own handle is untouched). The tag
  writer still reads and writes a staged copy through `Staged::handle`, a `try_clone` of the held
  one (`a_staged_copy_is_read_and_written_through_its_held_handle_whatever_its_name_now_names`,
  `a_whole_copy_is_written_back_through_the_handle_it_was_made_from_not_by_reopening_its_path`),
  since a name may have been taken away or a decoy put under it.
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
  fail past 260, and the paths written there must stay openable by them (the catalog keeps the
  canonical, verbatim spelling; a URI drops the prefix). The weighing is in `reach.rs` and tested on every host. Trailing dots and spaces are trimmed from every component on every platform, and a
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
  not made owner-only (the profile folder is private by inheritance). **A writer locks
  `config.toml.writing`, not the folder**: `std` cannot open a directory on Windows (that wants
  `FILE_FLAG_BACKUP_SEMANTICS`) and a directory takes no byte-range lock, so the folder lock Linux
  uses failed every write with *cannot write*; the lock file stays beside the settings and no sweep
  takes it.
- **The spool is the system temporary folder** and its anonymous file is
  `FILE_FLAG_DELETE_ON_CLOSE`; free space on the temporary folder's volume is read through `fs4` and the
  ceiling leaves `LEFT_FREE_ON_DISC` of it, as `fstatvfs` does on Unix.

## The terminal and interrupts

- **Interrupts are `ctrlc`'s one handler** (`interrupts.rs`, `signals` on Windows): a process-wide
  reaction replaced by whoever watches (`quit_when_told`, `cancel_when_told`, `leave_when_told`) and
  cleared when its `Interrupting` drops; the first interrupt asks to quit or cancels the pass, the
  second leaves with 130.
  Once the handler is installed, `resonate_win32::hear_console_interrupts` clears the inherited
  ignore-Ctrl-C attribute (`SetConsoleCtrlHandler(None, false)`). Without it an SSH-launched
  process ignored `GenerateConsoleCtrlEvent(CTRL_C_EVENT, 0)` even though a typed Ctrl-C and
  Ctrl-Break worked. The ConPTY helper starts each process in a new process group, which disables
  Ctrl-C initially, so the interrupt test exercises this regardless of its launcher's attribute.
- **A key at a time is `crossterm`'s raw mode**, not termios: key events become the bytes the Unix
  readers parse (arrows as `ESC [ A`), so `Keyed` and `Struck` are one state machine on both. Raw
  mode swallows Ctrl-C as a key, which `interrupted()` turns back into the interrupt. A piped stdin
  is read as bytes. `Readout` draws only where `crossterm::ansi_support::supports_ansi()` holds,
  and the log's colours are written only there too (`readout::understands_escapes`, which turns on
  virtual terminal processing where the console offers it), so an old console shows no raw escapes.
- **The window leaves a console of its own.** The binary is a console program (`play`, `abx` and
  the readout need the terminal they were started from), so a launch from Explorer or a shortcut
  opens a console beside the window. A bare `resonate` (no subcommand) calls
  `resonate_win32::leave_a_console_of_its_own` before logging starts: where the console has no
  other process it is freed and its window closes; started from a shell, the shell shares it and
  the window's log stays there.
- **The window draws its own caption.** gpui's Windows backend reports `Decorations::Server`, so
  `chrome::the_window_controls_are_ours` and `chrome::the_header_moves_the_window` are true on Windows whatever the decorations say, and the window
  opens with `TitlebarOptions::appears_transparent` (gpui then hides the system caption and keeps
  the resize frame). `THE_SYSTEM_PRESSES_THE_CAPTION`: the header is `WindowControlArea::Drag`
  (Windows moves the window, zooms on a double press and opens the system menu on a right press,
  so `titlebar` adds no handlers of its own there), and each window control is `occlude`d (so the
  drag area beneath is not hit) and marked `Min`, `Max` or `Close`, pressed by the system through
  gpui's non-client handling rather than by `Control::act` (gpui's `zoom` only maximises, and the
  system's maximise is a toggle that offers Snap Layouts). Everything else clickable in the header
  stops the press, which gpui reads as the non-client press handled.
  The header is never cached (`ui.md`): gpui's cached-view replay omits window-control hitboxes,
  which made its caption and all three buttons inert after a reused frame on Windows 11 under KVM.

## The integration suites

The `windows` CI job runs every crate's tests, so the integration suites run there as they do on
Linux; what is Unix-only is gated per test with `#[cfg(unix)]`, never per suite, and clippy runs with
`--all-targets` so a helper dead on one platform fails the build where it is not gated.

- **Covered on Windows:** `resonate-library`'s `library`, `playlists`, `scanned`, `studies`,
  `search` and `synced` (the scan, the catalog, enrichment, tag writing, organise, deleting, the
  vault import, playlists, studies), `resonate-codec`'s `encoded` (every fixture ffmpeg, `flac` and
  the rest build, each skipped where its tool is absent), `resonate-vault`'s `vault`, and the binary's
  `signals` (the MCP server answering and leaving when its input closes, `mcp` feature).
- **A test tree is canonical.** `Tree::new` in `library` and `studies` canonicalises its root, as
  the scan does (`\\?\C:\...` on Windows), so the paths a test writes are the paths the catalog
  stores. The binary's file-argument tests name drive-qualified Windows URIs and expect verbatim
  paths, including paths relative to the working directory; a list fixture canonicalises its
  folder first. A test closes its catalog before removing its folder: Windows refuses to remove
  the open SQLite file.
- **A symlink needs a privilege Windows does not always give.** `tests/support::link_to_directory`
  and `link_to_file` create `symlink_dir`/`symlink_file` and answer `false` (the test returns, with
  a note on stderr) where the account lacks `SeCreateSymbolicLinkPrivilege` (`ERROR_PRIVILEGE_NOT_HELD`,
  1314, or `PermissionDenied`) and any other refusal is a panic; `unlink_directory` is `remove_dir`
  there, since a directory link is taken away as a directory. The hosted runner is an administrator,
  so the link tests run in CI.
- **The read-only attribute stands for a file's permission bits.** `forbid_writing`, `allow_writing`
  and `can_write` set `0o444`/`0o644` on Unix and the read-only attribute on Windows, which
  `writing::ensure_writable` already reads; the two tag-write failure tests
  (`a_write_that_fails_is_not_noted_as_one_to_put_back`, the cut-short walk back) run on both.
- **A file out of reach is held exclusively.** `studies.rs`' `OutOfReach` is `chmod 000` on Unix and
  an open with `share_mode(0)` held for the pass on Windows; either reads as `Error::Io`, which
  `is_out_of_reach` takes for the same verdict.
- **The vault's dedupe-before-staging test swaps the staging folder for a file**, which refuses a
  staged write on both platforms in place of a closed directory mode.
- **A loopback pair stands for a socket pair** (`bus.rs`' `connected_pair`): `UnixStream::pair` has
  no Windows counterpart, and the pipe peer takes any `Read + Write`.
- **Unix-only on purpose, with the reason:** file names that are not text (`os::unix::ffi`; Windows
  names are UTF-16 and a lone surrogate is not a name a tagger writes); directory modes (a folder
  unreadable, unsearchable or unwritable: the read-only attribute on a folder stops nothing, and a
  denying ACL, which `resonate-win32` could now write, is not written by the tests); a second filesystem (`/dev/shm`, `st_dev`: the three
  organise tests that copy across it, and the volume-noting test);
  the signal tests (`kill -HUP`) and `terminal.rs` (a pseudo-terminal, `setsid`, termios), whose
  Windows counterpart is `console.rs`, below.
- **A file's identity is its volume serial and file index on Windows.** `resonate_win32::file_identity`
  (`GetFileInformationByHandle`) stands where Unix reads `dev` and `ino`, so the in-place Ogg comment
  test (`an_ogg_comment_grown_and_shrunk_...`) proves a shrunk comment lands in the same file on both
  (`a_file_written_in_place_keeps_its_identity_and_one_renamed_over_it_does_not`).
- **`console.rs` drives `play` on a pseudo console of its own** (`resonate_win32::PseudoConsole`,
  the binary's dev-dependency with `console`): it starts `play` on thirty seconds of silence inside
  ConPTY, waits until the console's input mode has left line input and echo, tells it to leave by
  a Ctrl-C typed into the console's input (raw mode reads it as a key, `interrupted()` turns it into
  the interrupt), by `GenerateConsoleCtrlEvent(CTRL_C_EVENT)` or by `CTRL_BREAK_EVENT` (the
  termination's counterpart; `ctrlc` takes every event), and requires it to leave with no
  `STATUS_CONTROL_C_EXIT` and line input and echo back. The mode and the events must come from a
  process on that console, so the test runs itself inside it as an errand
  (`an_errand_run_inside_the_console`, which does nothing without `RESONATE_CONSOLE_ERRAND`): it
  prints `console-mode-<n>=<mode>.`, read back off the console's output with the escape sequences
  stripped, or sends the event. It skips where `resonate sinks` lists no `wasapi:` sink (the hosted
  runner) or no pseudo console opens. **A console closed under `play` is the hang-up's
  counterpart** (`a_console_closed_under_play_leaves_it`): dropping the `PseudoConsole` is
  `ClosePseudoConsole`, which sends `CTRL_CLOSE_EVENT` (`ctrlc`'s `termination` feature takes it as
  the interrupt), and `play` must leave within the test's patience; nothing is asked of the console
  afterwards, since it is gone.

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
  client lists the folder and `writer_of` says which entries live (a held lock, on Windows an open
  handle; the file's content is never read, its name says everything). A dead player's
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

`awake.rs` (`resonate-awake` thread, the binary's, since `resonate-mpris` finds no bus; a Mac's too)
looks at every zone's player each second (`play`: its one player; the window: `Zones::listed`,
through a weak handle) and holds a `keepawake::KeepAwake` (display and idle, as the portal's
`IDLE`/`SUSPEND` do on Linux; on a Mac idle alone, `HOLDS_THE_DISPLAY_TOO`, so the screen may still
sleep) while any is `Playing` or `Buffering`. `SetThreadExecutionState` belongs to the thread that
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
the features `Data_Xml_Dom`, `Foundation`, `Networking_Connectivity` and `UI_Notifications`;
the media transport below adds `Media`, `Media_Playback`, `Storage` and `Storage_Streams`, and
nothing else is enabled). `RoInitialize` and `CoInitializeEx` are `unsafe fn`s there, so a thread enters the
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
  (`notify::actions`, the keys Linux uses) with `activationType="foreground"`. Text is escaped for
  XML and characters XML 1.0 refuses are left out. A toast is tagged (`track`, or `listen` for what
  Listen named, which has no buttons) in the group `resonate`, so the second telling when the cover
  lands replaces the first in the Action Center; teardown hides the track's toast. `ToastNotification::Activated` carries
  the button's key as its argument and `notify::pressed` turns it into the `Command`, settled on
  the zone, so a press reaches the transport as it does on Linux; a press of a toast left in the
  Action Center after the player has gone cannot be answered.
- **The process carries the identity too.** `CreateToastNotifierWithId` names it, and the
  binary's `run` calls `resonate_win32::name_the_process(APP_USER_MODEL_ID)`
  (`SetCurrentProcessExplicitAppUserModelID`) once logging is up, before any window, a refusal
  logged and played on. **A toast was known to show only where a Start-menu shortcut carries that
  identity** (the `System.AppUserModel.ID` property of a `.lnk` in the Start menu's `Programs`),
  which the installer makes (*The installer*, below); whether the process's own identity lets the
  zip's toasts show without one is not proven on a machine (`docs/FEATURES.md`). The identity is one constant, `resonate_mpris::APP_USER_MODEL_ID`
  (`identity.rs`): `desk_toast.rs` creates its notifier under it and a test holds the installer
  script's shortcut to it.
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

## The media transport

Media keys, the volume overlay's and the lock screen's player, and the buttons on them are the
System Media Transport Controls, which `resonate-mpris` publishes to and is commanded from as it is
from MPRIS on Linux (`smtc.rs`, `cfg(windows)`; `transport.rs`, portable and tested on every host).

- **The session comes from a `Windows.Media.Playback.MediaPlayer`, never from a window.** An
  unpackaged Win32 process has no `GetForCurrentView`, and `ISystemMediaTransportControlsInterop::
  GetForWindow(hwnd)` is COM interop the `windows` crate leaves `unsafe` (and `resonate play` has no
  window). A `MediaPlayer` that never plays carries a `SystemMediaTransportControls` of its own;
  its `CommandManager` is switched off (`SetIsEnabled(false)`), so the buttons are ours and not the
  idle player's, and the controls are enabled. All safe projections: the features `Media`,
  `Media_Playback`, `Storage` and `Storage_Streams` beside the toasts' (`dependencies.md`).
  `souvlaki` was weighed and left: on Windows it needs the window handle (`GetForWindow`), pins an
  older `windows` crate beside ours and is a second abstraction over the `Host` seam.
- **The first zone owns it.** Windows has one media session per process here, so the zone that
  claims `org.mpris.MediaPlayer2.resonate` on Linux (the window's first zone, the only one of
  `resonate play`) and registers with BlueZ owns it: `Host::answers_media_keys`, false by default
  (so no test's host touches the machine's controls), the binary's `Desktop` answering true for the
  first zone as it does `answers_headsets`. A second zone shown in the window is not what the keys
  reach; it hands over or is driven from the window.
- **One thread, the bus's poll and arithmetic.** `Mpris::start` spawns `resonate-transport` on
  Windows whether the zone is served on a bus or on the pipe, beside the `Answering` and the
  `resonate-toasts` thread. It enters the apartment (`Apartment`), opens the `Transport`, and every
  `POLL` (200 ms) takes the same `snapshot` the bus publishes from and maps it by `controls_of`
  into a portable `transport::Controls`: `Status::of` is `PlaybackStatus`'s mapping (`Buffering`
  plays, `Idle` stops) with `Closed` where nothing is queued; `Buttons::of` offers Play, Pause,
  Next and Previous exactly where `CanPlay`, `CanPause`, `CanGoNext` and `CanGoPrevious` say so
  (`service::can_go_next`, `can_go_previous`), Stop beside Pause; shuffle and repeat
  (`repeat_code`: `None`, `Track`, `List`); title (the notification's, so a file with no title
  shows its stem), artist, album, album artist and track number off the playing tags; the cover the
  bus names (`art::Pictures`' `file:///` URI, read through `StorageFile::GetFileFromPathAsync` and
  `RandomAccessStreamReference::CreateFromFile`, any other URI `CreateFromUri`); and the timeline in
  100 ns ticks (`ticks`), the end the track's length, the seek range closed where `CanSeek` is false
  or the length unknown. `Moved::between` tells only what moved: the display on a track or cover
  change (`ClearAll` then every field, `Update`), the timeline on a seek (`PlayerState::seeks`,
  as `Seeked`), a status, length or display change, and once every `TIMELINE_EVERY` (1 s) while a
  track is current. Teardown removes the handlers, sets `Closed`, clears the display and disables
  the controls.
- **A request is the MPRIS method of that name.** `ButtonPressed` maps through `Pressed::of` to
  `PlayerInterface::play`, `pause`, `stop`, `next` or `previous` (so a press does nothing where the
  bus says it could not), `PlaybackPositionChangeRequested` to `set_position` on the current row
  (ignored before the start, past the end or where nothing can be sought), and
  `ShuffleEnabledChangeRequested` and `AutoRepeatModeChangeRequested` to the `Shuffle` and
  `LoopStatus` setters. The handlers run on the Windows Runtime's own threads and `settle` as a
  bus call does; what Windows is then shown is what the next poll reads, never the request.
- **The volume overlay's level is the system's.** SMTC carries no application volume, so the
  overlay's slider turns the endpoint, which the output pane already follows (`set_device_volume`).

## Not on Windows yet

The MPRIS service answers "no bus": `resonate-mpris` finds no session bus and the binary carries
on without it as it does on a Linux machine with none (a running window is still reached on its
pipe, the machine kept awake by `awake.rs`, the media transport, toasts and the metered connection
above). The launcher's icon-follows-the-accent is a no-op. All of it is on the roadmap.

## Release

`packaging/windows/build.ps1 -Architecture x86_64|aarch64` builds one archive and one installer per
architecture:

- **`x86_64`** (the default) builds `-C target-cpu=x86-64-v3` for the host and names its files
  `resonate-<version>-x86_64_v3-windows.zip` and `resonate-<version>-x86_64_v3-windows-setup.exe`.
- **`aarch64`** installs the `aarch64-pc-windows-msvc` target, builds `--target` it at `-C
  target-cpu=generic` (the target's own baseline, ARMv8-A with NEON, written out because a PowerShell
  variable set to the empty string is removed rather than emptied and so could not replace the
  checkout's `target-cpu=native`; every Windows 11 ARM64 machine is above it, and a higher floor
  would buy little over NEON and shut out machines for it) and names its files
  `resonate-<version>-aarch64-windows.zip` and `resonate-<version>-aarch64-windows-setup.exe`.

Both add `-C target-feature=+crt-static`, and `.cargo/config.toml` does the same for every
`windows`/`msvc` build from the checkout (a `target.cfg` entry replaces `build.rustflags`, so it
repeats `target-cpu=native`): a fresh Windows has no Visual C++ redistributable, and a build linking
the C runtime dynamically dies before `main` with `STATUS_DLL_NOT_FOUND` for `VCRUNTIME140.dll`
(seen on a clean Windows 11). `packaging.rs` holds both. A cross-build under `RUSTFLAGS=` replaces
them, so one meant to be run elsewhere passes `RUSTFLAGS="-C target-feature=+crt-static"`.

The zip holds `resonate.exe`, the licence and a README; the setup program installs the same three.
`release.yml`'s `windows-zip` is a matrix of the two on `windows-latest`, each its own artifact
(`resonate-windows-<architecture>`), all four files in the `SHA256SUMS`. The ARM64 files are
**cross-built on the x64 runner**, not on a `windows-11-arm`
one: the hosted ARM64 runner's availability follows the repository's plan, its image carries fewer
tools, and a release is immutable, so the more predictable runner builds it; the x64 image already carries the
MSVC ARM64 tools and LLVM that `ring` needs, and a step runs `clang --version` first so a missing
compiler fails by name. A cross-built archive or installer is never run in CI, which is why the
roadmap still lists the ARM64 build among what only hardware settles.

### The executable's icon and version

`crates/resonate/build.rs` embeds a resource file when the target is Windows, through
`resources.rs`: the icon `packaging/windows/resonate.ico` as `RT_GROUP_ICON` 1 (the lowest name,
which Explorer shows and which gpui's `load_icon` loads for the window and the taskbar) and a
version resource (`FileVersion` and `ProductVersion` the manifest's `major.minor.patch.0`,
`Comments` the full version so `-rcN` stays readable, `CompanyName` and `ProductName` `Resonate`,
`OriginalFilename` `resonate.exe`, `LegalCopyright` naming the licence). The version resource is
written by `win-resource-builder` (pure Rust; `dependencies.md` says why it, why not `rc.exe`, and
why its `images` feature is off); the icon is `icon_resources.rs`, which reads the `.ico` and writes
an `RT_ICON` record per image (ids 1 to N) and the `RT_GROUP_ICON` directory naming them. `joined`
puts the icon records after the `.res` file's empty first record and before the version record, and
the one `.res` is passed as `cargo::rustc-link-arg-bins`, which `link.exe` and `lld-link` take for
either machine, so `CARGO_CFG_TARGET_ARCH` is not read. **The modules are `#[cfg(windows)]`, the
host's cfg, because the dependency is `[target.'cfg(windows)'.build-dependencies]`, which Cargo
resolves against the host**: a Windows host (the runner, for both architectures) embeds, a Linux
host cross-checking with `cargo xwin` neither compiles the modules nor pulls the crate, so a
cross-built executable carries no icon and no Linux package build gains a dependency. The build
script also tests `CARGO_CFG_TARGET_OS`. The `.ico` is committed, ten PNG images from 16 to 256 px
rendered from `packaging/resonate.svg` by `packaging/windows/icon.py` (`rsvg-convert`, `python3 -I
packaging/windows/icon.py`). The Linux desktop's accent-following icon is not carried over: the
executable's icon is the packaged colours. `tests/icon_resources.rs` includes `icon_resources.rs`
and runs it on every host over the committed file: each image is a numbered `RT_ICON` holding its
bytes, the group's entries match the `.ico`'s directory with the ids in the offsets' place, the
icon records join in ahead of a resource file, and a file that is no icon is refused by name
(`the_icon_holds_the_sizes_windows_asks_for_as_png_of_those_sides` holds the file). `resources.rs`
itself runs only on a Windows host; its `.res` was read back with `llvm-cvtres` and
`llvm-readobj --coff-resources` on Linux, and the icon records were identical to the crate's own.

### The installer

`packaging/windows/resonate.iss` is an **Inno Setup** script, compiled by `build.ps1` with `ISCC`
(on the runner's path, else installed with `choco install innosetup`). Inno rather than a WiX MSI:
the script is one text file in which a list of registry rows is plain lines, `[Icons]` takes the
shortcut's `AppUserModelID` directly, winget installs it (`InstallerType: inno`, silent switches
known), it is free of WiX's maintenance-fee terms and needs no .NET tool on the runner, and it
cannot be validated on Linux either way. It is **per user**: `PrivilegesRequired=lowest`, no
override, so it installs to `%LOCALAPPDATA%\Programs\Resonate`, writes `HKCU` alone and never asks
for elevation. `x86_64` is `x64os`, `aarch64` is `arm64`, so each runs only on its own Windows.
`build.ps1` passes the version, its numeric `major.minor.patch.0`, the architecture, the paths and
the output name as `/D` definitions the script reads (`the_build_script_compiles_one_installer_per_architecture_beside_its_archive`).

- **The Start menu shortcut** (`{autoprograms}\Resonate`) carries `AppUserModelID:
  "{#AppUserModelId}"`, defined once at the head of the script as `org.resonate.Resonate`, the
  value of `resonate_mpris::APP_USER_MODEL_ID`, the constant the toast code creates its notifier
  under. Windows has shown a toast only for an identity a Start-menu shortcut carries, so installing
  is the sure way to toasts; the zip and a Scoop install have no such shortcut, and lean on the
  process's own identity above
  (`the_start_menu_shortcut_carries_the_identity_the_toasts_are_raised_under`).
- **File associations are the registration Windows allows a program to make**, all under
  `HKCU\Software\Classes`: the ProgID `Resonate.Audio` (name, `DefaultIcon`, `shell\open\command`
  `"resonate.exe" "%1"`), `OpenWithProgids` on every extension so Resonate is offered in "Open
  with", `Applications\resonate.exe` with `SupportedTypes`, and `Software\Resonate\Capabilities`
  with `FileAssociations` and `RegisteredApplications`, so Settings > Default apps lists it. It
  claims no default: Windows leaves that to the listener. The extensions are `AUDIO_EXTENSIONS`
  (everything the scan admits, which is the `MIME_TYPES` formats and `.rf64`, `.w64`), in the
  three places that name them. A test holds each place to the list, another holds the bus's `MIME_TYPES` to the
  extensions registered (`the_installer_registers_every_extension_the_scan_admits_and_no_other`,
  `the_windows_installer_registers_an_extension_for_every_media_type_the_bus_offers`), so a format
  added to the scan fails until the script lists it. The list is written out line by line in the
  script, not generated, as the desktop entry's is.
- **`resonate://` is a URL protocol the installer registers** (`HKCU\Software\Classes\resonate`: `URL:Resonate`, an empty `URL Protocol`, `DefaultIcon`, and `shell\open\command` `"resonate.exe" "%1"`, the scheme defined once as `LinkScheme`), as the desktop entry declares `x-scheme-handler/resonate`, so a shared playlist or party link clicked in a browser reaches the program as an argument (`binary.md`: an invitation, not a song) and a running window is handed it over its pipe as a file is. A pairing link `resonate://pair/` is delivered too and ignored, as on Linux (`the_installer_hands_resonate_links_to_the_program_as_the_desktop_entry_does`). The zip and a Scoop install register nothing.
- **Uninstalling removes the program, the shortcut and every key and value setup wrote** (each row
  deletes its key or value, an extension's key only if left empty) and `ChangesAssociations=yes`
  tells the shell. Settings, catalog, vault and profiles in `%APPDATA%` and `%LOCALAPPDATA%` stay
  (`the_installer_writes_the_current_users_registry_and_removes_what_it_wrote`).
- The publisher is `Resonate`; the script, the manifests and the scripts name no maintainer and
  no repository (`nothing_the_windows_installer_is_made_of_names_a_maintainer_or_a_repository`).
  The installer is not run in CI; a Windows runner compiles it at release. The 0.5.0 x64 installer
  ran on Windows 11 under KVM, registering the audio and link commands and uninstalling them while
  preserving the settings. With the app closed, uninstall removed its executable; a reinstall
  succeeded. ARM64 and the shell's actual default-app selection still need hardware checks.

### Signing

`packaging/windows/sign.ps1` signs the given files with the Windows SDK's `signtool` (SHA-256,
timestamped) from a PFX in `RESONATE_WINDOWS_PFX_BASE64` and `RESONATE_WINDOWS_PFX_PASSWORD`, which
`release.yml` fills from the secrets `WINDOWS_CERTIFICATE_BASE64` and `WINDOWS_CERTIFICATE_PASSWORD`.
Where either is empty (no certificate exists yet) it says so and leaves the files unsigned, so a
release without the secrets is built as before. `build.ps1` signs `resonate.exe` before the zip and the
installer take it, and the installer after it is compiled (the uninstaller inside it is not signed).
The certificate is written to a temporary file for the signing and removed at the end. Nothing here
has run: it waits on a certificate (`docs/FEATURES.md`).

`GameModeWatch` registers effective power mode V2 notifications and reports `EffectivePowerModeGameMode`, the active system mode rather than the Game Bar preference. Its callback only stores an atomic; dropping the watch unregisters it. The binary watches once for its lifetime and reports the state into `core::gaming` every 100 ms. Registration failure leaves enrichment available.
