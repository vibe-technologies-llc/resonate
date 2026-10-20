import ResonateNative

func track(
    _ id: UInt64,
    title: String = "A song",
    artist: String = "An artist",
    album: String = "An album",
    durationMs: UInt64 = 200_000,
    favourite: Bool = false,
    queueId: UInt64? = nil
) -> Track {
    Track(
        id: id,
        uri: "resonate-folder://local/\(id).flac",
        name: "\(id).flac",
        title: title,
        artist: artist,
        album: album,
        durationMs: durationMs,
        rate: 44_100,
        codec: "FLAC",
        favourite: favourite,
        albumArtist: artist,
        genre: "",
        date: "",
        trackNumber: nil,
        discNumber: nil,
        bits: 16,
        channels: 2,
        recording: nil,
        release: nil,
        artistMbid: nil,
        artRevision: 0,
        queueId: queueId
    )
}

func snapshot(
    _ transport: Transport = .paused,
    current: Track? = nil,
    queueIndex: UInt32 = 0,
    positionMs: UInt64 = 0,
    durationMs: UInt64 = 0,
    shuffle: Bool = false,
    repeat repeatMode: UInt32 = 0,
    queueRevision: UInt64 = 1,
    catalogRevision: UInt64 = 1
) -> Snapshot {
    Snapshot(
        transport: transport,
        current: current,
        queueIndex: queueIndex,
        positionMs: positionMs,
        durationMs: durationMs,
        volume: 1,
        shuffle: shuffle,
        repeat: repeatMode,
        outputRate: 0,
        underruns: 0,
        failure: nil,
        sleep: nil,
        sleepRemainingMs: nil,
        queueRevision: queueRevision,
        catalogRevision: catalogRevision
    )
}
