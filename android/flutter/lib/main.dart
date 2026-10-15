import 'package:flutter/material.dart';

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
      final hasBanner =
          model.state['scanning'] == true ||
          model.syncActivity != null ||
          model.notice != null;
      return ArtworkAtmosphere(
        model: model,
        track: model.current,
        subdued: _tab.value == 3,
        child: Scaffold(
          backgroundColor: Colors.transparent,
          extendBody: true,
          body: Stack(
            fit: StackFit.expand,
            children: [
              SafeArea(
                top: _tab.value != 1 || hasBanner,
                bottom: false,
                child: Column(
                  children: [
                    StatusBanner(model: model, open: open),
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
                  child: ProgressiveBlurBackdrop(
                    top: true,
                    child: SizedBox.expand(),
                  ),
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
      builder: (context, currentTab, _) => ProgressiveBlurBackdrop(
        child: SafeArea(
          top: false,
          child: Padding(
            padding: const EdgeInsets.fromLTRB(12, 12, 12, 6),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                if (model.current.isNotEmpty)
                  Padding(
                    padding: const EdgeInsets.only(bottom: 6),
                    child: MiniPlayer(
                      model: model,
                      onOpen: () => open('player'),
                    ),
                  ),
                Glass(
                  radius: AppGeometry.chromeRadius,
                  child: NavigationBar(
                    backgroundColor: Colors.transparent,
                    elevation: 0,
                    height: MediaQuery.textScalerOf(context).scale(12) > 18
                        ? 72
                        : 64,
                    labelBehavior:
                        MediaQuery.sizeOf(context).width < 360 ||
                            MediaQuery.textScalerOf(context).scale(12) > 18
                        ? NavigationDestinationLabelBehavior.onlyShowSelected
                        : NavigationDestinationLabelBehavior.alwaysShow,
                    selectedIndex: currentTab,
                    onDestinationSelected: (index) {
                      AppShell.currentTab.value = index;
                      if (!isRoot) {
                        Navigator.of(context)
                            .popUntil((route) => route.isFirst);
                      }
                    },
                    destinations: const [
                      NavigationDestination(
                        icon: Icon(Icons.music_note_outlined),
                        selectedIcon: Icon(Icons.music_note_rounded),
                        label: 'Songs',
                      ),
                      NavigationDestination(
                        icon: Icon(Icons.library_music_outlined),
                        selectedIcon: Icon(Icons.library_music_rounded),
                        label: 'Library',
                      ),
                      NavigationDestination(
                        icon: Icon(Icons.queue_music_outlined),
                        selectedIcon: Icon(Icons.queue_music_rounded),
                        label: 'Queue',
                      ),
                      NavigationDestination(
                        icon: Icon(Icons.settings_outlined),
                        selectedIcon: Icon(Icons.settings_rounded),
                        label: 'Settings',
                      ),
                      NavigationDestination(
                        icon: Icon(Icons.search_rounded),
                        selectedIcon: Icon(Icons.search_rounded),
                        label: 'Search',
                      ),
                    ],
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
