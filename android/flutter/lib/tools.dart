import 'dart:async';
import 'dart:ui' as ui;

import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart' show ScrollCacheExtent;

import 'model.dart';
import 'visuals.dart';

class LyricsPage extends StatelessWidget {
  const LyricsPage({super.key, required this.model, required this.track});
  final PlayerModel model;
  final RowData track;
  @override
  Widget build(BuildContext context) => ArtworkAtmosphere(
    model: model,
    track: track,
    immersive: true,
    child: Scaffold(
      backgroundColor: Colors.transparent,
      extendBody: true,
      extendBodyBehindAppBar: true,
      appBar: AppBar(
        backgroundColor: Colors.transparent,
        scrolledUnderElevation: 0,
        flexibleSpace: const EdgeTint(top: true, child: SizedBox.expand()),
        title: const Text('Lyrics'),
        actions: [
          ActionIcon(
            icon: Icons.refresh_rounded,
            label: 'Refresh lyrics',
            onPressed: track.isEmpty
                ? null
                : () => model.call('lyrics', {
                    'id': track['id'],
                    'refresh': true,
                  }),
          ),
        ],
      ),
      body: Padding(
        padding: EdgeInsets.fromLTRB(
          24,
          MediaQuery.paddingOf(context).top + kToolbarHeight,
          24,
          0,
        ),
        child: LyricsPane(model: model, track: track),
      ),
    ),
  );
}

const _lyricCacheExtent = 900.0;

class LyricsPane extends StatefulWidget {
  const LyricsPane({super.key, required this.model, required this.track});
  final PlayerModel model;
  final RowData track;
  @override
  State<LyricsPane> createState() => _LyricsPaneState();
}

class _LyricsPaneState extends State<LyricsPane> {
  final _scroll = ScrollController();
  int? _followed;
  bool _touching = false;
  Timer? _resume;
  final Map<int, GlobalKey> _keys = {};
  final Map<int, _LyricLine> _built = {};
  @override
  void initState() {
    super.initState();
    widget.model.call('lyricFrames', {'enabled': true});
    _load();
  }

  @override
  void didUpdateWidget(LyricsPane oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.track['id'] != widget.track['id']) _load();
  }

  void _load() {
    _followed = null;
    _touching = false;
    _resume?.cancel();
    _keys.clear();
    _built.clear();
    if (widget.track.isNotEmpty) {
      widget.model.call('lyrics', {'id': widget.track['id']});
    }
  }

  @override
  void dispose() {
    widget.model.call('lyricFrames', {'enabled': false});
    _resume?.cancel();
    _scroll.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => ListenableBuilder(
    listenable: Listenable.merge([
      widget.model,
      widget.model.hot('lyricFrame'),
    ]),
    builder: (context, _) {
      final sheet = row(widget.model.state['lyrics']);
      final lines = rows(sheet['lines']);
      if (widget.model.state['lyricsLoading'] == true) {
        return const Center(child: CircularProgressIndicator());
      }
      if (lines.isEmpty || sheet['trackId'] != widget.track['id']) {
        return const EmptyState(
          icon: Icons.lyrics_outlined,
          title: 'Let the music speak',
          detail: 'No lyrics are available for this song. Saved lyrics work offline; enable online features to search for more.',
        );
      }
      final isCurrent = widget.model.current['id'] == widget.track['id'];
      final frame = isCurrent
          ? row(widget.model.state['lyricFrame'])
          : <String, dynamic>{};
      final active = (frame['active'] as List? ?? []).map(integer).toSet();
      final followed = frame['readAt'] == null
          ? null
          : integer(frame['readAt']);
      if (followed != null && followed != _followed && !_touching) {
        _followed = followed;
        WidgetsBinding.instance.addPostFrameCallback((_) {
          if (!mounted || _touching) return;
          _reveal(followed, lines.length, motion(context, 500));
        });
      }
      final wipes = {
        for (final wipe in rows(frame['wipes'])) integer(wipe['line']): wipe,
      };
      final unsynced = sheet['detail'] == 'UNSYNCED';
      final blurAway =
          !MediaQuery.accessibleNavigationOf(context) &&
          !unsynced &&
          followed != null;
      return Stack(
        children: [
          ShaderMask(
            blendMode: BlendMode.dstIn,
            shaderCallback: (bounds) => const LinearGradient(
              begin: Alignment.topCenter,
              end: Alignment.bottomCenter,
              colors: [
                Colors.transparent,
                Colors.white,
                Colors.white,
                Colors.transparent,
              ],
              stops: [0, .08, .9, 1],
            ).createShader(bounds),
            child: NotificationListener<ScrollNotification>(
              onNotification: (notification) {
                if (notification is ScrollStartNotification &&
                    notification.dragDetails != null) {
                  setState(() => _touching = true);
                  _resume?.cancel();
                }
                if (notification is ScrollEndNotification && _touching) {
                  _resume?.cancel();
                  _resume = Timer(const Duration(seconds: 5), () {
                    if (mounted) {
                      setState(() {
                        _touching = false;
                        _followed = null;
                      });
                    }
                  });
                }
                return false;
              },
              child: ListView.builder(
                controller: _scroll,
                scrollCacheExtent: const ScrollCacheExtent.pixels(
                  _lyricCacheExtent,
                ),
                padding: const EdgeInsets.only(top: 48),
                itemCount: lines.length + 1,
                itemBuilder: (context, i) {
                  if (i == lines.length) {
                    return Padding(
                      padding: const EdgeInsets.only(top: 30, bottom: 100),
                      child: Text(
                        '${sheet['origin'] ?? ''}',
                        style: Theme.of(context).textTheme.labelSmall,
                      ),
                    );
                  }
                  final line = lines[i];
                  final lit = active.contains(i);
                  final fresh = _LyricLine(
                    model: widget.model,
                    text: '${line['text']}',
                    blank: line['blank'] == true,
                    rightVoice: integer(line['voice']) == 2,
                    lit: lit,
                    unsynced: unsynced,
                    blurred: blurAway && (i - followed).abs() > 2,
                    seekMs: isCurrent && line['atMs'] != null
                        ? integer(line['atMs'])
                        : null,
                    wipe: lit ? LyricWipe.of(wipes[i]) : null,
                  );
                  final kept = _built[i];
                  final shown = kept != null && kept.sameAs(fresh)
                      ? kept
                      : fresh;
                  _built[i] = shown;
                  return KeyedSubtree(
                    key: _keys.putIfAbsent(i, GlobalKey.new),
                    child: shown,
                  );
                },
              ),
            ),
          ),
          if (_touching && isCurrent && !unsynced)
            Positioned(
              bottom: 12,
              right: 0,
              child: Glass(
                radius: 24,
                child: TextButton.icon(
                  icon: const Icon(Icons.my_location_rounded, size: 18),
                  label: const Text('Follow lyrics'),
                  onPressed: () {
                    _resume?.cancel();
                    setState(() {
                      _touching = false;
                      _followed = null;
                    });
                  },
                ),
              ),
            ),
        ],
      );
    },
  );

  void _reveal(int line, int count, Duration duration, [int attempts = 3]) {
    final target = _keys[line]?.currentContext;
    if (target != null) {
      Scrollable.ensureVisible(
        target,
        alignment: .35,
        duration: duration,
        curve: Curves.easeOutCubic,
      );
      return;
    }
    if (attempts == 0 || count == 0 || !_scroll.hasClients) return;
    final position = _scroll.position;
    position.jumpTo(
      (position.maxScrollExtent * line / count).clamp(
        position.minScrollExtent,
        position.maxScrollExtent,
      ),
    );
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted && !_touching) {
        _reveal(line, count, duration, attempts - 1);
      }
    });
  }
}

class LyricWipe {
  const LyricWipe({
    required this.sung,
    required this.blendStart,
    required this.blendEnd,
    required this.share,
  });

  static LyricWipe? of(RowData? wipe) {
    if (wipe == null) return null;
    final blend = row(wipe['blend']);
    return LyricWipe(
      sung: integer(wipe['sungUtf16']),
      blendStart: integer(blend['startUtf16']),
      blendEnd: integer(blend['endUtf16']),
      share: decimal(blend['share']),
    );
  }

  final int sung;
  final int blendStart;
  final int blendEnd;
  final double share;

  @override
  bool operator ==(Object other) =>
      other is LyricWipe &&
      other.sung == sung &&
      other.blendStart == blendStart &&
      other.blendEnd == blendEnd &&
      other.share == share;

  @override
  int get hashCode => Object.hash(sung, blendStart, blendEnd, share);
}

class _LyricLine extends StatelessWidget {
  const _LyricLine({
    required this.model,
    required this.text,
    required this.blank,
    required this.rightVoice,
    required this.lit,
    required this.unsynced,
    required this.blurred,
    required this.seekMs,
    required this.wipe,
  });
  final PlayerModel model;
  final String text;
  final bool blank;
  final bool rightVoice;
  final bool lit;
  final bool unsynced;
  final bool blurred;
  final int? seekMs;
  final LyricWipe? wipe;

  bool sameAs(_LyricLine other) =>
      other.model == model &&
      other.text == text &&
      other.blank == blank &&
      other.rightVoice == rightVoice &&
      other.lit == lit &&
      other.unsynced == unsynced &&
      other.blurred == blurred &&
      other.seekMs == seekMs &&
      other.wipe == wipe;

  @override
  Widget build(BuildContext context) {
    final bright = lit || unsynced;
    return RepaintBoundary(
      child: Padding(
        padding: EdgeInsets.symmetric(vertical: blank ? 16 : 12),
        child: InkWell(
          onTap: seekMs == null
              ? null
              : () => model.call('seek', {'position': seekMs}),
          child: AnimatedScale(
            alignment: rightVoice
                ? Alignment.centerRight
                : Alignment.centerLeft,
            scale: lit ? 1 : .96,
            duration: motion(context, 350),
            curve: Curves.easeOutCubic,
            child: ImageFiltered(
              enabled: blurred,
              imageFilter: ui.ImageFilter.blur(sigmaX: .65, sigmaY: .65),
              child: AnimatedDefaultTextStyle(
                duration: motion(context, 260),
                style: Theme.of(context).textTheme.headlineMedium!.copyWith(
                  fontSize: 30,
                  fontWeight: FontWeight.w800,
                  letterSpacing: -.7,
                  height: 1.4,
                  color: Theme.of(context).colorScheme.onSurface
                      .withValues(alpha: bright ? 1 : .35),
                ),
                child: LyricWords(
                  text: text,
                  wipe: wipe == null
                      ? null
                      : {
                          'sungUtf16': wipe!.sung,
                          'blend': {
                            'startUtf16': wipe!.blendStart,
                            'endUtf16': wipe!.blendEnd,
                            'share': wipe!.share,
                          },
                        },
                  align: rightVoice ? TextAlign.right : TextAlign.left,
                ),
              ),
            ),
          ),
        ),
      ),
    );
  }
}

class LyricWords extends StatelessWidget {
  const LyricWords({
    super.key,
    required this.text,
    this.wipe,
    this.align = TextAlign.left,
  });
  final String text;
  final RowData? wipe;
  final TextAlign align;

  @override
  Widget build(BuildContext context) {
    if (wipe == null) return Text(text, textAlign: align);
    final color =
        DefaultTextStyle.of(context).style.color ??
        Theme.of(context).colorScheme.onSurface;
    final sung = integer(wipe!['sungUtf16']).clamp(0, text.length);
    final blend = row(wipe!['blend']);
    final start = integer(blend['startUtf16']).clamp(sung, text.length);
    final end = integer(blend['endUtf16']).clamp(start, text.length);
    final dim = color.withValues(alpha: .38);
    return Text.rich(
      TextSpan(
        children: [
          TextSpan(
            text: text.substring(0, sung),
            style: TextStyle(color: color),
          ),
          TextSpan(
            text: text.substring(sung, start),
            style: TextStyle(color: dim),
          ),
          TextSpan(
            text: text.substring(start, end),
            style: TextStyle(
              color: Color.lerp(
                dim,
                color,
                decimal(blend['share']).clamp(0, 1),
              ),
            ),
          ),
          TextSpan(
            text: text.substring(end),
            style: TextStyle(color: dim),
          ),
        ],
      ),
      textAlign: align,
      semanticsLabel: text,
    );
  }
}

class InspectorPage extends StatefulWidget {
  const InspectorPage({super.key, required this.model, required this.track});
  final PlayerModel model;
  final RowData track;
  @override
  State<InspectorPage> createState() => _InspectorPageState();
}

class _InspectorPageState extends State<InspectorPage> {
  @override
  void initState() {
    super.initState();
    if (widget.track.isNotEmpty) {
      widget.model.call('inspector', {'id': widget.track['id']});
    }
  }

  @override
  Widget build(BuildContext context) => ListenableBuilder(
    listenable: widget.model,
    builder: (_, _) {
      final held = row(widget.model.state['inspector']);
      final inspector = row(held['track'])['id'] == widget.track['id']
          ? held
          : <String, dynamic>{};
      return Scaffold(
        appBar: AppBar(title: const Text('Song details')),
        body: ListView(
          padding: const EdgeInsets.fromLTRB(24, 24, 24, 180),
          children: [
            Center(
              child: Artwork(
                model: widget.model,
                track: widget.track,
                size: 180,
                radius: 24,
              ),
            ),
            const SizedBox(height: 24),
            Text(
              title(widget.track),
              style: Theme.of(context).textTheme.headlineSmall,
            ),
            Text(artist(widget.track)),
            const SectionTitle('Audio'),
            ..._details(context, {
              'Format': inspector['container'],
              'Codec': widget.track['codec'],
              'Sample format': inspector['sampleFormat'],
              'Sample rate': '${widget.track['rate']} Hz',
              'Bit depth': widget.track['bits'],
              'Channels': widget.track['channels'],
              'Duration': clock(widget.track['durationMs']),
              'Seekable': inspector['seekable'],
            }),
            const SectionTitle('Tags'),
            ..._details(context, {
              'Album': widget.track['album'],
              'Album artist': widget.track['albumArtist'],
              'Genre': widget.track['genre'],
              'Date': widget.track['date'],
              'Track': widget.track['trackNumber'],
              'Disc': widget.track['discNumber'],
              'Encoder': inspector['encoder'],
              'Copyright': inspector['copyright'],
              'Comment': inspector['comment'],
              'ISRC': inspector['isrc'],
              'Recording': widget.track['recording'],
              'Release': widget.track['release'],
            }),
            TextButton(
              onPressed: () => widget.model.call('enrich', {
                'ids': [widget.track['id']],
              }),
              child: const Text('Find artwork and details'),
            ),
            if (widget.model.online)
              TextButton(
                onPressed: () =>
                    widget.model.call('artist', {'id': widget.track['id']}),
                child: const Text('Look up artist'),
              ),
            if (row(widget.model.state['artist']).isNotEmpty) ...[
              SectionTitle('${row(widget.model.state['artist'])['name']}'),
              ..._details(context, {
                'Type': row(widget.model.state['artist'])['kind'],
                'Country': row(widget.model.state['artist'])['country'],
                'Area': row(widget.model.state['artist'])['area'],
                'Genres':
                    (row(widget.model.state['artist'])['genres'] as List? ?? [])
                        .join(', '),
              }),
              for (final link
                  in row(widget.model.state['artist'])['links'] as List? ?? [])
                TextButton(
                  onPressed: () => widget.model.call('openUrl', {'url': link}),
                  child: Text('$link'),
                ),
            ],
          ],
        ),
      );
    },
  );
}

List<Widget> _details(BuildContext context, RowData values) => values.entries
    .where((e) => e.value != null && '${e.value}'.isNotEmpty)
    .map(
      (e) => Padding(
        padding: const EdgeInsets.symmetric(vertical: 10),
        child: Row(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            SizedBox(
              width: 115,
              child: Text(
                e.key,
                style: TextStyle(
                  color: Theme.of(context).colorScheme.onSurfaceVariant,
                ),
              ),
            ),
            Expanded(child: SelectableText('${e.value}')),
          ],
        ),
      ),
    )
    .toList();

class AnalysisPage extends StatefulWidget {
  const AnalysisPage({super.key, required this.model, required this.track});
  final PlayerModel model;
  final RowData track;
  @override
  State<AnalysisPage> createState() => _AnalysisPageState();
}

class _AnalysisPageState extends State<AnalysisPage> {
  @override
  Widget build(BuildContext context) => ListenableBuilder(
    listenable: widget.model,
    builder: (_, _) {
      final model = widget.model;
      final report = row(model.state['analysis']);
      final valid = report['trackId'] == widget.track['id'];
      final recognised = row(model.state['recognition']);
      final candidates = recognised['trackId'] == widget.track['id']
          ? rows(recognised['candidates'])
          : <RowData>[];
      final topInset = MediaQuery.paddingOf(context).top + kToolbarHeight;
      return Ambient(
        child: Scaffold(
          backgroundColor: Colors.transparent,
          extendBody: true,
          extendBodyBehindAppBar: true,
          appBar: AppBar(
            backgroundColor: Colors.transparent,
            scrolledUnderElevation: 0,
            flexibleSpace: const EdgeTint(top: true, child: SizedBox.expand()),
            title: const Text('Analysis'),
          ),
          body: widget.track.isEmpty
              ? const EmptyState(
                  icon: Icons.insights_rounded,
                  title: 'Look closer',
                  detail: 'Choose a song to inspect its audio quality.',
                )
              : ListView(
                  padding: EdgeInsets.fromLTRB(24, topInset + 24, 24, 180),
                  children: [
                    ListTile(
                      contentPadding: EdgeInsets.zero,
                      leading: Artwork(
                        model: model,
                        track: widget.track,
                        size: 64,
                      ),
                      title: Text(title(widget.track)),
                      subtitle: Text(artist(widget.track)),
                    ),
                    const SizedBox(height: 24),
                    FilledButton.icon(
                      onPressed: () => model.call(
                        model.state['analysisBusy'] == true
                            ? 'cancelAnalysis'
                            : 'analyse',
                        {'id': widget.track['id']},
                      ),
                      icon: Icon(
                        model.state['analysisBusy'] == true
                            ? Icons.stop_rounded
                            : Icons.insights_rounded,
                      ),
                      label: Text(
                        model.state['analysisBusy'] == true
                            ? 'Cancel analysis'
                            : 'Analyse original audio',
                      ),
                    ),
                    if (model.state['analysisBusy'] == true)
                      Padding(
                        padding: const EdgeInsets.symmetric(vertical: 20),
                        child: ValueListenableBuilder<Object?>(
                          valueListenable: model.hot('analysisProgress'),
                          builder: (_, progress, _) => LinearProgressIndicator(
                            value: progress == null ? null : decimal(progress),
                          ),
                        ),
                      ),
                    if (valid) ...[
                      SectionTitle('${report['verdict']}'),
                      ..._details(context, {
                        'Loudness': report['loudnessLufs'] == null
                            ? null
                            : '${report['loudnessLufs']} LUFS',
                        'Dynamic range': report['dynamicRange'],
                        'True peak': report['truePeak'],
                        'RMS': report['rms'],
                        'Clipped samples': report['clipped'],
                        'Bits in use': report['bitsInUse'],
                      }),
                      const SectionTitle('Waveform'),
                      SignalPlot(
                        values: (report['waveform'] as List? ?? [])
                            .map(decimal)
                            .toList(),
                        bars: true,
                      ),
                      const SectionTitle('Spectrum'),
                      SignalPlot(
                        values: (report['spectrum'] as List? ?? [])
                            .map((v) => (decimal(v) + 100) / 100)
                            .toList(),
                      ),
                      ...(report['findings'] as List? ?? []).map(
                        (finding) => Padding(
                          padding: const EdgeInsets.symmetric(vertical: 8),
                          child: Text('$finding'),
                        ),
                      ),
                    ],
                    const SectionTitle('Recognise this recording'),
                    TextFormField(
                      initialValue: '${model.state['recognitionKey'] ?? ''}',
                      obscureText: true,
                      decoration: const InputDecoration(
                        labelText: 'AcoustID key (optional)',
                      ),
                      onFieldSubmitted: (v) =>
                          model.call('recognitionKey', {'value': v}),
                    ),
                    const SizedBox(height: 12),
                    TextButton(
                      onPressed: model.state['recognitionBusy'] == true
                          ? null
                          : () => model.call('recognise', {
                              'id': widget.track['id'],
                            }),
                      child: Text(
                        model.state['recognitionBusy'] == true
                            ? 'Listening…'
                            : 'Recognise recording',
                      ),
                    ),
                    ...candidates.map((candidate) {
                      final song = row(candidate['song']);
                      return ListTile(
                        title: Text('${song['title']}'),
                        subtitle: Text('${song['artist']}'),
                      );
                    }),
                  ],
                ),
        ),
      );
    },
  );
}

class VisualizerPage extends StatefulWidget {
  const VisualizerPage({super.key, required this.model});
  final PlayerModel model;
  @override
  State<VisualizerPage> createState() => _VisualizerPageState();
}

class _VisualizerPageState extends State<VisualizerPage>
    with WidgetsBindingObserver {
  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    widget.model.call('visualize', {'enabled': true});
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    widget.model.call('visualize', {
      'enabled': state == AppLifecycleState.resumed,
    });
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    widget.model.call('visualize', {'enabled': false});
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => ListenableBuilder(
    listenable: widget.model,
    builder: (_, _) {
      final topInset = MediaQuery.paddingOf(context).top + kToolbarHeight;
      return Ambient(
        child: Scaffold(
          backgroundColor: Colors.transparent,
          extendBody: true,
          extendBodyBehindAppBar: true,
          appBar: AppBar(
            backgroundColor: Colors.transparent,
            scrolledUnderElevation: 0,
            flexibleSpace: const EdgeTint(top: true, child: SizedBox.expand()),
            title: const Text('Visualizer'),
          ),
          body: Center(
            child: Padding(
              padding: EdgeInsets.fromLTRB(32, topInset + 32, 32, 32),
              child: Column(
                mainAxisSize: MainAxisSize.min,
                children: [
                  ValueListenableBuilder<Object?>(
                    valueListenable: widget.model.hot('samples'),
                    builder: (_, samples, _) => SignalPlot(
                      values: (samples as List? ?? []).map(decimal).toList(),
                      height: 280,
                      bars: true,
                    ),
                  ),
                  const SizedBox(height: 24),
                  Text(
                    widget.model.current.isEmpty
                        ? 'Play a song to see it move'
                        : title(widget.model.current),
                    textAlign: TextAlign.center,
                    style: Theme.of(context).textTheme.titleLarge,
                  ),
                  const SizedBox(height: 8),
                  const Text('Live audio from the Resonate engine'),
                ],
              ),
            ),
          ),
        ),
      );
    },
  );
}

class WantedPane extends StatelessWidget {
  const WantedPane({super.key, required this.model});
  final PlayerModel model;
  @override
  Widget build(BuildContext context) {
    final wanted = rows(model.state['wanted']);
    return wanted.isEmpty
        ? const EmptyState(
            icon: Icons.bookmark_border_rounded,
            title: 'Keep a little inspiration',
            detail: 'Save songs from online search and find them here later.',
          )
        : RefreshIndicator(
            onRefresh: () async {
              await model.call('loadWanted');
            },
            child: ListView.builder(
              padding: const EdgeInsets.only(bottom: 180),
              itemCount: wanted.length,
              itemBuilder: (_, index) {
                final want = wanted[index];
                final song = row(want['song']);
                return ListTile(
                  leading: const Icon(Icons.bookmark_rounded),
                  title: Text('${song['title']}'),
                  subtitle: Text('${song['artist']} · ${song['album']}'),
                  trailing: PopupMenuButton<String>(
                    onSelected: (v) => v == 'remove'
                        ? model.call('removeWanted', {'id': want['id']})
                        : chooseDownload(context, model, {'id': want['id']}),
                    itemBuilder: (_) => [
                      const PopupMenuItem(
                        value: 'download',
                        child: Text('Download'),
                      ),
                      const PopupMenuItem(
                        value: 'remove',
                        child: Text('Remove'),
                      ),
                    ],
                  ),
                );
              },
            ),
          );
  }
}

Future<void> chooseDownload(
  BuildContext context,
  PlayerModel model,
  RowData song,
) => showModalBottomSheet<void>(
  context: context,
  showDragHandle: true,
  builder: (sheet) => SafeArea(
    child: ListView(
      shrinkWrap: true,
      children: [
        const ListTile(
          title: Text('Choose a music folder'),
          subtitle: Text(musicFolderGuidance),
        ),
        ListTile(
          leading: const Icon(Icons.download_rounded),
          title: const Text('Downloads'),
          subtitle: const Text('Save directly in your Downloads folder'),
          onTap: () {
            model.call('downloadToDownloads', song);
            Navigator.pop(sheet);
          },
        ),
        ...rows(model.state['folders'])
            .where(
              (f) =>
                  f['readable'] == true &&
                  f['writable'] == true &&
                  f['uri'] != 'content://media/external_primary/downloads',
            )
            .map(
              (folder) => ListTile(
                leading: const Icon(Icons.folder_outlined),
                title: Text('${folder['name']}'),
                onTap: () {
                  model.call('download', {...song, 'uri': folder['uri']});
                  Navigator.pop(sheet);
                },
              ),
            ),
        ListTile(
          leading: const Icon(Icons.add_rounded),
          title: const Text('Add music folder'),
          onTap: () {
            Navigator.pop(sheet);
            model.call('chooseFolder');
          },
        ),
      ],
    ),
  ),
);
