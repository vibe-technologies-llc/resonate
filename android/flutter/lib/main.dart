import 'package:flutter/material.dart';
import 'package:flutter/semantics.dart';

import 'library.dart';
import 'model.dart';
import 'player.dart';
import 'search.dart';
import 'settings.dart';
import 'tools.dart';
import 'visuals.dart';

void main() {
  WidgetsFlutterBinding.ensureInitialized();
  PaintingBinding.instance.imageCache.maximumSizeBytes = 64 * 1024 * 1024;
  runApp(ResonateApp(model: PlayerModel()));
}

final _messenger = GlobalKey<ScaffoldMessengerState>();

typedef OpenPage = void Function(String page, [RowData data]);

class ResonateApp extends StatelessWidget {
  const ResonateApp({super.key, required this.model});
  final PlayerModel model;

  ThemeData _theme(Brightness brightness) {
    final dark = brightness == Brightness.dark;
    final scheme =
        ColorScheme.fromSeed(
          seedColor: const Color(0xFF8772E7),
          brightness: brightness,
          primary: dark ? accent : const Color(0xFF6650A7),
          surface: dark ? const Color(0xFF14151B) : const Color(0xFFF8F9FC),
        ).copyWith(
          surfaceContainerLow: dark ? const Color(0xFF202127) : Colors.white,
          surfaceContainer: dark
              ? const Color(0xFF282930)
              : const Color(0xFFF0F1F6),
          onSurfaceVariant: dark
              ? const Color(0xFFCACBD4)
              : const Color(0xFF51535F),
          outlineVariant: dark
              ? const Color(0xFF3B3D47)
              : const Color(0xFFDCDEE7),
        );
    final text = ThemeData(brightness: brightness).textTheme
        .copyWith(
          headlineLarge: const TextStyle(
            fontSize: 32,
            height: 1.2,
            fontWeight: FontWeight.w700,
            letterSpacing: -.5,
          ),
          headlineMedium: const TextStyle(
            fontSize: 26,
            height: 1.2,
            fontWeight: FontWeight.w700,
            letterSpacing: -.4,
          ),
          headlineSmall: const TextStyle(
            fontSize: 22,
            height: 1.25,
            fontWeight: FontWeight.w700,
            letterSpacing: -.2,
          ),
          titleLarge: const TextStyle(
            fontSize: 20,
            height: 1.3,
            fontWeight: FontWeight.w600,
          ),
          titleMedium: const TextStyle(
            fontSize: 17,
            height: 1.35,
            fontWeight: FontWeight.w600,
          ),
          titleSmall: const TextStyle(
            fontSize: 15,
            height: 1.35,
            fontWeight: FontWeight.w500,
          ),
          bodyLarge: const TextStyle(fontSize: 17, height: 1.35),
          bodyMedium: const TextStyle(fontSize: 15, height: 1.35),
          bodySmall: const TextStyle(fontSize: 13, height: 1.35),
          labelLarge: const TextStyle(
            fontSize: 14,
            height: 1.3,
            fontWeight: FontWeight.w600,
          ),
          labelMedium: const TextStyle(
            fontSize: 12,
            height: 1.3,
            fontWeight: FontWeight.w500,
          ),
          labelSmall: const TextStyle(
            fontSize: 11,
            height: 1.3,
            fontWeight: FontWeight.w500,
          ),
        )
        .apply(bodyColor: scheme.onSurface, displayColor: scheme.onSurface);
    final secondary = text.copyWith(
      bodySmall: text.bodySmall?.copyWith(color: scheme.onSurfaceVariant),
    );
    const capsule = StadiumBorder();
    final chosen = Color.alphaBlend(
      scheme.primary.withValues(alpha: dark ? .32 : .2),
      scheme.surfaceContainer,
    );
    const buttonSize = Size(AppGeometry.touchTarget, AppGeometry.touchTarget);
    return ThemeData(
      useMaterial3: true,
      colorScheme: scheme,
      textTheme: secondary,
      scaffoldBackgroundColor: dark
          ? const Color(0xFF111218)
          : const Color(0xFFF8F9FC),
      visualDensity: VisualDensity.standard,
      appBarTheme: AppBarTheme(
        backgroundColor: Colors.transparent,
        foregroundColor: scheme.onSurface,
        surfaceTintColor: Colors.transparent,
        scrolledUnderElevation: 0,
        centerTitle: false,
        titleSpacing: 8,
        titleTextStyle: text.titleMedium,
        actionsPadding: const EdgeInsets.only(right: 8),
      ),
      cardTheme: CardThemeData(
        color: scheme.surfaceContainerLow,
        surfaceTintColor: Colors.transparent,
        elevation: 0,
        margin: EdgeInsets.zero,
        clipBehavior: Clip.antiAlias,
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(AppGeometry.cardRadius),
        ),
      ),
      popupMenuTheme: PopupMenuThemeData(
        color: scheme.surfaceContainerHigh,
        surfaceTintColor: Colors.transparent,
        elevation: 4,
        position: PopupMenuPosition.under,
        menuPadding: const EdgeInsets.symmetric(vertical: 6),
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(AppGeometry.controlRadius),
          side: BorderSide(color: scheme.outlineVariant),
        ),
        labelTextStyle: WidgetStatePropertyAll(text.bodyMedium),
      ),
      snackBarTheme: SnackBarThemeData(
        backgroundColor: scheme.surfaceContainer,
        contentTextStyle: text.bodyMedium?.copyWith(color: scheme.onSurface),
        actionTextColor: scheme.primary,
        behavior: SnackBarBehavior.floating,
        elevation: 2,
        insetPadding: const EdgeInsets.fromLTRB(16, 0, 16, 12),
        actionOverflowThreshold: .55,
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(AppGeometry.controlRadius),
          side: BorderSide(color: scheme.outlineVariant.withValues(alpha: .6)),
        ),
      ),
      inputDecorationTheme: InputDecorationTheme(
        filled: true,
        fillColor: scheme.surfaceContainer,
        contentPadding: const EdgeInsets.symmetric(
          horizontal: 16,
          vertical: 16,
        ),
        border: OutlineInputBorder(
          borderRadius: BorderRadius.circular(AppGeometry.controlRadius),
          borderSide: BorderSide.none,
        ),
      ),
      filledButtonTheme: FilledButtonThemeData(
        style: FilledButton.styleFrom(
          minimumSize: buttonSize,
          shape: capsule,
          textStyle: text.labelLarge,
          padding: const EdgeInsets.symmetric(horizontal: 20),
        ),
      ),
      outlinedButtonTheme: OutlinedButtonThemeData(
        style: OutlinedButton.styleFrom(
          minimumSize: buttonSize,
          shape: capsule,
          textStyle: text.labelLarge,
          side: BorderSide(color: scheme.outlineVariant),
          padding: const EdgeInsets.symmetric(horizontal: 20),
        ),
      ),
      textButtonTheme: TextButtonThemeData(
        style: TextButton.styleFrom(
          minimumSize: buttonSize,
          shape: capsule,
          textStyle: text.labelLarge,
        ),
      ),
      chipTheme: ChipThemeData(
        shape: capsule,
        side: BorderSide.none,
        showCheckmark: false,
        backgroundColor: scheme.surfaceContainer,
        selectedColor: chosen,
        labelStyle: text.labelLarge,
        padding: const EdgeInsets.symmetric(horizontal: 4, vertical: 2),
      ),
      segmentedButtonTheme: SegmentedButtonThemeData(
        style: SegmentedButton.styleFrom(
          shape: capsule,
          selectedBackgroundColor: chosen,
          side: BorderSide(color: scheme.outlineVariant),
          textStyle: text.labelLarge,
        ),
      ),
      bottomSheetTheme: BottomSheetThemeData(
        backgroundColor: scheme.surfaceContainerLow,
        surfaceTintColor: Colors.transparent,
        showDragHandle: true,
        shape: const RoundedRectangleBorder(
          borderRadius: BorderRadius.vertical(
            top: Radius.circular(AppGeometry.heroRadius),
          ),
        ),
      ),
      dialogTheme: DialogThemeData(
        backgroundColor: scheme.surfaceContainerLow,
        surfaceTintColor: Colors.transparent,
        shape: roundedShape(AppGeometry.heroRadius),
        titleTextStyle: text.headlineSmall,
        contentTextStyle: text.bodyMedium?.copyWith(
          color: scheme.onSurfaceVariant,
        ),
      ),
      iconButtonTheme: IconButtonThemeData(
        style: IconButton.styleFrom(
          minimumSize: const Size.square(AppGeometry.touchTarget),
        ),
      ),
      listTileTheme: ListTileThemeData(
        minTileHeight: 64,
        minVerticalPadding: 8,
        horizontalTitleGap: 14,
        contentPadding: const EdgeInsets.symmetric(
          horizontal: AppGeometry.pageInset,
          vertical: 2,
        ),
        iconColor: scheme.onSurfaceVariant,
        titleTextStyle: text.bodyLarge?.copyWith(fontWeight: FontWeight.w500),
        subtitleTextStyle: text.bodyMedium?.copyWith(
          color: scheme.onSurfaceVariant,
        ),
      ),
      dividerTheme: DividerThemeData(
        color: scheme.outlineVariant,
        thickness: .5,
        space: 1,
      ),
      sliderTheme: const SliderThemeData(
        trackHeight: 4,
        showValueIndicator: ShowValueIndicator.onDrag,
      ),
      pageTransitionsTheme: const PageTransitionsTheme(
        builders: {TargetPlatform.android: ResonatePageTransitionsBuilder()},
      ),
    );
  }

  @override
  Widget build(BuildContext context) => PlayerScope(
    model: model,
    child: MaterialApp(
      title: 'Resonate',
      scaffoldMessengerKey: _messenger,
      debugShowCheckedModeBanner: false,
      restorationScopeId: 'resonate',
      theme: _theme(Brightness.light),
      darkTheme: _theme(Brightness.dark),
      home: AppShell(model: model),
    ),
  );
}

class AppShell extends StatefulWidget {
  const AppShell({super.key, required this.model});
  final PlayerModel model;
  static final ValueNotifier<int> currentTab = ValueNotifier(0);
  @override
  State<AppShell> createState() => _AppShellState();
}

class _AppShellState extends State<AppShell>
    with RestorationMixin, WidgetsBindingObserver {
  final _tab = RestorableInt(0);
  int _direction = 0;
  @override
  String get restorationId => 'shell';
  @override
  void restoreState(RestorationBucket? oldBucket, bool initialRestore) {
    registerForRestoration(_tab, 'tab');
    if (_tab.value < 0 || _tab.value >= _navigationRoots.length) {
      _tab.value = 0;
    }
    AppShell.currentTab.value = _tab.value;
  }

  @override
  void initState() {
    super.initState();
    AppShell.currentTab.addListener(_syncTab);
    WidgetsBinding.instance.addObserver(this);
    widget.model.call('loadPlaylists');
    widget.model.call('refreshFolders');
  }

  void _syncTab() {
    if (_tab.value != AppShell.currentTab.value) {
      final next = AppShell.currentTab.value;
      _direction = (next - _tab.value).sign;
      setState(() => _tab.value = next);
    }
  }

  @override
  void dispose() {
    AppShell.currentTab.removeListener(_syncTab);
    _tab.dispose();
    WidgetsBinding.instance.removeObserver(this);
    super.dispose();
  }

  void open(String page, [RowData data = const {}]) {
    Navigator.of(context).restorablePush(
      resonatePageRoute,
      arguments: {'page': page, 'data': data},
    );
  }

  @override
  Widget build(BuildContext context) => ListenableBuilder(
    listenable: widget.model,
    builder: (context, _) {
      final model = widget.model;
      return ArtworkAtmosphere(
        model: model,
        track: model.current,
        subdued: _tab.value == 1,
        child: Scaffold(
          backgroundColor: Colors.transparent,
          extendBody: true,
          body: Stack(
            fit: StackFit.expand,
            children: [
              SafeArea(
                top: _tab.value != 0,
                bottom: false,
                child: Column(
                  children: [
                    Expanded(
                      child: TweenAnimationBuilder<double>(
                        key: ValueKey(_tab.value),
                        tween: Tween(
                          begin: _direction == 0 ? 1.0 : 0.0,
                          end: 1.0,
                        ),
                        duration: _direction == 0
                            ? Duration.zero
                            : motion(context, 450),
                        curve: Curves.easeOutCubic,
                        builder: (context, value, child) {
                          final rtl =
                              Directionality.of(context) == TextDirection.rtl;
                          final sign = _direction * (rtl ? -1 : 1);
                          return Opacity(
                            opacity: value,
                            child: Transform.translate(
                              offset: Offset(
                                (1 - value) * 20 * sign,
                                (1 - value) * 6,
                              ),
                              child: child,
                            ),
                          );
                        },
                        child: IndexedStack(
                          index: _tab.value,
                          children: [
                            LibraryPage(model: model, open: open),
                            SettingsPage(model: model, open: open),
                            SearchPage(model: model, open: open),
                          ],
                        ),
                      ),
                    ),
                  ],
                ),
              ),
              Positioned(
                top: 0,
                left: 0,
                right: 0,
                height: MediaQuery.paddingOf(context).top + 28,
                child: const IgnorePointer(
                  child: EdgeTint(top: true, child: SizedBox.expand()),
                ),
              ),
            ],
          ),
          bottomNavigationBar: AppBottomBar(
            model: model,
            open: open,
            isRoot: true,
          ),
        ),
      );
    },
  );
}

const _navigationRoots = <(IconData, IconData, String)>[
  (Icons.library_music_outlined, Icons.library_music_rounded, 'Library'),
  (Icons.settings_outlined, Icons.settings_rounded, 'Settings'),
  (Icons.search_rounded, Icons.search_rounded, 'Search'),
];

const _searchTab = 2;

class AppBottomBar extends StatelessWidget {
  const AppBottomBar({
    super.key,
    required this.model,
    required this.open,
    this.isRoot = false,
  });
  final PlayerModel model;
  final OpenPage open;
  final bool isRoot;

  @override
  Widget build(BuildContext context) => ListenableBuilder(
    listenable: model,
    builder: (context, _) => ValueListenableBuilder<int>(
      valueListenable: AppShell.currentTab,
      builder: (context, currentTab, _) => EdgeTint(
        child: SafeArea(
          top: false,
          child: Padding(
            padding: const EdgeInsets.fromLTRB(
              AppSpace.m,
              AppSpace.m,
              AppSpace.m,
              AppSpace.s,
            ),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                StatusBanner(model: model, open: open, noticeOnly: !isRoot),
                if (model.current.isNotEmpty)
                  Padding(
                    padding: const EdgeInsets.only(bottom: AppSpace.s),
                    child: MiniPlayer(
                      model: model,
                      onOpen: () => open('player'),
                    ),
                  ),
                Builder(
                  builder: (context) {
                    final enlarged =
                        MediaQuery.textScalerOf(context).scale(12) > 16;
                    void selectTab(int index) {
                      AppShell.currentTab.value = index;
                      if (!isRoot) {
                        Navigator.of(context)
                            .popUntil((route) => route.isFirst);
                      }
                    }

                    return Row(
                      children: [
                        Expanded(
                          child: Glass(
                            radius: AppGeometry.chromeRadius,
                            child: _TabPill(
                              selectedIndex: currentTab,
                              onDestinationSelected: selectTab,
                              onlySelectedLabel: enlarged,
                            ),
                          ),
                        ),
                        const SizedBox(width: AppSpace.m),
                        _SearchButton(
                          selected: currentTab == _searchTab,
                          onPressed: () => selectTab(_searchTab),
                        ),
                      ],
                    );
                  },
                ),
              ],
            ),
          ),
        ),
      ),
    ),
  );
}

class _TabPill extends StatelessWidget {
  const _TabPill({
    required this.selectedIndex,
    required this.onDestinationSelected,
    required this.onlySelectedLabel,
  });

  final int selectedIndex;
  final ValueChanged<int> onDestinationSelected;
  final bool onlySelectedLabel;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return Material(
      color: Colors.transparent,
      child: SizedBox(
        height: AppGeometry.barHeight,
        child: Row(
          children: [
            for (var index = 0; index < _searchTab; index++)
              Expanded(
                child: _Tab(
                  icon: index == selectedIndex
                      ? _navigationRoots[index].$2
                      : _navigationRoots[index].$1,
                  label: _navigationRoots[index].$3,
                  selected: index == selectedIndex,
                  showLabel: !onlySelectedLabel || index == selectedIndex,
                  scheme: scheme,
                  onTap: () => onDestinationSelected(index),
                ),
              ),
          ],
        ),
      ),
    );
  }
}

class _Tab extends StatelessWidget {
  const _Tab({
    required this.icon,
    required this.label,
    required this.selected,
    required this.showLabel,
    required this.scheme,
    required this.onTap,
  });

  final IconData icon;
  final String label;
  final bool selected;
  final bool showLabel;
  final ColorScheme scheme;
  final VoidCallback onTap;

  static const _inset = 6.0;
  static const _pillHeight = 30.0;

  @override
  Widget build(BuildContext context) {
    final colour = selected ? scheme.onSurface : scheme.onSurfaceVariant;
    return Semantics(
      container: true,
      role: SemanticsRole.tab,
      selected: selected,
      label: label,
      onTap: onTap,
      child: ExcludeSemantics(
        child: Tooltip(
          message: label,
          child: InkWell(
            onTap: onTap,
            splashFactory: NoSplash.splashFactory,
            highlightColor: Colors.transparent,
            hoverColor: Colors.transparent,
            borderRadius: BorderRadius.circular(AppGeometry.chromeRadius),
            child: Padding(
              padding: const EdgeInsets.all(_inset),
              child: Stack(
                children: [
                  Positioned(
                    top: 0,
                    left: 0,
                    right: 0,
                    height: _pillHeight,
                    child: AnimatedOpacity(
                      duration: motion(context, 400),
                      curve: Curves.fastOutSlowIn,
                      opacity: selected ? 1 : 0,
                      child: DecoratedBox(
                        decoration: ShapeDecoration(
                          color: scheme.primary.withValues(alpha: .18),
                          shape: const StadiumBorder(),
                        ),
                        child: const SizedBox.expand(),
                      ),
                    ),
                  ),
                  AnimatedAlign(
                    duration: motion(context, 400),
                    curve: Curves.fastOutSlowIn,
                    alignment: showLabel
                        ? Alignment.topCenter
                        : Alignment.center,
                    child: SizedBox(
                      height: _pillHeight,
                      child: Center(child: Icon(icon, color: colour)),
                    ),
                  ),
                  if (showLabel)
                    Positioned(
                      bottom: 0,
                      left: 0,
                      right: 0,
                      child: MediaQuery.withClampedTextScaling(
                        maxScaleFactor: 1.2,
                        child: Text(
                          label,
                          maxLines: 1,
                          softWrap: false,
                          overflow: TextOverflow.ellipsis,
                          textAlign: TextAlign.center,
                          style: Theme.of(context).textTheme.labelMedium
                              ?.copyWith(
                                color: colour,
                                height: 1.2,
                                fontWeight: selected
                                    ? FontWeight.w600
                                    : FontWeight.w500,
                              ),
                        ),
                      ),
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

class _SearchButton extends StatelessWidget {
  const _SearchButton({required this.selected, required this.onPressed});

  final bool selected;
  final VoidCallback onPressed;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final label = _navigationRoots[_searchTab].$3;
    return Semantics(
      container: true,
      role: SemanticsRole.tab,
      selected: selected,
      label: label,
      onTap: onPressed,
      child: ExcludeSemantics(
        child: Tooltip(
          message: label,
          child: SizedBox.square(
            dimension: AppGeometry.barHeight,
            child: Glass(
              radius: AppGeometry.barHeight / 2,
              child: Material(
                color: Colors.transparent,
                child: InkWell(
                  customBorder: const CircleBorder(),
                  onTap: onPressed,
                  child: AnimatedContainer(
                    duration: motion(context, 300),
                    curve: Curves.fastOutSlowIn,
                    decoration: BoxDecoration(
                      shape: BoxShape.circle,
                      color: selected
                          ? scheme.primary.withValues(alpha: .18)
                          : Colors.transparent,
                    ),
                    child: Center(
                      child: Icon(
                        _navigationRoots[_searchTab].$2,
                        size: 26,
                        color: selected
                            ? scheme.onSurface
                            : scheme.onSurfaceVariant,
                      ),
                    ),
                  ),
                ),
              ),
            ),
          ),
        ),
      ),
    );
  }
}

class PlayerScope extends InheritedWidget {
  const PlayerScope({super.key, required this.model, required super.child});
  final PlayerModel model;
  static PlayerModel of(BuildContext context) =>
      context.dependOnInheritedWidgetOfExactType<PlayerScope>()!.model;
  @override
  bool updateShouldNotify(PlayerScope oldWidget) => model != oldWidget.model;
}

@pragma('vm:entry-point')
Route<void> resonatePageRoute(BuildContext context, Object? arguments) {
  final request = row(arguments);
  final page = request['page'] as String;
  final data = row(request['data']);
  final model = PlayerScope.of(context);
  void open(String next, [RowData details = const {}]) {
    Navigator.of(context).restorablePush(
      resonatePageRoute,
      arguments: {'page': next, 'data': details},
    );
  }

  final Widget child = switch (page) {
    'player' => NowPlaying(model: model, open: open),
    'collection' => CollectionPage(model: model, data: data, open: open),
    'songs' => SongsScreen(model: model, open: open),
    'online' || 'search' => SearchPage(model: model, open: open),
    'lyrics' => LyricsPage(
      model: model,
      track: data.isEmpty ? model.current : data,
    ),
    'analysis' => AnalysisPage(
      model: model,
      track: data.isEmpty ? model.current : data,
    ),
    'visualizer' => VisualizerPage(model: model),
    'inspector' => InspectorPage(model: model, track: data),
    _ => SettingsDetail(model: model, page: page, open: open),
  };
  if (page != 'player') {
    final noticed = switch (page) {
      'lyrics' ||
      'analysis' ||
      'visualizer' ||
      'inspector' ||
      'online' ||
      'search' => NoticeFooter(model: model, child: child),
      _ => child,
    };
    return MaterialPageRoute<void>(builder: (_) => noticed);
  }
  return PageRouteBuilder<void>(
    opaque: false,
    allowSnapshotting: false,
    transitionDuration: Duration.zero,
    reverseTransitionDuration: Duration.zero,
    pageBuilder: (_, _, _) => child,
  );
}
