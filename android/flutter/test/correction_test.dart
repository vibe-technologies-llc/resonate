import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:resonate_android/model.dart';
import 'package:resonate_android/settings.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  const channel = MethodChannel('resonate/correction-test');
  late StreamController<dynamic> events;
  late PlayerModel model;
  late List<MethodCall> calls;

  final found = [
    {
      'id': 'oratory1990/hd650',
      'name': 'Sennheiser HD 650',
      'source': 'oratory1990',
    },
    {'id': 'crinacle/hd650', 'name': 'Sennheiser HD 650', 'source': 'Crinacle'},
  ];

  setUp(() {
    events = StreamController<dynamic>.broadcast(sync: true);
    calls = [];
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(channel, (call) async {
          calls.add(call);
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

  Future<void> mount(WidgetTester tester) async {
    await tester.pumpWidget(
      MaterialApp(
        home: Scaffold(
          body: ListenableBuilder(
            listenable: model,
            builder: (context, _) => CorrectionPane(model: model),
          ),
        ),
      ),
    );
    await tester.pumpAndSettle();
  }

  Future<void> listed(WidgetTester tester) async {
    events.add({'corrections': found, 'correctionBusy': false});
    await tester.pumpAndSettle();
  }

  testWidgets(
    'a search is asked once the typing has stopped, with what was typed',
    (tester) async {
      await mount(tester);
      final field = find.widgetWithText(
        TextField,
        'AutoEq headphone correction',
      );

      await tester.enterText(field, 'HD');
      await tester.pump(const Duration(milliseconds: 200));
      await tester.enterText(field, 'HD 650');
      await tester.pump(const Duration(milliseconds: 200));
      expect(calls.where((call) => call.method == 'corrections'), isEmpty);

      await tester.pump(const Duration(milliseconds: 200));
      final asked = calls.where((call) => call.method == 'corrections');
      expect(asked, hasLength(1));
      expect(asked.single.arguments, {'text': 'HD 650'});
    },
  );

  testWidgets(
    'every answer is a row naming the headphone and where it was measured',
    (tester) async {
      await mount(tester);
      expect(find.byType(ListTile), findsNothing);
      expect(find.byType(LinearProgressIndicator), findsNothing);

      events.add({'correctionBusy': true});
      await tester.pump();
      expect(find.byType(LinearProgressIndicator), findsOneWidget);

      await listed(tester);
      expect(find.byType(LinearProgressIndicator), findsNothing);
      expect(find.text('Sennheiser HD 650'), findsNWidgets(2));
      expect(find.text('oratory1990'), findsOneWidget);
      expect(find.text('Crinacle'), findsOneWidget);
    },
  );

  testWidgets('a correction is applied by its id only after it is confirmed', (
    tester,
  ) async {
    await mount(tester);
    await listed(tester);

    await tester.tap(find.text('Crinacle'));
    await tester.pumpAndSettle();
    expect(find.text('Replace equaliser curve?'), findsOneWidget);
    expect(find.text('Apply Sennheiser HD 650?'), findsOneWidget);
    await tester.tap(find.text('Cancel'));
    await tester.pumpAndSettle();
    expect(calls.where((call) => call.method == 'applyCorrection'), isEmpty);

    await tester.tap(find.text('Crinacle'));
    await tester.pumpAndSettle();
    await tester.tap(find.text('Apply'));
    await tester.pumpAndSettle();
    final applied = calls.where((call) => call.method == 'applyCorrection');
    expect(applied, hasLength(1));
    expect(applied.single.arguments, {'id': 'crinacle/hd650'});
    expect(tester.takeException(), isNull);
  });
}
