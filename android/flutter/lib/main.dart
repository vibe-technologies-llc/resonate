import 'package:flutter/material.dart';

import 'library.dart';
import 'model.dart';
import 'player.dart';
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
  }

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    widget.model.addListener(_showNotice);
    widget.model.call('loadPlaylists');
    widget.model.call('refreshFolders');
  }

  @override
  void dispose() {
    widget.model.removeListener(_showNotice);
    _tab.dispose();
    WidgetsBinding.instance.removeObserver(this);
    super.dispose();
  }

  String? _shownNotice;
  void _showNotice() {
    final message = widget.model.notice;
    if (message == _shownNotice) return;
    _shownNotice = message;
    if (message == null) return;
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (!mounted) return;
      _messenger.currentState?.removeCurrentSnackBar();
      _messenger.currentState?.showSnackBar(
        SnackBar(
          content: Text(message),
          behavior: SnackBarBehavior.floating,
          action: SnackBarAction(
            label: 'Dismiss',
            onPressed: widget.model.dismissNotice,
          ),
        ),
      );
    });
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
        child: Scaffold(
          backgroundColor: Colors.transparent,
          body: SafeArea(
            bottom: false,
            child: Column(
              children: [
                if (model.state['scanning'] == true)
                  const LinearProgressIndicator(minHeight: 2),
                Expanded(
                  child: AnimatedSwitcher(
                    duration: motion(context, 260),
                    child: KeyedSubtree(
                      key: ValueKey(_tab.value),
                      child: switch (_tab.value) {
                        0 => SongsPage(model: model, open: open),
                        1 => LibraryPage(model: model, open: open),
                        2 => QueuePage(model: model, open: open),
                        _ => SettingsPage(model: model, open: open),
                      },
                    ),
                  ),
                ),
              ],
            ),
          ),
          bottomNavigationBar: SafeArea(
            top: false,
            child: Padding(
              padding: const EdgeInsets.fromLTRB(12, 4, 12, 8),
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
                      selectedIndex: _tab.value,
                      onDestinationSelected: (index) =>
                          setState(() => _tab.value = index),
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
                      ],
                    ),
                  ),
                ],
              ),
            ),
          ),
        ),
      );
    },
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
    'online' => OnlinePage(model: model, open: open),
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
    transitionDuration: motion(context, 500),
    reverseTransitionDuration: motion(context, 350),
    pageBuilder: (_, _, _) => child,
    transitionsBuilder: (_, animation, _, child) => SlideTransition(
      position: Tween(
        begin: const Offset(0, 1),
        end: Offset.zero,
      ).animate(CurvedAnimation(parent: animation, curve: Curves.easeOutCubic)),
      child: child,
    ),
  );
}
