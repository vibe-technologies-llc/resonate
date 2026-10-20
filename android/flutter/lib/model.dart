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

int queueLanding(int from, int adjustedTo) =>
    adjustedTo > from ? adjustedTo + 1 : adjustedTo;

const recentSearchesKept = 10;

const _artworkBuckets = [96, 192, 384, 512, 768, 1024];
const defaultArtworkBucket = 768;
const colourSampleBucket = 96;

int artworkBucket(double? logicalSize, double pixelRatio) {
  if (logicalSize == null || logicalSize <= 0) return defaultArtworkBucket;
  final pixels = (logicalSize * pixelRatio).ceil();
  return _artworkBuckets.firstWhere(
    (bucket) => bucket >= pixels,
    orElse: () => _artworkBuckets.last,
  );
}

class Lit {
  const Lit(this._ranges);
  static const none = Lit({});

  final Map<(String, String), List<int>> _ranges;

  List<int> of(String column, String text) =>
      _ranges[(column, text)] ?? const [];
}

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

  static const _hotKeys = {
    'samples',
    'lyricFrame',
    'analysisProgress',
    'downloadedBytes',
  };

  final MethodChannel _methods;
  late final StreamSubscription<dynamic> _subscription;
  final RowData state = {};
  final Map<String, ValueNotifier<Object?>> _hot = {
    for (final key in _hotKeys) key: ValueNotifier<Object?>(null),
  };
  final Map<String, int> _artStamps = {};
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
  List<RowData> _favouriteTracks = [];
  Map<String, RowData> _byId = {};
  List<RowData> get allTracks => _allTracks;
  List<RowData> get favouriteTracks => _favouriteTracks;
  ValueListenable<Object?> hot(String key) => _hot[key]!;
  int artStamp(Object? id) => _artStamps['$id'] ?? 0;
  String get _generation => '${state['artworkGeneration']}';
  String _stamped(Object? id) {
    final stamp = artStamp(id);
    return stamp == 0 ? '' : ':$stamp';
  }

  String colourKey(RowData track) =>
      '${track['id']}:${track['artRevision']}:$_generation${_stamped(track['id'])}';
  String artworkKey(RowData track, int bucket) => '${colourKey(track)}:$bucket';
  bool isFavourite(RowData track) =>
      (_byId['${track['id']}'] ?? track)['favourite'] == true;
  List<RowData> _queue = [];
  List<RowData> get queue => _queue;
  RowData get snapshot => row(state['snapshot']);
  RowData get current => row(snapshot['current']);
  RowData get preferences => row(state['preferences']);
  bool get playing => ['PLAYING', 'LOADING'].contains(snapshot['transport']);
  String? get waiting => snapshot['waiting'] as String?;
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
    for (final key in _hotKeys) {
      if (patch.containsKey(key)) _hot[key]!.value = patch[key];
    }
    if (patch.isNotEmpty && patch.keys.every(_hotKeys.contains)) {
      connected = true;
      return;
    }
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
      _favouriteTracks = _allTracks
          .where((track) => track['favourite'] == true)
          .toList();
    }
    if (patch.containsKey('queue')) _queue = rows(patch['queue']);
    if (patch.containsKey('favourite')) _favourited(row(patch['favourite']));
    if (patch.containsKey('artChanged')) {
      for (final id in patch['artChanged'] as List? ?? const []) {
        _artStamps['$id'] = artStamp(id) + 1;
      }
    }
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

  void _favourited(RowData change) {
    final id = '${change['id']}';
    final enabled = change['enabled'] == true;
    _byId[id]?['favourite'] = enabled;
    for (final list in [_tracks, _queue]) {
      for (final track in list) {
        if ('${track['id']}' == id) track['favourite'] = enabled;
      }
    }
    _favouriteTracks = _allTracks
        .where((track) => track['favourite'] == true)
        .toList();
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
      return await _methods.invokeMethod<dynamic>(method, arguments);
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

  Future<Uint8List?> artwork(
    RowData track, [
    int bucket = defaultArtworkBucket,
  ]) {
    final key = artworkKey(track, bucket);
    final cached = _pictures.remove(key);
    if (cached != null) {
      _pictures[key] = cached;
      return cached;
    }
    final future = _methods
        .invokeMethod<Uint8List>('artwork', {'id': track['id'], 'size': bucket})
        .catchError((Object _) => null);
    _pictures[key] = future;
    while (_pictures.length > 48) {
      _pictures.remove(_pictures.keys.first);
    }
    return future;
  }

  RowData trackById(Object? id) => _byId['$id'] ?? {};

  void beginPlaylist(Object? id) {
    state['playlistRows'] = null;
    unawaited(call('selectPlaylist', {'id': id}));
  }

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

  List<String> get recentSearches =>
      (state['recentSearches'] as List? ?? const [])
          .map((entry) => '$entry')
          .toList();

  void keepRecentSearch(String text) {
    final query = text.trim();
    if (query.isEmpty) return;
    final next = [
      query,
      ...recentSearches.where(
        (entry) => entry.toLowerCase() != query.toLowerCase(),
      ),
    ].take(recentSearchesKept).toList();
    _rememberSearches(next);
  }

  void forgetRecentSearches() => _rememberSearches(const []);

  void _rememberSearches(List<String> values) {
    state['recentSearches'] = values;
    notifyListeners();
    unawaited(call('recentSearches', {'values': values}));
  }

  Future<Lit> lit(String query, RowData result) async {
    final asked = <(String, String)>{};
    for (final track in rows(result['tracks'])) {
      asked.add(('TITLE', title(track)));
      asked.add(('ARTIST', '${track['artist'] ?? ''}'));
    }
    for (final album in rows(result['albums'])) {
      asked.add(('ALBUM', '${album['title'] ?? ''}'));
      asked.add(('ARTIST', '${album['artist'] ?? ''}'));
    }
    for (final artist in rows(result['artists'])) {
      asked.add(('ARTIST', '${artist['name'] ?? ''}'));
    }
    asked.removeWhere((entry) => entry.$2.isEmpty);
    if (query.isEmpty || asked.isEmpty) return Lit.none;
    final items = asked.toList();
    final answer = await call('highlights', {
      'query': query,
      'items': [
        for (final (column, text) in items) {'column': column, 'text': text},
      ],
    });
    if (answer is! List || answer.length != items.length) return Lit.none;
    return Lit({
      for (var i = 0; i < items.length; i++)
        if (answer[i] is List && (answer[i] as List).isNotEmpty)
          items[i]: [for (final unit in answer[i] as List) integer(unit)],
    });
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
    for (final notifier in _hot.values) {
      notifier.dispose();
    }
    positionMs.dispose();
    sleepRemainingMs.dispose();
    super.dispose();
  }
}
