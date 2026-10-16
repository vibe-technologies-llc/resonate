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
            fontSize: 30,
            height: 1.2,
            fontWeight: FontWeight.w700,
            letterSpacing: -.6,
          ),
          headlineMedium: const TextStyle(
            fontSize: 26,
            height: 1.25,
            fontWeight: FontWeight.w700,
            letterSpacing: -.4,
          ),
          headlineSmall: const TextStyle(
            fontSize: 22,
            height: 1.3,
            fontWeight: FontWeight.w600,
          ),
          titleLarge: const TextStyle(
            fontSize: 18,
            height: 1.3,
            fontWeight: FontWeight.w600,
          ),
          titleMedium: const TextStyle(
            fontSize: 16,
            height: 1.4,
            fontWeight: FontWeight.w600,
          ),
          bodyLarge: const TextStyle(fontSize: 16, height: 1.4),
          bodyMedium: const TextStyle(fontSize: 14, height: 1.4),
          bodySmall: TextStyle(
            fontSize: 12,
            height: 1.4,
            color: scheme.onSurfaceVariant,
          ),
          labelLarge: const TextStyle(
            fontSize: 14,
            fontWeight: FontWeight.w600,
          ),
          labelMedium: const TextStyle(
            fontSize: 12,
            fontWeight: FontWeight.w500,
          ),
          labelSmall: const TextStyle(
            fontSize: 11,
            fontWeight: FontWeight.w500,
          ),
        )
        .apply(bodyColor: scheme.onSurface, displayColor: scheme.onSurface)
        .copyWith(
          bodySmall: TextStyle(
            fontSize: 12,
            height: 1.4,
            color: scheme.onSurfaceVariant,
          ),
        );
    final controlShape = RoundedRectangleBorder(
      borderRadius: BorderRadius.circular(AppGeometry.controlRadius),
    );
    return ThemeData(
      useMaterial3: true,
      colorScheme: scheme,
      textTheme: text,
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
        titleTextStyle: text.titleLarge,
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
          minimumSize: const Size(48, 48),
          shape: controlShape,
        ),
      ),
      outlinedButtonTheme: OutlinedButtonThemeData(
        style: OutlinedButton.styleFrom(
          minimumSize: const Size(48, 48),
          shape: controlShape,
        ),
      ),
      textButtonTheme: TextButtonThemeData(
        style: TextButton.styleFrom(
          minimumSize: const Size(48, 48),
          shape: controlShape,
        ),
      ),
      iconButtonTheme: IconButtonThemeData(
        style: IconButton.styleFrom(
          minimumSize: const Size.square(AppGeometry.touchTarget),
        ),
      ),
      listTileTheme: ListTileThemeData(
        minTileHeight: 64,
        minVerticalPadding: 12,
        horizontalTitleGap: 16,
        contentPadding: const EdgeInsets.symmetric(horizontal: 16, vertical: 4),
        titleTextStyle: text.bodyLarge?.copyWith(fontWeight: FontWeight.w500),
        subtitleTextStyle: text.bodyMedium?.copyWith(
          color: scheme.onSurfaceVariant,
        ),
      ),
      navigationBarTheme: NavigationBarThemeData(
        indicatorColor: scheme.primary.withValues(alpha: .16),
        labelTextStyle: WidgetStateProperty.resolveWith(
          (states) => text.labelMedium?.copyWith(
            color: states.contains(WidgetState.selected)
                ? scheme.onSurface
                : scheme.onSurfaceVariant,
            fontWeight: states.contains(WidgetState.selected)
                ? FontWeight.w600
                : FontWeight.w500,
          ),
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
        builders: {
          TargetPlatform.android: PredictiveBackPageTransitionsBuilder(),
        },
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
  static final ValueNotifier<int> currentTab = ValueNotifier(1);
  @override
  State<AppShell> createState() => _AppShellState();
}

class _AppShellState extends State<AppShell>
    with RestorationMixin, WidgetsBindingObserver {
  final _tab = RestorableInt(1);
  @override
  String get restorationId => 'shell';
  @override
  void restoreState(RestorationBucket? oldBucket, bool initialRestore) {
    registerForRestoration(_tab, 'tab');
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
      setState(() => _tab.value = AppShell.currentTab.value);
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
        shortFade: _tab.value == 0,
        subdued: _tab.value == 3,
        child: Scaffold(
          backgroundColor: Colors.transparent,
          extendBody: true,
          body: Stack(
            fit: StackFit.expand,
            children: [
              SafeArea(
                top: _tab.value != 1,
                bottom: false,
                child: Column(
                  children: [
                    Expanded(
                      child: AnimatedSwitcher(
                        duration: motion(context, 260),
                        child: KeyedSubtree(
                          key: ValueKey(_tab.value),
                          child: switch (_tab.value) {
                            0 => SongsPage(model: model, open: open),
                            1 => LibraryPage(model: model, open: open),
                            2 => QueuePage(model: model, open: open),
                            3 => SettingsPage(model: model, open: open),
                            _ => SearchPage(model: model, open: open),
                          },
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
  (Icons.music_note_outlined, Icons.music_note_rounded, 'Songs'),
  (Icons.library_music_outlined, Icons.library_music_rounded, 'Library'),
  (Icons.queue_music_outlined, Icons.queue_music_rounded, 'Queue'),
  (Icons.settings_outlined, Icons.settings_rounded, 'Settings'),
  (Icons.search_rounded, Icons.search_rounded, 'Search'),
];

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
            padding: const EdgeInsets.fromLTRB(12, 12, 12, 6),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                if (isRoot) StatusBanner(model: model, open: open),
                if (model.current.isNotEmpty)
                  Padding(
                    padding: const EdgeInsets.only(bottom: 6),
                    child: MiniPlayer(
                      model: model,
                      onOpen: () => open('player'),
                    ),
                  ),
                LayoutBuilder(
                  builder: (context, constraints) {
                    final tabWidth =
                        constraints.maxWidth / _navigationRoots.length;
                    final compact =
                        tabWidth < 80 ||
                        MediaQuery.textScalerOf(context).scale(12) > 16;
                    void selectTab(int index) {
                      AppShell.currentTab.value = index;
                      if (!isRoot) {
                        Navigator.of(context)
                            .popUntil((route) => route.isFirst);
                      }
                    }

                    return Glass(
                      radius: AppGeometry.chromeRadius,
                      child: compact
                          ? _CompactNavigationBar(
                              selectedIndex: currentTab,
                              onDestinationSelected: selectTab,
                            )
                          : NavigationBar(
                              backgroundColor: Colors.transparent,
                              elevation: 0,
                              height: 80,
                              selectedIndex: currentTab,
                              onDestinationSelected: selectTab,
                              destinations: [
                                for (final (icon, selectedIcon, label)
                                    in _navigationRoots)
                                  NavigationDestination(
                                    icon: Icon(icon),
                                    selectedIcon: Icon(selectedIcon),
                                    label: label,
                                  ),
                              ],
                            ),
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

class _CompactNavigationBar extends StatelessWidget {
  const _CompactNavigationBar({
    required this.selectedIndex,
    required this.onDestinationSelected,
  });

  final int selectedIndex;
  final ValueChanged<int> onDestinationSelected;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return Material(
      color: Colors.transparent,
      child: SizedBox(
        height: 72,
        child: Row(
          children: [
            for (var index = 0; index < _navigationRoots.length; index++)
              Expanded(
                child: Semantics(
                  container: true,
                  role: SemanticsRole.tab,
                  selected: index == selectedIndex,
                  label: _navigationRoots[index].$3,
                  onTap: () => onDestinationSelected(index),
                  child: ExcludeSemantics(
                    child: Tooltip(
                      message: _navigationRoots[index].$3,
                      child: InkWell(
                        onTap: () => onDestinationSelected(index),
                        child: Padding(
                          padding: const EdgeInsets.all(8),
                          child: SizedBox(
                            height: 56,
                            child: index == selectedIndex
                                ? Column(
                                    crossAxisAlignment:
                                        CrossAxisAlignment.stretch,
                                    children: [
                                      SizedBox(
                                        height: 32,
                                        child: DecoratedBox(
                                          decoration: ShapeDecoration(
                                            color: scheme.primary.withValues(
                                              alpha: .16,
                                            ),
                                            shape: const StadiumBorder(),
                                          ),
                                          child: Center(
                                            child: Icon(
                                              _navigationRoots[index].$2,
                                              color: scheme.onSurface,
                                            ),
                                          ),
                                        ),
                                      ),
                                      const SizedBox(height: 4),
                                      SizedBox(
                                        height: 20,
                                        child: FittedBox(
                                          fit: BoxFit.scaleDown,
                                          child: Text(
                                            _navigationRoots[index].$3,
                                            maxLines: 1,
                                            style: Theme.of(context)
                                                .textTheme
                                                .labelMedium
                                                ?.copyWith(
                                                  color: scheme.onSurface,
                                                  fontWeight: FontWeight.w600,
                                                ),
                                          ),
                                        ),
                                      ),
                                    ],
                                  )
                                : Center(
                                    child: Icon(
                                      _navigationRoots[index].$1,
                                      color: scheme.onSurfaceVariant,
                                    ),
                                  ),
                          ),
                        ),
                      ),
                    ),
                  ),
                ),
              ),
          ],
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
  if (page != 'player') return MaterialPageRoute<void>(builder: (_) => child);
  return PageRouteBuilder<void>(
    opaque: false,
    allowSnapshotting: false,
    transitionDuration: Duration.zero,
    reverseTransitionDuration: Duration.zero,
    pageBuilder: (_, _, _) => child,
  );
}
