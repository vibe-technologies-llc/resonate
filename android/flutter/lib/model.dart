import 'dart:async';
import 'dart:collection';

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';

typedef RowData = Map<String, dynamic>;

RowData row(Object? value) => value is Map
    ? value.map((key, value) => MapEntry(key.toString(), value))
    : {};
List<RowData> rows(Object? value) =>
    value is List ? value.map(row).toList() : [];
int integer(Object? value) => int.tryParse('$value') ?? 0;
double decimal(Object? value) => double.tryParse('$value') ?? 0;
String title(RowData track) => '${track['title'] ?? ''}'.isNotEmpty
    ? '${track['title']}'
    : '${track['name'] ?? 'Unknown song'}';
String artist(RowData track) => '${track['artist'] ?? ''}'.isNotEmpty
    ? '${track['artist']}'
    : 'Unknown artist';
String clock(Object? milliseconds) {
  final seconds = integer(milliseconds) ~/ 1000;
  return '${seconds ~/ 60}:${(seconds % 60).toString().padLeft(2, '0')}';
}

String humanize(String value) => value.toLowerCase().split('_').join(' ');

class PlayerModel extends ChangeNotifier {
  PlayerModel({MethodChannel? methods, Stream<dynamic>? events})
    : _methods = methods ?? const MethodChannel('resonate/player') {
    _subscription =
        (events ??
                const EventChannel('resonate/state').receiveBroadcastStream())
            .listen(
              _receive,
              onError: (Object error) {
                failure = error.toString();
                notifyListeners();
              },
            );
  }

  final MethodChannel _methods;
  late final StreamSubscription<dynamic> _subscription;
  final RowData state = {};
  String? failure;
  bool connected = false;
  final LinkedHashMap<String, Future<Uint8List?>> _pictures = LinkedHashMap();
  List<RowData> albums = [];
  List<RowData> artists = [];
  String _collectionRevision = '';
  int _collectionsRequest = 0;

  List<RowData> _tracks = [];
  List<RowData> get tracks => _tracks;
  List<RowData> _allTracks = [];
  Map<String, RowData> _byId = {};
  List<RowData> get allTracks => _allTracks;
  List<RowData> _queue = [];
  List<RowData> get queue => _queue;
  RowData get snapshot => row(state['snapshot']);
  RowData get current => row(snapshot['current']);
  RowData get preferences => row(state['preferences']);
  bool get playing => ['PLAYING', 'LOADING'].contains(snapshot['transport']);
  bool get online => state['online'] == true;
  String? get notice => failure ?? state['status'] as String?;

  void _receive(dynamic event) {
    final patch = row(event);
    state.addAll(patch);
    if (patch.containsKey('tracks')) _tracks = rows(patch['tracks']);
    if (patch.containsKey('allTracks')) {
      _allTracks = rows(patch['allTracks']);
      _byId = {for (final track in _allTracks) '${track['id']}': track};
    }
    if (patch.containsKey('queue')) _queue = rows(patch['queue']);
    connected = true;
    if (patch.containsKey('artworkGeneration')) _pictures.clear();
    final revision =
        '${snapshot['catalogRevision']}:${state['artworkGeneration']}';
    if (patch.containsKey('allTracks') ||
        patch.containsKey('artworkGeneration') ||
        (patch.containsKey('snapshot') && revision != _collectionRevision)) {
      _collectionRevision = revision;
      unawaited(refreshCollections());
    }
    notifyListeners();
  }

  Future<dynamic> call(String method, [RowData arguments = const {}]) async {
    try {
      final answer = await _methods.invokeMethod<dynamic>(method, arguments);
      final clearedFailure = failure != null;
      failure = null;
      if (clearedFailure) notifyListeners();
      return answer;
    } on PlatformException catch (error) {
      failure = error.message ?? 'The action could not be completed';
      notifyListeners();
      return null;
    } on MissingPluginException {
      failure = 'The Android player is unavailable';
      notifyListeners();
      return null;
    }
  }

  Future<void> refreshCollections() async {
    final request = ++_collectionsRequest;
    final result = row(await call('collections'));
    if (request != _collectionsRequest || result.isEmpty) return;
    albums = rows(result['albums']);
    artists = rows(result['artists']);
    notifyListeners();
  }

  Future<Uint8List?> artwork(RowData track) {
    final key =
        '${track['id']}:${track['artRevision']}:${state['artworkGeneration']}';
    final cached = _pictures.remove(key);
    if (cached != null) {
      _pictures[key] = cached;
      return cached;
    }
    final future = _methods
        .invokeMethod<Uint8List>('artwork', {'id': track['id']})
        .catchError((Object _) => null);
    _pictures[key] = future;
    while (_pictures.length > 48) {
      _pictures.remove(_pictures.keys.first);
    }
    return future;
  }

  RowData trackById(Object? id) => _byId['$id'] ?? {};

  Future<void> play(List<RowData> tracks, [int index = 0]) async {
    if (tracks.isEmpty) return;
    await call('play', {
      'ids': tracks.map((track) => track['id']).toList(),
      'index': index,
    });
  }

  Future<void> dismissNotice() async {
    failure = null;
    await call('dismissStatus');
    notifyListeners();
  }

  @override
  void dispose() {
    _subscription.cancel();
    super.dispose();
  }
}
