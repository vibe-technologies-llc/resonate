import 'dart:math' as math;
import 'dart:ui' as ui;

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import 'model.dart';

const accent = Color(0xFFB9AAFF);
const mint = Color(0xFF9AE5CD);

Duration motion(BuildContext context, [int milliseconds = 350]) =>
    MediaQuery.disableAnimationsOf(context)
    ? Duration.zero
    : Duration(milliseconds: milliseconds);

class Glass extends StatelessWidget {
  const Glass({
    super.key,
    required this.child,
    this.radius = 28,
    this.padding = EdgeInsets.zero,
  });
  final Widget child;
  final double radius;
  final EdgeInsetsGeometry padding;

  @override
  Widget build(BuildContext context) {
    final dark = Theme.of(context).brightness == Brightness.dark;
    return ClipRRect(
      borderRadius: BorderRadius.circular(radius),
      child: BackdropFilter(
        filter: ui.ImageFilter.blur(sigmaX: 24, sigmaY: 24),
        child: DecoratedBox(
          decoration: BoxDecoration(
            color: (dark ? const Color(0xFF24242D) : Colors.white).withValues(
              alpha: dark ? .54 : .65,
            ),
            borderRadius: BorderRadius.circular(radius),
            border: Border.all(
              color: (dark ? Colors.white : Colors.black).withValues(
                alpha: .08,
              ),
            ),
          ),
          child: Padding(padding: padding, child: child),
        ),
      ),
    );
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
  Future<Uint8List?>? _future;
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
    final key =
        '${widget.track['id']}:${widget.track['artRevision']}:${widget.model.state['artworkGeneration']}';
    if (_key == key) return;
    _key = key;
    _future = widget.track.isEmpty ? null : widget.model.artwork(widget.track);
    if (widget.onColors != null) {
      _future
          ?.then((bytes) async {
            if (bytes == null) {
              if (mounted && _key == key) widget.onColors?.call([accent, mint]);
              return;
            }
            final codec = await ui.instantiateImageCodec(
              bytes,
              targetWidth: 16,
              targetHeight: 16,
            );
            final frame = await codec.getNextFrame();
            final data = await frame.image.toByteData();
            frame.image.dispose();
            codec.dispose();
            if (data == null || !mounted || _key != key) return;
            final colors = <Color>[];
            for (var half = 0; half < 2; half++) {
              var r = 0, g = 0, b = 0, weight = 0;
              for (var pixel = half * 128; pixel < (half + 1) * 128; pixel++) {
                final at = pixel * 4;
                final red = data.getUint8(at),
                    green = data.getUint8(at + 1),
                    blue = data.getUint8(at + 2);
                final high = math.max(red, math.max(green, blue));
                final saturation = high - math.min(red, math.min(green, blue));
                if (data.getUint8(at + 3) < 220 ||
                    high < 32 ||
                    saturation < 20) {
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
                    : Color.fromARGB(
                        255,
                        r ~/ weight,
                        g ~/ weight,
                        b ~/ weight,
                      ),
              );
            }
            widget.onColors?.call(colors);
          })
          .catchError((Object _) {});
    }
  }

  @override
  Widget build(BuildContext context) => SizedBox(
    width: widget.size,
    height: widget.size,
    child: ClipRRect(
      borderRadius: BorderRadius.circular(widget.radius),
      child: FutureBuilder<Uint8List?>(
        future: _future,
        builder: (context, snapshot) => AnimatedSwitcher(
          duration: motion(context, 220),
          child: snapshot.hasData
              ? Image.memory(
                  snapshot.data!,
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
              : const CoverFallback(),
        ),
      ),
    ),
  );
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

class ArtworkAtmosphere extends StatelessWidget {
  const ArtworkAtmosphere({
    super.key,
    required this.model,
    required this.track,
    required this.child,
    this.colors = const [accent, mint],
    this.immersive = false,
  });
  final PlayerModel model;
  final RowData track;
  final Widget child;
  final List<Color> colors;
  final bool immersive;

  @override
  Widget build(BuildContext context) => FutureBuilder<Uint8List?>(
    future: track.isEmpty ? null : model.artwork(track),
    builder: (context, image) => Ambient(
      animated: model.playing,
      colors: colors,
      artwork: track.isNotEmpty && image.connectionState == ConnectionState.done
          ? image.data
          : null,
      immersive: immersive,
      child: child,
    ),
  );
}

class Ambient extends StatefulWidget {
  const Ambient({
    super.key,
    required this.child,
    this.colors = const [accent, mint],
    this.animated = false,
    this.artwork,
    this.immersive = false,
  });
  final Uint8List? artwork;
  final bool immersive;
  final Widget child;
  final List<Color> colors;
  final bool animated;
  @override
  State<Ambient> createState() => _AmbientState();
}

class _AmbientState extends State<Ambient> with SingleTickerProviderStateMixin {
  late final AnimationController _drift = AnimationController(
    vsync: this,
    duration: const Duration(seconds: 22),
  );
  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    _update();
  }

  @override
  void didUpdateWidget(Ambient oldWidget) {
    super.didUpdateWidget(oldWidget);
    _update();
  }

  void _update() {
    if (widget.animated &&
        !MediaQuery.disableAnimationsOf(context) &&
        TickerMode.valuesOf(context).enabled) {
      if (!_drift.isAnimating) _drift.repeat(reverse: true);
    } else {
      _drift.stop();
    }
  }

  @override
  void dispose() {
    _drift.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final dark = Theme.of(context).brightness == Brightness.dark;
    return Stack(
      fit: StackFit.expand,
      children: [
        ColoredBox(color: Theme.of(context).scaffoldBackgroundColor),
        RepaintBoundary(
          child: TweenAnimationBuilder<Color?>(
            tween: ColorTween(end: widget.colors.first),
            duration: motion(context, 900),
            builder: (context, color, _) => CustomPaint(
              painter: _AmbientPainter(
                _drift,
                color ?? accent,
                widget.colors.last,
                dark,
              ),
            ),
          ),
        ),
        if (widget.artwork != null)
          Positioned.fill(
            child: ClipRect(
              child: AnimatedBuilder(
                animation: _drift,
                child: RepaintBoundary(
                  child: Opacity(
                    opacity: widget.immersive
                        ? (dark ? .58 : .38)
                        : (dark ? .22 : .15),
                    child: ImageFiltered(
                      imageFilter: ui.ImageFilter.blur(sigmaX: 32, sigmaY: 32),
                      child: Image.memory(
                        widget.artwork!,
                        fit: BoxFit.cover,
                        cacheWidth: 128,
                        gaplessPlayback: true,
                        excludeFromSemantics: true,
                      ),
                    ),
                  ),
                ),
                builder: (context, child) => Transform.translate(
                  offset: Offset(
                    24 * math.sin(_drift.value * math.pi * 2),
                    32 * math.cos(_drift.value * math.pi * 2),
                  ),
                  child: Transform.rotate(
                    angle: (_drift.value - .5) * .16,
                    child: Transform.scale(scale: 1.4, child: child),
                  ),
                ),
              ),
            ),
          ),
        IgnorePointer(
          child: DecoratedBox(
            decoration: BoxDecoration(
              gradient: LinearGradient(
                begin: Alignment.topCenter,
                end: Alignment.bottomCenter,
                colors: [
                  Colors.transparent,
                  Theme.of(context).scaffoldBackgroundColor
                      .withValues(alpha: .55),
                ],
              ),
            ),
          ),
        ),
        widget.child,
      ],
    );
  }
}

class _AmbientPainter extends CustomPainter {
  _AmbientPainter(this.drift, this.first, this.second, this.dark)
    : super(repaint: drift);
  final Animation<double> drift;
  final Color first, second;
  final bool dark;
  @override
  void paint(Canvas canvas, Size size) {
    final t = drift.value;
    final points = [
      Offset(size.width * (.15 + t * .3), size.height * (.08 + t * .13)),
      Offset(size.width * (.95 - t * .25), size.height * (.54 - t * .12)),
    ];
    for (var i = 0; i < 2; i++) {
      final radius = size.longestSide * .63;
      canvas.drawCircle(
        points[i],
        radius,
        Paint()
          ..shader = ui.Gradient.radial(points[i], radius, [
            (i == 0 ? first : second).withValues(alpha: dark ? .24 : .16),
            Colors.transparent,
          ]),
      );
    }
  }

  @override
  bool shouldRepaint(_AmbientPainter old) =>
      first != old.first || second != old.second || dark != old.dark;
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
          child: Text(
            title,
            style: Theme.of(context).textTheme.titleLarge
                ?.copyWith(fontWeight: FontWeight.w700),
          ),
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
