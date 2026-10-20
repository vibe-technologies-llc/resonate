import 'dart:math' as math;
import 'dart:ui' as ui;

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import 'main.dart' show OpenPage;
import 'model.dart';

const accent = Color(0xFFB9AAFF);
const mint = Color(0xFF9AE5CD);

abstract final class AppGeometry {
  static const pageInset = 24.0;
  static const pageTop = 20.0;
  static const cardRadius = 20.0;
  static const controlRadius = 16.0;
  static const chromeRadius = 24.0;
  static const touchTarget = 48.0;
}

EdgeInsets pageInsets(
  BuildContext context, {
  double top = AppGeometry.pageTop,
}) => EdgeInsets.fromLTRB(
  AppGeometry.pageInset,
  top,
  AppGeometry.pageInset,
  MediaQuery.paddingOf(context).bottom + AppGeometry.pageInset,
);

const musicFolderGuidance =
    'Use Downloads directly with the Downloads option. For other folders, choose Add music folder.';

Duration motion(BuildContext context, [int milliseconds = 350]) =>
    MediaQuery.disableAnimationsOf(context)
    ? Duration.zero
    : Duration(milliseconds: milliseconds);

class ResonatePageTransitionsBuilder extends PageTransitionsBuilder {
  const ResonatePageTransitionsBuilder();

  @override
  Duration get transitionDuration => const Duration(milliseconds: 400);

  @override
  Duration get reverseTransitionDuration => const Duration(milliseconds: 350);

  @override
  Widget buildTransitions<T>(
    PageRoute<T> route,
    BuildContext context,
    Animation<double> animation,
    Animation<double> secondaryAnimation,
    Widget child,
  ) {
    if (route.popGestureInProgress) {
      return const PredictiveBackPageTransitionsBuilder().buildTransitions(
        route,
        context,
        animation,
        secondaryAnimation,
        child,
      );
    }
    if (MediaQuery.disableAnimationsOf(context)) {
      return child;
    }
    final rtl = Directionality.of(context) == TextDirection.rtl;
    final slide = Tween<Offset>(
      begin: Offset(rtl ? -0.08 : 0.08, 0),
      end: Offset.zero,
    ).chain(CurveTween(curve: Curves.easeOutCubic));
    final fade = Tween<double>(
      begin: 0,
      end: 1,
    ).chain(CurveTween(curve: Curves.easeOutCubic));
    return FadeTransition(
      opacity: fade.animate(animation),
      child: SlideTransition(position: slide.animate(animation), child: child),
    );
  }
}

MenuStyle selectionMenuStyle(BuildContext context) {
  final scheme = Theme.of(context).colorScheme;
  return MenuStyle(
    backgroundColor: WidgetStatePropertyAll(scheme.surfaceContainerHigh),
    elevation: const WidgetStatePropertyAll(3),
    shape: WidgetStatePropertyAll(
      RoundedRectangleBorder(
        borderRadius: BorderRadius.circular(AppGeometry.controlRadius),
        side: BorderSide(color: scheme.outlineVariant),
      ),
    ),
    padding: const WidgetStatePropertyAll(EdgeInsets.symmetric(vertical: 4)),
  );
}

class Glass extends StatelessWidget {
  const Glass({
    super.key,
    required this.child,
    this.radius = AppGeometry.chromeRadius,
    this.padding = EdgeInsets.zero,
  });
  final Widget child;
  final double radius;
  final EdgeInsetsGeometry padding;

  @override
  Widget build(BuildContext context) {
    return ClipRRect(
      borderRadius: BorderRadius.circular(radius),
      child: DecoratedBox(
        decoration: BoxDecoration(
          color: Theme.of(context).colorScheme.surfaceContainer,
          borderRadius: BorderRadius.circular(radius),
          border: Border.all(
            color: Theme.of(context).colorScheme.outlineVariant
                .withValues(alpha: .6),
          ),
        ),
        child: Padding(padding: padding, child: child),
      ),
    );
  }
}

class EdgeTint extends StatelessWidget {
  const EdgeTint({
    super.key,
    required this.child,
    this.top = false,
    this.strength = 1,
    this.tintStrength = 1,
  });

  final Widget child;
  final bool top;
  final double strength;
  final double tintStrength;

  @override
  Widget build(BuildContext context) {
    final tint = Theme.of(context).scaffoldBackgroundColor;
    return Stack(
      children: [
        Positioned.fill(
          child: IgnorePointer(
            child: DecoratedBox(
              decoration: BoxDecoration(
                gradient: LinearGradient(
                  begin: top ? Alignment.bottomCenter : Alignment.topCenter,
                  end: top ? Alignment.topCenter : Alignment.bottomCenter,
                  stops: const [0.0, 0.28, 0.60, 0.88, 1.0],
                  colors: [
                    Colors.transparent,
                    tint.withValues(alpha: 0.04 * strength * tintStrength),
                    tint.withValues(alpha: 0.22 * strength * tintStrength),
                    tint.withValues(alpha: 0.65 * strength * tintStrength),
                    tint.withValues(alpha: 0.90 * strength * tintStrength),
                  ],
                ),
              ),
            ),
          ),
        ),
        child,
      ],
    );
  }
}

const _coloursKept = 256;
final Map<String, List<Color>> _artworkColorsCache = {};
final Map<String, Uint8List> _artworkBytesCache = {};

Future<List<Color>> extractArtworkColors(
  Uint8List? bytes, [
  String cacheKey = '',
]) async {
  if (bytes == null || bytes.isEmpty) return const [accent, mint];
  if (cacheKey.isNotEmpty && _artworkColorsCache.containsKey(cacheKey)) {
    return _artworkColorsCache[cacheKey]!;
  }
  try {
    final codec = await ui.instantiateImageCodec(
      bytes,
      targetWidth: 16,
      targetHeight: 16,
    );
    final frame = await codec.getNextFrame();
    final data = await frame.image.toByteData();
    frame.image.dispose();
    codec.dispose();
    if (data == null) return const [accent, mint];
    final colors = <Color>[];
    for (var half = 0; half < 2; half++) {
      var r = 0, g = 0, b = 0, weight = 0;
      for (var pixel = half * 128; pixel < (half + 1) * 128; pixel++) {
        final at = pixel * 4;
        final red = data.getUint8(at);
        final green = data.getUint8(at + 1);
        final blue = data.getUint8(at + 2);
        final high = math.max(red, math.max(green, blue));
        final saturation = high - math.min(red, math.min(green, blue));
        if (data.getUint8(at + 3) < 220 || high < 32 || saturation < 20) {
          continue;
        }
        final w = (saturation + 16).toInt();
        r += red * w;
        g += green * w;
        b += blue * w;
        weight += w;
      }
      colors.add(
        weight == 0
            ? (half == 0 ? accent : mint)
            : Color.fromARGB(255, r ~/ weight, g ~/ weight, b ~/ weight),
      );
    }
    if (cacheKey.isNotEmpty) {
      _artworkColorsCache[cacheKey] = colors;
      while (_artworkColorsCache.length > _coloursKept) {
        _artworkColorsCache.remove(_artworkColorsCache.keys.first);
      }
    }
    return colors;
  } catch (_) {
    return const [accent, mint];
  }
}

class Artwork extends StatefulWidget {
  const Artwork({
    super.key,
    required this.model,
    required this.track,
    this.size,
    this.radius = 16,
    this.onColors,
  });
  final PlayerModel model;
  final RowData track;
  final double? size;
  final double radius;
  final ValueChanged<List<Color>>? onColors;

  @override
  State<Artwork> createState() => _ArtworkState();
}

class _ArtworkState extends State<Artwork> {
  Uint8List? _bytes;
  String _key = '';

  @override
  void didUpdateWidget(Artwork oldWidget) {
    super.didUpdateWidget(oldWidget);
    _load();
  }

  @override
  void initState() {
    super.initState();
    _load();
  }

  void _load() {
    final bucket = artworkBucket(
      widget.size,
      WidgetsBinding
              .instance
              .platformDispatcher
              .implicitView
              ?.devicePixelRatio ??
          1.0,
    );
    final key = widget.model.artworkKey(widget.track, bucket);
    final colours = widget.model.colourKey(widget.track);
    if (_key == key) return;
    _key = key;
    if (widget.track.isEmpty) {
      if (_bytes != null) setState(() => _bytes = null);
      return;
    }
    final cached = _artworkBytesCache[key];
    if (cached != null) {
      _bytes = cached;
      if (widget.onColors != null) {
        if (_artworkColorsCache.containsKey(colours)) {
          _publishColors(key, _artworkColorsCache[colours]!);
        } else {
          extractArtworkColors(cached, colours).then((colors) {
            _publishColors(key, colors);
          });
        }
      }
    } else {
      _bytes = null;
    }
    widget.model
        .artwork(widget.track, bucket)
        .then((bytes) async {
          if (bytes != null) {
            _artworkBytesCache[key] = bytes;
            while (_artworkBytesCache.length > 96) {
              _artworkBytesCache.remove(_artworkBytesCache.keys.first);
            }
          }
          if (!mounted || _key != key) return;
          if (_bytes != bytes) {
            setState(() => _bytes = bytes);
          }
          if (widget.onColors != null) {
            final colors = bytes == null
                ? const [accent, mint]
                : await extractArtworkColors(bytes, colours);
            _publishColors(key, colors);
          }
        })
        .catchError((Object _) {});
  }

  void _publishColors(String key, List<Color> colors) {
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted && _key == key) widget.onColors?.call(colors);
    });
    WidgetsBinding.instance.ensureVisualUpdate();
  }

  @override
  Widget build(BuildContext context) => SizedBox(
    width: widget.size,
    height: widget.size,
    child: ClipRRect(
      borderRadius: BorderRadius.circular(widget.radius),
      child: AnimatedSwitcher(
        duration: motion(context, 220),
        child: _bytes != null
            ? Image.memory(
                _bytes!,
                key: ValueKey(_key),
                fit: BoxFit.cover,
                width: widget.size,
                height: widget.size,
                gaplessPlayback: true,
                cacheWidth: widget.size == null
                    ? 768
                    : (widget.size! * MediaQuery.devicePixelRatioOf(context))
                          .ceil()
                          .clamp(96, 1024),
                errorBuilder: (_, _, _) => const CoverFallback(),
              )
            : const CoverFallback(key: ValueKey('fallback')),
      ),
    ),
  );
}

class PlaylistCover extends StatelessWidget {
  const PlaylistCover({
    super.key,
    required this.model,
    required this.covers,
    this.size,
    this.radius = 12,
  });
  final PlayerModel model;
  final Object? covers;
  final double? size;
  final double radius;

  @override
  Widget build(BuildContext context) {
    final ids = (covers as List? ?? const []).take(4).toList();
    final scheme = Theme.of(context).colorScheme;
    return SizedBox(
      width: size,
      height: size,
      child: ClipRRect(
        borderRadius: BorderRadius.circular(radius),
        child: ids.isEmpty
            ? ColoredBox(
                color: scheme.secondaryContainer,
                child: Icon(
                  Icons.queue_music_rounded,
                  color: scheme.onSecondaryContainer,
                ),
              )
            : ids.length == 1
            ? Artwork(
                model: model,
                track: model.trackById(ids.first),
                size: size,
                radius: 0,
              )
            : LayoutBuilder(
                builder: (context, box) {
                  final tile = box.maxWidth.isFinite ? box.maxWidth / 2 : null;
                  Widget cover(int index) => index < ids.length
                      ? Artwork(
                          model: model,
                          track: model.trackById(ids[index]),
                          size: tile,
                          radius: 0,
                        )
                      : ColoredBox(color: scheme.surfaceContainerHighest);
                  return Column(
                    children: [
                      for (var line = 0; line < 2; line++)
                        Expanded(
                          child: Row(
                            children: [
                              for (var column = 0; column < 2; column++)
                                Expanded(child: cover(line * 2 + column)),
                            ],
                          ),
                        ),
                    ],
                  );
                },
              ),
      ),
    );
  }
}

class CoverFallback extends StatelessWidget {
  const CoverFallback({super.key});
  @override
  Widget build(BuildContext context) => DecoratedBox(
    decoration: const BoxDecoration(
      gradient: LinearGradient(
        colors: [Color(0xFF60538B), Color(0xFF243B47)],
        begin: Alignment.topLeft,
        end: Alignment.bottomRight,
      ),
    ),
    child: Center(
      child: FractionallySizedBox(
        widthFactor: .42,
        child: FittedBox(
          child: Icon(
            Icons.graphic_eq_rounded,
            color: Colors.white.withValues(alpha: .65),
          ),
        ),
      ),
    ),
  );
}

class ArtworkAtmosphere extends StatefulWidget {
  const ArtworkAtmosphere({
    super.key,
    required this.model,
    required this.track,
    required this.child,
    this.colors,
    this.immersive = false,
    this.subdued = false,
    this.shortFade = false,
  });
  final PlayerModel model;
  final RowData track;
  final Widget child;
  final List<Color>? colors;
  final bool immersive;
  final bool subdued;
  final bool shortFade;

  @override
  State<ArtworkAtmosphere> createState() => _ArtworkAtmosphereState();
}

class _ArtworkAtmosphereState extends State<ArtworkAtmosphere> {
  List<Color> _extractedColors = const [accent, mint];
  String _key = '';

  @override
  void initState() {
    super.initState();
    _load();
  }

  @override
  void didUpdateWidget(ArtworkAtmosphere oldWidget) {
    super.didUpdateWidget(oldWidget);
    _load();
  }

  void _load() {
    final key = widget.model.colourKey(widget.track);
    if (_key == key) return;
    _key = key;
    if (widget.track.isEmpty) {
      setState(() {
        _extractedColors = const [accent, mint];
      });
      return;
    }
    if (_artworkColorsCache.containsKey(key)) {
      _extractedColors = _artworkColorsCache[key]!;
    }
    widget.model
        .artwork(widget.track, colourSampleBucket)
        .then((bytes) async {
          if (!mounted || _key != key) return;
          if (bytes == null) {
            setState(() {
              _extractedColors = const [accent, mint];
            });
            return;
          }
          final colors = await extractArtworkColors(bytes, key);
          if (!mounted || _key != key) return;
          setState(() {
            _extractedColors = colors;
          });
        })
        .catchError((Object _) {});
  }

  @override
  Widget build(BuildContext context) {
    final activeColors = widget.colors ?? _extractedColors;
    return Ambient(
      colors: activeColors,
      immersive: widget.immersive,
      subdued: widget.subdued,
      shortFade: widget.shortFade,
      child: widget.child,
    );
  }
}

class Ambient extends StatelessWidget {
  const Ambient({
    super.key,
    required this.child,
    this.colors = const [accent, mint],
    this.immersive = false,
    this.subdued = false,
    this.shortFade = false,
  });
  final bool immersive;
  final bool subdued;
  final bool shortFade;
  final Widget child;
  final List<Color> colors;

  @override
  Widget build(BuildContext context) {
    final dark = Theme.of(context).brightness == Brightness.dark;
    final surface = Theme.of(context).scaffoldBackgroundColor;
    final strength = (immersive ? .38 : .22) * (subdued ? .25 : 1);
    return Stack(
      fit: StackFit.expand,
      children: [
        ColoredBox(color: surface, child: const SizedBox.expand()),
        TweenAnimationBuilder<Color?>(
          tween: ColorTween(end: colors.first),
          duration: motion(context, 750),
          builder: (context, color1, _) => TweenAnimationBuilder<Color?>(
            tween: ColorTween(
              end: colors.length > 1 ? colors[1] : colors.first,
            ),
            duration: motion(context, 750),
            builder: (context, color2, _) {
              final first = Color.alphaBlend(
                (color1 ?? accent).withValues(
                  alpha: strength * (dark ? 1 : .85),
                ),
                surface,
              );
              final second = Color.alphaBlend(
                (color2 ?? mint).withValues(alpha: strength),
                surface,
              );
              final tinted = DecoratedBox(
                decoration: BoxDecoration(
                  gradient: LinearGradient(
                    begin: shortFade ? Alignment.topCenter : Alignment.topLeft,
                    end: shortFade
                        ? Alignment.bottomCenter
                        : Alignment.bottomRight,
                    stops: shortFade
                        ? const [0, .14, .35, .58, .8, 1]
                        : const [0, .55, 1],
                    colors: shortFade
                        ? [
                            first,
                            second,
                            Color.lerp(second, surface, .55)!,
                            Color.lerp(second, surface, .85)!,
                            surface,
                            surface,
                          ]
                        : [first, second, surface],
                  ),
                ),
              );
              return shortFade
                  ? Align(
                      alignment: Alignment.topCenter,
                      child: SizedBox(
                        width: double.infinity,
                        height: 220,
                        child: tinted,
                      ),
                    )
                  : tinted;
            },
          ),
        ),
        child,
      ],
    );
  }
}

List<InlineSpan> litSpans(
  String text,
  List<int> ranges,
  TextStyle base,
  ColorScheme colours,
) {
  final matched = base.copyWith(
    color: colours.onSurface,
    fontWeight: FontWeight.w800,
  );
  final unmatched = base.copyWith(color: colours.onSurfaceVariant);
  final spans = <InlineSpan>[];
  var at = 0;
  for (var i = 0; i + 1 < ranges.length; i += 2) {
    final start = ranges[i].clamp(at, text.length);
    final end = ranges[i + 1].clamp(start, text.length);
    if (end == start) continue;
    if (start > at) {
      spans.add(TextSpan(text: text.substring(at, start), style: unmatched));
    }
    spans.add(TextSpan(text: text.substring(start, end), style: matched));
    at = end;
  }
  if (at < text.length) {
    spans.add(TextSpan(text: text.substring(at), style: unmatched));
  }
  return spans;
}

class LitText extends StatelessWidget {
  const LitText(
    this.text, {
    super.key,
    this.ranges = const [],
    this.style,
    this.maxLines = 1,
  });
  final String text;
  final List<int> ranges;
  final TextStyle? style;
  final int maxLines;
  @override
  Widget build(BuildContext context) {
    if (ranges.length < 2) {
      return Text(
        text,
        style: style,
        maxLines: maxLines,
        overflow: TextOverflow.ellipsis,
      );
    }
    final base = DefaultTextStyle.of(context).style.merge(style);
    return Text.rich(
      TextSpan(
        children: litSpans(text, ranges, base, Theme.of(context).colorScheme),
      ),
      maxLines: maxLines,
      overflow: TextOverflow.ellipsis,
    );
  }
}

class ActionIcon extends StatelessWidget {
  const ActionIcon({
    super.key,
    required this.icon,
    required this.label,
    required this.onPressed,
    this.selected = false,
    this.size = 24,
  });
  final IconData icon;
  final String label;
  final VoidCallback? onPressed;
  final bool selected;
  final double size;
  @override
  Widget build(BuildContext context) => IconButton(
    tooltip: label,
    onPressed: onPressed == null
        ? null
        : () {
            HapticFeedback.selectionClick();
            onPressed!();
          },
    color: selected ? Theme.of(context).colorScheme.primary : null,
    icon: AnimatedSwitcher(
      duration: motion(context, 180),
      transitionBuilder: (child, animation) =>
          ScaleTransition(scale: animation, child: child),
      child: Icon(icon, key: ValueKey(icon), size: size),
    ),
  );
}

class SectionTitle extends StatelessWidget {
  const SectionTitle(this.title, {super.key, this.action});
  final String title;
  final Widget? action;
  @override
  Widget build(BuildContext context) => Padding(
    padding: const EdgeInsets.only(top: 26, bottom: 12),
    child: Row(
      children: [
        Expanded(
          child: Text(title, style: Theme.of(context).textTheme.titleLarge),
        ),
        ?action,
      ],
    ),
  );
}

class EmptyState extends StatelessWidget {
  const EmptyState({
    super.key,
    required this.icon,
    required this.title,
    required this.detail,
    this.action,
  });
  final IconData icon;
  final String title, detail;
  final Widget? action;
  @override
  Widget build(BuildContext context) => Center(
    child: SingleChildScrollView(
      child: Padding(
        padding: const EdgeInsets.all(32),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            Container(
              padding: const EdgeInsets.all(24),
              decoration: BoxDecoration(
                shape: BoxShape.circle,
                color: Theme.of(context).colorScheme.primary
                    .withValues(alpha: .12),
              ),
              child: Icon(
                icon,
                size: 40,
                color: Theme.of(context).colorScheme.primary,
              ),
            ),
            const SizedBox(height: 24),
            Text(
              title,
              style: Theme.of(context).textTheme.headlineSmall,
              textAlign: TextAlign.center,
            ),
            const SizedBox(height: 10),
            Text(
              detail,
              textAlign: TextAlign.center,
              style: TextStyle(
                color: Theme.of(context).colorScheme.onSurfaceVariant,
                height: 1.5,
              ),
            ),
            if (action != null)
              Padding(padding: const EdgeInsets.only(top: 24), child: action),
          ],
        ),
      ),
    ),
  );
}

class SignalPlot extends StatelessWidget {
  const SignalPlot({
    super.key,
    required this.values,
    this.bars = false,
    this.height = 180,
  });
  final List<double> values;
  final bool bars;
  final double height;
  @override
  Widget build(BuildContext context) => RepaintBoundary(
    child: SizedBox(
      height: height,
      width: double.infinity,
      child: CustomPaint(
        painter: _SignalPainter(
          values,
          Theme.of(context).colorScheme.primary,
          bars,
        ),
      ),
    ),
  );
}

class _SignalPainter extends CustomPainter {
  _SignalPainter(this.values, this.color, this.bars);
  final List<double> values;
  final Color color;
  final bool bars;
  @override
  void paint(Canvas canvas, Size size) {
    if (values.isEmpty) return;
    final count = math.min(values.length, 256);
    final paint = Paint()
      ..color = color
      ..strokeWidth = 2
      ..strokeCap = StrokeCap.round;
    final path = Path();
    for (var i = 0; i < count; i++) {
      final value = values[i * values.length ~/ count].abs().clamp(0.0, 1.0);
      final x = i * size.width / math.max(count - 1, 1);
      if (bars) {
        paint.strokeWidth = (size.width / count * .65).clamp(1, 12);
        canvas.drawLine(
          Offset(x, size.height * .5 - value * size.height * .48),
          Offset(x, size.height * .5 + value * size.height * .48),
          paint,
        );
      } else {
        final y = size.height * (1 - value);
        if (i == 0) {
          path.moveTo(x, y);
        } else {
          path.lineTo(x, y);
        }
      }
    }
    if (!bars) {
      canvas.drawPath(path, paint..style = PaintingStyle.stroke);
    }
  }

  @override
  bool shouldRepaint(_SignalPainter old) =>
      values != old.values || color != old.color;
}

class LiveRegion extends StatelessWidget {
  const LiveRegion({super.key, this.label, this.onTap, required this.child});
  final String? label;
  final VoidCallback? onTap;
  final Widget child;

  @override
  Widget build(BuildContext context) => Semantics(
    liveRegion: true,
    container: true,
    label: label,
    button: onTap != null,
    onTap: onTap,
    child: label == null ? child : ExcludeSemantics(child: child),
  );
}

class NoticeFooter extends StatelessWidget {
  const NoticeFooter({super.key, required this.model, required this.child});
  final PlayerModel model;
  final Widget child;

  @override
  Widget build(BuildContext context) => Stack(
    fit: StackFit.expand,
    children: [
      child,
      Positioned(
        left: 0,
        right: 0,
        bottom: 0,
        child: Material(
          type: MaterialType.transparency,
          child: ListenableBuilder(
            listenable: model,
            builder: (context, _) => model.notice == null
                ? const SizedBox.shrink()
                : SafeArea(top: false, child: StatusCard(model: model)),
          ),
        ),
      ),
    ],
  );
}

class StatusBanner extends StatelessWidget {
  const StatusBanner({
    super.key,
    required this.model,
    required this.open,
    this.noticeOnly = false,
  });
  final PlayerModel model;
  final OpenPage open;
  final bool noticeOnly;

  @override
  Widget build(BuildContext context) {
    final scanning = !noticeOnly && model.state['scanning'] == true;
    final activity = noticeOnly ? null : model.syncActivity;
    final notice = model.notice;

    if (!scanning && activity == null && notice == null) {
      return const SizedBox.shrink();
    }

    return AnimatedSize(
      duration: motion(context, 240),
      curve: Curves.fastOutSlowIn,
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          if (scanning) ScanProgress(model: model),
          if (activity != null)
            SyncProgressBanner(activity: activity, open: open),
          if (notice != null && !scanning) StatusCard(model: model),
        ],
      ),
    );
  }
}

class ScanProgress extends StatelessWidget {
  const ScanProgress({super.key, required this.model});
  final PlayerModel model;

  @override
  Widget build(BuildContext context) {
    final scanned = model.allTracks.length;
    final text =
        model.state['status'] as String? ??
        (scanned > 0
            ? 'Reading music folder… $scanned ${scanned == 1 ? 'song' : 'songs'}'
            : 'Reading music folder…');
    return LiveRegion(
      label: 'Reading music folder',
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 4),
        child: Glass(
          radius: 16,
          padding: const EdgeInsets.fromLTRB(16, 8, 16, 8),
          child: Row(
            children: [
              const SizedBox(
                width: 16,
                height: 16,
                child: CircularProgressIndicator(strokeWidth: 2),
              ),
              const SizedBox(width: 12),
              Expanded(
                child: Text(
                  text,
                  style: const TextStyle(
                    fontSize: 13,
                    fontWeight: FontWeight.w500,
                  ),
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

class SyncProgressBanner extends StatelessWidget {
  const SyncProgressBanner({
    super.key,
    required this.activity,
    required this.open,
  });
  final SyncActivity activity;
  final OpenPage open;

  @override
  Widget build(BuildContext context) {
    final title = activity.isFetching
        ? 'Fetching ${activity.songs} ${activity.songs == 1 ? 'song' : 'songs'} from your devices'
        : 'Syncing with your devices…';
    final detail = activity.fraction != null
        ? '${(activity.fraction! * 100).round()}%'
        : null;

    return LiveRegion(
      label: activity.isFetching
          ? 'Fetching songs from your devices'
          : 'Syncing with your devices',
      onTap: () => open('sync'),
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 4),
        child: Glass(
          radius: 16,
          child: InkWell(
            borderRadius: BorderRadius.circular(16),
            onTap: () => open('sync'),
            child: Padding(
              padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 8),
              child: Row(
                children: [
                  SizedBox(
                    width: 18,
                    height: 18,
                    child: CircularProgressIndicator(
                      value: activity.fraction,
                      strokeWidth: 2.2,
                    ),
                  ),
                  const SizedBox(width: 12),
                  Expanded(
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        Text(
                          title,
                          style: const TextStyle(
                            fontSize: 13,
                            fontWeight: FontWeight.w600,
                          ),
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                        ),
                        if (detail != null)
                          Text(
                            detail,
                            style: TextStyle(
                              fontSize: 11,
                              color: Theme.of(context)
                                  .colorScheme
                                  .onSurfaceVariant,
                              fontFeatures: const [
                                ui.FontFeature.tabularFigures(),
                              ],
                            ),
                          ),
                      ],
                    ),
                  ),
                  const SizedBox(width: 8),
                  Icon(
                    Icons.chevron_right_rounded,
                    size: 18,
                    color: Theme.of(context).colorScheme.outline,
                  ),
                ],
              ),
            ),
          ),
        ),
      ),
    );
  }
}

class StatusCard extends StatelessWidget {
  const StatusCard({super.key, required this.model});
  final PlayerModel model;

  @override
  Widget build(BuildContext context) {
    final message = model.notice;
    if (message == null) return const SizedBox.shrink();
    final isFailure = model.failure != null;

    return LiveRegion(
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 4),
        child: Glass(
          radius: 16,
          padding: const EdgeInsets.fromLTRB(16, 4, 8, 4),
          child: Row(
            children: [
              if (isFailure) ...[
                const Icon(
                  Icons.warning_amber_rounded,
                  color: Colors.orange,
                  size: 20,
                ),
                const SizedBox(width: 10),
              ],
              Expanded(
                child: Text(
                  message,
                  style: const TextStyle(
                    fontSize: 13,
                    fontWeight: FontWeight.w500,
                  ),
                  maxLines: 3,
                  overflow: TextOverflow.ellipsis,
                ),
              ),
              const SizedBox(width: 8),
              TextButton(
                style: TextButton.styleFrom(
                  visualDensity: VisualDensity.compact,
                  padding: const EdgeInsets.symmetric(
                    horizontal: 10,
                    vertical: 4,
                  ),
                ),
                onPressed: model.dismissNotice,
                child: const Text(
                  'Dismiss',
                  style: TextStyle(fontWeight: FontWeight.w600, fontSize: 13),
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}
