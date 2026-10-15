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
    final scheme = ColorScheme.fromSeed(
      seedColor: const Color(0xFF8772E7),
      brightness: brightness,
      primary: dark ? accent : const Color(0xFF6650A7),
      surface: dark ? const Color(0xFF14141C) : const Color(0xFFF8F7FC),
    );
    return ThemeData(
      useMaterial3: true,
      colorScheme: scheme,
      scaffoldBackgroundColor: dark
          ? const Color(0xFF101017)
          : const Color(0xFFF8F7FC),
      visualDensity: VisualDensity.standard,
      appBarTheme: const AppBarTheme(
        backgroundColor: Colors.transparent,
        scrolledUnderElevation: 0,
      ),
      inputDecorationTheme: InputDecorationTheme(
        filled: true,
        fillColor: scheme.onSurface.withValues(alpha: .045),
        border: OutlineInputBorder(
          borderRadius: BorderRadius.circular(18),
          borderSide: BorderSide.none,
        ),
      ),
      filledButtonTheme: FilledButtonThemeData(
        style: FilledButton.styleFrom(
          minimumSize: const Size(48, 50),
          shape: RoundedRectangleBorder(
            borderRadius: BorderRadius.circular(18),
          ),
        ),
      ),
      listTileTheme: const ListTileThemeData(
        contentPadding: EdgeInsets.symmetric(horizontal: 16, vertical: 4),
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
            padding: const EdgeInsets.fromLTRB(12, 24, 12, 8),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                if (model.current.isNotEmpty)
                  Padding(
                    padding: const EdgeInsets.only(bottom: 10),
                    child: MiniPlayer(
                      model: model,
                      onOpen: () => open('player'),
                    ),
                  ),
                Glass(
                  radius: 30,
                  child: NavigationBar(
                    backgroundColor: Colors.transparent,
                    elevation: 0,
                    height: 72,
                    selectedIndex: currentTab,
                    onDestinationSelected: (index) {
                      AppShell.currentTab.value = index;
                      if (!isRoot) {
                        Navigator.of(context).popUntil((route) => route.isFirst);
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
                        selectedIcon:
                            Icon(Icons.library_music_rounded),
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
