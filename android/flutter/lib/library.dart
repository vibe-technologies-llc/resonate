import 'dart:async';

import 'package:flutter/material.dart';

import 'main.dart' show AppBottomBar, OpenPage;
import 'model.dart';
import 'visuals.dart';

class LibraryPage extends StatefulWidget {
  const LibraryPage({super.key, required this.model, required this.open});
  final PlayerModel model;
  final OpenPage open;
  @override
  State<LibraryPage> createState() => _LibraryPageState();
}

class _LibraryPageState extends State<LibraryPage> {
  String _shelf = 'All';
  @override
  Widget build(BuildContext context) {
    final model = widget.model;
    final favourites = model.favouriteTracks;
    final albums = model.albums;
    final artists = model.artists;
    final playlists = rows(model.state['playlists']);
    return RefreshIndicator(
      onRefresh: () async {
        await model.call('rescan');
        await model.call('loadPlaylists');
      },
      child: CustomScrollView(
        key: const PageStorageKey('library'),
        slivers: [
          SliverToBoxAdapter(
            child: SizedBox(height: MediaQuery.paddingOf(context).top),
          ),
          SliverToBoxAdapter(
            child: Padding(
              padding: const EdgeInsets.fromLTRB(
                AppGeometry.pageInset,
                AppGeometry.pageTop,
                AppGeometry.pageInset,
                0,
              ),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Row(
                    children: [
                      Expanded(
                        child: Text(
                          'Your library',
                          style: Theme.of(context).textTheme.headlineLarge,
                        ),
                      ),
                      ActionIcon(
                        icon: Icons.tune_rounded,
                        label: 'Library tools',
                        onPressed: () => widget.open('tools'),
                      ),
                    ],
                  ),
                  const SizedBox(height: 8),
                  Text(
                    '${model.allTracks.length} ${model.allTracks.length == 1 ? 'song' : 'songs'} · ${model.albums.length} ${model.albums.length == 1 ? 'album' : 'albums'}',
                    style: TextStyle(
                      color: Theme.of(context).colorScheme.onSurfaceVariant,
                    ),
                  ),
                  if (model.allTracks.isEmpty)
                    Padding(
                      padding: const EdgeInsets.only(top: 18),
                      child: FilledButton.icon(
                        onPressed: () => model.call('chooseFolder'),
                        icon: const Icon(Icons.add_rounded),
                        label: const Text('Add music folder'),
                      ),
                    ),
                  const SizedBox(height: 18),
                  SingleChildScrollView(
                    scrollDirection: Axis.horizontal,
                    child: Row(
                      children: ['All', 'Playlists', 'Albums', 'Artists']
                          .map(
                            (label) => Padding(
                              padding: const EdgeInsets.only(right: 8),
                              child: ChoiceChip(
                                label: Text(label),
                                selected: _shelf == label,
                                onSelected: (_) =>
                                    setState(() => _shelf = label),
                                showCheckmark: false,
                              ),
                            ),
                          )
                          .toList(),
                    ),
                  ),
                  if (_shelf == 'All' || _shelf == 'Playlists')
                    Padding(
                      padding: const EdgeInsets.only(top: 20),
                      child: InkWell(
                        borderRadius: BorderRadius.circular(26),
                        onTap: () => widget.open('collection', {
                          'kind': 'favourites',
                          'title': 'Favourites',
                        }),
                        child: Container(
                          padding: const EdgeInsets.all(20),
                          decoration: BoxDecoration(
                            borderRadius: BorderRadius.circular(26),
                            gradient: const LinearGradient(
                              colors: [Color(0xFF6D4C99), Color(0xFFA34B7A)],
                            ),
                          ),
                          child: Row(
                            children: [
                              const Icon(
                                Icons.favorite_rounded,
                                color: Colors.white,
                                size: 38,
                              ),
                              const SizedBox(width: 18),
                              Expanded(
                                child: Column(
                                  crossAxisAlignment: CrossAxisAlignment.start,
                                  children: [
                                    const Text(
                                      'Favourites',
                                      style: TextStyle(
                                        color: Colors.white,
                                        fontWeight: FontWeight.w700,
                                        fontSize: 22,
                                      ),
                                    ),
                                    Text(
                                      '${favourites.length} songs you love',
                                      style: const TextStyle(
                                        color: Colors.white70,
                                      ),
                                    ),
                                  ],
                                ),
                              ),
                              const Icon(
                                Icons.arrow_forward_rounded,
                                color: Colors.white70,
                              ),
                            ],
                          ),
                        ),
                      ),
                    ),
                  if (_shelf == 'All')
                    Padding(
                      padding: const EdgeInsets.only(top: 12),
                      child: Row(
                        children: [
                          Expanded(
                            child: _Shortcut(
                              icon: Icons.auto_awesome_rounded,
                              label: 'Made for you',
                              onTap: () => widget.open('suggestions'),
                            ),
                          ),
                          const SizedBox(width: 12),
                          Expanded(
                            child: _Shortcut(
                              icon: Icons.travel_explore_rounded,
                              label: 'Discover',
                              onTap: () => widget.open('online'),
                            ),
                          ),
                        ],
                      ),
                    ),
                  if (_shelf == 'All' || _shelf == 'Playlists')
                    SectionTitle(
                      'Playlists',
                      action: Row(
                        mainAxisSize: MainAxisSize.min,
                        children: [
                          ActionIcon(
                            icon: Icons.undo_rounded,
                            label: 'Undo last playlist edit',
                            onPressed: () => model.call('undoPlaylist'),
                          ),
                          ActionIcon(
                            icon: Icons.add_rounded,
                            label: 'Create playlist',
                            onPressed: () => namePlaylist(context, model),
                          ),
                        ],
                      ),
                    ),
                  if ((_shelf == 'All' || _shelf == 'Playlists') &&
                      playlists.isEmpty)
                    const Text(
                      'A mood, a moment, a journey. Make it a playlist.',
                    ),
                  if (_shelf == 'All' || _shelf == 'Playlists')
                    ...playlists.map(
                      (p) => ListTile(
                        contentPadding: EdgeInsets.zero,
                        leading: PlaylistCover(
                          model: model,
                          covers: p['covers'],
                          size: 48,
                          radius: 12,
                        ),
                        title: Text('${p['name']}'),
                        subtitle: Text('${p['songs']} songs'),
                        trailing: const Icon(Icons.chevron_right_rounded),
                        onTap: () => widget.open('collection', {
                          ...p,
                          'title': p['name'],
                          'kind': 'playlist',
                        }),
                      ),
                    ),
                  if (_shelf == 'Albums' ||
                      (_shelf == 'All' && albums.isNotEmpty))
                    const SectionTitle('Albums'),
                ],
              ),
            ),
          ),
          if (_shelf == 'All' && albums.isNotEmpty)
            SliverToBoxAdapter(
              child: SizedBox(
                height: 236,
                child: ListView.separated(
                  padding: const EdgeInsets.symmetric(horizontal: 24),
                  scrollDirection: Axis.horizontal,
                  itemCount: albums.length.clamp(0, 12),
                  separatorBuilder: (_, _) => const SizedBox(width: 16),
                  itemBuilder: (context, index) => SizedBox(
                    width: 160,
                    child: _AlbumTile(
                      model: model,
                      album: albums[index],
                      open: widget.open,
                    ),
                  ),
                ),
              ),
            ),
          if (_shelf == 'Albums')
            SliverPadding(
              padding: const EdgeInsets.symmetric(horizontal: 24),
              sliver: SliverGrid.builder(
                gridDelegate: const SliverGridDelegateWithMaxCrossAxisExtent(
                  maxCrossAxisExtent: 240,
                  mainAxisSpacing: 18,
                  crossAxisSpacing: 16,
                  childAspectRatio: .72,
                ),
                itemCount: albums.length,
                itemBuilder: (_, index) => _AlbumTile(
                  model: model,
                  album: albums[index],
                  open: widget.open,
                ),
              ),
            ),
          if ((_shelf == 'All' || _shelf == 'Artists') && artists.isNotEmpty)
            SliverPadding(
              padding: const EdgeInsets.symmetric(horizontal: 24),
              sliver: SliverList.builder(
                itemCount: artists.length + 1,
                itemBuilder: (context, index) => index == 0
                    ? const SectionTitle('Artists')
                    : ListTile(
                        contentPadding: EdgeInsets.zero,
                        leading: Artwork(
                          model: model,
                          track: model.trackById(
                            artists[index - 1]['artworkTrack'],
                          ),
                          size: 52,
                          radius: 26,
                        ),
                        title: Text(
                          '${artists[index - 1]['name']}'.isEmpty
                              ? 'Unknown artist'
                              : '${artists[index - 1]['name']}',
                        ),
                        subtitle: Text('${artists[index - 1]['songs']} songs'),
                        trailing: const Icon(Icons.chevron_right_rounded),
                        onTap: () => widget.open('collection', {
                          'kind': 'artist',
                          'key': artists[index - 1]['name'],
                          'title': artists[index - 1]['name'],
                          'artworkTrack': artists[index - 1]['artworkTrack'],
                        }),
                      ),
              ),
            ),
          if (model.allTracks.isEmpty)
            SliverToBoxAdapter(
              child: EmptyState(
                icon: Icons.library_music_rounded,
                title: 'Make room for your music',
                detail: 'Add a music folder to bring your albums, artists and favourites together.',
              ),
            ),
          SliverToBoxAdapter(
            child: SizedBox(
              height:
                  MediaQuery.paddingOf(context).bottom + AppGeometry.pageInset,
            ),
          ),
        ],
      ),
    );
  }
}

class _Shortcut extends StatelessWidget {
  const _Shortcut({
    required this.icon,
    required this.label,
    required this.onTap,
  });
  final IconData icon;
  final String label;
  final VoidCallback onTap;
  @override
  Widget build(BuildContext context) => Card.filled(
    margin: EdgeInsets.zero,
    child: InkWell(
      borderRadius: BorderRadius.circular(12),
      onTap: onTap,
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Icon(icon, color: Theme.of(context).colorScheme.primary),
            const SizedBox(height: 12),
            Text(label, style: const TextStyle(fontWeight: FontWeight.w600)),
          ],
        ),
      ),
    ),
  );
}

class _AlbumTile extends StatelessWidget {
  const _AlbumTile({
    required this.model,
    required this.album,
    required this.open,
  });
  final PlayerModel model;
  final RowData album;
  final OpenPage open;
  @override
  Widget build(BuildContext context) => InkWell(
    borderRadius: BorderRadius.circular(18),
    onTap: () => open('collection', {...album, 'kind': 'album'}),
    child: Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        AspectRatio(
          aspectRatio: 1,
          child: Artwork(
            model: model,
            track: model.trackById(album['artworkTrack']),
            radius: 18,
          ),
        ),
        const SizedBox(height: 10),
        Text(
          '${album['title']}',
          maxLines: 1,
          overflow: TextOverflow.ellipsis,
          style: const TextStyle(fontWeight: FontWeight.w600),
        ),
        const SizedBox(height: 3),
        Text(
          '${album['artist']}',
          maxLines: 1,
          overflow: TextOverflow.ellipsis,
          style: TextStyle(
            fontSize: 12,
            color: Theme.of(context).colorScheme.onSurfaceVariant,
          ),
        ),
      ],
    ),
  );
}

class SongsPage extends StatelessWidget {
  const SongsPage({super.key, required this.model, required this.open});
  final PlayerModel model;
  final OpenPage open;

  @override
  Widget build(BuildContext context) {
    final tracks = model.tracks;
    return Column(
      children: [
        Padding(
          padding: const EdgeInsets.fromLTRB(
            AppGeometry.pageInset,
            AppGeometry.pageTop,
            AppGeometry.pageInset,
            12,
          ),
          child: Column(
            children: [
              Row(
                children: [
                  Expanded(
                    child: Text(
                      'Songs',
                      style: Theme.of(context).textTheme.headlineLarge,
                    ),
                  ),
                  ActionIcon(
                    icon: Icons.filter_list_rounded,
                    label: 'Filter songs',
                    selected: model.hasActiveFilter,
                    onPressed: () => songFilters(context, model),
                  ),
                  ActionIcon(
                    icon: Icons.tune_rounded,
                    label: 'Library tools',
                    onPressed: () => open('tools'),
                  ),
                ],
              ),
              const SizedBox(height: 12),
              Row(
                children: [
                  Text(
                    '${tracks.length} ${tracks.length == 1 ? 'song' : 'songs'}',
                  ),
                  const Spacer(),
                  TextButton.icon(
                    onPressed: tracks.isEmpty ? null : () => model.play(tracks),
                    icon: const Icon(Icons.play_arrow_rounded),
                    label: const Text('Play all'),
                  ),
                ],
              ),
            ],
          ),
        ),
        Expanded(
          child: tracks.isEmpty
              ? Padding(
                  padding: const EdgeInsets.only(bottom: 110),
                  child: EmptyState(
                    icon: Icons.music_note_rounded,
                    title: model.allTracks.isEmpty
                        ? 'Your next favourite awaits'
                        : 'No songs found',
                    detail: model.allTracks.isEmpty
                        ? 'Choose a music folder to start listening.'
                        : 'Try a different search or clear your filters.',
                    action: FilledButton(
                      onPressed: () => model.allTracks.isEmpty
                          ? model.call('chooseFolder')
                          : model.clearFilters(),
                      child: Text(
                        model.allTracks.isEmpty
                            ? 'Add music folder'
                            : 'Clear filters',
                      ),
                    ),
                  ),
                )
              : ListView.builder(
                  key: const PageStorageKey('songs'),
                  padding: EdgeInsets.only(
                    bottom:
                        MediaQuery.paddingOf(context).bottom +
                        AppGeometry.pageInset,
                  ),
                  itemCount: tracks.length,
                  itemBuilder: (_, index) => SongTile(
                    model: model,
                    track: tracks[index],
                    onTap: () => model.play(tracks, index),
                    open: open,
                  ),
                ),
        ),
      ],
    );
  }
}

class SongTile extends StatelessWidget {
  const SongTile({
    super.key,
    required this.model,
    required this.track,
    required this.onTap,
    required this.open,
    this.trailing,
    this.number,
    this.semanticNumber,
  });
  final PlayerModel model;
  final RowData track;
  final VoidCallback onTap;
  final OpenPage open;
  final Widget? trailing;
  final int? number;
  final int? semanticNumber;
  @override
  Widget build(BuildContext context) {
    final playing = '${model.current['id']}' == '${track['id']}';
    final spokenNumber = semanticNumber ?? number;
    return Semantics(
      label: spokenNumber == null
          ? 'Play ${title(track)}'
          : 'Play song $spokenNumber: ${title(track)}',
      child: ListTile(
        onTap: onTap,
        leading: number == null
            ? Artwork(model: model, track: track, size: 50, radius: 12)
            : SizedBox(
                width: 30,
                child: Text(
                  '$number',
                  textAlign: TextAlign.center,
                  style: TextStyle(
                    color: playing
                        ? Theme.of(context).colorScheme.primary
                        : null,
                  ),
                ),
              ),
        title: Text(
          title(track),
          maxLines: 1,
          overflow: TextOverflow.ellipsis,
          style: TextStyle(
            fontWeight: FontWeight.w600,
            color: playing ? Theme.of(context).colorScheme.primary : null,
          ),
        ),
        subtitle: Text(
          artist(track),
          maxLines: 1,
          overflow: TextOverflow.ellipsis,
        ),
        trailing:
            trailing ??
            ActionIcon(
              icon: Icons.more_horiz_rounded,
              label: 'More about ${title(track)}',
              onPressed: () => songActions(context, model, track, open),
            ),
      ),
    );
  }
}

Future<void> openAlbumOf(
  PlayerModel model,
  RowData track,
  OpenPage open,
) async {
  final key = await model.call('albumKey', {
    'album': track['album'],
    'artist': track['artist'],
    'albumArtist': track['albumArtist'],
  });
  if (key == null) return;
  open('collection', {
    'kind': 'album',
    'key': key,
    'title': track['album'],
    'artist': track['albumArtist'] == ''
        ? track['artist']
        : track['albumArtist'],
    'artworkTrack': track['id'],
  });
}

Future<void> songActions(
  BuildContext context,
  PlayerModel model,
  RowData track,
  OpenPage open,
) {
  final favourite = model.isFavourite(track);
  return showModalBottomSheet<void>(
    context: context,
    showDragHandle: true,
    isScrollControlled: true,
    builder: (sheet) => SafeArea(
      child: SingleChildScrollView(
        child: Column(
          mainAxisSize: MainAxisSize.min,
          children: [
            ListTile(
              leading: Artwork(model: model, track: track, size: 52),
              title: Text(title(track)),
              subtitle: Text(artist(track)),
            ),
            const Divider(),
            ...<({IconData icon, String label, VoidCallback action})>[
              (
                icon: favourite
                    ? Icons.favorite_rounded
                    : Icons.favorite_border_rounded,
                label: favourite
                    ? 'Remove from favourites'
                    : 'Add to favourites',
                action: () => model.call('favourite', {
                  'id': track['id'],
                  'enabled': !favourite,
                }),
              ),
              (
                icon: Icons.playlist_play_rounded,
                label: 'Play next',
                action: () => model.call('playNext', {
                  'ids': [track['id']],
                }),
              ),
              (
                icon: Icons.queue_music_rounded,
                label: 'Add to queue',
                action: () => model.call('addToQueue', {
                  'ids': [track['id']],
                }),
              ),
              (
                icon: Icons.playlist_add_rounded,
                label: 'Add to playlist',
                action: () => playlistPicker(context, model, [track]),
              ),
              (
                icon: Icons.album_outlined,
                label: 'Go to album',
                action: () => openAlbumOf(model, track, open),
              ),
              (
                icon: Icons.person_outline_rounded,
                label: 'Go to artist',
                action: () => open('collection', {
                  'kind': 'artist',
                  'key': track['artist'],
                  'title': artist(track),
                  'artworkTrack': track['id'],
                }),
              ),
              (
                icon: Icons.lyrics_outlined,
                label: 'Lyrics',
                action: () => open('lyrics', track),
              ),
              (
                icon: Icons.insights_rounded,
                label: 'Analysis',
                action: () => open('analysis', track),
              ),
              (
                icon: Icons.info_outline_rounded,
                label: 'Song details',
                action: () => open('inspector', track),
              ),
            ].map(
              (action) => ListTile(
                leading: Icon(action.icon),
                title: Text(action.label),
                onTap: () {
                  Navigator.pop(sheet);
                  action.action();
                },
              ),
            ),
          ],
        ),
      ),
    ),
  );
}

Future<void> namePlaylist(
  BuildContext context,
  PlayerModel model, [
  RowData? playlist,
]) async {
  var draft = playlist?['name'] as String? ?? '';
  final name = await showDialog<String>(
    context: context,
    builder: (context) => AlertDialog(
      title: Text(playlist == null ? 'Create playlist' : 'Rename playlist'),
      content: TextFormField(
        initialValue: draft,
        onChanged: (value) => draft = value,
        autofocus: true,
        maxLength: 200,
        decoration: const InputDecoration(labelText: 'Playlist name'),
        onFieldSubmitted: (value) {
          if (value.trim().isNotEmpty) Navigator.pop(context, value.trim());
        },
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.pop(context),
          child: const Text('Cancel'),
        ),
        TextButton(
          onPressed: () {
            if (draft.trim().isNotEmpty) {
              Navigator.pop(context, draft.trim());
            }
          },
          child: const Text('Save'),
        ),
      ],
    ),
  );
  if (name != null) {
    await model.call(playlist == null ? 'createPlaylist' : 'renamePlaylist', {
      ...?playlist,
      'name': name,
    });
  }
}

Future<void> playlistPicker(
  BuildContext context,
  PlayerModel model,
  List<RowData> tracks,
) async {
  await model.call('loadPlaylists');
  if (!context.mounted) return;
  await showModalBottomSheet<void>(
    context: context,
    showDragHandle: true,
    builder: (sheet) => SafeArea(
      child: ListenableBuilder(
        listenable: model,
        builder: (_, _) => ListView(
          shrinkWrap: true,
          children: [
            const ListTile(
              title: Text(
                'Add to playlist',
                style: TextStyle(fontSize: 22, fontWeight: FontWeight.bold),
              ),
            ),
            ListTile(
              leading: const Icon(Icons.add_rounded),
              title: const Text('Create playlist'),
              onTap: () => namePlaylist(sheet, model),
            ),
            ...rows(model.state['playlists']).map(
              (p) => ListTile(
                title: Text('${p['name']}'),
                subtitle: Text('${p['songs']} songs'),
                onTap: () {
                  model.call('addToPlaylist', {
                    'id': p['id'],
                    'ids': tracks.map((t) => t['id']).toList(),
                  });
                  Navigator.pop(sheet);
                },
              ),
            ),
          ],
        ),
      ),
    ),
  );
}

Future<void> songFilters(BuildContext context, PlayerModel model) async {
  final filter = {...row(model.state['filter'])};
  await showModalBottomSheet<void>(
    context: context,
    showDragHandle: true,
    isScrollControlled: true,
    builder: (sheet) => StatefulBuilder(
      builder: (context, update) => SafeArea(
        child: SingleChildScrollView(
          child: Padding(
            padding: EdgeInsets.fromLTRB(
              24,
              0,
              24,
              MediaQuery.viewInsetsOf(context).bottom + 24,
            ),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                const SectionTitle('Shape your listening'),
                for (final entry in {
                  'favourites': 'Favourites only',
                  'unplayed': 'Never played',
                  'descending': 'Descending order',
                }.entries)
                  SwitchListTile(
                    title: Text(entry.value),
                    value: filter[entry.key] == true,
                    onChanged: (value) =>
                        update(() => filter[entry.key] = value),
                  ),
                for (final key in ['artist', 'album', 'genre'])
                  Padding(
                    padding: const EdgeInsets.only(bottom: 12),
                    child: TextFormField(
                      initialValue: '${filter[key] ?? ''}',
                      decoration: InputDecoration(labelText: humanize(key)),
                      onChanged: (value) => filter[key] = value,
                    ),
                  ),
                DropdownButtonFormField<String>(
                  initialValue: filter['order'] as String? ?? 'TITLE',
                  decoration: const InputDecoration(labelText: 'Sort by'),
                  items:
                      ['TITLE', 'ARTIST', 'ALBUM', 'ADDED', 'PLAYS', 'DURATION']
                          .map(
                            (v) => DropdownMenuItem(
                              value: v,
                              child: Text(humanize(v)),
                            ),
                          )
                          .toList(),
                  onChanged: (v) => filter['order'] = v,
                ),
                const SizedBox(height: 12),
                DropdownButtonFormField<int>(
                  initialValue: integer(filter['minimumRate']),
                  decoration: const InputDecoration(
                    labelText: 'Minimum sample rate',
                  ),
                  items: [0, 44100, 88200, 96000, 192000]
                      .map(
                        (v) => DropdownMenuItem(
                          value: v,
                          child: Text(
                            v == 0 ? 'Any sample rate' : '${v / 1000} kHz',
                          ),
                        ),
                      )
                      .toList(),
                  onChanged: (v) => filter['minimumRate'] = v,
                ),
                const SizedBox(height: 12),
                DropdownButtonFormField<int>(
                  initialValue: integer(filter['minimumBits']),
                  decoration: const InputDecoration(
                    labelText: 'Minimum bit depth',
                  ),
                  items: [0, 16, 24, 32]
                      .map(
                        (v) => DropdownMenuItem(
                          value: v,
                          child: Text(v == 0 ? 'Any bit depth' : '$v bit'),
                        ),
                      )
                      .toList(),
                  onChanged: (v) => filter['minimumBits'] = v,
                ),
                const SizedBox(height: 20),
                Row(
                  children: [
                    TextButton(
                      onPressed: () {
                        model.clearFilters();
                        Navigator.pop(sheet);
                      },
                      child: const Text('Reset'),
                    ),
                    const Spacer(),
                    FilledButton(
                      onPressed: () {
                        model.call('filter', filter);
                        Navigator.pop(sheet);
                      },
                      child: const Text('Apply filters'),
                    ),
                  ],
                ),
              ],
            ),
          ),
        ),
      ),
    ),
  );
}

class CollectionPage extends StatefulWidget {
  const CollectionPage({
    super.key,
    required this.model,
    required this.data,
    required this.open,
  });
  final PlayerModel model;
  final RowData data;
  final OpenPage open;
  @override
  State<CollectionPage> createState() => _CollectionPageState();
}

class _CollectionPageState extends State<CollectionPage> {
  List<RowData>? _tracks;
  List<RowData> _artistAlbums = [];
  bool _failed = false;
  Object? get kind => widget.data['kind'];
  bool get playlist => kind == 'playlist';
  bool get held => kind == 'favourites' || kind == 'all';
  @override
  void initState() {
    super.initState();
    if (playlist) {
      widget.model.beginPlaylist(widget.data['id']);
    } else if (!held) {
      _load();
    }
  }

  Future<void> _load() async {
    if (_failed) setState(() => _failed = false);
    final answer = await widget.model.call('collectionTracks', widget.data);
    if (kind == 'artist') {
      _artistAlbums = rows(
        await widget.model.call('albumsBy', {'name': widget.data['key']}),
      );
    }
    if (!mounted) return;
    setState(() {
      if (answer == null) {
        _failed = true;
      } else {
        _tracks = rows(answer);
      }
    });
  }

  @override
  Widget build(BuildContext context) => ListenableBuilder(
    listenable: widget.model,
    builder: (context, _) {
      final model = widget.model;
      final data = playlist
          ? (rows(model.state['playlists'])
                    .where((p) => p['id'] == widget.data['id'])
                    .firstOrNull ??
                widget.data)
          : widget.data;
      final tracks = playlist
          ? (model.state['playlistRows'] == null
                ? null
                : rows(model.state['playlistRows']))
          : kind == 'favourites'
          ? model.favouriteTracks
          : kind == 'all'
          ? model.allTracks
          : _tracks?.map((track) {
              final latest = model.trackById(track['id']);
              return latest.isEmpty ? track : latest;
            }).toList();
      final cover = model.trackById(widget.data['artworkTrack']);
      return ArtworkAtmosphere(
        model: model,
        track: cover.isNotEmpty ? cover : tracks?.firstOrNull ?? model.current,
        child: Scaffold(
          backgroundColor: Colors.transparent,
          extendBody: true,
          extendBodyBehindAppBar: true,
          appBar: AppBar(
            backgroundColor: Colors.transparent,
            scrolledUnderElevation: 0,
            flexibleSpace: const EdgeTint(top: true, child: SizedBox.expand()),
            title: Text(
              '${data['name'] ?? widget.data['title']}',
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
            ),
            actions: [
              if (playlist)
                PopupMenuButton<String>(
                  tooltip: 'Playlist options',
                  onSelected: (action) async {
                    switch (action) {
                      case 'sync':
                        _syncSongs(context);
                      case 'rename':
                        namePlaylist(context, model, data);
                      case 'add':
                        _addSongs(context);
                      case 'undo':
                        model.call('undoPlaylist');
                      case 'delete':
                        final confirmed = await showDialog<bool>(
                          context: context,
                          builder: (context) => AlertDialog(
                            title: const Text('Delete playlist?'),
                            content: const Text(
                              'Your music files will stay in your library.',
                            ),
                            actions: [
                              TextButton(
                                onPressed: () => Navigator.pop(context, false),
                                child: const Text('Cancel'),
                              ),
                              TextButton(
                                onPressed: () => Navigator.pop(context, true),
                                child: const Text('Delete'),
                              ),
                            ],
                          ),
                        );
                        if (confirmed == true) {
                          await model.call('deletePlaylist', data);
                          if (context.mounted) Navigator.pop(context);
                        }
                    }
                  },
                  itemBuilder: (_) => [
                    const PopupMenuItem(
                      value: 'sync',
                      child: Text('Songs on your devices'),
                    ),
                    const PopupMenuItem(value: 'add', child: Text('Add songs')),
                    const PopupMenuItem(value: 'rename', child: Text('Rename')),
                    const PopupMenuItem(
                      value: 'undo',
                      child: Text('Undo last edit'),
                    ),
                    const PopupMenuItem(
                      value: 'delete',
                      child: Text('Delete playlist'),
                    ),
                  ],
                ),
            ],
          ),
          body: tracks == null
              ? _failed
                    ? EmptyState(
                        icon: Icons.error_outline_rounded,
                        title: 'This could not be loaded',
                        detail: 'Check the notice below, then try again.',
                        action: FilledButton(
                          onPressed: _load,
                          child: const Text('Try again'),
                        ),
                      )
                    : const Center(child: CircularProgressIndicator())
              : CustomScrollView(
                  slivers: [
                    SliverToBoxAdapter(
                      child: SizedBox(
                        height:
                            MediaQuery.paddingOf(context).top + kToolbarHeight,
                      ),
                    ),
                    SliverToBoxAdapter(
                      child: Padding(
                        padding: const EdgeInsets.all(24),
                        child: Column(
                          children: [
                            SizedBox(
                              width: 220,
                              height: 220,
                              child: DecoratedBox(
                                decoration: BoxDecoration(
                                  borderRadius: BorderRadius.circular(24),
                                  boxShadow: [
                                    BoxShadow(
                                      color: Colors.black.withValues(alpha: .2),
                                      blurRadius: 32,
                                      offset: const Offset(0, 16),
                                    ),
                                  ],
                                ),
                                child: kind == 'favourites'
                                    ? Container(
                                        decoration: BoxDecoration(
                                          borderRadius: BorderRadius.circular(
                                            24,
                                          ),
                                          gradient: const LinearGradient(
                                            colors: [
                                              Color(0xFFAD679D),
                                              Color(0xFF6657AB),
                                            ],
                                          ),
                                        ),
                                        child: const Icon(
                                          Icons.favorite_rounded,
                                          size: 84,
                                          color: Colors.white,
                                        ),
                                      )
                                    : playlist
                                    ? PlaylistCover(
                                        model: model,
                                        covers: data['covers'],
                                        radius: 24,
                                      )
                                    : Artwork(
                                        model: model,
                                        track:
                                            cover.isEmpty && tracks.isNotEmpty
                                            ? tracks.first
                                            : cover,
                                        radius: 24,
                                      ),
                              ),
                            ),
                            const SizedBox(height: 24),
                            Text(
                              '${data['name'] ?? widget.data['title']}',
                              textAlign: TextAlign.center,
                              style: Theme.of(context).textTheme.headlineMedium
                                  ?.copyWith(fontWeight: FontWeight.w800),
                            ),
                            const SizedBox(height: 8),
                            Text(
                              '${tracks.length} songs · ${tracks.fold<int>(0, (sum, t) => sum + integer(t['durationMs'])) ~/ 60000} min',
                            ),
                            const SizedBox(height: 22),
                            Row(
                              children: [
                                Expanded(
                                  child: FilledButton.icon(
                                    onPressed: tracks.isEmpty
                                        ? null
                                        : () => model.play(tracks),
                                    icon: const Icon(Icons.play_arrow_rounded),
                                    label: const Text('Play'),
                                  ),
                                ),
                                const SizedBox(width: 12),
                                Expanded(
                                  child: FilledButton.tonalIcon(
                                    onPressed: tracks.isEmpty
                                        ? null
                                        : () async {
                                            await model.play(tracks);
                                            await model.call('shuffle', {
                                              'enabled': true,
                                            });
                                          },
                                    icon: const Icon(Icons.shuffle_rounded),
                                    label: const Text('Shuffle'),
                                  ),
                                ),
                              ],
                            ),
                          ],
                        ),
                      ),
                    ),
                    if (tracks.isEmpty)
                      const SliverToBoxAdapter(
                        child: EmptyState(
                          icon: Icons.music_note_rounded,
                          title: 'A little quiet here',
                          detail:
                              'Add some songs to make this collection yours.',
                        ),
                      ),
                    if (_artistAlbums.isNotEmpty)
                      SliverToBoxAdapter(
                        child: Padding(
                          padding: const EdgeInsets.symmetric(horizontal: 24),
                          child: Column(
                            children: [
                              const SectionTitle('Albums'),
                              SizedBox(
                                height: 236,
                                child: ListView.separated(
                                  scrollDirection: Axis.horizontal,
                                  itemCount: _artistAlbums.length,
                                  separatorBuilder: (_, _) =>
                                      const SizedBox(width: 16),
                                  itemBuilder: (_, index) => SizedBox(
                                    width: 160,
                                    child: _AlbumTile(
                                      model: model,
                                      album: _artistAlbums[index],
                                      open: widget.open,
                                    ),
                                  ),
                                ),
                              ),
                              const SectionTitle('Songs'),
                            ],
                          ),
                        ),
                      ),
                    SliverList.builder(
                      itemCount: tracks.length,
                      itemBuilder: (_, index) => SongTile(
                        model: model,
                        track: tracks[index],
                        semanticNumber: index + 1,
                        number: widget.data['kind'] == 'album'
                            ? index + 1
                            : null,
                        onTap: () => model.play(tracks, index),
                        open: widget.open,
                        trailing: playlist
                            ? PopupMenuButton<String>(
                                tooltip: 'Options for song ${index + 1}',
                                onSelected: (action) {
                                  if (action == 'remove') {
                                    model.call('removeFromPlaylist', {
                                      ...data,
                                      'position': index,
                                    });
                                  }
                                  if (action == 'up') {
                                    model.call('moveInPlaylist', {
                                      ...data,
                                      'from': index,
                                      'to': index - 1,
                                    });
                                  }
                                  if (action == 'down') {
                                    model.call('moveInPlaylist', {
                                      ...data,
                                      'from': index,
                                      'to': index + 1,
                                    });
                                  }
                                },
                                itemBuilder: (_) => [
                                  if (index > 0)
                                    const PopupMenuItem(
                                      value: 'up',
                                      child: Text('Move up'),
                                    ),
                                  if (index < tracks.length - 1)
                                    const PopupMenuItem(
                                      value: 'down',
                                      child: Text('Move down'),
                                    ),
                                  const PopupMenuItem(
                                    value: 'remove',
                                    child: Text('Remove from playlist'),
                                  ),
                                ],
                              )
                            : null,
                      ),
                    ),
                    const SliverToBoxAdapter(child: SizedBox(height: 180)),
                  ],
                ),
          bottomNavigationBar: AppBottomBar(model: model, open: widget.open),
        ),
      );
    },
  );

  Future<void> _syncSongs(BuildContext context) async {
    await widget.model.call('playlistSync', {'id': widget.data['id']});
    if (!context.mounted) return;
    await showModalBottomSheet<void>(
      context: context,
      showDragHandle: true,
      builder: (sheet) => SafeArea(
        child: ListenableBuilder(
          listenable: widget.model,
          builder: (_, _) {
            final model = widget.model;
            final sync = row(model.state['playlistSync']);
            final standing = row(model.state['syncStatus']);
            final devices = rows(standing['devices']).where(
              (d) =>
                  d['thisDevice'] != true &&
                  (d['nearby'] == true || d['online'] == true),
            );
            return ListView(
              shrinkWrap: true,
              children: [
                const ListTile(title: Text('Songs on your devices')),
                SwitchListTile(
                  title: const Text('Keep its songs here'),
                  value: sync['kept'] == true,
                  onChanged: (value) => model.call('keepSongs', {
                    'id': widget.data['id'],
                    'enabled': value,
                  }),
                ),
                ListTile(
                  title: Text(
                    '${sync['missing'] ?? 0} songs missing on this device',
                  ),
                ),
                ...devices.map(
                  (device) => ListTile(
                    leading: const Icon(Icons.download_rounded),
                    title: Text('Fetch from ${device['name']}'),
                    onTap: () => model.call('fetchMissing', {
                      'id': widget.data['id'],
                      'device': device['id'],
                    }),
                  ),
                ),
                if (devices.isEmpty)
                  const ListTile(
                    subtitle: Text(
                      'Connect another device in Settings → Device sync.',
                    ),
                  ),
              ],
            );
          },
        ),
      ),
    );
  }

  Future<void> _addSongs(BuildContext context) async {
    final selected = <String>{};
    var query = '';
    await showModalBottomSheet<void>(
      context: context,
      showDragHandle: true,
      isScrollControlled: true,
      builder: (sheet) => StatefulBuilder(
        builder: (_, update) {
          final matches = widget.model.allTracks
              .where(
                (t) => '${title(t)} ${artist(t)}'.toLowerCase().contains(
                  query.toLowerCase(),
                ),
              )
              .toList();
          return SafeArea(
            child: SizedBox(
              height: MediaQuery.sizeOf(context).height * .8,
              child: Column(
                children: [
                  Padding(
                    padding: const EdgeInsets.all(16),
                    child: TextField(
                      onChanged: (v) => update(() => query = v),
                      decoration: const InputDecoration(
                        labelText: 'Search songs',
                      ),
                    ),
                  ),
                  Expanded(
                    child: ListView.builder(
                      itemCount: matches.length,
                      itemBuilder: (_, index) {
                        final track = matches[index];
                        final id = '${track['id']}';
                        return CheckboxListTile(
                          title: Text(title(track)),
                          subtitle: Text(artist(track)),
                          value: selected.contains(id),
                          onChanged: (v) => update(() {
                            if (v == true) {
                              selected.add(id);
                            } else {
                              selected.remove(id);
                            }
                          }),
                        );
                      },
                    ),
                  ),
                  Padding(
                    padding: const EdgeInsets.all(16),
                    child: FilledButton(
                      onPressed: selected.isEmpty
                          ? null
                          : () {
                              widget.model.call('addToPlaylist', {
                                'id': widget.data['id'],
                                'ids': selected.toList(),
                              });
                              Navigator.pop(sheet);
                            },
                      child: Text('Add ${selected.length} songs'),
                    ),
                  ),
                ],
              ),
            ),
          );
        },
      ),
    );
  }
}
