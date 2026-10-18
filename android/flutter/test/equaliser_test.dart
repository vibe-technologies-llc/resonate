import 'dart:async';
import 'dart:ui' as ui;

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:resonate_android/model.dart';
import 'package:resonate_android/settings.dart';

const centres = [
  31.25,
  62.5,
  125.0,
  250.0,
  500.0,
  1000.0,
  2000.0,
  4000.0,
  8000.0,
  16000.0,
];

RowData preset(String name, String detail, double preamp, List<double> gains) =>
    {
      'name': name,
      'detail': detail,
      'curve': {
        'name': gains.isEmpty ? '' : name,
        'preampDb': preamp,
        'bands': [
          for (var i = 0; i < gains.length; i++)
            {
              'filter': 'PEAKING',
              'frequencyHz': centres[i],
              'gainDb': gains[i],
              'q': 1.4142135,
            },
        ],
      },
    };

final presets = [
  preset('Flat', 'An uncoloured starting point.', 0, []),
  preset('Bass boost', 'More weight and punch in the low end.', -6, [
    4, 4, 2, 0, -1, -1, 0, 0, 0, 0,
  ]),
  preset(
    'Bass only',
    'More bass. Mids, treble and output gain stay unchanged.',
    0,
    [4, 4, 2, 0, 0, 0, 0, 0, 0, 0],
  ),
  preset('Bass cut', 'Less rumble, with the rest left clear.', 0, [
    -5, -4, -3, -1, 0, 0, 0, 0, 0, 0,
  ]),
  preset('Vocals', 'Bring voices and lead instruments forward.', -5, [
    -2, -1, 0, 1, 2, 3, 3, 1, 0, -1,
  ]),
  preset('Warm', 'Fuller lows and a softer top end.', -5, [
    2, 3, 2, 1, 0, 0, -1, -2, -2, -2,
  ]),
  preset('Bright', 'A little more sparkle and detail.', -5, [
    -1, -1, 0, 0, 0, 1, 2, 3, 3, 2,
  ]),
  preset('Rock', 'Punchy lows, crisp guitars and drums.', -5, [
    3, 2, 0, -1, -2, 0, 2, 3, 3, 2,
  ]),
  preset('Electronic', 'Deep bass with a lively, open top end.', -6, [
    4, 3, 1, 0, -2, -1, 1, 2, 3, 3,
  ]),
];

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  const channel = MethodChannel('resonate/eq-test');
  late StreamController<dynamic> events;
  late PlayerModel model;
  late List<MethodCall> calls;
  late RowData preferences;

  setUp(() {
    events = StreamController<dynamic>.broadcast(sync: true);
    calls = [];
    preferences = {
      'equaliserEnabled': false,
      'equaliser': {'name': '', 'preampDb': 0.0, 'bands': []},
    };
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(channel, (call) async {
          calls.add(call);
          final args = row(call.arguments);
          switch (call.method) {
            case 'equaliserInfo':
              return {
                'limits': {
                  'bandsAtMost': 32,
                  'preampAtMostDb': 24.0,
                  'gainAtMostDb': 24.0,
                },
                'frequencies': centres,
                'response': [0.0, 0.0],
                'presets': presets,
              };
            case 'fittedPreamp':
              return -6.25;
            case 'equaliser':
              preferences = {
                ...preferences,
                'equaliser': {
                  'name': args['name'],
                  'preampDb': args['preamp'],
                  'bands': args['bands'],
                },
              };
              events.add({'preferences': preferences});
            case 'preferences':
              preferences = {...preferences, ...args};
              events.add({'preferences': preferences});
          }
          return null;
        });
    model = PlayerModel(methods: channel, events: events.stream);
  });

  tearDown(() async {
    model.dispose();
    await events.close();
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(channel, null);
  });

  Future<void> mount(
    WidgetTester tester, {
    double textScale = 1,
    Brightness brightness = Brightness.light,
  }) async {
    events.add({'preferences': preferences});
    await tester.pumpWidget(
      MaterialApp(
        theme: ThemeData(useMaterial3: true, brightness: brightness),
        home: MediaQuery(
          data: MediaQueryData(
            size: tester.view.physicalSize,
            textScaler: TextScaler.linear(textScale),
          ),
          child: Scaffold(body: EqualiserPane(model: model)),
        ),
      ),
    );
    await tester.pumpAndSettle();
  }

  testWidgets('presets apply through the native curve and survive reopening', (
    tester,
  ) async {
    await mount(tester);
    for (final name in [
      'Flat',
      'Bass boost',
      'Bass only',
      'Bass cut',
      'Vocals',
      'Warm',
      'Bright',
      'Rock',
      'Electronic',
    ]) {
      final chip = find.widgetWithText(ChoiceChip, name);
      await tester.ensureVisible(chip);
      calls.clear();
      await tester.tap(chip);
      await tester.pumpAndSettle();
      expect(calls.where((call) => call.method == 'equaliser'), hasLength(1));
      expect(preferences['equaliserEnabled'], true);
      final curve = row(preferences['equaliser']);
      expect(rows(curve['bands']), hasLength(name == 'Flat' ? 0 : 10));
      expect(decimal(curve['preampDb']), lessThanOrEqualTo(0));
      expect(tester.widget<ChoiceChip>(chip).selected, true);
    }
    await tester.pumpWidget(const SizedBox.shrink());
    await mount(tester);
    expect(
      tester
          .widget<ChoiceChip>(find.widgetWithText(ChoiceChip, 'Electronic'))
          .selected,
      true,
    );
    expect(find.byType(DropdownMenu<String>), findsNothing);
    expect(tester.takeException(), isNull);
  });

  testWidgets(
    'Bass only raises the lows without reducing other bands or the preamp',
    (tester) async {
      preferences['equaliser'] = {
        'name': 'Old profile',
        'preampDb': -8.0,
        'bands': [],
      };
      await mount(tester);
      await tester.tap(find.widgetWithText(ChoiceChip, 'Bass only'));
      await tester.pumpAndSettle();
      final curve = row(preferences['equaliser']);
      expect(curve['preampDb'], 0.0);
      final bands = rows(curve['bands']);
      expect(bands, hasLength(10));
      for (final band in bands) {
        expect(
          decimal(band['gainDb']),
          decimal(band['frequencyHz']) <= 125 ? greaterThan(0) : 0.0,
        );
      }
      expect(
        tester
            .widget<ChoiceChip>(find.widgetWithText(ChoiceChip, 'Bass only'))
            .selected,
        true,
      );
    },
  );

  final importedBands = [
    {'filter': 'LOW_SHELF', 'frequencyHz': 100.0, 'gainDb': 3.0, 'q': 0.7},
    {'filter': 'NOTCH', 'frequencyHz': 2000.0, 'gainDb': 0.0, 'q': 4.0},
  ];

  testWidgets(
    'frequency sliders have names and commit accessibility adjustments',
    (tester) async {
      final semantics = tester.ensureSemantics();
      preferences['equaliser'] = {
        'name': 'My headphones',
        'preampDb': -4.0,
        'bands': importedBands,
      };
      await mount(tester);
      await tester.ensureVisible(find.byType(Slider).first);
      await tester.pumpAndSettle();
      final node = tester.getSemantics(find.bySemanticsLabel('100 Hz Bass'));
      expect(
        node.getSemanticsData().hasAction(ui.SemanticsAction.increase),
        true,
      );
      node.owner!.performAction(node.id, ui.SemanticsAction.increase);
      await tester.pumpAndSettle();
      expect(calls.where((call) => call.method == 'equaliser'), hasLength(1));
      expect(
        decimal(rows(row(preferences['equaliser'])['bands']).first['gainDb']),
        greaterThan(3),
      );
      semantics.dispose();
    },
  );

  testWidgets(
    'a slider commits on release and preserves imported filters and preamp',
    (tester) async {
      preferences['equaliser'] = {
        'name': 'My headphones',
        'preampDb': -4.0,
        'bands': importedBands,
      };
      await mount(tester);
      final slider = find.byType(Slider).first;
      await tester.ensureVisible(slider);
      calls.clear();
      final gesture = await tester.startGesture(tester.getCenter(slider));
      await gesture.moveBy(const Offset(50, 0));
      await tester.pump();
      expect(calls.where((call) => call.method == 'equaliser'), isEmpty);
      await gesture.up();
      await tester.pumpAndSettle();
      expect(calls.where((call) => call.method == 'equaliser'), hasLength(1));
      final curve = row(preferences['equaliser']);
      final bands = rows(curve['bands']);
      expect(curve['name'], '');
      expect(curve['preampDb'], -4.0);
      expect(bands, hasLength(2));
      expect(bands.first['filter'], 'LOW_SHELF');
      expect(bands.first['frequencyHz'], 100.0);
      expect(bands.first['q'], 0.7);
      expect(bands.first['gainDb'], isNot(3.0));
      expect(bands.last, importedBands.last);
      expect(tester.takeException(), isNull);
    },
  );

  testWidgets('advanced bands are selectable without touching the graph', (
    tester,
  ) async {
    preferences['equaliser'] = {
      'name': 'My headphones',
      'preampDb': -4.0,
      'bands': importedBands,
    };
    await mount(tester);
    await tester.ensureVisible(find.text('Advanced'));
    await tester.tap(find.text('Advanced'));
    await tester.pumpAndSettle();
    await tester.scrollUntilVisible(
      find.text('2 · 2.00 kHz'),
      200,
      scrollable: find.byType(Scrollable).first,
    );
    await tester.tap(find.text('2 · 2.00 kHz'));
    await tester.pumpAndSettle();
    await tester.scrollUntilVisible(
      find.text('Band 2'),
      -150,
      scrollable: find.byType(Scrollable).first,
    );
    expect(find.text('Band 2'), findsOneWidget);
    expect(calls.where((call) => call.method == 'equaliser'), isEmpty);
    expect(preferences['equaliser'], {
      'name': 'My headphones',
      'preampDb': -4.0,
      'bands': importedBands,
    });
    expect(tester.takeException(), isNull);
  });

  testWidgets('fit the preamp asks the native peak and keeps every band', (
    tester,
  ) async {
    preferences['equaliser'] = {
      'name': 'My headphones',
      'preampDb': 2.0,
      'bands': importedBands,
    };
    await mount(tester);
    final fit = find.widgetWithText(TextButton, 'Fit the preamp');
    await tester.scrollUntilVisible(
      fit,
      200,
      scrollable: find.byType(Scrollable).first,
    );
    calls.clear();
    await tester.tap(fit);
    await tester.pumpAndSettle();
    final asked = calls.firstWhere((call) => call.method == 'fittedPreamp');
    expect(row(asked.arguments)['preamp'], 2.0);
    expect(rows(row(asked.arguments)['bands']), hasLength(2));
    final curve = row(preferences['equaliser']);
    expect(curve['preampDb'], -6.25);
    expect(curve['bands'], importedBands);
    expect(tester.takeException(), isNull);
  });

  testWidgets('reset can be cancelled and keeps the enable switch unchanged', (
    tester,
  ) async {
    preferences['equaliser'] = {
      'name': 'My headphones',
      'preampDb': -4.0,
      'bands': importedBands,
    };
    await mount(tester);
    await tester.tap(find.text('Reset'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Cancel'));
    await tester.pumpAndSettle();
    expect(calls.where((call) => call.method == 'equaliser'), isEmpty);
    await tester.tap(find.text('Reset'));
    await tester.pumpAndSettle();
    await tester.tap(find.widgetWithText(FilledButton, 'Reset'));
    await tester.pumpAndSettle();
    expect(preferences['equaliserEnabled'], false);
    expect(preferences['equaliser'], {
      'name': '',
      'preampDb': 0.0,
      'bands': [],
    });
    expect(tester.takeException(), isNull);
  });

  for (final brightness in Brightness.values) {
    testWidgets('EQ fits a narrow screen with large text in $brightness', (
      tester,
    ) async {
      tester.view.physicalSize = const Size(320, 760);
      tester.view.devicePixelRatio = 1;
      addTearDown(tester.view.resetPhysicalSize);
      addTearDown(tester.view.resetDevicePixelRatio);
      await mount(tester, textScale: 2, brightness: brightness);
      await tester.scrollUntilVisible(
        find.text('Bass boost'),
        150,
        scrollable: find.byType(Scrollable).first,
      );
      final preset = find.widgetWithText(ChoiceChip, 'Bass boost');
      await tester.ensureVisible(preset);
      await tester.pumpAndSettle();
      expect(preset.hitTestable(), findsOneWidget);
      await tester.tap(preset);
      await tester.pumpAndSettle();
      expect(row(preferences['equaliser'])['name'], 'Bass boost');
      await tester.scrollUntilVisible(
        find.text('Sliders'),
        150,
        scrollable: find.byType(Scrollable).first,
      );
      expect(find.text('Advanced'), findsOneWidget);
      expect(tester.takeException(), isNull);
    });
  }
}
