import 'dart:io';

import 'package:flutter_test/flutter_test.dart';

const bridge = '../app/src/main/java/org/resonate/android/FlutterBridge.kt';

class Sent {
  const Sent(this.names, this.keys, this.where);

  final Set<String> names;
  final Set<String> keys;
  final String where;
}

int closing(String text, int open) {
  const pairs = {'(': ')', '[': ']', '{': '}'};
  final stack = <String>[];
  var at = open;
  while (at < text.length) {
    final c = text[at];
    if (c == "'" || c == '"') {
      at++;
      while (at < text.length && text[at] != c) {
        if (text[at] == r'\') at++;
        at++;
      }
    } else if (pairs.containsKey(c)) {
      stack.add(pairs[c]!);
    } else if (stack.isNotEmpty && c == stack.last) {
      stack.removeLast();
      if (stack.isEmpty) return at;
    }
    at++;
  }
  throw StateError('Nothing closes the bracket at $open');
}

List<String> topLevel(String text) {
  const opening = '([{';
  const closed = ')]}';
  final parts = <String>[];
  var depth = 0;
  var from = 0;
  for (var at = 0; at < text.length; at++) {
    final c = text[at];
    if (c == "'" || c == '"') {
      at++;
      while (at < text.length && text[at] != c) {
        if (text[at] == r'\') at++;
        at++;
      }
    } else if (opening.contains(c)) {
      depth++;
    } else if (closed.contains(c)) {
      depth--;
    } else if (c == ',' && depth == 0) {
      parts.add(text.substring(from, at));
      from = at + 1;
    }
  }
  if (text.substring(from).trim().isNotEmpty) parts.add(text.substring(from));
  return parts;
}

int firstColon(String entry) {
  var depth = 0;
  for (var at = 0; at < entry.length; at++) {
    final c = entry[at];
    if (c == "'" || c == '"') {
      at++;
      while (at < entry.length && entry[at] != c) {
        if (entry[at] == r'\') at++;
        at++;
      }
    } else if ('([{'.contains(c)) {
      depth++;
    } else if (')]}'.contains(c)) {
      depth--;
    } else if (c == ':' && depth == 0) {
      return at;
    }
  }
  return -1;
}

final literal = RegExp(r"'([A-Za-z]+)'");
final named = RegExp(r'(?:^|\?|:|=>)\s*$');
final sending = RegExp(
  r'(?<![?\w])(?:[\w.]*\.call|call|[\w.]*\.invokeMethod(?:<[\w?<>]+>)?)\(',
);

List<Sent> sentBy(String source, String file) {
  final found = <Sent>[];
  for (final start in sending.allMatches(source)) {
    final open = start.end - 1;
    final arguments = topLevel(
      source.substring(open + 1, closing(source, open)),
    );
    if (arguments.isEmpty) continue;
    final names = {
      for (final m in literal.allMatches(arguments.first))
        if (named.hasMatch(arguments.first.substring(0, m.start))) m.group(1)!,
    };
    if (names.isEmpty) continue;
    final keys = <String>{};
    if (arguments.length > 1) {
      final map = arguments[1].trim();
      if (map.startsWith('{')) {
        for (final entry in topLevel(map.substring(1, map.length - 1))) {
          final text = entry.trim();
          if (text.startsWith('...')) continue;
          final colon = firstColon(text);
          final key = colon < 0
              ? null
              : literal.firstMatch(text.substring(0, colon));
          if (key != null &&
              text.substring(0, colon).trim() == "'${key.group(1)}'") {
            keys.add(key.group(1)!);
          }
        }
      }
    }
    final line = '\n'.allMatches(source.substring(0, start.start)).length + 1;
    found.add(Sent(names, keys, '$file:$line'));
  }
  return found;
}

final reads = [
  RegExp(r'(?:text|number|id|flag)\("(\w+)"\)'),
  RegExp(r'\b(?:args|band|item)\["(\w+)"\]'),
];

Set<String> keysIn(String text) => {
  for (final read in reads)
    for (final m in read.allMatches(text)) m.group(1)!,
};

String helper(String source, String signature) {
  final from = source.indexOf(signature);
  if (from < 0) throw StateError('FlutterBridge has no $signature');
  final next = RegExp(r'\n {4,8}(?:private )?fun ')
      .firstMatch(source.substring(from + signature.length));
  return source.substring(
    from,
    next == null ? source.length : from + signature.length + next.start,
  );
}

Map<String, Set<String>> readBy(String source) {
  final filter = keysIn(helper(source, 'fun filter()'));
  final expansions = <RegExp, Set<String>>{
    RegExp(r'\bids\(\)|\btracks\(\)'): {'ids'},
    RegExp(r'\btrack\(\)'): {'id'},
    RegExp(r'\bid\(\)'): {'id'},
    RegExp(r'\bsong\(\)'): {'recording', 'id'},
    RegExp(r'\bfilter\(\)'): filter,
    RegExp(r'\bprovider\(args\)'): keysIn(
      helper(source, 'private fun provider('),
    ),
    RegExp(r'\bscrobbling\(args\)'): keysIn(
      helper(source, 'private fun scrobbling('),
    ),
    RegExp(r'\bbands\(args\)'): {'bands'},
  };
  final start = source.indexOf('when (call.method) {');
  final body = source.substring(
    start,
    closing(source, source.indexOf('{', start)),
  );
  final branch = RegExp(r'^ {12}"(\w+)" ->', multiLine: true);
  final heads = branch.allMatches(body).toList();
  final else_ = body.indexOf(RegExp(r'^ {12}else ->', multiLine: true));
  final answered = <String, Set<String>>{};
  for (var i = 0; i < heads.length; i++) {
    final end = i + 1 < heads.length ? heads[i + 1].start : else_;
    final text = body.substring(heads[i].start, end);
    final keys = keysIn(text);
    for (final expansion in expansions.entries) {
      if (expansion.key.hasMatch(text)) keys.addAll(expansion.value);
    }
    answered[heads[i].group(1)!] = keys;
  }
  return answered;
}

void main() {
  final kotlin = File(bridge).readAsStringSync();
  final answered = readBy(kotlin);
  final lib = Directory('lib')
      .listSync()
      .whereType<File>()
      .where((f) => f.path.endsWith('.dart'));
  final sent = [
    for (final file in lib)
      ...sentBy(file.readAsStringSync(), file.path.split('/').last),
  ];

  test('the bridge answers more than twenty actions', () {
    expect(answered.length, greaterThan(20));
    expect(sent.length, greaterThan(20));
  });

  test('every action Dart sends is one FlutterBridge answers', () {
    final unanswered = [
      for (final call in sent)
        for (final name in call.names)
          if (!answered.containsKey(name)) '$name at ${call.where}',
    ];
    expect(unanswered, isEmpty);
  });

  test('every action FlutterBridge answers is one Dart names', () {
    final named = {
      for (final file in lib)
        ...literal.allMatches(file.readAsStringSync()).map((m) => m.group(1)!),
    };
    final unasked = answered.keys
        .where((name) => !named.contains(name))
        .toList();
    expect(unasked, isEmpty);
  });

  test('every argument Dart sends by name is one the action reads', () {
    final unread = [
      for (final call in sent)
        for (final key in call.keys)
          if (!call.names.any((name) => answered[name]?.contains(key) ?? true))
            '$key sent to ${call.names.join(' or ')} at ${call.where}',
    ];
    expect(unread, isEmpty);
  });
}
