import 'dart:async';
import 'dart:ui' as ui;

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:resonate_android/library.dart';
import 'package:resonate_android/main.dart';
import 'package:resonate_android/model.dart';
import 'package:resonate_android/player.dart';
import 'package:resonate_android/search.dart';
import 'package:resonate_android/tools.dart';
import 'package:resonate_android/settings.dart';
import 'package:resonate_android/visuals.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  const channel = MethodChannel('resonate/test');
  late StreamController<dynamic> events;
  late PlayerModel model;
  late List<MethodCall> calls;
  Uint8List? artwork;
  Object? collectionAnswer;
  Object? searchAnswer;
  Object? albumKeyAnswer;
  Object? highlightsAnswer;
  final first = <String, dynamic>{
    'id': '18446744073709551614',
    'queueId': '81',
    'title': 'Echoes',
    'name': 'Echoes.flac',
    'artist': 'An artist / with punctuation',
    'album': 'A record',
    'albumArtist': 'An artist / with punctuation',
    'favourite': false,
    'durationMs': '60000',
    'rate': 96000,
    'bits': 24,
    'codec': 'FLAC',
    'artRevision': '0',
  };

  setUp(() {
    events = StreamController<dynamic>.broadcast(sync: true);
    calls = [];
    artwork = null;
    collectionAnswer = [first];
    searchAnswer = null;
    albumKeyAnswer = null;
    highlightsAnswer = null;
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(channel, (call) async {
          calls.add(call);
          return switch (call.method) {
            'artwork' => artwork,
            'collections' => {'albums': [], 'artists': []},
            'collectionTracks' => collectionAnswer,
            'searchLibrary' => searchAnswer,
            'albumKey' => albumKeyAnswer,
            'highlights' => highlightsAnswer,
            'fails' => throw PlatformException(
              code: 'native',
              message: 'The disk is full',
            ),
            'equaliserInfo' => {
              'limits': {'preampAtMostDb': 24.0, 'gainAtMostDb': 24.0},
              'frequencies': [31.25, 62.5],
              'gains': [0.0, 0.0],
              'response': [0.0, 0.0],
            },
            _ => null,
          };
        });
    model = PlayerModel(methods: channel, events: events.stream);
  });
  tearDown(() async {
    model.dispose();
    await events.close();
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(channel, null);
  });

  Future<void> mount(WidgetTester tester, {bool populated = false}) async {
    await tester.pumpWidget(ResonateApp(model: model));
    events.add({
      'tracks': populated ? [first] : [],
      'allTracks': populated ? [first] : [],
      'queue': populated ? [first] : [],
      'snapshot': populated
          ? {
              'current': first,
              'transport': 'PAUSED',
              'durationMs': '60000',
              'positionMs': '10000',
              'volume': 1.0,
              'repeat': 0,
              'catalogRevision': '0',
            }
          : {},
      'online': false,
      'playlists': [],
    });
    await tester.pumpAndSettle();
  }

  testWidgets('empty library has real folder import and all navigation roots', (
    tester,
  ) async {
    await mount(tester);
    expect(find.text('Your library'), findsOneWidget);
    for (final label in ['Songs', 'Library', 'Queue', 'Settings']) {
      expect(find.text(label), findsOneWidget);
    }
    await tester.scrollUntilVisible(
      find.text('Add music folder'),
      250,
      scrollable: find.byType(Scrollable).first,
    );
    await tester.pumpAndSettle();
    await tester.ensureVisible(find.text('Add music folder'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Add music folder'));
    await tester.pump();
    expect(calls.any((call) => call.method == 'chooseFolder'), isTrue);
    expect(tester.takeException(), isNull);
  });

  testWidgets('position updates only rebuild the progress control', (
    tester,
  ) async {
    await mount(tester, populated: true);
    var modelChanges = 0;
    var progressChanges = 0;
    model.addListener(() => modelChanges++);
    model.positionMs.addListener(() => progressChanges++);

    events.add({
      'snapshot': {...model.snapshot, 'positionMs': '20000'},
    });
    await tester.pump();

    expect(modelChanges, 0);
    expect(progressChanges, 1);
    expect(model.positionMs.value, 20000);
    expect(
      tester
          .widget<LinearProgressIndicator>(find.byType(LinearProgressIndicator))
          .value,
      closeTo(1 / 3, .001),
    );

    events.add({
      'snapshot': {...model.snapshot, 'transport': 'PLAYING'},
    });
    expect(modelChanges, 1);
  });

  testWidgets(
    'song playback preserves full unsigned ids and explicit position',
    (tester) async {
      await mount(tester, populated: true);
      await tester.tap(find.text('Songs'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('Echoes').first);
      await tester.pump();
      final play = calls.lastWhere((call) => call.method == 'play');
      expect(play.arguments['ids'], [first['id']]);
      expect(play.arguments['index'], 0);
    },
  );

  testWidgets(
    'player opens, switches modes and sends transport through native controller',
    (tester) async {
      await mount(tester, populated: true);
      await tester.tap(find.text('Echoes').first);
      await tester.pumpAndSettle();
      expect(find.text('NOW PLAYING'), findsOneWidget);
      await tester.ensureVisible(find.byTooltip('Next').last);
      await tester.tap(find.byTooltip('Next').last);
      await tester.pump();
      expect(calls.last.method, 'next');
      await tester.tap(find.text('Lyrics'));
      await tester.pumpAndSettle();
      expect(
        calls.any(
          (call) =>
              call.method == 'lyrics' && call.arguments['id'] == first['id'],
        ),
        isTrue,
      );
      await tester.tap(find.text('Queue').last);
      await tester.pumpAndSettle();
      expect(find.text('Up next'), findsOneWidget);
      expect(tester.takeException(), isNull);
    },
  );

  testWidgets(
    'sleep timer shares the player pill and still calls native timer',
    (tester) async {
      await mount(tester, populated: true);
      await tester.tap(find.text('Echoes').first);
      await tester.pumpAndSettle();
      final pill = find.ancestor(
        of: find.text('Lyrics'),
        matching: find.byType(Glass),
      );
      expect(
        find.descendant(of: pill, matching: find.byTooltip('Sleep timer')),
        findsOneWidget,
      );
      await tester.tap(find.byTooltip('Sleep timer'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('30 minutes'));
      await tester.pumpAndSettle();
      final timer = calls.lastWhere((call) => call.method == 'sleep');
      expect(timer.arguments, {'kind': 'after', 'seconds': 1800});
    },
  );

  testWidgets('native word wipes preserve emoji and blend the current letter', (
    tester,
  ) async {
    await tester.pumpWidget(
      const MaterialApp(
        home: Scaffold(
          body: DefaultTextStyle(
            style: TextStyle(color: Colors.white),
            child: LyricWords(
              text: 'Hi 🎵 again',
              wipe: {
                'sungUtf16': 3,
                'blend': {'startUtf16': 3, 'endUtf16': 5, 'share': .5},
              },
            ),
          ),
        ),
      ),
    );
    final text = tester.widget<Text>(
      find.descendant(of: find.byType(LyricWords), matching: find.byType(Text)),
    );
    final spans = (text.textSpan as TextSpan).children!.cast<TextSpan>();
    expect(text.textSpan!.toPlainText(), 'Hi 🎵 again');
    expect(spans[2].text, '🎵');
    expect(
      spans[2].style!.color,
      Color.lerp(Colors.white.withValues(alpha: .38), Colors.white, .5),
    );
    expect(tester.takeException(), isNull);
  });

  testWidgets('repeated playlist songs play the tapped row', (tester) async {
    events.add({
      'allTracks': [first],
    });
    final playlist = {
      'id': '7',
      'revision': '3',
      'name': 'Repeats',
      'kind': 'playlist',
      'title': 'Repeats',
    };
    await tester.pumpWidget(
      MaterialApp(
        home: CollectionPage(
          model: model,
          data: playlist,
          open: (_, [data = const {}]) {},
        ),
      ),
    );
    events.add({
      'playlistRows': [first, first],
      'playlists': [playlist],
    });
    await tester.pumpAndSettle();
    await tester.scrollUntilVisible(
      find.text('Echoes').last,
      120,
      scrollable: find.byType(Scrollable).first,
    );
    await tester.pumpAndSettle();
    await tester.tap(find.text('Echoes').last);
    await tester.pump();
    final play = calls.lastWhere((call) => call.method == 'play');
    expect(play.arguments['ids'], [first['id'], first['id']]);
    expect(play.arguments['index'], 1);
  });

  testWidgets('settings search and child route survive restoration', (
    tester,
  ) async {
    await mount(tester);
    await tester.tap(find.text('Settings'));
    await tester.pumpAndSettle();
    await tester.enterText(find.byType(TextField), 'ReplayGain');
    await tester.pumpAndSettle();
    expect(find.text('Equaliser'), findsNothing);
    events.add({
      'preferences': {
        'replayGain': 'OFF',
        'quality': 'HIGH',
        'phase': 'LINEAR',
        'restoration': 'OFF',
        'bufferMs': 250,
      },
    });
    await tester.tap(find.text('Sound processing'));
    await tester.pumpAndSettle();
    expect(find.text('True-peak clip prevention'), findsOneWidget);
    await tester.restartAndRestore();
    await tester.pumpAndSettle();
    expect(find.text('True-peak clip prevention'), findsOneWidget);
    await tester.tap(find.byType(DropdownMenu<String>).first);
    await tester.pumpAndSettle();
    await tester.tap(find.text('Track'));
    await tester.pumpAndSettle();
    expect(calls.lastWhere((call) => call.method == 'preferences').arguments, {
      'replayGain': 'TRACK',
    });
    await tester.pageBack();
    await tester.pumpAndSettle();
    expect(find.text('ReplayGain'), findsOneWidget);
    expect(find.text('Equaliser'), findsNothing);
  });

  testWidgets('slider commits once on release', (tester) async {
    final committed = <double>[];
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: CommitSlider(
            value: 0,
            min: 0,
            max: 100,
            label: 'Preamp',
            onCommit: committed.add,
          ),
        ),
      ),
    );
    final gesture = await tester.startGesture(
      tester.getCenter(find.byType(Slider)),
    );
    await gesture.moveBy(const Offset(70, 0));
    await tester.pump();
    expect(committed, isEmpty);
    await gesture.up();
    await tester.pump();
    expect(committed.length, 1);
  });

  testWidgets('reduced motion and narrow player stay usable with large text', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(360, 760);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    events.add({
      'allTracks': [first],
      'snapshot': {
        'current': first,
        'transport': 'PAUSED',
        'durationMs': '60000',
        'volume': 1.0,
      },
    });
    await tester.pumpWidget(
      MaterialApp(
        home: MediaQuery(
          data: const MediaQueryData(
            size: Size(360, 760),
            disableAnimations: true,
            textScaler: TextScaler.linear(1.5),
          ),
          child: NowPlaying(model: model, open: (_, [data = const {}]) {}),
        ),
      ),
    );
    await tester.pumpAndSettle();
    expect(find.text('NOW PLAYING'), findsOneWidget);

    expect(motion(tester.element(find.byType(NowPlaying))), Duration.zero);
  });

  testWidgets('an open album follows native favourite changes', (tester) async {
    events.add({
      'allTracks': [first],
    });
    await tester.pumpWidget(
      MaterialApp(
        home: CollectionPage(
          model: model,
          data: {'kind': 'album', 'key': 'album-key', 'title': 'A record'},
          open: (_, [data = const {}]) {},
        ),
      ),
    );
    await tester.pumpAndSettle();
    events.add({
      'allTracks': [
        {...first, 'favourite': true},
      ],
    });
    await tester.pumpAndSettle();
    await tester.ensureVisible(find.byTooltip('More about Echoes'));
    await tester.pumpAndSettle();
    await tester.tap(find.byTooltip('More about Echoes'));
    await tester.pumpAndSettle();
    expect(find.text('Remove from favourites'), findsOneWidget);
  });

  for (final brightness in Brightness.values) {
    testWidgets('favourite banner stays compact in $brightness theme', (
      tester,
    ) async {
      tester.view.physicalSize = const Size(600, 844);
      tester.view.devicePixelRatio = 1;
      tester.platformDispatcher.platformBrightnessTestValue = brightness;
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);
      addTearDown(tester.platformDispatcher.clearPlatformBrightnessTestValue);

      await mount(tester, populated: true);
      await tester.tap(find.byTooltip('Add to favourites'));
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 300));

      final banner = find.byType(SnackBar);
      final message = find.text('Added to favourites');
      final action = find.descendant(
        of: banner,
        matching: find.text('Add to playlist'),
      );
      final theme = Theme.of(tester.element(banner));
      expect(theme.colorScheme.brightness, brightness);
      expect(
        theme.snackBarTheme.backgroundColor,
        theme.colorScheme.surfaceContainer,
      );
      expect(theme.snackBarTheme.actionTextColor, theme.colorScheme.primary);
      expect(
        (tester.getCenter(message).dy - tester.getCenter(action).dy).abs(),
        lessThan(12),
      );
      expect(tester.getSize(banner).height, lessThan(80));

      await tester.tap(action);
      await tester.pumpAndSettle();
      expect(find.text('Create playlist'), findsOneWidget);
      expect(tester.takeException(), isNull);
    });
  }

  testWidgets('favourite banner fades away after four seconds', (tester) async {
    await mount(tester, populated: true);
    await tester.tap(find.byTooltip('Add to favourites'));
    await tester.pump();
    await tester.pump(const Duration(milliseconds: 300));

    expect(find.text('Added to favourites'), findsOneWidget);
    await tester.pump(const Duration(seconds: 3));
    expect(find.text('Added to favourites'), findsOneWidget);
    await tester.pump(const Duration(seconds: 1));
    await tester.pump(const Duration(milliseconds: 300));
    expect(find.text('Added to favourites'), findsNothing);
  });

  testWidgets('new status replaces stale scan feedback', (tester) async {
    await mount(tester, populated: true);
    events.add({'status': 'Reading music folder…'});
    await tester.pumpAndSettle();
    expect(find.text('Reading music folder…'), findsOneWidget);
    events.add({'status': 'Read 1 song'});
    await tester.pumpAndSettle();
    expect(find.text('Reading music folder…'), findsNothing);
    expect(find.text('Read 1 song'), findsOneWidget);
    expect(
      tester.getBottomLeft(find.byType(StatusBanner)).dy,
      lessThanOrEqualTo(tester.getTopLeft(find.byType(MiniPlayer)).dy),
    );
  });

  test('queue identity and favourites follow native updates', () async {
    events.add({
      'allTracks': [first],
      'tracks': [first],
      'queue': [
        first,
        {...first, 'queueId': '82'},
      ],
    });
    expect(model.queue.map((track) => track['queueId']), ['81', '82']);
    events.add({
      'tracks': [
        {...first, 'favourite': true},
      ],
    });
    expect(model.tracks.single['favourite'], isTrue);
    expect(model.allTracks.single['favourite'], isFalse);
  });
  testWidgets(
    'sync preserves nearby mode and hides pairing codes until requested',
    (tester) async {
      const code = 'PAIRING-CODE-FIXTURE';
      events.add({
        'syncConfig': {'key': code, 'enabled': true, 'nearbyOnly': true},
        'syncStatus': {'code': code, 'standing': 'NEARBY', 'devices': []},
      });
      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(
            body: AccountPane(model: model, kind: 'sync'),
          ),
        ),
      );
      await tester.pumpAndSettle();
      expect(find.text(code), findsNothing);
      expect(find.widgetWithText(TextField, 'Sync server'), findsNothing);
      expect(find.text('Device name and server saved'), findsOneWidget);
      await tester.enterText(
        find.widgetWithText(TextField, 'Device name'),
        'Kitchen phone',
      );
      await tester.pump();
      await tester.ensureVisible(find.text('Save device name and server'));
      await tester.tap(find.text('Save device name and server'));
      await tester.pump();
      final saved = calls.lastWhere((call) => call.method == 'sync').arguments;
      expect(saved['device'], 'Kitchen phone');
      expect(saved.containsKey('nearbyOnly'), isFalse);
      await tester.ensureVisible(find.text('Show pairing code'));
      await tester.tap(find.text('Show pairing code'));
      await tester.pumpAndSettle();
      expect(find.text(code), findsOneWidget);
      await tester.tap(find.text('Hide pairing code'));
      await tester.pumpAndSettle();
      expect(find.text(code), findsNothing);
      await tester.ensureVisible(find.text('Sync favourites'));
      await tester.tap(find.text('Sync favourites'));
      await tester.pump();
      expect(calls.lastWhere((call) => call.method == 'sync').arguments, {
        'favourites': true,
      });
    },
  );
  testWidgets('player reveals the library while dragging and snapping back', (
    tester,
  ) async {
    await mount(tester, populated: true);
    await tester.tap(find.text('Echoes').first);
    await tester.pumpAndSettle();
    expect(find.text('Your library'), findsOneWidget);
    expect(
      ModalRoute.of(tester.element(find.byType(NowPlaying)))!.opaque,
      isFalse,
    );
    final backdrop = find.descendant(
      of: find.byType(NowPlaying),
      matching: find.byType(EdgeTint),
    );
    final gesture = await tester.startGesture(
      tester.getCenter(find.text('NOW PLAYING')),
    );
    await gesture.moveBy(const Offset(0, 70));
    await tester.pump();
    expect(tester.widget<EdgeTint>(backdrop).strength, inExclusiveRange(0, 1));
    await tester.pump(const Duration(milliseconds: 200));
    await gesture.up();
    await tester.pumpAndSettle();
    expect(find.text('NOW PLAYING'), findsOneWidget);
    expect(tester.widget<EdgeTint>(backdrop).strength, 1);
    await tester.drag(find.text('NOW PLAYING'), const Offset(0, 320));
    await tester.pumpAndSettle();
    expect(find.text('NOW PLAYING'), findsNothing);
    expect(find.text('Your library'), findsOneWidget);
    expect(tester.takeException(), isNull);
  });

  testWidgets('closing the player respects reduced motion', (tester) async {
    tester.platformDispatcher.accessibilityFeaturesTestValue =
        const FakeAccessibilityFeatures(disableAnimations: true);
    addTearDown(tester.platformDispatcher.clearAccessibilityFeaturesTestValue);
    await mount(tester, populated: true);
    await tester.tap(find.text('Echoes').first);
    await tester.pumpAndSettle();
    expect(find.text('NOW PLAYING'), findsOneWidget);
    await tester.tap(find.byTooltip('Close player'));
    await tester.pump();
    await tester.pump();
    expect(find.text('NOW PLAYING'), findsNothing);
    expect(find.text('Your library'), findsOneWidget);
    expect(tester.takeException(), isNull);
  });
  testWidgets('sync activity banner displays progress and opens sync pane', (
    tester,
  ) async {
    await mount(tester, populated: true);
    events.add({
      'syncConfig': {'key': 'code', 'enabled': true},
      'syncStatus': {
        'standing': 'CONNECTED',
        'transfers': [
          {
            'title': 'Solaris',
            'artist': 'Artist',
            'size': 1000,
            'received': 640,
            'state': 'MOVING',
          },
        ],
      },
    });
    await tester.pumpAndSettle();
    expect(find.text('Fetching 1 song from your devices'), findsOneWidget);
    expect(find.text('64%'), findsOneWidget);
    await tester.tap(find.text('Fetching 1 song from your devices'));
    await tester.pumpAndSettle();
    expect(find.text('Device sync'), findsWidgets);
  });
  testWidgets('dismissing status banner clears notice', (tester) async {
    await mount(tester, populated: true);
    events.add({'status': 'Kept 4 songs from your devices'});
    await tester.pumpAndSettle();
    expect(find.text('Kept 4 songs from your devices'), findsOneWidget);
    expect(find.text('Dismiss'), findsOneWidget);
    await tester.tap(find.text('Dismiss'));
    await tester.pumpAndSettle();
    expect(find.text('Kept 4 songs from your devices'), findsNothing);
    expect(calls.any((call) => call.method == 'dismissStatus'), isTrue);
  });
  testWidgets(
    'collection pages retain bottom navigation and show song thumbnails',
    (tester) async {
      events.add({
        'allTracks': [first],
        'snapshot': {
          'current': first,
          'transport': 'PAUSED',
          'durationMs': '60000',
          'positionMs': '10000',
          'volume': 1.0,
        },
      });
      final playlist = {
        'id': '7',
        'name': 'Favourites',
        'kind': 'favourites',
        'title': 'Favourites',
      };
      await tester.pumpWidget(
        MaterialApp(
          home: CollectionPage(
            model: model,
            data: playlist,
            open: (_, [data = const {}]) {},
          ),
        ),
      );
      events.add({
        'playlistRows': [first],
        'allTracks': [
          {...first, 'favourite': true},
        ],
      });
      await tester.pumpAndSettle();
      expect(find.byType(AppBottomBar), findsOneWidget);
      expect(find.byType(MiniPlayer), findsOneWidget);
      final tile = find.byType(SongTile);
      expect(tile, findsOneWidget);
      expect(
        find.descendant(of: tile, matching: find.byType(Artwork)),
        findsOneWidget,
      );
    },
  );
  testWidgets('album songs show track numbers instead of thumbnails', (
    tester,
  ) async {
    events.add({
      'allTracks': [first],
    });
    await tester.pumpWidget(
      MaterialApp(
        home: CollectionPage(
          model: model,
          data: {'kind': 'album', 'key': 'album-key', 'title': 'A record'},
          open: (_, [data = const {}]) {},
        ),
      ),
    );
    await tester.pumpAndSettle();
    final tile = find.byType(SongTile);
    expect(tile, findsOneWidget);
    expect(find.descendant(of: tile, matching: find.text('1')), findsOneWidget);
  });
  testWidgets('playback pill and now playing do not use Hero animations', (
    tester,
  ) async {
    await mount(tester, populated: true);
    expect(find.byType(Hero), findsNothing);
    await tester.tap(find.text('Echoes').first);
    await tester.pumpAndSettle();
    expect(find.byType(Hero), findsNothing);
  });
  testWidgets('songs page falls back to allTracks and resets filter', (
    tester,
  ) async {
    events.add({
      'allTracks': [first],
      'tracks': <Map<String, dynamic>>[],
    });
    await mount(tester, populated: true);
    await tester.tap(find.text('Songs'));
    await tester.pumpAndSettle();
    expect(find.text('No songs found'), findsNothing);
    expect(find.byType(SongTile), findsOneWidget);
    events.add({
      'filter': {'favourites': true},
      'tracks': <Map<String, dynamic>>[],
    });
    await tester.pumpAndSettle();
    expect(find.text('No songs found'), findsOneWidget);
    await tester.tap(find.text('Clear filters'));
    await tester.pumpAndSettle();
    expect(calls.any((c) => c.method == 'filter'), isTrue);
  });

  testWidgets(
    'cached artwork survives lyrics transitions and timed following',
    (tester) async {
      artwork = await tester.runAsync(() async {
        final recorder = ui.PictureRecorder();
        Canvas(recorder).drawColor(Colors.purple, BlendMode.src);
        final picture = recorder.endRecording();
        final image = await picture.toImage(16, 16);
        final bytes = await image.toByteData(format: ui.ImageByteFormat.png);
        image.dispose();
        picture.dispose();
        return bytes!.buffer.asUint8List();
      });
      final track = {...first, 'id': 'cached-lyrics-artwork'};
      await tester.runAsync(
        () => extractArtworkColors(
          artwork,
          "${track['id']}:${track['artRevision']}:null",
        ),
      );
      events.add({
        'snapshot': {
          'current': track,
          'transport': 'PAUSED',
          'durationMs': '60000',
        },
        'lyrics': {
          'trackId': track['id'],
          'detail': 'LINES',
          'lines': List.generate(
            30,
            (i) => {'text': 'A line sung in time $i', 'atMs': i * 1000},
          ),
        },
        'lyricFrame': {
          'readAt': 12,
          'active': [12],
        },
      });
      await tester.pumpWidget(
        MaterialApp(
          home: NowPlaying(model: model, open: (_, [data = const {}]) {}),
        ),
      );
      await tester.pumpAndSettle();
      for (final mode in ['Lyrics', 'Art', 'Lyrics', 'Queue', 'Lyrics']) {
        await tester.tap(find.text(mode));
        await tester.pump();
        await tester.pumpAndSettle();
        expect(tester.takeException(), isNull);
      }
      final pictureRequests = calls
          .where((call) => call.method == 'artwork')
          .length;
      events.add({'artworkGeneration': '33'});
      await tester.pumpAndSettle();
      expect(
        calls.where((call) => call.method == 'artwork'),
        hasLength(pictureRequests + 2),
      );
      events.add({
        'lyricFrame': {
          'readAt': 22,
          'active': [22],
        },
      });
      await tester.pumpAndSettle();
      expect(find.text('A line sung in time 22').hitTestable(), findsOneWidget);
      expect(tester.takeException(), isNull);
    },
  );

  testWidgets('lyrics and queue fit a short viewport with large text', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(700, 420);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    events.add({
      'snapshot': {
        'current': first,
        'transport': 'PAUSED',
        'durationMs': '60000',
      },
      'queue': [first],
    });
    await tester.pumpWidget(
      MaterialApp(
        home: MediaQuery(
          data: const MediaQueryData(
            size: Size(700, 420),
            textScaler: TextScaler.linear(2),
          ),
          child: NowPlaying(model: model, open: (_, [data = const {}]) {}),
        ),
      ),
    );
    await tester.pumpAndSettle();
    for (final mode in ['Lyrics', 'Queue']) {
      await tester.tap(find.byTooltip(mode));
      await tester.pumpAndSettle();
      expect(tester.takeException(), isNull);
    }
  });
  testWidgets('edge tint leaves the content boundary sharp', (tester) async {
    const key = ValueKey('tint-sample');
    await tester.pumpWidget(
      const MaterialApp(
        home: Center(
          child: RepaintBoundary(
            key: key,
            child: SizedBox(
              width: 64,
              height: 64,
              child: Stack(
                children: [
                  Row(
                    children: [
                      Expanded(
                        child: ColoredBox(
                          color: Colors.black,
                          child: SizedBox.expand(),
                        ),
                      ),
                      Expanded(
                        child: ColoredBox(
                          color: Colors.white,
                          child: SizedBox.expand(),
                        ),
                      ),
                    ],
                  ),
                  EdgeTint(
                    top: true,
                    tintStrength: .5,
                    child: SizedBox.expand(),
                  ),
                ],
              ),
            ),
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();
    final boundary = tester.renderObject<RenderRepaintBoundary>(
      find.byKey(key),
    );
    final pixels = await tester.runAsync(() async {
      final image = await boundary.toImage();
      final bytes = await image.toByteData();
      image.dispose();
      return bytes!;
    });
    final tinted = pixels!.getUint8((8 * 64 + 24) * 4);
    final white = pixels.getUint8((8 * 64 + 40) * 4);
    final clear = pixels.getUint8((60 * 64 + 24) * 4);
    expect(tinted, greaterThan(clear + 20));
    expect(white, greaterThan(tinted + 80));
    expect(find.byType(BackdropFilter), findsNothing);
  });

  testWidgets('Songs tint fades into the page near the first rows', (
    tester,
  ) async {
    const key = ValueKey('songs-tint');
    await tester.pumpWidget(
      MaterialApp(
        theme: ThemeData(scaffoldBackgroundColor: Colors.black),
        home: Scaffold(
          body: Center(
            child: RepaintBoundary(
              key: key,
              child: const SizedBox(
                width: 100,
                height: 400,
                child: Ambient(
                  colors: [Colors.red, Colors.red],
                  shortFade: true,
                  child: SizedBox.expand(),
                ),
              ),
            ),
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();
    final boundary = tester.renderObject<RenderRepaintBoundary>(
      find.byKey(key),
    );
    final pixels = await tester.runAsync(() async {
      final image = await boundary.toImage();
      final bytes = await image.toByteData();
      image.dispose();
      return bytes!;
    });
    int redAt(int y) => pixels!.getUint8((y * 100 + 50) * 4);
    expect(redAt(0), greaterThan(redAt(80)));
    expect(redAt(80), greaterThan(redAt(140)));
    expect(redAt(180), 0);
    expect(redAt(260), 0);
  });
  testWidgets('playlist naming keeps its text field alive through dismissal', (
    tester,
  ) async {
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: Builder(
            builder: (context) => TextButton(
              onPressed: () => namePlaylist(context, model),
              child: const Text('New playlist'),
            ),
          ),
        ),
      ),
    );
    for (final action in ['Save', 'Cancel', 'keyboard', 'back', 'barrier']) {
      await tester.tap(find.text('New playlist'));
      await tester.pumpAndSettle();
      await tester.enterText(find.byType(TextField), 'Evening music');
      switch (action) {
        case 'keyboard':
          await tester.testTextInput.receiveAction(TextInputAction.done);
        case 'back':
          await tester.binding.handlePopRoute();
        case 'barrier':
          await tester.tapAt(const Offset(10, 10));
        default:
          await tester.tap(find.text(action));
      }
      await tester.pump();
      await tester.pump(const Duration(milliseconds: 80));
      expect(tester.takeException(), isNull);
      await tester.pumpAndSettle();
      expect(tester.takeException(), isNull);
    }
    final created = calls
        .where((call) => call.method == 'createPlaylist')
        .toList();
    expect(created, hasLength(2));
    expect(
      created.map((call) => call.arguments),
      everyElement({'name': 'Evening music'}),
    );
  });

  testWidgets(
    'playlist rename closes safely and preserves the native identity',
    (tester) async {
      const playlist = {'id': '7', 'revision': '3', 'name': 'Old name'};
      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(
            body: Builder(
              builder: (context) => TextButton(
                onPressed: () => namePlaylist(context, model, playlist),
                child: const Text('Rename'),
              ),
            ),
          ),
        ),
      );
      await tester.tap(find.text('Rename'));
      await tester.pumpAndSettle();
      expect(
        tester.widget<TextField>(find.byType(TextField)).controller!.text,
        'Old name',
      );
      await tester.enterText(find.byType(TextField), ' New name ');
      await tester.tap(find.text('Save'));
      await tester.pump();
      await tester.pumpAndSettle();
      final renamed = calls.singleWhere(
        (call) => call.method == 'renamePlaylist',
      );
      expect(renamed.arguments, {...playlist, 'name': 'New name'});
      expect(tester.takeException(), isNull);
    },
  );

  testWidgets('creating a playlist keeps the live picker intact', (
    tester,
  ) async {
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: Builder(
            builder: (context) => TextButton(
              onPressed: () => playlistPicker(context, model, [first]),
              child: const Text('Choose playlist'),
            ),
          ),
        ),
      ),
    );
    await tester.tap(find.text('Choose playlist'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Create playlist'));
    await tester.pumpAndSettle();
    await tester.enterText(find.byType(TextField), 'Evening music');
    await tester.tap(find.text('Save'));
    events.add({
      'playlists': [
        {'id': '7', 'name': 'Evening music', 'songs': 0},
      ],
    });
    await tester.pump();
    await tester.pumpAndSettle();
    expect(find.text('Add to playlist'), findsOneWidget);
    expect(find.text('Evening music'), findsOneWidget);
    await tester.tap(find.text('Evening music'));
    await tester.pumpAndSettle();
    final added = calls.singleWhere((call) => call.method == 'addToPlaylist');
    expect(added.arguments, {
      'id': '7',
      'ids': [first['id']],
    });
    expect(tester.takeException(), isNull);
  });
  testWidgets(
    'saving the queue keeps its naming field alive through dismissal',
    (tester) async {
      events.add({
        'queue': [first],
        'snapshot': {'current': first},
      });
      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(
            body: QueuePage(model: model, open: (_, [data = const {}]) {}),
          ),
        ),
      );
      for (final action in ['Save', 'Cancel']) {
        await tester.tap(find.byTooltip('Save queue as playlist'));
        await tester.pumpAndSettle();
        await tester.enterText(find.byType(TextField), ' Queue saved ');
        await tester.tap(find.text(action));
        await tester.pump();
        await tester.pump(const Duration(milliseconds: 80));
        expect(tester.takeException(), isNull);
        await tester.pumpAndSettle();
        expect(tester.takeException(), isNull);
      }
      final saved = calls
          .where((call) => call.method == 'saveQueueAsPlaylist')
          .toList();
      expect(saved, hasLength(1));
      expect(saved.single.arguments, {'name': 'Queue saved'});
    },
  );
  testWidgets(
    'queue handoffs open one device picker and retain native indices',
    (tester) async {
      events.add({
        'queue': [first],
        'snapshot': {'current': first},
        'syncHandoffs': [
          {'device': 'Android', 'songs': 11},
          {'device': 'iPhone', 'songs': 13},
          {'device': 'Linux', 'songs': 441},
        ],
      });
      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(
            body: QueuePage(model: model, open: (_, [data = const {}]) {}),
          ),
        ),
      );
      await tester.pumpAndSettle();
      expect(find.text('Continue listening'), findsOneWidget);
      expect(find.text('3 devices available'), findsOneWidget);
      expect(find.text('Android'), findsNothing);
      await tester.tap(find.text('Continue listening'));
      await tester.pumpAndSettle();
      expect(find.text('Android'), findsOneWidget);
      expect(find.text('iPhone'), findsOneWidget);
      expect(find.text('Linux'), findsOneWidget);
      await tester.tap(find.text('iPhone'));
      await tester.pumpAndSettle();
      expect(calls.last.method, 'handoff');
      expect(calls.last.arguments, {'index': 1});
      expect(find.text('iPhone'), findsNothing);
      expect(tester.takeException(), isNull);
    },
  );
  testWidgets('library tools group compact rows below the toolbar', (
    tester,
  ) async {
    await tester.pumpWidget(
      MaterialApp(
        home: SettingsDetail(
          model: model,
          page: 'tools',
          open: (_, [data = const {}]) {},
        ),
      ),
    );
    await tester.pumpAndSettle();
    expect(
      tester.getBottomLeft(find.text('Your library')).dy,
      lessThan(tester.getTopLeft(find.text('A deeper listen')).dy),
    );
    final suggestionsCard = find.ancestor(
      of: find.text('Suggestions'),
      matching: find.byType(Card),
    );
    final statisticsCard = find.ancestor(
      of: find.text('Statistics'),
      matching: find.byType(Card),
    );
    expect(tester.element(suggestionsCard), tester.element(statisticsCard));
    expect(tester.takeException(), isNull);
  });
  testWidgets('music folder cards keep a visible gap', (tester) async {
    events.add({
      'folders': [
        {
          'name': 'Downloads',
          'uri': 'content://media/external_primary/downloads',
          'readable': false,
        },
        {'name': 'Music', 'uri': 'content://music', 'readable': true},
      ],
    });
    await tester.pumpWidget(
      MaterialApp(
        home: SettingsDetail(
          model: model,
          page: 'folders',
          open: (_, [data = const {}]) {},
        ),
      ),
    );
    await tester.pumpAndSettle();
    final downloadsCard = find.ancestor(
      of: find.text('Downloads'),
      matching: find.byType(Card),
    );
    final musicCard = find.ancestor(
      of: find.text('Music'),
      matching: find.byType(Card),
    );
    expect(
      tester.getTopLeft(musicCard).dy - tester.getBottomLeft(downloadsCard).dy,
      greaterThanOrEqualTo(12),
    );
    expect(tester.takeException(), isNull);
  });
  testWidgets('audio outputs show distinct device types and the active route', (
    tester,
  ) async {
    events.add({
      'outputs': [
        {'id': 7, 'name': 'sdk_gphone', 'kind': 'Built-in speaker'},
        {'id': 8, 'name': 'sdk_gphone', 'kind': 'Earpiece'},
      ],
      'selectedOutput': 7,
      'actualOutput': 8,
    });
    await tester.pumpWidget(
      MaterialApp(
        home: SettingsDetail(
          model: model,
          page: 'output',
          open: (_, [data = const {}]) {},
        ),
      ),
    );
    await tester.pumpAndSettle();
    expect(find.text('Current output'), findsOneWidget);
    expect(find.text('Requested: Built-in speaker'), findsOneWidget);
    expect(find.text('Earpiece'), findsWidgets);
    await tester.tap(find.text('Built-in speaker'));
    await tester.pump();
    expect(calls.last.method, 'output');
    expect(calls.last.arguments, {'id': 7});
    expect(tester.takeException(), isNull);
  });
  testWidgets(
    'artwork controls send native actions and reflect progress and restrictions',
    (tester) async {
      events.add({
        'online': true,
        'enrichmentAutomatic': true,
        'enrichmentUnmeteredOnly': false,
        'enrichmentStatus': {'running': false, 'stopped': 'FINISHED'},
      });
      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(
            body: SingleChildScrollView(
              child: ArtworkDetailsSection(model: model),
            ),
          ),
        ),
      );
      await tester.pumpAndSettle();
      expect(calls.any((call) => call.method == 'enrichmentStatus'), isTrue);
      await tester.tap(find.widgetWithText(SwitchListTile, 'Only on Wi-Fi'));
      await tester.pump();
      expect(calls.last.method, 'enrichmentUnmeteredOnly');
      expect(calls.last.arguments, {'enabled': true});
      await tester.tap(
        find.widgetWithText(
          SwitchListTile,
          'Fetch artwork and details automatically',
        ),
      );
      await tester.pump();
      expect(calls.last.method, 'enrichmentAutomatic');
      expect(calls.last.arguments, {'enabled': false});
      events.add({'enrichmentAutomatic': false});
      await tester.pumpAndSettle();
      expect(
        tester
            .widget<SwitchListTile>(
              find.widgetWithText(SwitchListTile, 'Only on Wi-Fi'),
            )
            .onChanged,
        isNull,
      );
      await tester.ensureVisible(find.text('Look again for missing covers'));
      await tester.tap(find.text('Look again for missing covers'));
      await tester.pump();
      expect(calls.last.method, 'retryMissingCovers');
      events.add({
        'coversAskedAgain': 3,
        'enrichmentStatus': {'running': true, 'due': 20, 'asked': 5},
      });
      await tester.pump();
      expect(find.text('Asking again for 3 albums'), findsOneWidget);
      expect(
        find.text('Finding artwork and details · 5 of 20'),
        findsOneWidget,
      );
      expect(
        tester
            .widget<LinearProgressIndicator>(
              find.byType(LinearProgressIndicator),
            )
            .value,
        .25,
      );
      await tester.ensureVisible(find.text('Stop'));
      await tester.tap(find.text('Stop'));
      await tester.pump();
      expect(calls.last.method, 'stopEnrichment');
      events.add({
        'enrichmentStatus': {'running': false, 'stopped': 'UNREACHABLE'},
      });
      await tester.pumpAndSettle();
      expect(
        find.text('Stopped: MusicBrainz could not be reached'),
        findsOneWidget,
      );
      expect(tester.takeException(), isNull);
    },
  );

  testWidgets('artwork settings are searchable and hidden while offline', (
    tester,
  ) async {
    await mount(tester);
    events.add({'online': true});
    await tester.tap(find.text('Settings'));
    await tester.pumpAndSettle();
    await tester.enterText(find.byType(TextField), 'missing covers');
    await tester.pumpAndSettle();
    expect(find.text('Artwork and details'), findsOneWidget);
    expect(find.text('No settings found'), findsNothing);
    events.add({'online': false});
    await tester.pumpAndSettle();
    expect(find.text('Artwork and details'), findsNothing);
    expect(find.text('No settings found'), findsOneWidget);
  });

  testWidgets('refreshing artwork needs confirmation and uses the batch seam', (
    tester,
  ) async {
    events.add({
      'online': true,
      'allTracks': [first],
    });
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: Builder(
            builder: (context) => TextButton(
              onPressed: () => refreshArtworkAndDetails(context, model),
              child: const Text('Open refresh'),
            ),
          ),
        ),
      ),
    );
    for (final action in ['Cancel', 'Refresh']) {
      await tester.tap(find.text('Open refresh'));
      await tester.pumpAndSettle();
      expect(find.textContaining('1 song.'), findsOneWidget);
      await tester.tap(find.text(action));
      await tester.pumpAndSettle();
      if (action == 'Cancel') {
        expect(
          calls.any((call) => call.method == 'refreshEnrichment'),
          isFalse,
        );
      }
    }
    expect(
      calls.where((call) => call.method == 'refreshEnrichment'),
      hasLength(1),
    );
    expect(calls.any((call) => call.method == 'enrich'), isFalse);
  });

  testWidgets('compact bottom controls retain touch targets with large text', (
    tester,
  ) async {
    tester.view.physicalSize = const Size(320, 720);
    tester.view.devicePixelRatio = 1;
    tester.platformDispatcher.textScaleFactorTestValue = 2;
    addTearDown(tester.view.reset);
    addTearDown(tester.platformDispatcher.clearTextScaleFactorTestValue);
    await mount(tester, populated: true);
    for (final label in ['Previous', 'Play', 'Next']) {
      final size = tester.getSize(find.byTooltip(label));
      expect(size.width, greaterThanOrEqualTo(48));
      expect(size.height, greaterThanOrEqualTo(48));
    }
    expect(find.byType(NavigationBar), findsNothing);
    expect(find.byTooltip('Songs'), findsOneWidget);
    await tester.tap(find.byTooltip('Settings'));
    await tester.pumpAndSettle();
    expect(find.text('Settings'), findsWidgets);
    expect(tester.takeException(), isNull);
  });

  for (final (viewport, textScale) in const [
    (Size(320, 720), 2.0),
    (Size(390, 844), 1.25),
  ]) {
    testWidgets('scaled navigation keeps equal insets at ${viewport.width}', (
      tester,
    ) async {
      tester.view.physicalSize = viewport;
      tester.view.devicePixelRatio = 1;
      tester.platformDispatcher.textScaleFactorTestValue = textScale;
      addTearDown(tester.view.reset);
      addTearDown(tester.platformDispatcher.clearTextScaleFactorTestValue);

      AppShell.currentTab.value = 1;
      await mount(tester, populated: true);

      final tab = find.byTooltip('Library');
      final pill = find.descendant(
        of: tab,
        matching: find.byType(DecoratedBox),
      );
      final label = find.descendant(of: tab, matching: find.text('Library'));
      final tabRect = tester.getRect(tab);
      final pillRect = tester.getRect(pill);

      expect(tabRect.height, 72);
      expect(pillRect.left - tabRect.left, closeTo(8, .1));
      expect(tabRect.right - pillRect.right, closeTo(8, .1));
      expect(pillRect.top - tabRect.top, closeTo(8, .1));
      expect(
        (tester.getCenter(pill).dx - tabRect.center.dx).abs(),
        lessThan(1),
      );
      expect(
        (tester.getCenter(label).dx - tabRect.center.dx).abs(),
        lessThan(1),
      );
      final searchTab = find.byTooltip('Search');
      final searchIcon = find.descendant(
        of: searchTab,
        matching: find.byIcon(Icons.search_rounded),
      );
      expect(
        tester.getCenter(searchIcon).dy,
        closeTo(tester.getCenter(searchTab).dy, .1),
      );
      expect(tester.takeException(), isNull);
    });
  }

  testWidgets(
    'Downloads works without a folder grant and music access is explicit',
    (tester) async {
      await mount(tester);
      await tester.tap(find.text('Settings'));
      await tester.pumpAndSettle();
      await tester.enterText(find.byType(TextField), 'folders');
      await tester.pumpAndSettle();
      await tester.tap(find.text('Music folders'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('Use Downloads'));
      await tester.pump();
      expect(calls.last.method, 'useDownloads');
      expect(calls.any((c) => c.method == 'chooseFolder'), isFalse);
      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(
            body: Builder(
              builder: (context) => TextButton(
                onPressed: () => chooseDownload(context, model, {'id': '9'}),
                child: const Text('Choose destination'),
              ),
            ),
          ),
        ),
      );
      await tester.tap(find.text('Choose destination'));
      await tester.pumpAndSettle();
      await tester.tap(find.text('Downloads'));
      await tester.pumpAndSettle();
      expect(calls.last.method, 'downloadToDownloads');
      expect(calls.last.arguments, {'id': '9'});
      expect(tester.takeException(), isNull);
    },
  );

  testWidgets(
    'download destinations explain Android restrictions and preserve granted folder URIs',
    (tester) async {
      const folder =
          'content://com.android.externalstorage.documents/tree/primary%3ADownload%2FResonate';
      events.add({
        'folders': [
          {
            'name': 'Resonate downloads',
            'uri': folder,
            'readable': true,
            'writable': true,
          },
          {
            'name': 'Read only',
            'uri': 'content://test/readonly',
            'readable': true,
            'writable': false,
          },
        ],
      });
      await tester.pumpWidget(
        MaterialApp(
          home: Scaffold(
            body: Builder(
              builder: (context) => TextButton(
                onPressed: () => chooseDownload(context, model, {'id': '9'}),
                child: const Text('Choose destination'),
              ),
            ),
          ),
        ),
      );
      await tester.tap(find.text('Choose destination'));
      await tester.pumpAndSettle();
      expect(find.text(musicFolderGuidance), findsOneWidget);
      expect(find.text('Read only'), findsNothing);
      await tester.tap(find.text('Resonate downloads'));
      await tester.pumpAndSettle();
      expect(calls.last.method, 'download');
      expect(calls.last.arguments, {'id': '9', 'uri': folder});
      expect(tester.takeException(), isNull);
    },
  );

  test('a queue drag is sent as the row it lands before, counted before', () {
    expect(queueLanding(0, 1), 2);
    expect(queueLanding(0, 4), 5);
    expect(queueLanding(3, 1), 1);
    expect(queueLanding(2, 2), 2);
  });

  test('artwork is asked at the smallest bucket that holds the drawn size', () {
    expect(artworkBucket(50, 3), 192);
    expect(artworkBucket(32, 3), 96);
    expect(artworkBucket(400, 3), 1024);
    expect(artworkBucket(null, 3), defaultArtworkBucket);
  });

  testWidgets('dragging a queue row to the end lands after the last row', (
    tester,
  ) async {
    final rows = [
      for (final id in ['81', '82', '83']) {...first, 'queueId': id},
    ];
    events.add({
      'queue': rows,
      'snapshot': {'current': rows.first},
    });
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: QueuePage(model: model, open: (_, [data = const {}]) {}),
        ),
      ),
    );
    await tester.drag(
      find.byIcon(Icons.drag_handle_rounded).first,
      const Offset(0, 600),
    );
    await tester.pumpAndSettle();
    expect(calls.lastWhere((call) => call.method == 'moveInQueue').arguments, {
      'from': 0,
      'to': 3,
    });
  });

  test(
    'a failure stays until it is dismissed, whatever succeeds after it',
    () async {
      await model.call('fails');
      expect(model.failure, 'The disk is full');

      await model.call('collections');
      expect(model.failure, 'The disk is full');

      await model.dismissNotice();
      expect(model.failure, isNull);
    },
  );

  test(
    'streams that move many times a second never notify the whole model',
    () {
      var changes = 0;
      var frames = 0;
      model.addListener(() => changes++);
      model.hot('lyricFrame').addListener(() => frames++);

      events.add({
        'lyricFrame': {'readAt': 3},
      });
      events.add({
        'samples': [0.5],
      });
      events.add({'downloadedBytes': '1024'});
      events.add({'analysisProgress': 0.5});

      expect(changes, 0);
      expect(frames, 1);
      expect(row(model.hot('lyricFrame').value)['readAt'], 3);
      expect(model.state['downloadedBytes'], '1024');

      events.add({'status': 'Read 1 song'});
      expect(changes, 1);
    },
  );

  test('a favourite changes in place without a new catalog', () {
    events.add({
      'allTracks': [first],
      'tracks': [first],
      'queue': [first],
    });
    var changes = 0;
    model.addListener(() => changes++);

    events.add({
      'favourite': {'id': first['id'], 'enabled': true},
    });

    expect(model.allTracks.single['favourite'], isTrue);
    expect(model.tracks.single['favourite'], isTrue);
    expect(model.queue.single['favourite'], isTrue);
    expect(model.favouriteTracks, hasLength(1));
    expect(model.isFavourite(first), isTrue);
    expect(changes, 1);

    events.add({
      'favourite': {'id': first['id'], 'enabled': false},
    });
    expect(model.favouriteTracks, isEmpty);
  });

  testWidgets(
    'a playlist is a spinner until its rows arrive, then empty or full',
    (tester) async {
      final playlist = {
        'id': '7',
        'revision': '3',
        'name': 'Later',
        'kind': 'playlist',
        'title': 'Later',
      };
      events.add({
        'playlistRows': [first],
        'playlists': [playlist],
      });
      await tester.pumpWidget(
        MaterialApp(
          home: CollectionPage(
            model: model,
            data: playlist,
            open: (_, [data = const {}]) {},
          ),
        ),
      );
      await tester.pump();

      expect(find.byType(CircularProgressIndicator), findsOneWidget);
      expect(find.text('Echoes'), findsNothing);
      expect(find.text('A little quiet here'), findsNothing);

      events.add({'playlistRows': <dynamic>[]});
      await tester.pumpAndSettle();

      expect(find.byType(CircularProgressIndicator), findsNothing);
      expect(find.text('A little quiet here'), findsOneWidget);
    },
  );

  testWidgets('a collection that cannot load says so and tries again', (
    tester,
  ) async {
    collectionAnswer = null;
    await tester.pumpWidget(
      MaterialApp(
        home: CollectionPage(
          model: model,
          data: {'kind': 'album', 'key': 'a-key', 'title': 'A record'},
          open: (_, [data = const {}]) {},
        ),
      ),
    );
    await tester.pumpAndSettle();

    expect(find.byType(CircularProgressIndicator), findsNothing);
    expect(find.text('Try again'), findsOneWidget);

    collectionAnswer = [first];
    await tester.tap(find.text('Try again'));
    await tester.pumpAndSettle();

    expect(find.text('Try again'), findsNothing);
    expect(find.text('Echoes'), findsWidgets);
  });

  testWidgets('the empty queue browses every song, not the unknown artist', (
    tester,
  ) async {
    events.add({
      'allTracks': [first],
    });
    await tester.pumpWidget(
      MaterialApp(
        home: CollectionPage(
          model: model,
          data: {'kind': 'all', 'title': 'Songs'},
          open: (_, [data = const {}]) {},
        ),
      ),
    );
    await tester.pumpAndSettle();

    expect(find.text('Echoes'), findsWidgets);
    expect(calls.where((call) => call.method == 'collectionTracks'), isEmpty);
  });

  testWidgets('a song links to its album by the native album key', (
    tester,
  ) async {
    albumKeyAnswer = 'a record\u001fan artist';
    RowData? opened;

    await openAlbumOf(model, first, (page, [data = const {}]) => opened = data);

    final asked = calls.lastWhere((call) => call.method == 'albumKey');
    expect(asked.arguments['album'], 'A record');
    expect(asked.arguments['artist'], first['artist']);
    expect(asked.arguments['albumArtist'], first['albumArtist']);
    expect(opened?['kind'], 'album');
    expect(opened?['key'], 'a record\u001fan artist');

    albumKeyAnswer = null;
    opened = null;
    await openAlbumOf(model, first, (page, [data = const {}]) => opened = data);
    expect(opened, isNull);
  });

  testWidgets('most played rows play from the row tapped', (tester) async {
    tester.view.physicalSize = const Size(800, 2400);
    tester.view.devicePixelRatio = 1;
    addTearDown(tester.view.resetPhysicalSize);
    addTearDown(tester.view.resetDevicePixelRatio);
    final second = {...first, 'id': '2', 'title': 'Another song'};
    events.add({
      'statistics': {
        'topTracks': [first, second],
      },
    });
    await tester.pumpWidget(
      MaterialApp(
        home: SettingsDetail(
          model: model,
          page: 'statistics',
          open: (_, [data = const {}]) {},
        ),
      ),
    );
    await tester.pumpAndSettle();
    await tester.ensureVisible(find.text('Another song'));
    await tester.tap(find.text('Another song'));
    await tester.pump();

    final play = calls.lastWhere((call) => call.method == 'play');
    expect(play.arguments['ids'], [first['id'], '2']);
    expect(play.arguments['index'], 1);
  });

  testWidgets('a mix opens as its own page and never sets the global filter', (
    tester,
  ) async {
    String? page;
    RowData? details;
    await tester.pumpWidget(
      MaterialApp(
        home: SettingsDetail(
          model: model,
          page: 'suggestions',
          open: (opened, [data = const {}]) {
            page = opened;
            details = data;
          },
        ),
      ),
    );
    await tester.pumpAndSettle();
    await tester.tap(find.text('Never heard'));
    await tester.pump();

    expect(page, 'collection');
    expect(details?['kind'], 'mix');
    expect(details?['unplayed'], isTrue);
    expect(calls.where((call) => call.method == 'filter'), isEmpty);
  });

  testWidgets('a notice shows on pushed pages whether or not they have a bar', (
    tester,
  ) async {
    await mount(tester);
    await tester.tap(find.text('Library'));
    await tester.pumpAndSettle();
    await tester.tap(find.byTooltip('Library tools'));
    await tester.pumpAndSettle();

    events.add({'status': 'The folder could not be removed'});
    await tester.pumpAndSettle();
    expect(find.text('The folder could not be removed'), findsOneWidget);

    await tester.tap(find.text('Analysis'));
    await tester.pumpAndSettle();
    expect(find.text('The folder could not be removed'), findsOneWidget);
    expect(find.text('Dismiss'), findsOneWidget);
  });

  testWidgets('a playlist wears its covers, or an icon with none', (
    tester,
  ) async {
    final second = {...first, 'id': '2'};
    await mount(tester);
    events.add({
      'allTracks': [first, second],
      'playlists': [
        {
          'id': '7',
          'name': 'Covered',
          'songs': '2',
          'revision': '1',
          'covers': [first['id'], '2'],
        },
        {
          'id': '8',
          'name': 'Bare',
          'songs': '0',
          'revision': '1',
          'covers': <dynamic>[],
        },
      ],
    });
    await tester.tap(find.text('Library'));
    await tester.pumpAndSettle();

    final covers = find.byType(PlaylistCover);
    expect(covers, findsNWidgets(2));
    expect(
      find.descendant(of: covers.first, matching: find.byType(Artwork)),
      findsNWidgets(2),
    );
    expect(
      find.descendant(of: covers.last, matching: find.byType(Artwork)),
      findsNothing,
    );
    expect(
      find.descendant(
        of: covers.last,
        matching: find.byIcon(Icons.queue_music_rounded),
      ),
      findsOneWidget,
    );
  });

  testWidgets('a recognition belongs to the song it was asked of', (
    tester,
  ) async {
    final candidate = {
      'score': 90,
      'song': {'title': 'Another song', 'artist': 'Someone'},
    };
    events.add({
      'recognition': {
        'trackId': '999',
        'candidates': [candidate],
      },
    });
    await tester.pumpWidget(
      MaterialApp(
        home: AnalysisPage(model: model, track: first),
      ),
    );
    await tester.pumpAndSettle();
    expect(find.text('Another song'), findsNothing);

    events.add({
      'recognition': {
        'trackId': first['id'],
        'candidates': [candidate],
      },
    });
    await tester.pumpAndSettle();
    expect(find.text('Another song'), findsOneWidget);
  });

  testWidgets('a library search that is cut says how many it shows', (
    tester,
  ) async {
    searchAnswer = {
      'tracks': [first],
      'trackCount': 700,
      'albums': <dynamic>[],
      'artists': <dynamic>[],
      'reads': <dynamic>[],
    };
    await tester.pumpWidget(
      MaterialApp(
        home: SearchPage(model: model, open: (_, [data = const {}]) {}),
      ),
    );
    await tester.enterText(find.byType(TextField), 'echo');
    await tester.pump(const Duration(milliseconds: 300));
    await tester.pumpAndSettle();

    expect(find.text('Songs (1 of 700)'), findsOneWidget);
    await tester.tap(find.text('Songs (1 of 700)'));
    await tester.pumpAndSettle();
    expect(
      find.textContaining('Showing the first 1 of 700 songs'),
      findsOneWidget,
    );
  });

  testWidgets('an online album opens its editions and an edition its songs', (
    tester,
  ) async {
    events.add({'online': true});
    await tester.pumpWidget(
      MaterialApp(
        home: SearchPage(model: model, open: (_, [data = const {}]) {}),
      ),
    );
    await tester.tap(find.text('Online'));
    await tester.pumpAndSettle();
    await tester.enterText(find.byType(TextField), 'meddle');
    await tester.tap(find.text('Albums'));
    await tester.pump();
    events.add({
      'onlineAlbums': [
        {
          'id': 'group-1',
          'title': 'Meddle',
          'artist': 'Pink Floyd',
          'date': '1971',
        },
      ],
    });
    await tester.pumpAndSettle();

    await tester.tap(find.text('Meddle'));
    await tester.pumpAndSettle();
    expect(
      calls.lastWhere((call) => call.method == 'onlineReleases').arguments,
      {'id': 'group-1'},
    );
    events.add({
      'onlineReleases': [
        {
          'id': 'release-1',
          'title': 'Meddle',
          'date': '1971-10-30',
          'country': 'GB',
        },
      ],
    });
    await tester.pumpAndSettle();

    await tester.tap(find.text('1971-10-30 · GB'));
    await tester.pumpAndSettle();
    expect(calls.lastWhere((call) => call.method == 'releaseSongs').arguments, {
      'id': 'release-1',
    });
    events.add({
      'onlineSongs': [
        {
          'recording': 'rec-1',
          'title': 'Echoes',
          'artist': 'Pink Floyd',
          'album': 'Meddle',
        },
      ],
    });
    await tester.pumpAndSettle();

    expect(find.text('Echoes'), findsOneWidget);
    await tester.tap(find.byTooltip('Back to editions'));
    await tester.pumpAndSettle();
    expect(find.text('1971-10-30 · GB'), findsOneWidget);
  });

  testWidgets('an online artist lists its albums a page at a time', (
    tester,
  ) async {
    events.add({'online': true});
    await tester.pumpWidget(
      MaterialApp(
        home: SearchPage(model: model, open: (_, [data = const {}]) {}),
      ),
    );
    await tester.tap(find.text('Online'));
    await tester.pumpAndSettle();
    await tester.enterText(find.byType(TextField), 'floyd');
    await tester.tap(find.text('Artists'));
    await tester.pump();
    events.add({
      'onlineArtists': [
        {'id': 'artist-1', 'name': 'Pink Floyd', 'kind': 'Group'},
      ],
    });
    await tester.pumpAndSettle();

    await tester.tap(find.text('Pink Floyd'));
    await tester.pumpAndSettle();
    expect(calls.lastWhere((call) => call.method == 'artistAlbums').arguments, {
      'id': 'artist-1',
    });
    events.add({
      'onlineAlbums': [
        {'id': 'g1', 'title': 'Meddle', 'artist': 'Pink Floyd', 'date': '1971'},
      ],
      'artistAlbumsNext': 25,
    });
    await tester.pumpAndSettle();

    expect(find.text('Albums by Pink Floyd'), findsOneWidget);
    await tester.tap(find.text('More albums'));
    await tester.pump();
    expect(calls.lastWhere((call) => call.method == 'artistAlbums').arguments, {
      'id': 'artist-1',
      'from': 25,
    });
  });

  testWidgets('a pasted link shows its songs while the field is empty', (
    tester,
  ) async {
    events.add({'online': true});
    await tester.pumpWidget(
      MaterialApp(
        home: SearchPage(model: model, open: (_, [data = const {}]) {}),
      ),
    );
    await tester.tap(find.text('Online'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Link'));
    await tester.pumpAndSettle();
    await tester.enterText(
      find.byType(TextField).last,
      'https://example.test/x',
    );
    await tester.tap(find.text('Open'));
    await tester.pumpAndSettle();
    expect(calls.lastWhere((call) => call.method == 'openLink').arguments, {
      'text': 'https://example.test/x',
    });

    events.add({
      'onlineCollection': {
        'name': 'Shared record',
        'songs': [
          {
            'recording': 'rec-1',
            'title': 'Echoes',
            'artist': 'Pink Floyd',
            'album': 'Meddle',
          },
        ],
        'releases': <dynamic>[],
        'artists': <dynamic>[],
      },
    });
    await tester.pumpAndSettle();

    expect(find.text('Shared record'), findsOneWidget);
    expect(find.text('Echoes'), findsOneWidget);
    await tester.tap(find.byTooltip('Close link'));
    await tester.pump();
    expect(calls.any((call) => call.method == 'clearCollection'), isTrue);
  });

  test(
    'recent searches are kept natively, newest first and without repeats',
    () {
      events.add({
        'recentSearches': ['floyd', 'air'],
      });

      model.keepRecentSearch('  AIR ');
      expect(model.recentSearches, ['AIR', 'floyd']);
      expect(calls.last.method, 'recentSearches');
      expect(calls.last.arguments, {
        'values': ['AIR', 'floyd'],
      });

      for (var i = 0; i < 12; i++) {
        model.keepRecentSearch('search $i');
      }
      expect(model.recentSearches.length, recentSearchesKept);
      expect(model.recentSearches.first, 'search 11');

      model.keepRecentSearch('   ');
      expect(model.recentSearches.first, 'search 11');

      model.forgetRecentSearches();
      expect(model.recentSearches, isEmpty);
      expect(calls.last.arguments, {'values': <String>[]});
    },
  );

  testWidgets('a library search lights the matched text', (tester) async {
    searchAnswer = {
      'tracks': [first],
      'trackCount': 1,
      'albums': <dynamic>[],
      'artists': <dynamic>[],
      'reads': <dynamic>[],
    };
    highlightsAnswer = [
      [0, 2],
      <dynamic>[],
    ];
    await tester.pumpWidget(
      MaterialApp(
        home: SearchPage(model: model, open: (_, [data = const {}]) {}),
      ),
    );
    await tester.enterText(find.byType(TextField), 'ec');
    await tester.pump(const Duration(milliseconds: 300));
    await tester.pumpAndSettle();

    final asked = calls.lastWhere((call) => call.method == 'highlights');
    expect(asked.arguments['query'], 'ec');
    expect(asked.arguments['items'], [
      {'column': 'TITLE', 'text': 'Echoes'},
      {'column': 'ARTIST', 'text': 'An artist / with punctuation'},
    ]);
    final lit = tester
        .widgetList<RichText>(find.byType(RichText))
        .firstWhere((text) => text.text.toPlainText() == 'Echoes');
    final spans = <TextSpan>[];
    lit.text.visitChildren((span) {
      if (span is TextSpan && span.text != null) spans.add(span);
      return true;
    });
    expect(spans.map((span) => span.text), ['Ec', 'hoes']);
    expect(spans.first.style!.fontWeight, FontWeight.w800);
    expect(spans.last.style!.fontWeight, isNot(FontWeight.w800));
  });

  testWidgets('a song sheet shares the link and removes a queue row', (
    tester,
  ) async {
    Future<void> openSheet({Object? queueId}) async {
      await tester.pumpWidget(
        MaterialApp(
          home: Builder(
            builder: (context) => TextButton(
              onPressed: () => songActions(
                context,
                model,
                first,
                (_, [data = const {}]) {},
                queueId: queueId,
              ),
              child: const Text('open'),
            ),
          ),
        ),
      );
      await tester.tap(find.text('open'));
      await tester.pumpAndSettle();
    }

    await openSheet();
    expect(find.text('Remove from queue'), findsNothing);
    await tester.ensureVisible(find.text('Share link'));
    await tester.tap(find.text('Share link'));
    await tester.pumpAndSettle();
    expect(calls.last.method, 'shareLink');
    expect(calls.last.arguments, {'id': first['id']});

    await openSheet(queueId: '81');
    await tester.ensureVisible(find.text('Remove from queue'));
    await tester.tap(find.text('Remove from queue'));
    await tester.pumpAndSettle();
    expect(calls.last.method, 'removeFromQueue');
    expect(calls.last.arguments, {'id': '81'});
  });

  testWidgets('status, scan and sync banners are live regions', (tester) async {
    await mount(tester, populated: true);
    bool live(String label) => tester
        .widgetList<Semantics>(find.byType(Semantics))
        .any(
          (semantics) =>
              semantics.properties.liveRegion == true &&
              semantics.properties.label == label,
        );

    events.add({'scanning': true});
    await tester.pump(const Duration(milliseconds: 300));
    expect(live('Reading music folder'), isTrue);
    events.add({'scanning': false});

    events.add({
      'syncConfig': {'key': 'code', 'enabled': true},
      'syncStatus': {
        'standing': 'CONNECTED',
        'transfers': [
          {
            'title': 'Solaris',
            'artist': 'Artist',
            'size': 1000,
            'received': 640,
            'state': 'MOVING',
          },
        ],
      },
    });
    await tester.pumpAndSettle();
    expect(live('Fetching songs from your devices'), isTrue);

    events.add({'status': 'Kept 4 songs from your devices'});
    await tester.pumpAndSettle();
    final notice = tester
        .widgetList<Semantics>(find.byType(Semantics))
        .where((semantics) => semantics.properties.liveRegion == true);
    expect(notice.length, greaterThanOrEqualTo(2));
  });

  testWidgets('a long lyric sheet builds only the lines near the one read', (
    tester,
  ) async {
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: SizedBox(
            height: 600,
            child: LyricsPane(model: model, track: first),
          ),
        ),
      ),
    );
    events.add({
      'snapshot': {'current': first, 'transport': 'PLAYING'},
      'lyrics': {
        'trackId': first['id'],
        'detail': 'SYNCED',
        'lines': [
          for (var i = 0; i < 400; i++) {'text': 'Line $i', 'atMs': i * 1000},
        ],
      },
      'lyricFrame': {
        'readAt': 0,
        'active': [0],
        'wipes': <dynamic>[],
      },
    });
    await tester.pumpAndSettle();
    expect(find.text('Line 0'), findsOneWidget);
    expect(find.byType(LyricWords).evaluate().length, lessThan(60));

    events.add({
      'lyricFrame': {
        'readAt': 300,
        'active': [300],
        'wipes': <dynamic>[],
      },
    });
    await tester.pumpAndSettle();
    expect(find.text('Line 300'), findsOneWidget);
    expect(find.text('Line 0'), findsNothing);
    expect(find.byType(LyricWords).evaluate().length, lessThan(60));
  });

  testWidgets('room correction is chosen, switched and forgotten', (
    tester,
  ) async {
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: SingleChildScrollView(
            child: RoomCorrectionSection(model: model),
          ),
        ),
      ),
    );
    expect(find.text('Choose a response…'), findsOneWidget);
    expect(
      tester.widget<SwitchListTile>(find.byType(SwitchListTile)).onChanged,
      isNull,
    );

    await tester.tap(find.text('Choose a response…'));
    expect(calls.last.method, 'chooseRoomResponse');

    events.add({
      'room': {
        'on': true,
        'name': 'Living room',
        'reading': false,
        'channels': 2,
        'rate': 48000,
        'lengthMs': 1250,
      },
    });
    await tester.pumpAndSettle();
    expect(find.text('Living room · Stereo · 48 kHz · 1.25 s'), findsOneWidget);
    expect(find.text('Choose another response…'), findsOneWidget);

    await tester.tap(find.byType(SwitchListTile));
    expect(calls.last.method, 'roomCorrection');
    expect(calls.last.arguments, {'enabled': false});

    await tester.tap(find.text('Forget the response'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Forget'));
    await tester.pumpAndSettle();
    expect(calls.last.method, 'forgetRoomCorrection');
  });
}
