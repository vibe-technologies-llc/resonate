generic-name = Music Player
summary = Bit-perfect music playback through PipeWire
keywords = music;audio;player;flac;pipewire;bit-perfect;
what-it-is = Resonate is a music player for audio enthusiasts. It is a native PipeWire client rather than a PulseAudio or ALSA compatibility layer: it reads the rates a sink advertises, switches the graph to the source's own rate where the hardware takes it, and resamples with its own polyphase filters where it does not, so a lossless file reaches the device bit-accurate wherever the device allows it.
what-it-keeps = It scans a music collection into a library of its own, with albums, artists, playlists, a search grammar, lyrics read from sidecars and tags, and ReplayGain. It puts an MPRIS interface on the session bus, so media keys and the desktop's own player controls reach the transport.
what-it-reads = FLAC, ALAC, AAC, MP3, Vorbis, Opus, uncompressed PCM in WAV, AIFF and CAF, Matroska and MP4, and DSD in DSF and DFF are decoded; a cue sheet beside a single-file rip is read as the album it cuts.
