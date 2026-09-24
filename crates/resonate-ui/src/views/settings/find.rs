use crate::{SettingKey, icons::Icon};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) enum Category {
    #[default]
    Output,
    Processing,
    Equaliser,
    Library,
    Online,
    Desktop,
    Appearance,
    About,
}

impl Category {
    pub(crate) const ALL: [Self; 8] = [
        Self::Output,
        Self::Processing,
        Self::Equaliser,
        Self::Library,
        Self::Online,
        Self::Desktop,
        Self::Appearance,
        Self::About,
    ];

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Output => "Output",
            Self::Processing => "Processing",
            Self::Equaliser => "Equaliser",
            Self::Library => "Library",
            Self::Online => "Online",
            Self::Desktop => "Desktop",
            Self::Appearance => "Appearance",
            Self::About => "About",
        }
    }

    pub(crate) const fn about(self) -> &'static str {
        match self {
            Self::Output => {
                "The device, the rate the stream opens at and how much is held ahead of it"
            }
            Self::Processing => "What may touch the samples between the decoder and the sink",
            Self::Equaliser => {
                "The correction applied to each device, and where its curves come from"
            }
            Self::Library => {
                "The folders a scan walks, the scan itself, and how the files are filed"
            }
            Self::Online => {
                "What is looked up about the library, and what a request says about you"
            }
            Self::Desktop => "What this player tells the rest of the session about itself",
            Self::Appearance => {
                "The palette the window is painted in, how large it is drawn, what its titlebar \
                 carries and what the wheel does over the volume"
            }
            Self::About => "What this build is, where it keeps things, and how to put it back",
        }
    }

    pub(crate) const fn icon(self) -> Icon {
        match self {
            Self::Output => Icon::Volume,
            Self::Processing => Icon::Inspector,
            Self::Equaliser => Icon::Equaliser,
            Self::Library => Icon::Folder,
            Self::Online => Icon::Globe,
            Self::Desktop => Icon::Info,
            Self::Appearance => Icon::Appearance,
            Self::About => Icon::Info,
        }
    }

    pub(crate) fn groups(self) -> impl Iterator<Item = Group> {
        Group::ALL
            .into_iter()
            .filter(move |group| group.category() == self)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Group {
    Device,
    SampleRate,
    GraphRate,
    Buffer,
    Dop,
    Bluetooth,
    Resampler,
    Dither,
    NoiseShaping,
    ReplayGain,
    TruePeak,
    LossySources,
    Equalising,
    BoundTo,
    Bands,
    Measured,
    Folders,
    Scanning,
    Refreshing,
    Tagging,
    Organising,
    Vault,
    Inbox,
    Resuming,
    Repeating,
    Lookups,
    AfterScan,
    Studies,
    Contact,
    Recognition,
    Listening,
    LookUpNow,
    Notifications,
    Discord,
    DiscordShows,
    Colour,
    Layout,
    WindowButtons,
    VolumeWheel,
    Scrollbars,
    Tabs,
    Build,
    Places,
    Everything,
}

impl Group {
    pub(crate) const ALL: [Self; 44] = [
        Self::Device,
        Self::SampleRate,
        Self::GraphRate,
        Self::Buffer,
        Self::Dop,
        Self::Bluetooth,
        Self::Resampler,
        Self::Dither,
        Self::NoiseShaping,
        Self::ReplayGain,
        Self::TruePeak,
        Self::LossySources,
        Self::Equalising,
        Self::BoundTo,
        Self::Bands,
        Self::Measured,
        Self::Folders,
        Self::Scanning,
        Self::Refreshing,
        Self::Tagging,
        Self::Organising,
        Self::Vault,
        Self::Inbox,
        Self::Resuming,
        Self::Repeating,
        Self::Lookups,
        Self::AfterScan,
        Self::Studies,
        Self::Contact,
        Self::Recognition,
        Self::Listening,
        Self::LookUpNow,
        Self::Notifications,
        Self::Discord,
        Self::DiscordShows,
        Self::Colour,
        Self::Layout,
        Self::WindowButtons,
        Self::VolumeWheel,
        Self::Scrollbars,
        Self::Tabs,
        Self::Build,
        Self::Places,
        Self::Everything,
    ];

    pub(crate) const fn category(self) -> Category {
        match self {
            Self::Device
            | Self::SampleRate
            | Self::GraphRate
            | Self::Buffer
            | Self::Dop
            | Self::Bluetooth => Category::Output,
            Self::Resampler
            | Self::Dither
            | Self::NoiseShaping
            | Self::ReplayGain
            | Self::TruePeak
            | Self::LossySources => Category::Processing,
            Self::Equalising | Self::BoundTo | Self::Bands | Self::Measured => Category::Equaliser,
            Self::Folders
            | Self::Scanning
            | Self::Refreshing
            | Self::Tagging
            | Self::Organising
            | Self::Vault
            | Self::Inbox
            | Self::Resuming
            | Self::Repeating => Category::Library,
            Self::Lookups
            | Self::AfterScan
            | Self::Studies
            | Self::Contact
            | Self::Recognition
            | Self::Listening
            | Self::LookUpNow => Category::Online,
            Self::Notifications | Self::Discord | Self::DiscordShows => Category::Desktop,
            Self::Colour
            | Self::Layout
            | Self::WindowButtons
            | Self::VolumeWheel
            | Self::Scrollbars
            | Self::Tabs => Category::Appearance,
            Self::Build | Self::Places | Self::Everything => Category::About,
        }
    }

    pub(crate) const fn is_experimental(self) -> bool {
        matches!(self, Self::LossySources)
    }

    pub(crate) const fn title(self) -> &'static str {
        match self {
            Self::Device => "Playback device",
            Self::SampleRate => "Sample rate",
            Self::GraphRate => "Graph rate",
            Self::Buffer => "Buffer",
            Self::Dop => "DSD over PCM",
            Self::Bluetooth => "Bluetooth",
            Self::Resampler => "Resampler",
            Self::Dither => "Dither",
            Self::NoiseShaping => "Noise shaping",
            Self::ReplayGain => "ReplayGain",
            Self::TruePeak => "True peak",
            Self::LossySources => "Artificially enhance lossy files",
            Self::Equalising => "Equaliser",
            Self::BoundTo => "Bound to each device",
            Self::Bands => "Bands",
            Self::Measured => "Measured corrections",
            Self::Folders => "Music folders",
            Self::Scanning => "Scanning",
            Self::Refreshing => "Refreshing",
            Self::Tagging => "Tagging",
            Self::Organising => "Organising",
            Self::Vault => "The vault",
            Self::Inbox => "The inbox",
            Self::Resuming => "Resuming",
            Self::Repeating => "Repeating a track",
            Self::Lookups => "Reference lookups",
            Self::AfterScan => "After a scan",
            Self::Studies => "Studying tracks",
            Self::Contact => "Contact",
            Self::Recognition => "Recognition",
            Self::Listening => "Listening",
            Self::LookUpNow => "Look up now",
            Self::Notifications => "Notifications",
            Self::Discord => "Discord",
            Self::DiscordShows => "What Discord shows",
            Self::Colour => "Colour",
            Self::Layout => "Layout",
            Self::WindowButtons => "Window buttons",
            Self::VolumeWheel => "The volume wheel",
            Self::Scrollbars => "Scrollbars",
            Self::Tabs => "Sidebar tabs",
            Self::Build => "This build",
            Self::Places => "Where things are kept",
            Self::Everything => "Start again",
        }
    }

    pub(crate) const fn hint(self) -> &'static str {
        match self {
            Self::Device => DEVICE_HINT,
            Self::SampleRate => SAMPLE_RATE_HINT,
            Self::GraphRate => GRAPH_RATE_HINT,
            Self::Buffer => BUFFER_HINT,
            Self::Dop => DOP_HINT,
            Self::Bluetooth => BLUETOOTH_HINT,
            Self::Resampler => RESAMPLER_HINT,
            Self::Dither => DITHER_HINT,
            Self::NoiseShaping => SHAPING_HINT,
            Self::ReplayGain => REPLAY_GAIN_HINT,
            Self::TruePeak => TRUE_PEAK_HINT,
            Self::LossySources => LOSSY_SOURCES_HINT,
            Self::Equalising => EQUALISING_HINT,
            Self::BoundTo => BOUND_TO_HINT,
            Self::Bands => BANDS_HINT,
            Self::Measured => MEASURED_HINT,
            Self::Folders => FOLDERS_HINT,
            Self::Scanning => SCANNING_HINT,
            Self::Refreshing => REFRESHING_HINT,
            Self::Tagging => TAGGING_HINT,
            Self::Organising => ORGANISING_HINT,
            Self::Vault => VAULT_HINT,
            Self::Inbox => INBOX_HINT,
            Self::Resuming => RESUMING_HINT,
            Self::Repeating => REPEATING_HINT,
            Self::Lookups => ONLINE_HINT,
            Self::AfterScan => AFTER_SCAN_HINT,
            Self::Studies => STUDIES_HINT,
            Self::Contact => CONTACT_HINT,
            Self::Recognition => RECOGNITION_HINT,
            Self::Listening => LISTENING_HINT,
            Self::LookUpNow => LOOKUP_HINT,
            Self::Notifications => NOTIFICATIONS_HINT,
            Self::Discord => DISCORD_HINT,
            Self::DiscordShows => DISCORD_SHOWS_HINT,
            Self::Colour => COLOUR_HINT,
            Self::Layout => LAYOUT_HINT,
            Self::WindowButtons => WINDOW_BUTTONS_HINT,
            Self::VolumeWheel => VOLUME_WHEEL_HINT,
            Self::Scrollbars => SCROLLBARS_HINT,
            Self::Tabs => TABS_HINT,
            Self::Build => BUILD_HINT,
            Self::Places => PLACES_HINT,
            Self::Everything => EVERYTHING_HINT,
        }
    }

    pub(crate) const fn also_called(self) -> &'static str {
        match self {
            Self::Device => "sink output card dac headphones speakers pipewire",
            Self::SampleRate => "bit perfect khz hz resample untouched",
            Self::GraphRate => "pipewire daemon switch rate",
            Self::Buffer => "latency milliseconds ms depth ring underrun",
            Self::Dop => "dsd dsf dff sacd marker packing",
            Self::Bluetooth => {
                "headphones earbuds airpods a2dp wireless cut off clipped start first second \
                 wake sleep idle power save experimental"
            }
            Self::Resampler => {
                "quality sinc kaiser taps filter conversion phase minimum intermediate pre-ringing"
            }
            Self::Dither => "truncation triangular rectangular noise",
            Self::NoiseShaping => {
                "lipshitz threshold hearing terhardt e-weighted psychoacoustic curve shaped"
            }
            Self::ReplayGain => "loudness volume normalisation gain album track pre-amp untagged",
            Self::TruePeak => "clipping intersample over limiter dbtp headroom lossy",
            Self::LossySources => {
                "lossy sources mp3 aac vorbis restore restoration repair enhance enhancement \
                 artificial experimental bandwidth extension sbr dsee holes swirly cider"
            }
            Self::Equalising => {
                "eq equalizer parametric filter correction headphone tone \
                                 bit-perfect"
            }
            Self::BoundTo => {
                "sink device per-device default binding preset assign headphones own custom"
            }
            Self::Bands => {
                "curve response graph frequency gain q peaking shelf preamp band drag handle \
                 custom own point"
            }
            Self::Measured => {
                "autoeq oratory1990 crinacle harman target download fetch \
                               measurement rig suggest"
            }
            Self::Folders => "roots directories add scan music path",
            Self::Scanning => "rescan stop enrich tags index",
            Self::Refreshing => {
                "reindex re-read reread rebuild refresh length duration timestamps modified \
                 cue sheet search index covers artwork missing database"
            }
            Self::Tagging => {
                "write back retag tags metadata musicbrainz ids cover art picture \
                              embed files"
            }
            Self::Organising => {
                "tidy rename move layout template pattern folder structure filename"
            }
            Self::Vault => {
                "archive import re-encode recompress flac strip metadata covers jxl \
                              managed store deduplicate validate"
            }
            Self::Inbox => "provider poll wants missing download drop folder fill obtain supply",
            Self::Resuming => "queue restart restore carry on position where left off",
            Self::Repeating => "loop repeat one single track skip next previous spotify",
            Self::Lookups => "musicbrainz network internet covers lyrics lrclib offline",
            Self::AfterScan => "enrich automatic handover",
            Self::Studies => {
                "analyse analysis decode fake lossless transcode verdict loudness true peak \
                 fingerprint cpu cores"
            }
            Self::Contact => "user-agent email identity request",
            Self::Recognition => {
                "acoustid audd key token fingerprint chromaprint identify recognise unnamed \
                 misnamed"
            }
            Self::Listening => {
                "listen shazam microphone mic desktop monitor what is playing identify song \
                 name that tune seconds clip"
            }
            Self::LookUpNow => "enrich refresh musicbrainz covers portraits",
            Self::Notifications => "notify popup toast banner song track change desktop osd",
            Self::Discord => {
                "rich presence status activity listening profile application id client"
            }
            Self::DiscordShows => {
                "rich presence status activity cover art album icon asset progress bar elapsed \
                 paused details"
            }
            Self::Colour => "theme accent palette dark swatch",
            Self::Layout => "text size type scale larger smaller density",
            Self::WindowButtons => {
                "titlebar minimize maximize minimise maximise zoom decorations chrome tiling \
                                     controls hide"
            }
            Self::VolumeWheel => "scroll mouse wheel touchpad volume slider louder quieter",
            Self::Scrollbars => "scroll bar thumb track overlay lists panes drag hide",
            Self::Tabs => "sidebar panes suggestions missing wanted hide show collection",
            Self::Build => "version typeface font features sinks",
            Self::Places => "config.toml library database path xdg",
            Self::Everything => "reset defaults factory put back",
        }
    }

    pub(crate) const fn keys(self) -> &'static [SettingKey] {
        match self {
            Self::Device => &[SettingKey::Sink],
            Self::SampleRate => &[SettingKey::BitPerfect],
            Self::GraphRate => &[SettingKey::ForceGraphRate],
            Self::Buffer => &[SettingKey::Buffer],
            Self::Dop => &[SettingKey::Dop],
            Self::Bluetooth => &[
                SettingKey::BluetoothWake,
                SettingKey::BluetoothLead,
                SettingKey::BluetoothAwake,
            ],
            Self::Resampler => &[SettingKey::Quality, SettingKey::FilterPhase],
            Self::Dither => &[SettingKey::Dither],
            Self::NoiseShaping => &[SettingKey::NoiseShaping],
            Self::ReplayGain => &[
                SettingKey::ReplayGain,
                SettingKey::PreAmp,
                SettingKey::Untagged,
            ],
            Self::TruePeak => &[SettingKey::TruePeak],
            Self::LossySources => &[SettingKey::Restoration],
            Self::Equalising => &[SettingKey::Equaliser],
            Self::BoundTo => &[SettingKey::EqualiserFor],
            Self::Organising => &[SettingKey::OrganiseAs],
            Self::Vault => &[],
            Self::Inbox => &[SettingKey::Inbox],
            Self::Resuming => &[SettingKey::Resume],
            Self::Repeating => &[SettingKey::SkipUnderRepeat],
            Self::Lookups => &[SettingKey::Online],
            Self::AfterScan => &[SettingKey::EnrichAfterScan],
            Self::Studies => &[SettingKey::Study],
            Self::Contact => &[SettingKey::Contact],
            Self::Recognition => &[SettingKey::AcoustidKey, SettingKey::AuddToken],
            Self::Listening => &[SettingKey::ListenFrom, SettingKey::ListenFor],
            Self::Notifications => &[SettingKey::Notify],
            Self::Discord => &[SettingKey::Discord, SettingKey::DiscordApp],
            Self::DiscordShows => &[
                SettingKey::DiscordShows,
                SettingKey::DiscordArt,
                SettingKey::DiscordIcon,
                SettingKey::DiscordProgress,
                SettingKey::DiscordPaused,
            ],
            Self::Colour => &[SettingKey::Theme, SettingKey::Accent],
            Self::Layout => &[SettingKey::TextSize],
            Self::WindowButtons => &[SettingKey::MinimiseButton, SettingKey::MaximiseButton],
            Self::VolumeWheel => &[SettingKey::ScrollVolume],
            Self::Scrollbars => &[SettingKey::Scrollbars],
            Self::Tabs => &[SettingKey::SuggestionsTab, SettingKey::MissingTab],
            Self::Bands
            | Self::Measured
            | Self::Folders
            | Self::Scanning
            | Self::Refreshing
            | Self::Tagging
            | Self::LookUpNow
            | Self::Build
            | Self::Places
            | Self::Everything => &[],
        }
    }

    fn matches(self, word: &str) -> bool {
        let reads = |text: &str| text.to_lowercase().contains(word);

        reads(self.title())
            || reads(self.also_called())
            || reads(self.category().label())
            || reads(self.hint())
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Narrowing {
    words: Vec<String>,
}

impl Narrowing {
    pub(crate) fn of(query: &str) -> Self {
        Self {
            words: query
                .split_whitespace()
                .map(str::to_lowercase)
                .filter(|word| !word.is_empty())
                .collect(),
        }
    }

    pub(crate) fn narrows(&self) -> bool {
        !self.words.is_empty()
    }

    pub(crate) fn allows(&self, group: Group) -> bool {
        self.words.iter().all(|word| group.matches(word))
    }

    pub(crate) fn found(&self, category: Category) -> usize {
        category
            .groups()
            .filter(|group| self.allows(*group))
            .count()
    }

    pub(crate) fn anywhere(&self) -> impl Iterator<Item = Group> + '_ {
        Group::ALL
            .into_iter()
            .filter(move |group| self.allows(*group))
    }
}

pub(crate) const DEVICE_HINT: &str = "Where Resonate sends audio. The badge on each device says \
                                      whether the track playing would reach it untouched or be \
                                      converted first, so a device that advertises the file's own \
                                      rate and depth is the one that stays bit-perfect.";

pub(crate) const RESAMPLER_HINT: &str = "Runs only when the device cannot take the file's own sample rate, and never touches a stream \
     the device accepts as it is. It is a windowed-sinc filter: an ideal low-pass is cut to a \
     finite length by a Kaiser window, whose β trades a wider transition band for a deeper \
     stopband, and the cutoff sits just under the lower of the two Nyquist limits so nothing folds \
     back as aliasing. The curve is built once into a table holding every phase the two rates can \
     meet, so each output instant reads its own exact weights. Downsampling stretches the window \
     by the ratio, so the same level costs more taps going down than up. Very high is built to beat \
     SoX's very-high setting on paper: flat to 96 % of Nyquist, half power at 97.6 % and 185 dB \
     down from Nyquist on, against SoX's 95 % and 175 dB. Hover a level for the filter it builds. \
     Linear phase rings as much before a transient as after it; minimum phase moves all of that \
     ringing after it and plays a few frames sooner, at the price of delaying the treble a little \
     more than the bass; intermediate sits between the two.";

pub(crate) const TRUE_PEAK_HINT: &str = "A sample can sit under full scale while the wave the \
                                         device rebuilds between two samples passes it, and a \
                                         lossy file often decodes past full scale outright. With \
                                         this on, a track the library has studied is turned down \
                                         by what its measured true peak asks, and wherever the \
                                         sound is processed a guard watches the output at eight \
                                         times its rate and rides the level down under -0.1 dBTP \
                                         just before an over, rather than letting it clip.";

pub(crate) const LOSSY_SOURCES_HINT: &str = "An MP3, AAC or Vorbis file has had a band cut away \
                                            above the encoder's lowpass and, at lower bitrates, \
                                            bands switched off from one moment to the next. Repair \
                                            lifts the droop the encoder's own filter leaves just \
                                            under its cutoff and fills a band that drops out of \
                                            an otherwise steady sound with noise shaped like it. \
                                            Extend also rebuilds the band above the cutoff from \
                                            the octave below it, the way AAC's SBR and Sony's \
                                            DSEE do, at a level that falls away faster than the \
                                            music does. Both touch lossy files alone.";

pub(crate) const DITHER_HINT: &str = "Applies wherever the output is shallower than the source or \
                                      anything is processed, on a 16-, 24- or 32-bit device alike. \
                                      Triangular trades a little steady noise for the distortion \
                                      rounding alone would add; None rounds to the nearest step.";

pub(crate) const SHAPING_HINT: &str = "Moves the dither noise above towards where the ear is least sensitive, so it runs only where \
     dither does. Threshold is designed for whatever rate the device runs at, so it shapes at every \
     rate. Lipshitz is a fit to 44.1 kHz: at any rate but 44.1 and 48 kHz its taps would put the \
     notch in the wrong place, so the stream falls back to flat dither rather than shaping it \
     wrongly. `resonate explain` prints the curve that survives beside the target depth.";

pub(crate) const SAMPLE_RATE_HINT: &str = "Which rate the device is opened at. Match the file asks for the file's own rate, so a 96 kHz \
     recording reaches a device that advertises 96 kHz untouched and nothing resamples. Follow the \
     graph stays on whatever rate PipeWire is already running and resamples into it, which never \
     interrupts another client but converts every stream whose rate differs.";

pub(crate) const GRAPH_RATE_HINT: &str = "Whether the stream asks PipeWire to run the whole graph at its rate. Asking is what carries a \
     rate change through to the hardware; leaving it to the daemon lets the graph stay where it is \
     and resample on the way. The daemon has the last word either way — a rate it refuses to \
     switch to is not in the sink's allowed rates.";

pub(crate) const BUFFER_HINT: &str = "How much decoded audio is held ahead of the device. A deeper \
                                      buffer rides out a busy machine, a shallower one costs less \
                                      memory and lets a sink change and a track change land \
                                      sooner. It is the size the ring is built at, so changing it \
                                      reopens the stream.";

pub(crate) const BLUETOOTH_HINT: &str = "Wireless headphones power their radio down when nothing \
     is sent to them, and the first moment of sound after that is lost while the link wakes. \
     With this on, a Bluetooth device is kept fed with silence through a pause so resuming loses \
     nothing, and a stream that starts after the link has slept opens on a short lead of silence \
     before the track, with the clock held at the start until the music is really heard. \
     Experimental: it keeps the headphones' radio busy through a pause, which costs them \
     battery.";

pub(crate) const DOP_HINT: &str = "Hands a DSD file to the device as DSD carried inside PCM words rather than decimating it to \
     PCM first. Nothing in ALSA or SPA advertises whether a device decodes DoP — only the DAC's \
     own detector knows — so this is a claim you make about your hardware and not one Resonate can \
     check. A DAC that does not decode it plays the markers as full-scale white noise. It is \
     taken only where the device advertises the exact rate and depth the carrier needs and \
     nothing else would touch the stream; `resonate explain` prints which packing was chosen and \
     why the other was not.";

pub(crate) const EQUALISING_HINT: &str = "Whether the curve bound to the playing device is \
     applied. An equaliser is a filter, so turning it on takes the stream out of bit-perfect: \
     every sample is recomputed in floating point before it reaches the device, the playback \
     bar's mode reads converted rather than bit-perfect, and `resonate explain` says so rather \
     than reporting a path it no longer takes. It runs after any resampling, so one profile means \
     one curve whatever rate the device is opened at, and a band above the output's Nyquist is \
     passed over rather than folded back as something it is not. A DSD stream packed as DoP \
     refuses it outright, because rewriting those samples would destroy the markers the DAC \
     reads.";

pub(crate) const BOUND_TO_HINT: &str = "Which curve is applied to which device: a kept profile, \
     or a curve of the device's own that needs no name. A correction is measured for one pair of \
     headphones, so it is bound to the device those headphones are plugged into rather than being \
     in force everywhere: plug in speakers and nothing is applied unless you bound something to \
     them too. The binding every device that names none of its own falls back to is written as \
     default, and the settings file keeps the pairs as an ordinary table you can edit by hand, \
     with true standing for a device's own curve.";

pub(crate) const BANDS_HINT: &str = "What the bound curve does to the sound, drawn at the rate \
     the device is open at, and shaped where it is drawn. Press anywhere on it to add a band there, \
     drag a handle to move one — sideways for its frequency, up and down for its gain — scroll over \
     one to narrow or widen it, and right-press it to take it out. A device with nothing bound is \
     given a curve of its own the first time it is pressed, so shaping the sound needs no profile \
     and no name. A pointer places a band to a tenth of a decibel; a number typed into a row is \
     exact, which a correction measured to the hundredth deserves. The preamp above the rows is \
     what keeps the loudest part of the curve from reaching full scale, and Fit the preamp sets it \
     from the curve itself. Curves are kept as EqualizerAPO text, so any other player reads what is \
     exported here.";

pub(crate) const MEASURED_HINT: &str = "Corrections measured by AutoEq for a few thousand \
     headphones, fetched one device at a time and kept here as an ordinary profile file. Type a \
     name to see what has been measured and by whom; who measured it and on which rig are shown \
     beside every result, because two rigs disagree about one pair of headphones more than two \
     measurements on one rig do. What is plugged in reads the chosen device's own name back \
     against the catalogue, and answers with nothing rather than a guess wherever that name is a \
     socket rather than a product.";

pub(crate) const REPLAY_GAIN_HINT: &str = "Plays every track at the loudness its tags declare. \
                                           Track levels each track on its own, Album keeps the \
                                           relative levels inside a record. A boost is capped \
                                           where the declared peak would reach full scale, so the \
                                           waveform keeps its shape. A track with no tag that the \
                                           library has studied is played at the gain its measured \
                                           loudness asks for. The pre-amp is added to every gain, \
                                           and a track neither tagged nor studied is played at the \
                                           gain chosen for it instead.";

pub(crate) const FOLDERS_HINT: &str = "The folders a scan walks. Adding one scans it straight \
                                       away; dropping one forgets every track that came from it, \
                                       and leaves the files alone.";

pub(crate) const SCANNING_HINT: &str = "Re-reads every folder above. A scan is incremental: a file \
                                        whose size and modification time are unchanged is skipped.";

pub(crate) const REFRESHING_HINT: &str = "Reads parts of the catalog again on purpose. Read \
                                           every file again re-reads each file even where its \
                                           size and time are unchanged — names, lengths, cue \
                                           sheets, covers and the search index — and keeps plays, \
                                           favourites and playlists. Look for missing covers asks \
                                           the archive again for every album with no picture.";

pub(crate) const TAGGING_HINT: &str = "Writes what a lookup answered for back into the files, so \
     another player reads the same names: the titles and artists a reference named, the release \
     and recording ids, the date, label, catalogue number and barcode, and the cover art where the \
     file carries none. A name this build only guessed at is never written, a field the file \
     already says is left alone, and a row a cue sheet cut out of a file it shares is passed over \
     whole. Preview lists what would be written and only Apply writes anything. Nothing is written \
     on its own: this runs when you press it and at no other time.";

pub(crate) const VAULT_HINT: &str = "A managed archive this build writes itself. Importing a \
                                     track decodes it, throws every tag and picture away, keeps \
                                     the smallest bit-exact copy it can make and points the \
                                     catalog at it. Covers are kept once each as lossless JXL, \
                                     and the library it was imported from is read and never \
                                     touched.";

pub(crate) const INBOX_HINT: &str = "A folder the tracks marked wanted are filled from. A file \
                                     directly inside it whose name is the recording's \
                                     MusicBrainz id, the track's or the ISRC is kept in the \
                                     vault and joins its album as a track; a file that only \
                                     shares a title is never taken. The folder is read and never \
                                     written to. A track is asked about as soon as it is marked \
                                     and then every half hour, no oftener than every six hours \
                                     each; Poll now asks about every one at once, as resonate \
                                     poll --again does.";

pub(crate) const ORGANISING_HINT: &str = "Where the files themselves are kept, written as a \
     template of the fields a track carries: {albumartist}, {artist}, {album}, {title}, {year}, \
     {disc}, {track} and {ext}, with each / a folder. Preview lists what would move and where, \
     and only Apply moves anything. A track nothing names an album for is left where it is, so is \
     one whose destination another file already holds, and a folder left empty is taken away. \
     Nothing is filed on its own: this runs when you press it and at no other time.";

pub(crate) const REPEATING_HINT: &str = "What a skip does while one track is repeating. On, \
                                         pressing next or previous moves on and repeats the \
                                         whole queue from there, the way a streaming player \
                                         does. Off, it moves on and repeats the track it lands \
                                         on instead.";

pub(crate) const RESUMING_HINT: &str = "Whether the queue, the row it is on and how far through \
                                        that row are kept in the library for the next run. On, \
                                        opening the window with no files named puts the queue back \
                                        where it was, paused on the row it was playing. Off, what \
                                        was kept is discarded and nothing is written.";

pub(crate) const ONLINE_HINT: &str = "Whether Resonate reaches the network at all. On, a scan ends by asking MusicBrainz about \
     every album and artist it has not yet been answered about, takes a cover from the Cover Art \
     Archive and a portrait from Wikimedia Commons where one is offered, and the lyrics pane asks \
     LRCLIB for a sheet the file does not carry. Off, nothing leaves the machine.";

pub(crate) const AFTER_SCAN_HINT: &str = "Whether a scan that finishes hands over to the reference \
                                          on its own. On, every scan ends by asking about what it \
                                          found; off, the library is asked only when Enrich is \
                                          pressed here or in the Library card, or when resonate \
                                          enrich runs.";

pub(crate) const STUDIES_HINT: &str = "Whether a lookup also decodes every track once, on half \
                                       the machine's cores, to judge whether its lossless claim \
                                       holds, measure its loudness and true peak and take its \
                                       print. Off, a lookup asks the reference alone, and a track \
                                       is studied only when the Analysis pane plays it or when \
                                       its print is wanted to name it.";

pub(crate) const RECOGNITION_HINT: &str = "An AcoustID client key, which lets the lookup and the \
                                           Analysis pane send a fingerprint of the audio and hear \
                                           back what it is, and an AudD token, which Listen asks \
                                           beside Shazam. Each service gives one to anyone who \
                                           registers; nothing is sent to either without it.";

pub(crate) const LISTENING_HINT: &str = "What Listen records — the desktop's own output or a \
                                         microphone — and for how long, before it asks Shazam \
                                         and whatever else is keyed what the song is. Ctrl+L or \
                                         the ear in the header opens it.";

pub(crate) const CONTACT_HINT: &str = "What the User-Agent carries beside the name and the \
                                       version, so a service can say who to write to when a client \
                                       misbehaves. MusicBrainz asks for one; nothing here needs \
                                       it. It is read at start, so what is typed here is sent from \
                                       the next one.";

pub(crate) const LOOKUP_HINT: &str = "Asks the reference now rather than waiting for the next \
                                      scan. Look up asks only about what has not been answered; \
                                      Refresh all asks about everything again, at one paced \
                                      request per album and per artist.";

pub(crate) const NOTIFICATIONS_HINT: &str = "Whether a track starting raises a popup through the \
                                             desktop's notification server. One is never raised \
                                             while this window is the one in front, and a session \
                                             with no notification daemon has none either way.";

pub(crate) const DISCORD_HINT: &str = "Shows what is playing on your Discord profile, under an \
                                       application you register yourself. Off until you switch it \
                                       on and give it that application's id; nothing reaches \
                                       Discord before then.";

pub(crate) const DISCORD_SHOWS_HINT: &str = "How much Discord is told: just that you are \
                                             listening, the track, or the track and its album, \
                                             with the album's cover, your application's icon or \
                                             no picture, and a bar that follows the track.";

pub(crate) const COLOUR_HINT: &str = "The palette the whole window is painted in and the one colour it leads with — the play mark, \
     a selected row, a chosen chip, the position rail and every primary control. Every palette is \
     dark, and each draws each accent in its own tones, so the accent stays part of the theme \
     rather than sitting on top of it. A palette's strip shows its sidebar, its panes, what it \
     raises above them and the accent that would be worn.";

pub(crate) const LAYOUT_HINT: &str = "How large the window draws itself. The type scale and every \
                                      measure that holds text — a row, the sidebar, the header, a \
                                      control — are read off one size, so nothing is left behind \
                                      when it changes. The window's own chrome and its corners are \
                                      not, because those belong to the compositor rather than to \
                                      the type.";

pub(crate) const WINDOW_BUTTONS_HINT: &str = "Whether the titlebar this window draws for itself \
     carries a minimise and a maximise button beside the close one. A tiling compositor, or a \
     session where a key already does both, has no use for them. Close always stays, because it \
     is the one way out a pointer has, and a double press on the titlebar still fills the screen \
     and gives the window back either way.";

pub(crate) const VOLUME_WHEEL_HINT: &str = "Whether turning the wheel over the volume slider \
     moves the volume, a notch at a time. Off, the slider answers only to a press or a drag and \
     the keys.";

pub(crate) const SCROLLBARS_HINT: &str = "Whether the lists and panes that scroll draw a bar \
     down their edge that shows where the view stands and can be dragged. Off, they still scroll \
     with the wheel and the keys.";

pub(crate) const TABS_HINT: &str = "Which of the collection's panes the sidebar lists. \
     Suggestions offers lists made out of what the catalog holds, and Missing names what the \
     releases are short of and what the artists have put out that the library does not hold. A \
     pane that is hidden is left out of the sidebar and of the keys that step through it, and \
     nothing else leads to it.";

pub(crate) const BUILD_HINT: &str = "What this copy of Resonate is: its version, the faces it \
                                     settled on out of the families this machine has installed, \
                                     and whether the parts that are compiled in behind a feature \
                                     are here at all.";

pub(crate) const PLACES_HINT: &str = "The settings file every control here writes to, and the \
                                      SQLite catalog the library pane reads from. Neither is a \
                                      setting: the settings file is named by --config and the \
                                      catalog by --library or the library key inside it.";

pub(crate) const EVERYTHING_HINT: &str = "Takes every key this pane can write out of the settings \
                                          file, so each setting falls back to what Resonate was \
                                          built with. The music folders are untouched — they live \
                                          in the catalog rather than in the file — and so do the \
                                          files themselves.";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_group_stands_in_exactly_one_category_and_every_category_holds_one() {
        for category in Category::ALL {
            assert!(
                category.groups().count() > 0,
                "{} is a category with nothing in it",
                category.label()
            );
        }
        assert_eq!(
            Category::ALL
                .into_iter()
                .map(|category| category.groups().count())
                .sum::<usize>(),
            Group::ALL.len()
        );
    }

    #[test]
    fn no_two_groups_share_a_title_and_none_is_written_without_a_hint() {
        for group in Group::ALL {
            let sharing = Group::ALL
                .into_iter()
                .filter(|other| other.title() == group.title())
                .count();
            assert_eq!(
                sharing,
                1,
                "{} is titled as another group is",
                group.title()
            );
            assert!(
                group.hint().len() > 40,
                "{} explains itself in {} characters",
                group.title(),
                group.hint().len()
            );
        }
    }

    #[test]
    fn a_setting_is_written_under_one_group_and_no_key_is_claimed_twice() {
        for group in Group::ALL {
            for key in group.keys() {
                let claiming = Group::ALL
                    .into_iter()
                    .filter(|other| other.keys().contains(key))
                    .count();
                assert_eq!(
                    claiming,
                    1,
                    "{:?} is put back by {} and by another group as well",
                    key,
                    group.title()
                );
            }
        }
    }

    #[test]
    fn an_empty_query_narrows_nothing_and_allows_every_group() {
        let narrowing = Narrowing::of("   ");

        assert!(!narrowing.narrows());
        for group in Group::ALL {
            assert!(narrowing.allows(group));
        }
        assert_eq!(narrowing.anywhere().count(), Group::ALL.len());
    }

    #[test]
    fn a_query_reads_a_group_by_its_title_however_it_is_cased() {
        let narrowing = Narrowing::of("DITHER");

        assert!(narrowing.narrows());
        assert!(narrowing.allows(Group::Dither));
        assert!(
            narrowing.allows(Group::NoiseShaping),
            "the shaping hint names dither"
        );
        assert!(!narrowing.allows(Group::Colour));
    }

    #[test]
    fn a_group_is_found_by_a_word_nothing_on_screen_writes() {
        assert!(Narrowing::of("sink").allows(Group::Device));
        assert!(Narrowing::of("latency").allows(Group::Buffer));
        assert!(Narrowing::of("sacd").allows(Group::Dop));
        assert!(Narrowing::of("musicbrainz").allows(Group::Lookups));
        assert!(Narrowing::of("font").allows(Group::Build));
        assert!(Narrowing::of("maximize").allows(Group::WindowButtons));
        assert!(Narrowing::of("titlebar").allows(Group::WindowButtons));
    }

    #[test]
    fn every_word_has_to_land_for_a_group_to_be_allowed() {
        assert!(Narrowing::of("dither noise").allows(Group::Dither));
        assert!(!Narrowing::of("dither folders").allows(Group::Dither));
    }

    #[test]
    fn a_query_is_counted_against_every_category_rather_than_the_one_in_front() {
        let narrowing = Narrowing::of("rate");

        assert!(narrowing.found(Category::Output) >= 2);
        assert_eq!(narrowing.found(Category::Appearance), 0);
        assert_eq!(
            narrowing.anywhere().count(),
            Category::ALL
                .into_iter()
                .map(|category| narrowing.found(category))
                .sum::<usize>()
        );
    }

    #[test]
    fn a_query_nothing_answers_to_finds_nothing_anywhere() {
        let narrowing = Narrowing::of("chartreuse");

        assert_eq!(narrowing.anywhere().count(), 0);
        for category in Category::ALL {
            assert_eq!(narrowing.found(category), 0);
        }
    }
}
