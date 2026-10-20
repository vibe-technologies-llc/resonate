import 'dart:math' as math;

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
    radius: AppGeometry.chromeRadius,
    child: Stack(
      children: [
        InkWell(
          onTap: onOpen,
          child: Padding(
            padding: const EdgeInsets.fromLTRB(
              AppSpace.m,
              AppSpace.xs,
              AppSpace.xs,
              AppSpace.xs,
            ),
            child: Row(
              children: [
                Artwork(
                  model: model,
                  track: model.current,
                  size: AppGeometry.miniArt,
                  radius: AppGeometry.thumbRadius,
                ),
                const SizedBox(width: AppSpace.m),
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
                          style: Theme.of(context).textTheme.titleSmall
                              ?.copyWith(fontWeight: FontWeight.w600),
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
        Positioned(
          left: AppGeometry.chromeRadius / 2,
          right: AppGeometry.chromeRadius / 2,
          bottom: 0,
          child: RepaintBoundary(
            child: ValueListenableBuilder<int>(
              valueListenable: model.positionMs,
              builder: (context, position, _) => Semantics(
                label: 'Song progress',
                value:
                    '${clock(position)} of ${clock(model.snapshot['durationMs'])}',
                child: LinearProgressIndicator(
                  value: integer(model.snapshot['durationMs']) > 0
                      ? (position / integer(model.snapshot['durationMs']))
                            .clamp(0, 1)
                      : 0,
                  minHeight: 2,
                  borderRadius: BorderRadius.circular(1),
                  backgroundColor: Colors.transparent,
                ),
              ),
            ),
          ),
        ),
      ],
    ),
  );
}

const _largePlay = 76.0;

class PlayButton extends StatelessWidget {
  const PlayButton({super.key, required this.model, this.large = false});
  final PlayerModel model;
  final bool large;
  @override
  Widget build(BuildContext context) => large
      ? FilledButton(
          style: FilledButton.styleFrom(
            shape: const CircleBorder(),
            fixedSize: const Size.square(_largePlay),
            minimumSize: const Size.square(_largePlay),
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
                size: 38,
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
            duration: const Duration(seconds: 4),
            persist: false,
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

class _NowPlayingState extends State<NowPlaying>
    with SingleTickerProviderStateMixin {
  String _mode = 'Artwork';
  List<Color> _colors = [accent, mint];
  late final AnimationController _animController;
  Animation<double>? _dragAnimation;
  double _dragOffset = 0.0;
  bool _dismissing = false;
  bool _presented = false;

  @override
  void initState() {
    super.initState();
    _animController = AnimationController(
      vsync: this,
      duration: const Duration(milliseconds: 240),
    );
    _animController.addListener(() {
      if (_dragAnimation != null) {
        setState(() => _dragOffset = _dragAnimation!.value);
      }
    });
  }

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    if (_presented) return;
    _presented = true;
    if (!MediaQuery.disableAnimationsOf(context)) {
      _dragOffset = MediaQuery.sizeOf(context).height;
      _snapBack(entering: true);
    }
  }

  @override
  void dispose() {
    _animController.dispose();
    super.dispose();
  }

  void _dismiss([double velocity = 0.0]) {
    if (_dismissing || !mounted) return;
    _dismissing = true;
    final screenHeight = MediaQuery.sizeOf(context).height;
    _dragAnimation = Tween<double>(begin: _dragOffset, end: screenHeight)
        .animate(
          CurvedAnimation(parent: _animController, curve: Curves.fastOutSlowIn),
        );
    final durationMs = velocity > 500 ? 160 : 240;
    _animController.duration = motion(context, durationMs);
    _animController.forward(from: 0.0).then((_) {
      if (mounted) Navigator.of(context).pop();
    });
  }

  void _snapBack({bool entering = false}) {
    if (_dismissing || !mounted) return;
    _dragAnimation = Tween<double>(begin: _dragOffset, end: 0.0).animate(
      CurvedAnimation(parent: _animController, curve: Curves.easeOutCubic),
    );
    _animController.duration = motion(context, entering ? 380 : 240);
    _animController.forward(from: 0.0);
  }

  void _onVerticalDragStart(DragStartDetails details) {
    if (_dismissing) return;
    _animController.stop();
  }

  void _onVerticalDragUpdate(DragUpdateDetails details) {
    if (_dismissing) return;
    final delta = details.primaryDelta ?? 0.0;
    if (delta > 0 || _dragOffset > 0) {
      setState(() {
        _dragOffset = math.max(0.0, _dragOffset + delta);
      });
    }
  }

  void _onVerticalDragEnd(DragEndDetails details) {
    if (_dismissing) return;
    final velocity = details.primaryVelocity ?? 0.0;
    if (velocity > 280 || _dragOffset > 100) {
      _dismiss(velocity);
    } else if (_dragOffset > 0) {
      _snapBack();
    }
  }

  @override
  Widget build(BuildContext context) => ListenableBuilder(
    listenable: widget.model,
    builder: (context, _) {
      final model = widget.model;
      final track = model.current;
      final compactModes =
          MediaQuery.sizeOf(context).width < 390 ||
          MediaQuery.textScalerOf(context).scale(14) > 18;
      final screenHeight = MediaQuery.sizeOf(context).height;
      final progress = (screenHeight > 0 ? _dragOffset / screenHeight : 0.0)
          .clamp(0.0, 1.0);
      final scale = (1.0 - progress * 0.12).clamp(0.88, 1.0);
      final topRadius = math.min(36.0, _dragOffset * 0.35);

      return PopScope(
        canPop: false,
        onPopInvokedWithResult: (didPop, _) {
          if (!didPop) _dismiss();
        },
        child: EdgeTint(
          top: true,
          strength: Curves.easeOutCubic.transform(1 - progress),
          tintStrength: .18,
          child: Transform.translate(
            offset: Offset(0, _dragOffset),
            child: Transform.scale(
              scale: scale,
              alignment: Alignment.topCenter,
              child: ClipRRect(
                borderRadius: BorderRadius.vertical(
                  top: Radius.circular(topRadius),
                ),
                child: Scaffold(
                  backgroundColor: Colors.transparent,
                  body: ArtworkAtmosphere(
                    model: model,
                    track: track,
                    immersive: true,
                    colors: _colors,
                    child: SafeArea(
                      child: Column(
                        children: [
                          GestureDetector(
                            behavior: HitTestBehavior.translucent,
                            onVerticalDragStart: _onVerticalDragStart,
                            onVerticalDragUpdate: _onVerticalDragUpdate,
                            onVerticalDragEnd: _onVerticalDragEnd,
                            child: Column(
                              children: [
                                Container(
                                  margin: const EdgeInsets.only(
                                    top: AppSpace.s,
                                    bottom: AppSpace.xs,
                                  ),
                                  width: 36,
                                  height: 5,
                                  decoration: ShapeDecoration(
                                    color: Theme.of(context)
                                        .colorScheme
                                        .onSurfaceVariant
                                        .withValues(alpha: .4),
                                    shape: const StadiumBorder(),
                                  ),
                                ),
                                Padding(
                                  padding: const EdgeInsets.symmetric(
                                    horizontal: AppSpace.m,
                                  ),
                                  child: Row(
                                    children: [
                                      ActionIcon(
                                        icon: Icons.keyboard_arrow_down_rounded,
                                        label: 'Close player',
                                        onPressed: () => _dismiss(),
                                      ),
                                      Expanded(
                                        child: Column(
                                          children: [
                                            Text(
                                              'NOW PLAYING',
                                              style: Theme.of(context)
                                                  .textTheme
                                                  .labelSmall
                                                  ?.copyWith(letterSpacing: 2),
                                            ),
                                            Text(
                                              '${track['album'] ?? 'Your music'}',
                                              maxLines: 1,
                                              overflow: TextOverflow.ellipsis,
                                              style: Theme.of(context)
                                                  .textTheme
                                                  .labelMedium,
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
                                                player: true,
                                              ),
                                      ),
                                    ],
                                  ),
                                ),
                              ],
                            ),
                          ),
                          StatusBanner(model: model, open: widget.open),
                          Expanded(
                            child: LayoutBuilder(
                              builder: (context, constraints) {
                                final compact = _mode != 'Artwork';
                                final artwork = Artwork(
                                  model: model,
                                  track: track,
                                  radius: compact
                                      ? AppGeometry.coverRadius
                                      : AppGeometry.heroRadius,
                                  size: compact
                                      ? 64
                                      : (constraints.maxWidth - 64).clamp(
                                          120,
                                          390,
                                        ),
                                  onColors: (colors) {
                                    if (mounted) {
                                      setState(() => _colors = colors);
                                    }
                                  },
                                );
                                if (compact) {
                                  return SingleChildScrollView(
                                    child: SizedBox(
                                      height: math.max(
                                        constraints.maxHeight,
                                        360 +
                                            MediaQuery.textScalerOf(context)
                                                .scale(100),
                                      ),
                                      child: Padding(
                                        padding: const EdgeInsets.fromLTRB(
                                          AppGeometry.pageInset,
                                          AppSpace.l,
                                          AppGeometry.pageInset,
                                          0,
                                        ),
                                        child: Column(
                                          children: [
                                            Row(
                                              children: [
                                                artwork,
                                                const SizedBox(
                                                  width: AppSpace.l,
                                                ),
                                                Expanded(
                                                  child: _PlayerTitles(
                                                    model: model,
                                                    open: widget.open,
                                                    compact: true,
                                                  ),
                                                ),
                                                FavouriteButton(
                                                  model: model,
                                                  track: track,
                                                ),
                                              ],
                                            ),
                                            const SizedBox(height: AppSpace.l),
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
                                                        key: const ValueKey(
                                                          'queue',
                                                        ),
                                                        model: model,
                                                        open: widget.open,
                                                        compact: true,
                                                      ),
                                              ),
                                            ),
                                            Scrubber(model: model),
                                            TransportControls(model: model),
                                            const SizedBox(height: AppSpace.m),
                                          ],
                                        ),
                                      ),
                                    ),
                                  );
                                }
                                return GestureDetector(
                                  behavior: HitTestBehavior.translucent,
                                  onVerticalDragStart: _onVerticalDragStart,
                                  onVerticalDragUpdate: _onVerticalDragUpdate,
                                  onVerticalDragEnd: _onVerticalDragEnd,
                                  child: SingleChildScrollView(
                                    physics: const ClampingScrollPhysics(),
                                    child: ConstrainedBox(
                                      constraints: BoxConstraints(
                                        minHeight: constraints.maxHeight,
                                      ),
                                      child: Padding(
                                        padding: const EdgeInsets.symmetric(
                                          horizontal: AppGeometry.pageInset,
                                          vertical: AppSpace.l,
                                        ),
                                        child: Column(
                                          mainAxisAlignment:
                                              MainAxisAlignment.spaceEvenly,
                                          children: [
                                            const SizedBox(height: AppSpace.m),
                                            GestureDetector(
                                              onHorizontalDragEnd: (details) {
                                                if ((details.primaryVelocity ??
                                                            0)
                                                        .abs() >
                                                    150) {
                                                  model.call(
                                                    details.primaryVelocity! < 0
                                                        ? 'next'
                                                        : 'previous',
                                                  );
                                                }
                                              },
                                              onVerticalDragStart:
                                                  _onVerticalDragStart,
                                              onVerticalDragUpdate:
                                                  _onVerticalDragUpdate,
                                              onVerticalDragEnd:
                                                  _onVerticalDragEnd,
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
                                                    borderRadius:
                                                        BorderRadius.circular(
                                                          AppGeometry
                                                              .heroRadius,
                                                        ),
                                                    boxShadow: [
                                                      BoxShadow(
                                                        color: Colors.black
                                                            .withValues(
                                                              alpha: .3,
                                                            ),
                                                        blurRadius: 28,
                                                        offset: const Offset(
                                                          0,
                                                          16,
                                                        ),
                                                      ),
                                                    ],
                                                  ),
                                                  child: artwork,
                                                ),
                                              ),
                                            ),
                                            const SizedBox(height: AppSpace.xl),
                                            Row(
                                              crossAxisAlignment:
                                                  CrossAxisAlignment.start,
                                              children: [
                                                Expanded(
                                                  child: _PlayerTitles(
                                                    model: model,
                                                    open: widget.open,
                                                  ),
                                                ),
                                                FavouriteButton(
                                                  model: model,
                                                  track: track,
                                                ),
                                              ],
                                            ),
                                            const SizedBox(height: AppSpace.l),
                                            Scrubber(
                                              model: model,
                                              format: true,
                                            ),
                                            const SizedBox(height: AppSpace.s),
                                            TransportControls(model: model),
                                            const SizedBox(height: AppSpace.l),
                                            Row(
                                              children: [
                                                Icon(
                                                  Icons.volume_down_rounded,
                                                  size: 20,
                                                  color: Theme.of(context)
                                                      .colorScheme
                                                      .onSurfaceVariant,
                                                ),
                                                Expanded(
                                                  child: CommitSlider(
                                                    capsule: true,
                                                    value: decimal(
                                                      model.snapshot['volume'],
                                                    ),
                                                    min: 0,
                                                    max: 1,
                                                    label: 'Volume',
                                                    onCommit: (v) => model.call(
                                                      'volume',
                                                      {'value': v},
                                                    ),
                                                  ),
                                                ),
                                                Icon(
                                                  Icons.volume_up_rounded,
                                                  size: 20,
                                                  color: Theme.of(context)
                                                      .colorScheme
                                                      .onSurfaceVariant,
                                                ),
                                              ],
                                            ),
                                          ],
                                        ),
                                      ),
                                    ),
                                  ),
                                );
                              },
                            ),
                          ),
                          Padding(
                            padding: const EdgeInsets.fromLTRB(
                              AppGeometry.pageInset,
                              AppSpace.xs,
                              AppGeometry.pageInset,
                              AppSpace.l,
                            ),
                            child: Glass(
                              radius: AppGeometry.chromeRadius,
                              padding: const EdgeInsets.all(AppSpace.xs),
                              child: Row(
                                children: [
                                  for (final entry in [
                                    ('Artwork', 'Art', Icons.album_outlined),
                                    ('Lyrics', 'Lyrics', Icons.lyrics_outlined),
                                    (
                                      'Queue',
                                      'Queue',
                                      Icons.queue_music_rounded,
                                    ),
                                    (
                                      'Sleep timer',
                                      'Sleep',
                                      Icons.bedtime_outlined,
                                    ),
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
                                            decoration: ShapeDecoration(
                                              shape: const StadiumBorder(),
                                              color:
                                                  (_mode == entry.$1 ||
                                                      (entry.$1 ==
                                                              'Sleep timer' &&
                                                          model.snapshot['sleep'] !=
                                                              null))
                                                  ? Theme.of(context)
                                                        .colorScheme
                                                        .primary
                                                        .withValues(alpha: .18)
                                                  : Colors.transparent,
                                            ),
                                            child: TextButton(
                                              style: TextButton.styleFrom(
                                                minimumSize: const Size.square(
                                                  AppGeometry.touchTarget,
                                                ),
                                                padding:
                                                    const EdgeInsets.symmetric(
                                                      horizontal: AppSpace.xs,
                                                    ),
                                              ),
                                              onPressed: () =>
                                                  entry.$1 == 'Sleep timer'
                                                  ? sleepTimer(context, model)
                                                  : setState(
                                                      () => _mode = entry.$1,
                                                    ),
                                              child: Row(
                                                mainAxisAlignment:
                                                    MainAxisAlignment.center,
                                                children: [
                                                  Icon(entry.$3, size: 20),
                                                  if (!compactModes) ...[
                                                    const SizedBox(
                                                      width: AppSpace.s,
                                                    ),
                                                    Flexible(
                                                      child: Text(
                                                        entry.$2,
                                                        maxLines: 1,
                                                      ),
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
                          ValueListenableBuilder<int?>(
                            valueListenable: model.sleepRemainingMs,
                            builder: (context, remaining, _) =>
                                remaining == null
                                ? const SizedBox.shrink()
                                : Padding(
                                    padding: const EdgeInsets.only(
                                      bottom: AppSpace.s,
                                    ),
                                    child: Row(
                                      mainAxisSize: MainAxisSize.min,
                                      children: [
                                        Icon(
                                          Icons.bedtime_rounded,
                                          size: 14,
                                          color: Theme.of(context)
                                              .colorScheme
                                              .onSurfaceVariant,
                                        ),
                                        const SizedBox(width: AppSpace.xs),
                                        Text(
                                          'Sleep in ${clock(remaining)}',
                                          style: Theme.of(context)
                                              .textTheme
                                              .bodySmall
                                              ?.copyWith(
                                                fontFeatures: tabularFigures,
                                              ),
                                        ),
                                      ],
                                    ),
                                  ),
                          ),
                        ],
                      ),
                    ),
                  ),
                ),
              ),
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
  Widget build(BuildContext context) {
    final text = Theme.of(context).textTheme;
    final scheme = Theme.of(context).colorScheme;
    final secondary = scheme.onSurface.withValues(alpha: .8);
    return AnimatedSwitcher(
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
            style: compact ? text.titleMedium : text.headlineSmall,
          ),
          InkWell(
            borderRadius: BorderRadius.circular(AppSpace.xs),
            onTap: () => open('collection', {
              'kind': 'artist',
              'key': model.current['artist'],
              'title': artist(model.current),
              'artworkTrack': model.current['id'],
            }),
            child: Padding(
              padding: EdgeInsets.symmetric(
                vertical: compact ? AppSpace.xs / 2 : AppSpace.xs,
              ),
              child: Text(
                artist(model.current),
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: (compact ? text.bodyMedium : text.titleLarge)?.copyWith(
                  fontWeight: FontWeight.w400,
                  color: secondary,
                ),
              ),
            ),
          ),
          if (!compact && '${model.current['album'] ?? ''}'.isNotEmpty)
            InkWell(
              borderRadius: BorderRadius.circular(AppSpace.xs),
              onTap: () => openAlbumOf(model, model.current, open),
              child: Text(
                '${model.current['album']}',
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
                style: text.bodyMedium?.copyWith(
                  color: scheme.onSurfaceVariant,
                ),
              ),
            ),
        ],
      ),
    );
  }
}

class Scrubber extends StatelessWidget {
  const Scrubber({super.key, required this.model, this.format = false});
  final PlayerModel model;
  final bool format;
  @override
  Widget build(BuildContext context) => ValueListenableBuilder<int>(
    valueListenable: model.positionMs,
    builder: (context, position, _) {
      final duration = decimal(model.snapshot['durationMs']);
      final track = model.current;
      final times = Theme.of(context).textTheme.bodySmall
          ?.copyWith(fontFeatures: tabularFigures);
      return Column(
        children: [
          CommitSlider(
            value: position.toDouble(),
            min: 0,
            max: duration > 0 ? duration : 1,
            label: 'Song progress',
            capsule: true,
            format: (value) => clock(value.toInt()),
            onCommit: (value) =>
                model.call('seek', {'position': value.toInt()}),
          ),
          Padding(
            padding: const EdgeInsets.symmetric(horizontal: AppSpace.xs),
            child: Row(
              children: [
                Text(clock(position), style: times),
                Expanded(
                  child: format
                      ? Center(
                          child: Pill(
                            model.waiting ??
                                '${track['codec'] ?? ''} · ${decimal(track['rate']) / 1000} kHz${track['bits'] == null ? '' : ' · ${track['bits']} bit'}',
                            icon: model.waiting == null
                                ? Icons.graphic_eq_rounded
                                : Icons.volume_off_rounded,
                          ),
                        )
                      : const SizedBox.shrink(),
                ),
                Text(
                  '-${clock((duration - position).clamp(0, double.infinity).toInt())}',
                  style: times,
                ),
              ],
            ),
          ),
        ],
      );
    },
  );
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
    this.capsule = false,
  });
  final double value, min, max;
  final String label;
  final ValueChanged<double> onCommit;
  final String Function(double)? format;
  final bool capsule;
  @override
  State<CommitSlider> createState() => _CommitSliderState();
}

class _CommitSliderState extends State<CommitSlider> {
  double? _draft;
  @override
  Widget build(BuildContext context) {
    final value = (_draft ?? widget.value).clamp(widget.min, widget.max);
    final slider = Slider(
      value: value,
      min: widget.min,
      max: widget.max,
      label: widget.format?.call(value) ?? value.toStringAsFixed(1),
      onChanged: (value) => setState(() => _draft = value),
      onChangeEnd: (value) {
        widget.onCommit(value);
        setState(() => _draft = null);
      },
    );
    if (!widget.capsule) {
      return Semantics(label: widget.label, child: slider);
    }
    final ink = Theme.of(context).colorScheme.onSurface;
    final dragging = _draft != null;
    return Semantics(
      label: widget.label,
      child: SliderTheme(
        data: SliderTheme.of(context).copyWith(
          trackHeight: dragging ? 14 : 7,
          activeTrackColor: ink.withValues(alpha: dragging ? .95 : .7),
          inactiveTrackColor: ink.withValues(alpha: .16),
          thumbColor: ink,
          overlayColor: Colors.transparent,
          trackShape: const RoundedRectSliderTrackShape(),
          thumbShape: const RoundSliderThumbShape(
            enabledThumbRadius: 0,
            disabledThumbRadius: 0,
            elevation: 0,
            pressedElevation: 0,
          ),
          overlayShape: const RoundSliderOverlayShape(overlayRadius: 14),
          activeTickMarkColor: Colors.transparent,
          inactiveTickMarkColor: Colors.transparent,
        ),
        child: slider,
      ),
    );
  }
}

class TransportControls extends StatelessWidget {
  const TransportControls({super.key, required this.model});
  final PlayerModel model;
  @override
  Widget build(BuildContext context) => Row(
    children: [
      for (final control in <Widget>[
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
      ])
        Expanded(child: Center(child: control)),
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
    final handoffs = rows(model.state['syncHandoffs']);
    final currentQueueId = '${model.current['queueId']}';
    final playing = queue.indexWhere(
      (t) => '${t['queueId']}' == currentQueueId,
    );
    final totalDurationMs = queue.fold<int>(
      0,
      (sum, t) => sum + integer(t['durationMs']),
    );

    return Column(
      children: [
        Padding(
          padding: EdgeInsets.fromLTRB(
            compact ? 0 : AppGeometry.pageInset,
            compact ? 0 : AppGeometry.pageTop,
            compact ? 0 : AppGeometry.pageInset,
            AppSpace.s,
          ),
          child: Row(
            children: [
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      compact ? 'Up next' : 'Queue',
                      style: compact
                          ? Theme.of(context).textTheme.titleLarge
                          : Theme.of(context).textTheme.headlineLarge,
                    ),
                    if (!compact && queue.isNotEmpty)
                      Text(
                        '${queue.length} ${queue.length == 1 ? 'song' : 'songs'} · ${clock(totalDurationMs)}',
                        style: Theme.of(context).textTheme.bodySmall,
                      ),
                  ],
                ),
              ),
              if (queue.isNotEmpty) ...[
                if (!compact)
                  ActionIcon(
                    label: 'Save queue as playlist',
                    icon: Icons.playlist_add_rounded,
                    onPressed: () => _saveAsPlaylist(context),
                  ),
                TextButton(
                  onPressed: () => model.call('clearQueue'),
                  child: const Text('Clear'),
                ),
              ],
            ],
          ),
        ),
        if (!compact && handoffs.isNotEmpty)
          Padding(
            padding: const EdgeInsets.symmetric(
              horizontal: AppGeometry.pageInset,
              vertical: AppSpace.xs,
            ),
            child: Card.filled(
              child: ListTile(
                leading: Icon(
                  Icons.devices_rounded,
                  color: Theme.of(context).colorScheme.primary,
                ),
                title: const Text('Continue listening'),
                subtitle: Text(
                  '${handoffs.length} ${handoffs.length == 1 ? 'device' : 'devices'} available',
                ),
                trailing: const Icon(Icons.chevron_right_rounded),
                onTap: () => _showHandoffs(context),
              ),
            ),
          ),
        Expanded(
          child: queue.isEmpty
              ? EmptyState(
                  icon: Icons.queue_music_rounded,
                  title: 'Your queue is empty',
                  detail: 'Songs you play, play next or add to the queue appear here.',
                  action: FilledButton(
                    onPressed: () =>
                        open('collection', {'kind': 'all', 'title': 'Songs'}),
                    child: const Text('Browse songs'),
                  ),
                )
              : ReorderableListView.builder(
                  buildDefaultDragHandles: false,
                  padding: EdgeInsets.only(
                    bottom: compact
                        ? AppSpace.l
                        : MediaQuery.paddingOf(context).bottom +
                              AppGeometry.pageInset,
                  ),
                  itemCount: queue.length,
                  onReorderItem: (from, to) {
                    model.call('moveInQueue', {
                      'from': from,
                      'to': queueLanding(from, to),
                    });
                  },
                  itemBuilder: (context, index) {
                    final track = queue[index];
                    final isPlaying = '${track['queueId']}' == currentQueueId;
                    final isHeard = playing >= 0 && index < playing;

                    Widget? sectionHeader;
                    if (playing > 0 && index == 0) {
                      sectionHeader = const _QueueHeader('History');
                    } else if (playing >= 0 && index == playing) {
                      sectionHeader = const _QueueHeader('Now playing');
                    } else if (playing >= 0 && index == playing + 1) {
                      sectionHeader = const _QueueHeader('Up next');
                    } else if (playing == -1 && index == 0) {
                      sectionHeader = const _QueueHeader('Up next');
                    }

                    final tile = SongTile(
                      model: model,
                      track: track,
                      onTap: () => model.call('jump', {'index': index}),
                      open: open,
                      trailing: Row(
                        mainAxisSize: MainAxisSize.min,
                        children: [
                          if (isPlaying)
                            Padding(
                              padding: const EdgeInsets.only(
                                right: AppSpace.xs,
                              ),
                              child: Icon(
                                Icons.graphic_eq_rounded,
                                color: Theme.of(context).colorScheme.primary,
                              ),
                            ),
                          ActionIcon(
                            icon: Icons.more_horiz_rounded,
                            label: 'More about ${title(track)}',
                            onPressed: () => songActions(
                              context,
                              model,
                              track,
                              open,
                              queueId: track['queueId'],
                            ),
                          ),
                          ReorderableDragStartListener(
                            index: index,
                            child: const Padding(
                              padding: EdgeInsets.all(AppSpace.m),
                              child: Icon(Icons.drag_handle_rounded),
                            ),
                          ),
                        ],
                      ),
                    );

                    final content = isHeard
                        ? Opacity(opacity: 0.55, child: tile)
                        : tile;

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
                        padding: const EdgeInsets.only(
                          right: AppGeometry.pageInset,
                        ),
                        child: Icon(
                          Icons.remove_circle_outline_rounded,
                          color: Theme.of(context).colorScheme.onErrorContainer,
                        ),
                      ),
                      child: sectionHeader != null
                          ? Column(
                              mainAxisSize: MainAxisSize.min,
                              crossAxisAlignment: CrossAxisAlignment.start,
                              children: [sectionHeader, content],
                            )
                          : content,
                    );
                  },
                ),
        ),
      ],
    );
  }

  void _showHandoffs(BuildContext context) {
    final handoffs = rows(model.state['syncHandoffs']);
    showModalBottomSheet<void>(
      context: context,
      showDragHandle: true,
      builder: (sheetContext) => SafeArea(
        top: false,
        child: ConstrainedBox(
          constraints: BoxConstraints(
            maxHeight: MediaQuery.sizeOf(sheetContext).height * .65,
          ),
          child: ListView(
            shrinkWrap: true,
            padding: const EdgeInsets.only(bottom: AppGeometry.pageInset),
            children: [
              const SheetTitle(
                'Continue listening',
                detail: 'Choose a device to continue its queue here.',
              ),
              for (final (index, handoff) in handoffs.indexed)
                Padding(
                  padding: const EdgeInsets.fromLTRB(
                    AppGeometry.pageInset,
                    0,
                    AppGeometry.pageInset,
                    AppSpace.s,
                  ),
                  child: Card.filled(
                    color: Theme.of(sheetContext)
                        .colorScheme
                        .surfaceContainerHigh,
                    child: ListTile(
                      leading: Icon(
                        Icons.devices_rounded,
                        color: Theme.of(sheetContext).colorScheme.primary,
                      ),
                      title: Text('${handoff['device']}'),
                      subtitle: Text(
                        '${handoff['songs']} ${integer(handoff['songs']) == 1 ? 'song' : 'songs'}',
                      ),
                      trailing: Icon(
                        Icons.play_arrow_rounded,
                        color: Theme.of(sheetContext).colorScheme.primary,
                      ),
                      onTap: () {
                        Navigator.pop(sheetContext);
                        model.call('handoff', {'index': index});
                      },
                    ),
                  ),
                ),
            ],
          ),
        ),
      ),
    );
  }

  Future<void> _saveAsPlaylist(BuildContext context) async {
    var draft = '';
    final name = await showDialog<String>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('Save queue as playlist'),
        content: TextFormField(
          initialValue: draft,
          onChanged: (value) => draft = value,
          autofocus: true,
          decoration: const InputDecoration(labelText: 'Playlist name'),
          onFieldSubmitted: (v) {
            if (v.trim().isNotEmpty) Navigator.pop(context, v.trim());
          },
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context),
            child: const Text('Cancel'),
          ),
          TextButton(
            onPressed: () {
              if (draft.trim().isNotEmpty) {
                Navigator.pop(context, draft.trim());
              }
            },
            child: const Text('Save'),
          ),
        ],
      ),
    );
    if (name != null && name.isNotEmpty) {
      await model.saveQueueAsPlaylist(name);
    }
  }
}

class _QueueHeader extends StatelessWidget {
  const _QueueHeader(this.title);
  final String title;
  @override
  Widget build(BuildContext context) => SectionTitle(
    title,
    padding: const EdgeInsets.fromLTRB(
      AppGeometry.pageInset,
      AppSpace.l,
      AppGeometry.pageInset,
      AppSpace.xs,
    ),
  );
}

Future<void> sleepTimer(BuildContext context, PlayerModel model) =>
    showModalBottomSheet<void>(
      context: context,
      showDragHandle: true,
      builder: (sheet) => SafeArea(
        child: ListView(
          shrinkWrap: true,
          children: [
            const SheetTitle('Sleep timer'),
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
                leading: Icon(
                  entry.key == 'cancel'
                      ? Icons.timer_off_outlined
                      : entry.key == 'track'
                      ? Icons.music_note_rounded
                      : Icons.queue_music_rounded,
                ),
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
