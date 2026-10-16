import 'dart:async';
import 'dart:collection';
import 'dart:math' as math;

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

class SyncActivity {
  const SyncActivity.syncing() : songs = 0, fraction = null, isFetching = false;
  const SyncActivity.fetching({required this.songs, this.fraction})
    : isFetching = true;

  final int songs;
  final double? fraction;
  final bool isFetching;
}

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
  final ValueNotifier<int> positionMs = ValueNotifier(0);
  final ValueNotifier<int?> sleepRemainingMs = ValueNotifier(null);
  String? failure;
  bool connected = false;
  final LinkedHashMap<String, Future<Uint8List?>> _pictures = LinkedHashMap();
  List<RowData> albums = [];
  List<RowData> artists = [];
  String _collectionRevision = '';
  int _collectionsRequest = 0;

  List<RowData> _tracks = [];
  bool get hasActiveFilter {
    final filter = row(state['filter']);
    if (filter.isEmpty) return false;
    final text = '${filter['text'] ?? ''}'.trim();
    final genre = '${filter['genre'] ?? ''}'.trim();
    final artist = '${filter['artist'] ?? ''}'.trim();
    final album = '${filter['album'] ?? ''}'.trim();
    final minRate = integer(filter['minimumRate']);
    final minBits = integer(filter['minimumBits']);
    final favourites = filter['favourites'] == true;
    final unplayed = filter['unplayed'] == true;
    final descending = filter['descending'] == true;
    final order = '${filter['order'] ?? ''}'.toUpperCase();
    final nonDefaultOrder = order.isNotEmpty && order != 'TITLE';
    return text.isNotEmpty ||
        genre.isNotEmpty ||
        artist.isNotEmpty ||
        album.isNotEmpty ||
        minRate > 0 ||
        minBits > 0 ||
        favourites ||
        unplayed ||
        descending ||
        nonDefaultOrder;
  }

  List<RowData> get tracks {
    if (_tracks.isNotEmpty) return _tracks;
    if (!hasActiveFilter && _allTracks.isNotEmpty) return _allTracks;
    return _tracks;
  }

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
  DateTime? _syncAskedAt;
  DateTime? get syncAskedAt => _syncAskedAt;

  SyncActivity? get syncActivity {
    final status = row(state['syncStatus']);
    final standing = '${status['standing'] ?? ''}'.toLowerCase();
    if (standing.isEmpty || standing == 'off') return null;

    final transfers = rows(status['transfers']);
    final active = transfers.where((t) {
      final s = '${t['state'] ?? ''}'.toLowerCase();
      return s == 'waiting' || s == 'moving';
    }).toList();

    if (active.isNotEmpty) {
      final sized = active.where((t) => integer(t['size']) > 0).toList();
      final totalSize = sized.fold<int>(
        0,
        (sum, t) => sum + integer(t['size']),
      );
      final received = sized.fold<int>(
        0,
        (sum, t) => sum + math.min(integer(t['received']), integer(t['size'])),
      );
      final fraction = (totalSize == 0 || sized.length != active.length)
          ? null
          : (received / totalSize).clamp(0.0, 1.0);
      return SyncActivity.fetching(songs: active.length, fraction: fraction);
    }

    final asked = _syncAskedAt;
    final recentlyAsked =
        asked != null && DateTime.now().difference(asked).inSeconds < 30;
    if ((recentlyAsked && status['settled'] != true) ||
        standing == 'connecting' ||
        integer(status['moving']) > 0) {
      return const SyncActivity.syncing();
    }
    return null;
  }

  void _receive(dynamic event) {
    final patch = row(event);
    final previousSnapshot = snapshot;
    state.addAll(patch);
    final nextSnapshot = snapshot;
    if (patch.containsKey('snapshot')) {
      positionMs.value = integer(nextSnapshot['positionMs']);
      sleepRemainingMs.value = nextSnapshot['sleepRemainingMs'] == null
          ? null
          : integer(nextSnapshot['sleepRemainingMs']);
    }
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
    if (patch.containsKey('syncStatus')) {
      final status = row(patch['syncStatus']);
      if (status['settled'] == true && _syncAskedAt != null) {
        if (DateTime.now().difference(_syncAskedAt!).inMilliseconds > 800) {
          _syncAskedAt = null;
        }
      }
    }
    if (patch.length != 1 ||
        !patch.containsKey('snapshot') ||
        !_sameSnapshotExceptTime(previousSnapshot, nextSnapshot)) {
      notifyListeners();
    }
  }

  bool _sameSnapshotExceptTime(RowData before, RowData after) {
    if (before.length != after.length) return false;
    for (final entry in after.entries) {
      if (entry.key == 'positionMs' || entry.key == 'sleepRemainingMs') {
        continue;
      }
      if (entry.key == 'current') {
        if (!mapEquals(row(before[entry.key]), row(entry.value))) return false;
      } else if (before[entry.key] != entry.value) {
        return false;
      }
    }
    return true;
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
    state['status'] = null;
    await call('dismissStatus');
    notifyListeners();
  }

  Future<RowData> searchLibrary(String text, [int limit = 500]) async {
    final result = await call('searchLibrary', {'text': text, 'limit': limit});
    return row(result);
  }

  Future<void> forceSync() async {
    _syncAskedAt = DateTime.now();
    notifyListeners();
    await call('forceSync');
  }

  Future<void> forgetSyncDevice(String id) async {
    await call('forgetSyncDevice', {'id': id});
  }

  Future<void> saveQueueAsPlaylist(String name) async {
    await call('saveQueueAsPlaylist', {'name': name});
  }

  Future<void> clearFilters() async {
    state['filter'] = null;
    notifyListeners();
    await call('filter');
    await call('search', {'text': ''});
  }

  @override
  void dispose() {
    _subscription.cancel();
    positionMs.dispose();
    sleepRemainingMs.dispose();
    super.dispose();
  }
}
