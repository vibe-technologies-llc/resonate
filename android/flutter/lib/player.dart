import 'package:flutter/material.dart';

import 'library.dart';
import 'main.dart' show OpenPage;
import 'model.dart';
import 'tools.dart' show LyricsPane;
import 'visuals.dart';

class MiniPlayer extends StatelessWidget {
  const MiniPlayer({super.key, required this.model, required this.onOpen});
  final PlayerModel model;
  final VoidCallback onOpen;
  @override
  Widget build(BuildContext context) => Glass(
    radius: 26,
    child: Column(
      mainAxisSize: MainAxisSize.min,
      children: [
        InkWell(
          onTap: onOpen,
          child: Padding(
            padding: const EdgeInsets.fromLTRB(10, 8, 6, 8),
            child: Row(
              children: [
                Hero(
                  tag: 'now-playing-art',
                  child: Artwork(
                    model: model,
                    track: model.current,
                    size: 48,
                    radius: 14,
                  ),
                ),
                const SizedBox(width: 12),
                Expanded(
                  child: AnimatedSwitcher(
                    duration: motion(context, 200),
                    child: Column(
                      key: ValueKey(model.current['id']),
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        Text(
                          title(model.current),
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: const TextStyle(fontWeight: FontWeight.w700),
                        ),
                        Text(
                          artist(model.current),
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: Theme.of(context).textTheme.bodySmall,
                        ),
                      ],
                    ),
                  ),
                ),
                if (MediaQuery.sizeOf(context).width > 380)
                  FavouriteButton(model: model, track: model.current),
                ActionIcon(
                  icon: Icons.skip_previous_rounded,
                  label: 'Previous',
                  onPressed: () => model.call('previous'),
                ),
                PlayButton(model: model),
                ActionIcon(
                  icon: Icons.skip_next_rounded,
                  label: 'Next',
                  onPressed: () => model.call('next'),
                ),
              ],
            ),
          ),
        ),
        Semantics(
          label: 'Song progress',
          value:
              '${clock(model.snapshot['positionMs'])} of ${clock(model.snapshot['durationMs'])}',
          child: LinearProgressIndicator(
            value: integer(model.snapshot['durationMs']) > 0
                ? (integer(model.snapshot['positionMs']) /
                          integer(model.snapshot['durationMs']))
                      .clamp(0, 1)
                : 0,
            minHeight: 2,
            backgroundColor: Colors.transparent,
          ),
        ),
      ],
    ),
  );
}

class PlayButton extends StatelessWidget {
  const PlayButton({super.key, required this.model, this.large = false});
  final PlayerModel model;
  final bool large;
  @override
  Widget build(BuildContext context) => large
      ? FilledButton(
          style: FilledButton.styleFrom(
            shape: const CircleBorder(),
            fixedSize: const Size(80, 80),
            padding: EdgeInsets.zero,
          ),
          onPressed: model.current.isEmpty ? null : () => model.call('toggle'),
          child: Semantics(
            label: model.playing ? 'Pause' : 'Play',
            child: AnimatedSwitcher(
              duration: motion(context, 200),
              child: Icon(
                model.playing ? Icons.pause_rounded : Icons.play_arrow_rounded,
                key: ValueKey(model.playing),
                size: 40,
              ),
            ),
          ),
        )
      : ActionIcon(
          icon: model.playing ? Icons.pause_rounded : Icons.play_arrow_rounded,
          label: model.playing ? 'Pause' : 'Play',
          onPressed: () => model.call('toggle'),
        );
}

class FavouriteButton extends StatelessWidget {
  const FavouriteButton({super.key, required this.model, required this.track});
  final PlayerModel model;
  final RowData track;
  @override
  Widget build(BuildContext context) => ActionIcon(
    icon: track['favourite'] == true
        ? Icons.favorite_rounded
        : Icons.favorite_border_rounded,
    label: track['favourite'] == true
        ? 'Remove from favourites'
        : 'Add to favourites',
    selected: track['favourite'] == true,
    onPressed: () {
      final enabled = track['favourite'] != true;
      model.call('favourite', {'id': track['id'], 'enabled': enabled});
      if (enabled) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(
            content: const Text('Added to favourites'),
            behavior: SnackBarBehavior.floating,
            action: SnackBarAction(
              label: 'Add to playlist',
              onPressed: () => playlistPicker(context, model, [track]),
            ),
          ),
        );
      }
    },
  );
}

class NowPlaying extends StatefulWidget {
  const NowPlaying({super.key, required this.model, required this.open});
  final PlayerModel model;
  final OpenPage open;
  @override
  State<NowPlaying> createState() => _NowPlayingState();
}

class _NowPlayingState extends State<NowPlaying> {
  String _mode = 'Artwork';
  List<Color> _colors = [accent, mint];
  @override
  Widget build(BuildContext context) => ListenableBuilder(
    listenable: widget.model,
    builder: (context, _) {
      final model = widget.model;
      final track = model.current;
      final compactModes =
          MediaQuery.sizeOf(context).width < 390 ||
          MediaQuery.textScalerOf(context).scale(14) > 18;
      return Scaffold(
        body: ArtworkAtmosphere(
          model: model,
          track: track,
          immersive: true,
          colors: _colors,
          child: SafeArea(
            child: Column(
              children: [
                Padding(
                  padding: const EdgeInsets.symmetric(horizontal: 12),
                  child: Row(
                    children: [
                      ActionIcon(
                        icon: Icons.keyboard_arrow_down_rounded,
                        label: 'Close player',
                        onPressed: () => Navigator.pop(context),
                      ),
                      Expanded(
                        child: Column(
                          children: [
                            Text(
                              'NOW PLAYING',
                              style: Theme.of(context).textTheme.labelSmall
                                  ?.copyWith(letterSpacing: 2),
                            ),
                            Text(
                              '${track['album'] ?? 'Your music'}',
                              maxLines: 1,
                              overflow: TextOverflow.ellipsis,
                              style: Theme.of(context).textTheme.labelMedium,
                            ),
                          ],
                        ),
                      ),
                      ActionIcon(
                        icon: Icons.more_horiz_rounded,
                        label: 'Player options',
                        onPressed: track.isEmpty
                            ? null
                            : () => songActions(
                                context,
                                model,
                                track,
                                widget.open,
                              ),
                      ),
                    ],
                  ),
                ),
                Expanded(
                  child: LayoutBuilder(
                    builder: (context, constraints) {
                      final compact = _mode != 'Artwork';
                      final artwork = Hero(
                        tag: 'now-playing-art',
                        child: Artwork(
                          model: model,
                          track: track,
                          radius: compact ? 14 : 26,
                          size: compact
                              ? 64
                              : (constraints.maxWidth - 64).clamp(120, 390),
                          onColors: (colors) {
                            if (mounted) setState(() => _colors = colors);
                          },
                        ),
                      );
                      if (compact) {
                        return Padding(
                          padding: const EdgeInsets.fromLTRB(24, 20, 24, 0),
                          child: Column(
                            children: [
                              Row(
                                children: [
                                  artwork,
                                  const SizedBox(width: 16),
                                  Expanded(
                                    child: _PlayerTitles(
                                      model: model,
                                      open: widget.open,
                                      compact: true,
                                    ),
                                  ),
                                  FavouriteButton(model: model, track: track),
                                ],
                              ),
                              const SizedBox(height: 16),
                              Expanded(
                                child: AnimatedSwitcher(
                                  duration: motion(context, 240),
                                  child: _mode == 'Lyrics'
                                      ? LyricsPane(
                                          key: ValueKey(
                                            'lyrics:${track['id']}',
                                          ),
                                          model: model,
                                          track: track,
                                        )
                                      : QueuePage(
                                          key: const ValueKey('queue'),
                                          model: model,
                                          open: widget.open,
                                          compact: true,
                                        ),
                                ),
                              ),
                              Scrubber(model: model),
                              TransportControls(model: model),
                              const SizedBox(height: 12),
                            ],
                          ),
                        );
                      }
                      return SingleChildScrollView(
                        child: ConstrainedBox(
                          constraints: BoxConstraints(
                            minHeight: constraints.maxHeight,
                          ),
                          child: Padding(
                            padding: const EdgeInsets.symmetric(
                              horizontal: 28,
                              vertical: 20,
                            ),
                            child: Column(
                              mainAxisAlignment: MainAxisAlignment.spaceEvenly,
                              children: [
                                const SizedBox(height: 12),
                                GestureDetector(
                                  onHorizontalDragEnd: (details) {
                                    if ((details.primaryVelocity ?? 0).abs() >
                                        150) {
                                      model.call(
                                        details.primaryVelocity! < 0
                                            ? 'next'
                                            : 'previous',
                                      );
                                    }
                                  },
                                  child: AnimatedScale(
                                    scale:
                                        model.playing ||
                                            MediaQuery.disableAnimationsOf(
                                              context,
                                            )
                                        ? 1
                                        : .87,
                                    duration: motion(context, 500),
                                    curve: Curves.easeOutBack,
                                    child: DecoratedBox(
                                      decoration: BoxDecoration(
                                        borderRadius: BorderRadius.circular(26),
                                        boxShadow: [
                                          BoxShadow(
                                            color: _colors.first.withValues(
                                              alpha: .24,
                                            ),
                                            blurRadius: 48,
                                            offset: const Offset(0, 24),
                                          ),
                                        ],
                                      ),
                                      child: artwork,
                                    ),
                                  ),
                                ),
                                const SizedBox(height: 28),
                                Row(
                                  children: [
                                    Expanded(
                                      child: _PlayerTitles(
                                        model: model,
                                        open: widget.open,
                                      ),
                                    ),
                                    FavouriteButton(model: model, track: track),
                                  ],
                                ),
                                const SizedBox(height: 18),
                                Scrubber(model: model),
                                const SizedBox(height: 8),
                                TransportControls(model: model),
                                const SizedBox(height: 16),
                                Row(
                                  mainAxisAlignment: MainAxisAlignment.center,
                                  children: [
                                    Icon(
                                      Icons.graphic_eq_rounded,
                                      size: 14,
                                      color: Theme.of(context)
                                          .colorScheme
                                          .primary,
                                    ),
                                    const SizedBox(width: 6),
                                    Flexible(
                                      child: Text(
                                        '${track['codec'] ?? ''} · ${decimal(track['rate']) / 1000} kHz${track['bits'] == null ? '' : ' · ${track['bits']} bit'}',
                                        style: Theme.of(context)
                                            .textTheme
                                            .labelSmall,
                                        textAlign: TextAlign.center,
                                      ),
                                    ),
                                  ],
                                ),
                                const SizedBox(height: 16),
                                Row(
                                  children: [
                                    const Icon(
                                      Icons.volume_down_rounded,
                                      size: 20,
                                    ),
                                    Expanded(
                                      child: CommitSlider(
                                        value: decimal(
                                          model.snapshot['volume'],
                                        ),
                                        min: 0,
                                        max: 1,
                                        label: 'Volume',
                                        onCommit: (v) =>
                                            model.call('volume', {'value': v}),
                                      ),
                                    ),
                                    const Icon(
                                      Icons.volume_up_rounded,
                                      size: 20,
                                    ),
                                  ],
                                ),
                              ],
                            ),
                          ),
                        ),
                      );
                    },
                  ),
                ),
                Padding(
                  padding: const EdgeInsets.fromLTRB(24, 4, 24, 16),
                  child: Glass(
                    radius: 32,
                    padding: const EdgeInsets.all(5),
                    child: Row(
                      children: [
                        for (final entry in [
                          ('Artwork', 'Art', Icons.album_outlined),
                          ('Lyrics', 'Lyrics', Icons.lyrics_outlined),
                          ('Queue', 'Queue', Icons.queue_music_rounded),
                          ('Sleep timer', 'Sleep', Icons.bedtime_outlined),
                        ])
                          Expanded(
                            child: Tooltip(
                              message: entry.$1,
                              child: Semantics(
                                selected: entry.$1 == 'Sleep timer'
                                    ? model.snapshot['sleep'] != null
                                    : _mode == entry.$1,
                                child: AnimatedContainer(
                                  duration: motion(context, 220),
                                  decoration: BoxDecoration(
                                    borderRadius: BorderRadius.circular(26),
                                    color:
                                        (_mode == entry.$1 ||
                                            (entry.$1 == 'Sleep timer' &&
                                                model.snapshot['sleep'] !=
                                                    null))
                                        ? Theme.of(context).colorScheme.primary
                                              .withValues(alpha: .2)
                                        : Colors.transparent,
                                  ),
                                  child: TextButton(
                                    style: TextButton.styleFrom(
                                      minimumSize: const Size(48, 48),
                                      padding: const EdgeInsets.symmetric(
                                        horizontal: 4,
                                      ),
                                    ),
                                    onPressed: () => entry.$1 == 'Sleep timer'
                                        ? sleepTimer(context, model)
                                        : setState(() => _mode = entry.$1),
                                    child: Row(
                                      mainAxisAlignment:
                                          MainAxisAlignment.center,
                                      children: [
                                        Icon(entry.$3, size: 20),
                                        if (!compactModes) ...[
                                          const SizedBox(width: 5),
                                          Flexible(
                                            child: Text(entry.$2, maxLines: 1),
                                          ),
                                        ],
                                      ],
                                    ),
                                  ),
                                ),
                              ),
                            ),
                          ),
                      ],
                    ),
                  ),
                ),
                if (model.snapshot['sleepRemainingMs'] != null)
                  Padding(
                    padding: const EdgeInsets.only(bottom: 8),
                    child: Text(
                      'Sleep in ${clock(model.snapshot['sleepRemainingMs'])}',
                      style: Theme.of(context).textTheme.labelSmall,
                    ),
                  ),
              ],
            ),
          ),
        ),
      );
    },
  );
}

class _PlayerTitles extends StatelessWidget {
  const _PlayerTitles({
    required this.model,
    required this.open,
    this.compact = false,
  });
  final PlayerModel model;
  final OpenPage open;
  final bool compact;
  @override
  Widget build(BuildContext context) => AnimatedSwitcher(
    layoutBuilder: (current, previous) => Stack(
      alignment: Alignment.centerLeft,
      children: [...previous, ?current],
    ),
    duration: motion(context, 220),
    child: Column(
      key: ValueKey(model.current['id']),
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(
          title(model.current),
          maxLines: 2,
          overflow: TextOverflow.ellipsis,
          style:
              (compact
                      ? Theme.of(context).textTheme.titleMedium
                      : Theme.of(context).textTheme.headlineSmall)
                  ?.copyWith(fontWeight: FontWeight.w700, letterSpacing: -.5),
        ),
        InkWell(
          onTap: () => open('collection', {
            'kind': 'artist',
            'key': model.current['artist'],
            'title': artist(model.current),
            'artworkTrack': model.current['id'],
          }),
          child: Padding(
            padding: const EdgeInsets.symmetric(vertical: 6),
            child: Text(
              artist(model.current),
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
              style: TextStyle(
                fontSize: compact ? 13 : 17,
                color: Theme.of(context).colorScheme.primary,
              ),
            ),
          ),
        ),
        if (!compact && '${model.current['album'] ?? ''}'.isNotEmpty)
          InkWell(
            onTap: () {
              final album = model.albums
                  .where((a) => a['title'] == model.current['album'])
                  .firstOrNull;
              if (album != null) {
                open('collection', {...album, 'kind': 'album'});
              }
            },
            child: Text(
              '${model.current['album']}',
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
              style: TextStyle(
                color: Theme.of(context).colorScheme.onSurfaceVariant,
              ),
            ),
          ),
      ],
    ),
  );
}

class Scrubber extends StatelessWidget {
  const Scrubber({super.key, required this.model});
  final PlayerModel model;
  @override
  Widget build(BuildContext context) {
    final duration = decimal(model.snapshot['durationMs']);
    return Column(
      children: [
        CommitSlider(
          value: decimal(model.snapshot['positionMs']),
          min: 0,
          max: duration > 0 ? duration : 1,
          label: 'Song progress',
          format: (value) => clock(value.toInt()),
          onCommit: (value) => model.call('seek', {'position': value.toInt()}),
        ),
        Row(
          mainAxisAlignment: MainAxisAlignment.spaceBetween,
          children:
              [
                    Text(clock(model.snapshot['positionMs'])),
                    Text(
                      '-${clock((duration - decimal(model.snapshot['positionMs'])).clamp(0, double.infinity).toInt())}',
                    ),
                  ]
                  .map(
                    (child) => DefaultTextStyle(
                      style: Theme.of(context).textTheme.labelSmall!.copyWith(
                        color: Theme.of(context).colorScheme.onSurfaceVariant,
                      ),
                      child: child,
                    ),
                  )
                  .toList(),
        ),
      ],
    );
  }
}

class CommitSlider extends StatefulWidget {
  const CommitSlider({
    super.key,
    required this.value,
    required this.min,
    required this.max,
    required this.label,
    required this.onCommit,
    this.format,
  });
  final double value, min, max;
  final String label;
  final ValueChanged<double> onCommit;
  final String Function(double)? format;
  @override
  State<CommitSlider> createState() => _CommitSliderState();
}

class _CommitSliderState extends State<CommitSlider> {
  double? _draft;
  @override
  Widget build(BuildContext context) {
    final value = (_draft ?? widget.value).clamp(widget.min, widget.max);
    return Semantics(
      label: widget.label,
      child: Slider(
        value: value,
        min: widget.min,
        max: widget.max,
        label: widget.format?.call(value) ?? value.toStringAsFixed(1),
        onChanged: (value) => setState(() => _draft = value),
        onChangeEnd: (value) {
          widget.onCommit(value);
          setState(() => _draft = null);
        },
      ),
    );
  }
}

class TransportControls extends StatelessWidget {
  const TransportControls({super.key, required this.model});
  final PlayerModel model;
  @override
  Widget build(BuildContext context) => Row(
    mainAxisAlignment: MainAxisAlignment.spaceEvenly,
    children: [
      ActionIcon(
        icon: Icons.shuffle_rounded,
        label: 'Shuffle',
        selected: model.snapshot['shuffle'] == true,
        onPressed: () => model.call('shuffle', {
          'enabled': model.snapshot['shuffle'] != true,
        }),
      ),
      ActionIcon(
        icon: Icons.skip_previous_rounded,
        label: 'Previous',
        size: 34,
        onPressed: () => model.call('previous'),
      ),
      PlayButton(model: model, large: true),
      ActionIcon(
        icon: Icons.skip_next_rounded,
        label: 'Next',
        size: 34,
        onPressed: () => model.call('next'),
      ),
      ActionIcon(
        icon: integer(model.snapshot['repeat']) == 1
            ? Icons.repeat_one_rounded
            : Icons.repeat_rounded,
        label:
            'Repeat: ${['off', 'song', 'queue'][integer(model.snapshot['repeat']).clamp(0, 2)]}',
        selected: integer(model.snapshot['repeat']) > 0,
        onPressed: () => model.call('repeat', {
          'mode': (integer(model.snapshot['repeat']) + 1) % 3,
        }),
      ),
    ],
  );
}

class QueuePage extends StatelessWidget {
  const QueuePage({
    super.key,
    required this.model,
    required this.open,
    this.compact = false,
  });
  final PlayerModel model;
  final OpenPage open;
  final bool compact;
  @override
  Widget build(BuildContext context) {
    final queue = model.queue;
    return Column(
      children: [
        Padding(
          padding: EdgeInsets.fromLTRB(
            compact ? 0 : 24,
            compact ? 0 : 20,
            12,
            10,
          ),
          child: Row(
            children: [
              Expanded(
                child: Text(
                  'Up next',
                  style: compact
                      ? Theme.of(context).textTheme.titleLarge
                      : Theme.of(context).textTheme.headlineLarge
                            ?.copyWith(fontWeight: FontWeight.w800),
                ),
              ),
              if (queue.isNotEmpty)
                TextButton(
                  onPressed: () => model.call('clearQueue'),
                  child: const Text('Clear'),
                ),
            ],
          ),
        ),
        if (!compact)
          ...rows(model.state['syncHandoffs']).indexed.map(
            (entry) => ListTile(
              leading: const Icon(Icons.devices_rounded),
              title: const Text('Continue listening'),
              subtitle: Text('${entry.$2['device']}'),
              trailing: TextButton(
                onPressed: () => model.call('handoff', {'index': entry.$1}),
                child: const Text('Continue'),
              ),
            ),
          ),
        Expanded(
          child: queue.isEmpty
              ? const EmptyState(
                  icon: Icons.queue_music_rounded,
                  title: 'A fresh start',
                  detail: 'Play a song or add music to your queue.',
                )
              : ReorderableListView.builder(
                  buildDefaultDragHandles: false,
                  itemCount: queue.length,
                  onReorderItem: (from, to) =>
                      model.call('moveInQueue', {'from': from, 'to': to}),
                  itemBuilder: (context, index) {
                    final track = queue[index];
                    return Dismissible(
                      key: ValueKey('queue:${track['queueId']}'),
                      direction: DismissDirection.endToStart,
                      confirmDismiss: (_) async {
                        await model.call('removeFromQueue', {
                          'id': track['queueId'],
                        });
                        return false;
                      },
                      background: Container(
                        color: Theme.of(context).colorScheme.errorContainer,
                        alignment: Alignment.centerRight,
                        padding: const EdgeInsets.only(right: 20),
                        child: const Icon(Icons.remove_circle_outline_rounded),
                      ),
                      child: SongTile(
                        model: model,
                        track: track,
                        onTap: () => model.call('jump', {'index': index}),
                        open: open,
                        trailing: Row(
                          mainAxisSize: MainAxisSize.min,
                          children: [
                            if ('${track['queueId']}' ==
                                '${model.current['queueId']}')
                              Icon(
                                Icons.graphic_eq_rounded,
                                color: Theme.of(context).colorScheme.primary,
                              ),
                            ReorderableDragStartListener(
                              index: index,
                              child: const Padding(
                                padding: EdgeInsets.all(12),
                                child: Icon(Icons.drag_handle_rounded),
                              ),
                            ),
                          ],
                        ),
                      ),
                    );
                  },
                ),
        ),
      ],
    );
  }
}

Future<void> sleepTimer(BuildContext context, PlayerModel model) =>
    showModalBottomSheet<void>(
      context: context,
      showDragHandle: true,
      builder: (sheet) => SafeArea(
        child: ListView(
          shrinkWrap: true,
          children: [
            const ListTile(
              title: Text(
                'Sleep timer',
                style: TextStyle(fontSize: 24, fontWeight: FontWeight.w700),
              ),
            ),
            for (final minutes in [15, 30, 45, 60, 90])
              ListTile(
                leading: const Icon(Icons.bedtime_outlined),
                title: Text('$minutes minutes'),
                onTap: () {
                  model.call('sleep', {
                    'kind': 'after',
                    'seconds': minutes * 60,
                  });
                  Navigator.pop(sheet);
                },
              ),
            for (final entry in {
              'track': 'End of song',
              'queue': 'End of queue',
              'cancel': 'Cancel timer',
            }.entries)
              ListTile(
                title: Text(entry.value),
                onTap: () {
                  model.call('sleep', {'kind': entry.key});
                  Navigator.pop(sheet);
                },
              ),
          ],
        ),
      ),
    );
