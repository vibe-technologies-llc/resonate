import 'dart:async';
import 'dart:math' as math;

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
    final text = Theme.of(context).textTheme;
    final scheme = Theme.of(context).colorScheme;
    final everything = _shelf == 'All';
    final showPlaylists = everything || _shelf == 'Playlists';
    const edge = EdgeInsets.symmetric(horizontal: AppGeometry.pageInset);
    const shown = 12;
    void openArtist(RowData artist) => widget.open('collection', {
      'kind': 'artist',
      'key': artist['name'],
      'title': artist['name'],
      'artworkTrack': artist['artworkTrack'],
    });
    void openAlbum(RowData album) =>
        widget.open('collection', {...album, 'kind': 'album'});
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
                AppSpace.l,
              ),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  LargeTitle(
                    'Your library',
                    actions: [_LibraryMenu(model: model, open: widget.open)],
                  ),
                  const SizedBox(height: AppSpace.xs),
                  Text(
                    '${model.allTracks.length} ${model.allTracks.length == 1 ? 'song' : 'songs'} · ${model.albums.length} ${model.albums.length == 1 ? 'album' : 'albums'}',
                    style: text.bodyMedium?.copyWith(
                      color: scheme.onSurfaceVariant,
                    ),
                  ),
                  if (model.allTracks.isEmpty)
                    Padding(
                      padding: const EdgeInsets.only(top: AppSpace.l),
                      child: FilledButton.icon(
                        onPressed: () => model.call('chooseFolder'),
                        icon: const Icon(Icons.add_rounded),
                        label: const Text('Add music folder'),
                      ),
                    ),
                ],
              ),
            ),
          ),
          SliverToBoxAdapter(
            child: ChoiceRow(
              labels: const ['All', 'Playlists', 'Albums', 'Artists'],
              selected: _shelf,
              onSelected: (label) => setState(() => _shelf = label),
              padding: edge,
            ),
          ),
          if (showPlaylists)
            SliverToBoxAdapter(
              child: Padding(
                padding: const EdgeInsets.fromLTRB(
                  AppGeometry.pageInset,
                  AppSpace.l,
                  AppGeometry.pageInset,
                  0,
                ),
                child: Card(
                  child: ListTileTheme.merge(
                    contentPadding: const EdgeInsets.symmetric(
                      horizontal: AppSpace.l,
                    ),
                    child: Column(
                      children: [
                        CollectionRow(
                          leading: const IconTile(
                            icon: Icons.favorite_rounded,
                            colors: AppGradients.favourites,
                          ),
                          title: const Text('Favourites'),
                          subtitle: Text('${favourites.length} songs you love'),
                          onTap: () => widget.open('collection', {
                            'kind': 'favourites',
                            'title': 'Favourites',
                          }),
                        ),
                        if (everything) ...[
                          CollectionRow(
                            leading: const IconTile(
                              icon: Icons.music_note_rounded,
                              colors: AppGradients.allSongs,
                            ),
                            title: const Text('All songs'),
                            onTap: () => widget.open('songs'),
                          ),
                          CollectionRow(
                            leading: const IconTile(
                              icon: Icons.auto_awesome_rounded,
                              colors: AppGradients.madeForYou,
                            ),
                            title: const Text('Made for you'),
                            onTap: () => widget.open('suggestions'),
                          ),
                          CollectionRow(
                            leading: const IconTile(
                              icon: Icons.bar_chart_rounded,
                              colors: AppGradients.statistics,
                            ),
                            title: const Text('Statistics'),
                            onTap: () => widget.open('statistics'),
                          ),
                        ],
                      ],
                    ),
                  ),
                ),
              ),
            ),
          if (showPlaylists) ...[
            SliverPadding(
              padding: edge,
              sliver: SliverToBoxAdapter(
                child: SectionTitle(
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
              ),
            ),
            if (playlists.isEmpty)
              SliverPadding(
                padding: edge,
                sliver: SliverToBoxAdapter(
                  child: Text(
                    'A mood, a moment, a journey. Make it a playlist.',
                    style: text.bodyMedium?.copyWith(
                      color: scheme.onSurfaceVariant,
                    ),
                  ),
                ),
              ),
            SliverList.builder(
              itemCount: playlists.length,
              itemBuilder: (context, index) {
                final p = playlists[index];
                return CollectionRow(
                  leading: PlaylistCover(
                    model: model,
                    covers: p['covers'],
                    size: AppGeometry.rowArt,
                  ),
                  title: Text('${p['name']}'),
                  subtitle: Text('${p['songs']} songs'),
                  onTap: () => widget.open('collection', {
                    ...p,
                    'title': p['name'],
                    'kind': 'playlist',
                  }),
                );
              },
            ),
          ],
          if (everything && albums.isNotEmpty) ...[
            SliverPadding(
              padding: edge,
              sliver: SliverToBoxAdapter(
                child: SectionTitle(
                  'Albums',
                  onSeeAll: albums.length > shown
                      ? () => setState(() => _shelf = 'Albums')
                      : null,
                ),
              ),
            ),
            SliverToBoxAdapter(
              child: Shelf(
                itemCount: albums.length.clamp(0, shown),
                itemBuilder: (context, index) => AlbumCard(
                  model: model,
                  album: albums[index],
                  onTap: () => openAlbum(albums[index]),
                ),
              ),
            ),
          ],
          if (_shelf == 'Albums') ...[
            const SliverPadding(
              padding: edge,
              sliver: SliverToBoxAdapter(child: SectionTitle('Albums')),
            ),
            SliverPadding(
              padding: edge,
              sliver: SliverLayoutBuilder(
                builder: (context, constraints) {
                  const across = AppSpace.l;
                  const widest = 160.0;
                  final width = constraints.crossAxisExtent;
                  final columns = ((width + across) / (widest + across))
                      .ceil()
                      .clamp(2, 12);
                  final tile = (width - across * (columns - 1)) / columns;
                  return SliverGrid.builder(
                    gridDelegate: SliverGridDelegateWithFixedCrossAxisCount(
                      crossAxisCount: columns,
                      mainAxisSpacing: 20,
                      crossAxisSpacing: across,
                      mainAxisExtent: tile + shelfCaptionHeight(context),
                    ),
                    itemCount: albums.length,
                    itemBuilder: (_, index) => AlbumCard(
                      model: model,
                      album: albums[index],
                      onTap: () => openAlbum(albums[index]),
                    ),
                  );
                },
              ),
            ),
          ],
          if (everything && artists.isNotEmpty) ...[
            SliverPadding(
              padding: edge,
              sliver: SliverToBoxAdapter(
                child: SectionTitle(
                  'Artists',
                  onSeeAll: artists.length > shown
                      ? () => setState(() => _shelf = 'Artists')
                      : null,
                ),
              ),
            ),
            SliverToBoxAdapter(
              child: Shelf(
                itemWidth: AppGeometry.artistCircle,
                coverHeight: AppGeometry.artistCircle,
                itemCount: artists.length.clamp(0, shown),
                itemBuilder: (context, index) => ArtistCard(
                  model: model,
                  artist: artists[index],
                  onTap: () => openArtist(artists[index]),
                ),
              ),
            ),
          ],
          if (_shelf == 'Artists' && artists.isNotEmpty) ...[
            const SliverPadding(
              padding: edge,
              sliver: SliverToBoxAdapter(child: SectionTitle('Artists')),
            ),
            SliverList.builder(
              itemCount: artists.length,
              itemBuilder: (context, index) {
                final artist = artists[index];
                return CollectionRow(
                  leading: Artwork(
                    model: model,
                    track: model.trackById(artist['artworkTrack']),
                    size: AppGeometry.rowArt,
                    radius: AppGeometry.rowArt / 2,
                  ),
                  title: Text(
                    '${artist['name']}'.isEmpty
                        ? 'Unknown artist'
                        : '${artist['name']}',
                  ),
                  subtitle: Text('${artist['songs']} songs'),
                  onTap: () => openArtist(artist),
                );
              },
            ),
          ],
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

class _LibraryMenu extends StatelessWidget {
  const _LibraryMenu({required this.model, required this.open});
  final PlayerModel model;
  final OpenPage open;
  @override
  Widget build(BuildContext context) => PopupMenuButton<String>(
    tooltip: 'More',
    icon: const Icon(Icons.more_horiz_rounded),
    onSelected: (choice) => switch (choice) {
      'folder' => model.call('chooseFolder'),
      'rescan' => model.call('rescan'),
      _ => open('tools'),
    },
    itemBuilder: (_) => const [
      PopupMenuItem(value: 'folder', child: Text('Add music folder')),
      PopupMenuItem(value: 'rescan', child: Text('Rescan library')),
      PopupMenuItem(value: 'tools', child: Text('Library tools')),
    ],
  );
}

class SongsScreen extends StatelessWidget {
  const SongsScreen({super.key, required this.model, required this.open});
  final PlayerModel model;
  final OpenPage open;

  @override
  Widget build(BuildContext context) => ListenableBuilder(
    listenable: model,
    builder: (context, _) => ArtworkAtmosphere(
      model: model,
      track: model.current,
      shortFade: true,
      child: Scaffold(
        backgroundColor: Colors.transparent,
        extendBody: true,
        extendBodyBehindAppBar: true,
        appBar: AppBar(
          backgroundColor: Colors.transparent,
          scrolledUnderElevation: 0,
          flexibleSpace: const EdgeTint(top: true, child: SizedBox.expand()),
          title: const Text('Songs'),
          actions: [
            ActionIcon(
              icon: Icons.filter_list_rounded,
              label: 'Filter songs',
              selected: model.hasActiveFilter,
              onPressed: () => songFilters(context, model),
            ),
          ],
        ),
        body: SongsPage(
          model: model,
          open: open,
          topInset: MediaQuery.paddingOf(context).top + kToolbarHeight,
        ),
        bottomNavigationBar: AppBottomBar(model: model, open: open),
      ),
    ),
  );
}

class SongsPage extends StatelessWidget {
  const SongsPage({
    super.key,
    required this.model,
    required this.open,
    required this.topInset,
  });
  final PlayerModel model;
  final OpenPage open;
  final double topInset;

  @override
  Widget build(BuildContext context) {
    final tracks = model.tracks;
    return Column(
      children: [
        Padding(
          padding: EdgeInsets.fromLTRB(
            AppGeometry.pageInset,
            topInset + AppSpace.s,
            AppSpace.s,
            AppSpace.xs,
          ),
          child: Row(
            children: [
              Text(
                '${tracks.length} ${tracks.length == 1 ? 'song' : 'songs'}',
                style: Theme.of(context).textTheme.bodyMedium?.copyWith(
                  color: Theme.of(context).colorScheme.onSurfaceVariant,
                ),
              ),
              const Spacer(),
              TextButton.icon(
                onPressed: tracks.isEmpty ? null : () => model.play(tracks),
                icon: const Icon(Icons.play_arrow_rounded),
                label: const Text('Play all'),
              ),
            ],
          ),
        ),
        Expanded(
          child: tracks.isEmpty
              ? Padding(
                  padding: EdgeInsets.only(
                    bottom:
                        MediaQuery.paddingOf(context).bottom +
                        AppGeometry.pageInset,
                  ),
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
    this.lit = Lit.none,
  });
  final Lit lit;
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
    final scheme = Theme.of(context).colorScheme;
    return Semantics(
      label: spokenNumber == null
          ? 'Play ${title(track)}'
          : 'Play song $spokenNumber: ${title(track)}',
      child: ListTile(
        onTap: onTap,
        leading: number == null
            ? Artwork(
                model: model,
                track: track,
                size: AppGeometry.songArt,
                radius: AppGeometry.thumbRadius,
              )
            : SizedBox(
                width: 28,
                child: Text(
                  '$number',
                  textAlign: TextAlign.center,
                  style: Theme.of(context).textTheme.bodyMedium?.copyWith(
                    fontFeatures: tabularFigures,
                    color: playing ? scheme.primary : scheme.onSurfaceVariant,
                  ),
                ),
              ),
        title: LitText(
          title(track),
          ranges: lit.of('TITLE', title(track)),
          style: TextStyle(
            fontWeight: playing ? FontWeight.w600 : FontWeight.w400,
            color: playing ? scheme.primary : null,
          ),
        ),
        subtitle: LitText(
          artist(track),
          ranges: lit.of('ARTIST', '${track['artist'] ?? ''}'),
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
  OpenPage open, {
  Object? queueId,
  bool player = false,
}) {
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
              leading: Artwork(
                model: model,
                track: track,
                size: AppGeometry.rowArt,
                radius: AppGeometry.coverRadius,
              ),
              title: Text(
                title(track),
                style: Theme.of(context).textTheme.titleMedium,
              ),
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
              (
                icon: Icons.share_rounded,
                label: 'Share link',
                action: () => model.call('shareLink', {'id': track['id']}),
              ),
              if (player) ...[
                (
                  icon: Icons.equalizer_rounded,
                  label: 'Equaliser',
                  action: () => open('equaliser'),
                ),
                (
                  icon: Icons.speaker_outlined,
                  label: 'Audio output',
                  action: () => open('output'),
                ),
                (
                  icon: Icons.graphic_eq_rounded,
                  label: 'Visualizer',
                  action: () => open('visualizer'),
                ),
              ],
              if (queueId != null)
                (
                  icon: Icons.remove_circle_outline_rounded,
                  label: 'Remove from queue',
                  action: () => model.call('removeFromQueue', {'id': queueId}),
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
            const SheetTitle('Add to playlist'),
            ListTile(
              leading: const IconTile(
                icon: Icons.add_rounded,
                size: AppGeometry.songArt,
              ),
              title: const Text('Create playlist'),
              onTap: () => namePlaylist(sheet, model),
            ),
            ...rows(model.state['playlists']).map(
              (p) => ListTile(
                leading: PlaylistCover(
                  model: model,
                  covers: p['covers'],
                  size: AppGeometry.songArt,
                  radius: AppGeometry.thumbRadius,
                ),
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
            padding: EdgeInsets.only(
              bottom:
                  MediaQuery.viewInsetsOf(context).bottom +
                  AppGeometry.pageInset,
            ),
            child: Column(
              mainAxisSize: MainAxisSize.min,
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                const SheetTitle('Shape your listening'),
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
                Padding(
                  padding: const EdgeInsets.fromLTRB(
                    AppGeometry.pageInset,
                    AppSpace.m,
                    AppGeometry.pageInset,
                    0,
                  ),
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: [
                      for (final key in ['artist', 'album', 'genre'])
                        Padding(
                          padding: const EdgeInsets.only(bottom: AppSpace.m),
                          child: TextFormField(
                            initialValue: '${filter[key] ?? ''}',
                            decoration: InputDecoration(
                              labelText: humanize(key),
                            ),
                            onChanged: (value) => filter[key] = value,
                          ),
                        ),
                      DropdownButtonFormField<String>(
                        initialValue: filter['order'] as String? ?? 'TITLE',
                        decoration: const InputDecoration(labelText: 'Sort by'),
                        items:
                            [
                                  'TITLE',
                                  'ARTIST',
                                  'ALBUM',
                                  'ADDED',
                                  'PLAYS',
                                  'DURATION',
                                ]
                                .map(
                                  (v) => DropdownMenuItem(
                                    value: v,
                                    child: Text(humanize(v)),
                                  ),
                                )
                                .toList(),
                        onChanged: (v) => filter['order'] = v,
                      ),
                      const SizedBox(height: AppSpace.m),
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
                                  v == 0
                                      ? 'Any sample rate'
                                      : '${v / 1000} kHz',
                                ),
                              ),
                            )
                            .toList(),
                        onChanged: (v) => filter['minimumRate'] = v,
                      ),
                      const SizedBox(height: AppSpace.m),
                      DropdownButtonFormField<int>(
                        initialValue: integer(filter['minimumBits']),
                        decoration: const InputDecoration(
                          labelText: 'Minimum bit depth',
                        ),
                        items: [0, 16, 24, 32]
                            .map(
                              (v) => DropdownMenuItem(
                                value: v,
                                child: Text(
                                  v == 0 ? 'Any bit depth' : '$v bit',
                                ),
                              ),
                            )
                            .toList(),
                        onChanged: (v) => filter['minimumBits'] = v,
                      ),
                      const SizedBox(height: AppSpace.xl),
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
                                style: TextButton.styleFrom(
                                  foregroundColor: Theme.of(context)
                                      .colorScheme
                                      .error,
                                ),
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
                      child: _CollectionHeader(
                        cover: SizedBox.square(
                          dimension: _coverSide(context),
                          child: kind == 'favourites'
                              ? const IconTile(
                                  icon: Icons.favorite_rounded,
                                  colors: AppGradients.favourites,
                                  size: AppGeometry.headerCover,
                                )
                              : playlist
                              ? PlaylistCover(
                                  model: model,
                                  covers: data['covers'],
                                  radius: AppGeometry.cardRadius,
                                )
                              : Artwork(
                                  model: model,
                                  track: cover.isEmpty && tracks.isNotEmpty
                                      ? tracks.first
                                      : cover,
                                  radius: AppGeometry.cardRadius,
                                ),
                        ),
                        title: '${data['name'] ?? widget.data['title']}',
                        artist: kind == 'album'
                            ? '${widget.data['artist'] ?? ''}'
                            : '',
                        onArtist: () => widget.open('collection', {
                          'kind': 'artist',
                          'key': widget.data['artist'],
                          'title': widget.data['artist'],
                          'artworkTrack': widget.data['artworkTrack'],
                        }),
                        summary:
                            '${tracks.length} songs · ${tracks.fold<int>(0, (sum, t) => sum + integer(t['durationMs'])) ~/ 60000} min',
                        onPlay: tracks.isEmpty
                            ? null
                            : () => model.play(tracks),
                        onShuffle: tracks.isEmpty
                            ? null
                            : () async {
                                await model.play(tracks);
                                await model.call('shuffle', {'enabled': true});
                              },
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
                    if (_artistAlbums.isNotEmpty) ...[
                      const SliverPadding(
                        padding: EdgeInsets.symmetric(
                          horizontal: AppGeometry.pageInset,
                        ),
                        sliver: SliverToBoxAdapter(
                          child: SectionTitle('Albums'),
                        ),
                      ),
                      SliverToBoxAdapter(
                        child: Shelf(
                          itemCount: _artistAlbums.length,
                          itemBuilder: (_, index) => AlbumCard(
                            model: model,
                            album: _artistAlbums[index],
                            onTap: () => widget.open('collection', {
                              ..._artistAlbums[index],
                              'kind': 'album',
                            }),
                          ),
                        ),
                      ),
                      const SliverPadding(
                        padding: EdgeInsets.symmetric(
                          horizontal: AppGeometry.pageInset,
                        ),
                        sliver: SliverToBoxAdapter(
                          child: SectionTitle('Songs'),
                        ),
                      ),
                    ],
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
                    SliverToBoxAdapter(
                      child: Builder(
                        builder: (body) => SizedBox(
                          height:
                              MediaQuery.paddingOf(body).bottom +
                              AppGeometry.pageInset,
                        ),
                      ),
                    ),
                  ],
                ),
          bottomNavigationBar: AppBottomBar(model: model, open: widget.open),
        ),
      );
    },
  );

  double _coverSide(BuildContext context) => math.min(
    MediaQuery.textScalerOf(context)
        .scale(AppGeometry.headerCover)
        .clamp(0, AppGeometry.headerCoverAtMost),
    MediaQuery.sizeOf(context).width - 2 * AppGeometry.pageInset,
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
                const SheetTitle('Songs on your devices'),
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
                    padding: const EdgeInsets.fromLTRB(
                      AppGeometry.pageInset,
                      0,
                      AppGeometry.pageInset,
                      AppSpace.s,
                    ),
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
                    padding: const EdgeInsets.all(AppGeometry.pageInset),
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

class _CollectionHeader extends StatelessWidget {
  const _CollectionHeader({
    required this.cover,
    required this.title,
    required this.artist,
    required this.onArtist,
    required this.summary,
    required this.onPlay,
    required this.onShuffle,
  });
  final Widget cover;
  final String title;
  final String artist;
  final VoidCallback onArtist;
  final String summary;
  final VoidCallback? onPlay;
  final VoidCallback? onShuffle;

  @override
  Widget build(BuildContext context) {
    final text = Theme.of(context).textTheme;
    final scheme = Theme.of(context).colorScheme;
    return Padding(
      padding: const EdgeInsets.all(AppGeometry.pageInset),
      child: Column(
        children: [
          DecoratedBox(
            decoration: BoxDecoration(
              borderRadius: BorderRadius.circular(AppGeometry.cardRadius),
              boxShadow: [
                BoxShadow(
                  color: Colors.black.withValues(alpha: .25),
                  blurRadius: 18,
                  offset: const Offset(0, 10),
                ),
              ],
            ),
            child: ClipRRect(
              borderRadius: BorderRadius.circular(AppGeometry.cardRadius),
              child: cover,
            ),
          ),
          const SizedBox(height: AppSpace.l),
          Text(title, textAlign: TextAlign.center, style: text.headlineSmall),
          if (artist.isNotEmpty)
            TextButton(
              onPressed: onArtist,
              style: TextButton.styleFrom(
                textStyle: text.titleMedium,
                foregroundColor: scheme.primary,
              ),
              child: Text(
                artist,
                textAlign: TextAlign.center,
                maxLines: 1,
                overflow: TextOverflow.ellipsis,
              ),
            )
          else
            const SizedBox(height: AppSpace.xs),
          Text(summary, textAlign: TextAlign.center, style: text.bodySmall),
          const SizedBox(height: AppSpace.l),
          PlayShuffle(onPlay: onPlay, onShuffle: onShuffle),
        ],
      ),
    );
  }
}
