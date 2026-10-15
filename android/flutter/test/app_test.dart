import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:resonate_android/library.dart';
import 'package:resonate_android/main.dart';
import 'package:resonate_android/model.dart';
import 'package:resonate_android/player.dart';
import 'package:resonate_android/tools.dart';
import 'package:resonate_android/settings.dart';
import 'package:resonate_android/visuals.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  const channel = MethodChannel('resonate/test');
  late StreamController<dynamic> events;
  late PlayerModel model;
  late List<MethodCall> calls;
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
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(channel, (call) async {
          calls.add(call);
          return switch (call.method) {
            'collections' => {'albums': [], 'artists': []},
            'collectionTracks' => [first],
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

  testWidgets('new status replaces stale scan feedback', (tester) async {
    await mount(tester, populated: true);
    events.add({'status': 'Reading music folder…'});
    await tester.pumpAndSettle();
    expect(find.text('Reading music folder…'), findsOneWidget);
    events.add({'status': 'Read 1 song'});
    await tester.pumpAndSettle();
    expect(find.text('Reading music folder…'), findsNothing);
    expect(find.text('Read 1 song'), findsOneWidget);
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
      await tester.ensureVisible(find.text('Save sync settings'));
      await tester.tap(find.text('Save sync settings'));
      await tester.pump();
      expect(
        calls
            .lastWhere((call) => call.method == 'sync')
            .arguments['nearbyOnly'],
        isTrue,
      );
      await tester.ensureVisible(find.text('Show pairing code'));
      await tester.tap(find.text('Show pairing code'));
      await tester.pumpAndSettle();
      expect(find.text(code), findsOneWidget);
      await tester.tap(find.text('Hide pairing code'));
      await tester.pumpAndSettle();
      expect(find.text(code), findsNothing);
    },
  );
}
