import 'dart:async';

import 'package:flutter/material.dart';

import 'library.dart';
import 'main.dart' show OpenPage;
import 'model.dart';
import 'tools.dart' show chooseDownload;
import 'visuals.dart';

enum _OnlineView { results, releases, release, link }

class SearchPage extends StatefulWidget {
  const SearchPage({super.key, required this.model, required this.open});
  final PlayerModel model;
  final OpenPage open;
  @override
  State<SearchPage> createState() => _SearchPageState();
}

class _SearchPageState extends State<SearchPage> {
  final _controller = TextEditingController();
  Timer? _debounce;
  String _place = 'Your library';
  String _libraryScope = 'Top';
  String _onlineType = 'Songs';
  RowData _libraryResult = {};
  Lit _lit = Lit.none;
  bool _searching = false;
  _OnlineView _view = _OnlineView.results;
  String? _artistId;
  String _artistName = '';
  String _albumTitle = '';
  String _releaseTitle = '';

  @override
  void dispose() {
    _debounce?.cancel();
    _controller.dispose();
    super.dispose();
  }

  void _onSearchChanged(String text) {
    _debounce?.cancel();
    final trimmed = text.trim();
    if (trimmed.isEmpty) {
      setState(() {
        _libraryResult = {};
        _searching = false;
      });
      return;
    }
    _debounce = Timer(const Duration(milliseconds: 180), () async {
      if (_place == 'Your library') {
        setState(() => _searching = true);
        final result = await widget.model.searchLibrary(trimmed);
        final lit = await widget.model.lit(trimmed, result);
        if (mounted && _controller.text.trim() == trimmed) {
          setState(() {
            _libraryResult = result;
            _lit = lit;
            _searching = false;
          });
        }
      } else {
        _performOnlineSearch(trimmed);
      }
    });
  }

  void _performOnlineSearch(String query) {
    if (query.length < 2) return;
    if (_view != _OnlineView.results || _artistId != null) {
      setState(() {
        _view = _OnlineView.results;
        _artistId = null;
      });
    }
    widget.model.call(
      switch (_onlineType) {
        'Albums' => 'searchAlbums',
        'Artists' => 'searchArtists',
        _ => 'searchOnline',
      },
      {'text': query},
    );
  }

  void _switchPlace(String place) {
    setState(() => _place = place);
    final text = _controller.text.trim();
    if (text.isNotEmpty) {
      if (place == 'Online') {
        _performOnlineSearch(text);
      } else {
        _onSearchChanged(text);
      }
    }
  }

  @override
  Widget build(BuildContext context) => ListenableBuilder(
    listenable: widget.model,
    builder: (context, _) => _page(context),
  );

  Widget _page(BuildContext context) {
    final model = widget.model;
    final query = _controller.text.trim();
    final tracks = rows(_libraryResult['tracks']);
    final albums = rows(_libraryResult['albums']);
    final artists = rows(_libraryResult['artists']);
    final reads = (_libraryResult['reads'] as List? ?? [])
        .map((e) => '$e')
        .toList();
    final didYouMean = _libraryResult['didYouMean'] as String?;
    final trackCount = integer(_libraryResult['trackCount'] ?? tracks.length);

    return Scaffold(
      backgroundColor: Colors.transparent,
      body: SafeArea(
        bottom: false,
        child: Column(
          children: [
            Padding(
              padding: const EdgeInsets.fromLTRB(
                AppGeometry.pageInset,
                AppGeometry.pageTop,
                AppGeometry.pageInset,
                AppSpace.s,
              ),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  const LargeTitle('Search'),
                  const SizedBox(height: AppSpace.l),
                  TextField(
                    controller: _controller,
                    onChanged: _onSearchChanged,
                    onSubmitted: (text) {
                      widget.model.keepRecentSearch(text);
                      _onSearchChanged(text);
                    },
                    decoration: InputDecoration(
                      hintText: _place == 'Online'
                          ? 'Search songs, albums, artists…'
                          : 'Search your library',
                      prefixIcon: const Icon(Icons.search_rounded),
                      suffixIcon: query.isNotEmpty
                          ? IconButton(
                              tooltip: 'Clear search',
                              icon: const Icon(Icons.close_rounded),
                              onPressed: () {
                                _controller.clear();
                                _onSearchChanged('');
                              },
                            )
                          : null,
                    ),
                  ),
                  const SizedBox(height: AppSpace.m),
                  Row(
                    children: [
                      Expanded(
                        child: ChoiceRow(
                          labels: const ['Your library', 'Online'],
                          selected: _place,
                          onSelected: _switchPlace,
                        ),
                      ),
                      if (_place == 'Online')
                        ActionIcon(
                          icon: Icons.bookmark_border_rounded,
                          label: 'Saved for later',
                          onPressed: () => widget.open('wanted'),
                        ),
                    ],
                  ),
                ],
              ),
            ),
            if (_searching || model.state['searching'] == true)
              const LinearProgressIndicator(minHeight: 2),
            Expanded(
              child: _place == 'Your library'
                  ? _buildLibraryContent(
                      context,
                      query,
                      tracks,
                      albums,
                      artists,
                      reads,
                      didYouMean,
                      trackCount,
                    )
                  : _buildOnlineContent(context, query),
            ),
          ],
        ),
      ),
    );
  }

  static const _sectionPadding = EdgeInsets.fromLTRB(
    AppGeometry.pageInset,
    AppSpace.l,
    AppGeometry.pageInset,
    AppSpace.xs,
  );

  static const _pageEdges = EdgeInsets.symmetric(
    horizontal: AppGeometry.pageInset,
  );

  Widget _bottomSpace(BuildContext context) => SizedBox(
    height: MediaQuery.paddingOf(context).bottom + AppGeometry.pageInset,
  );

  void _openAlbum(String query, RowData album) {
    widget.model.keepRecentSearch(query);
    widget.open('collection', {...album, 'kind': 'album'});
  }

  void _openLibraryArtist(String query, RowData artist) {
    widget.model.keepRecentSearch(query);
    widget.open('collection', {
      'kind': 'artist',
      'key': artist['name'],
      'title': artist['name'],
      'artworkTrack': artist['artworkTrack'],
    });
  }

  AlbumCard _albumCard(String query, RowData album) => AlbumCard(
    model: widget.model,
    album: album,
    onTap: () => _openAlbum(query, album),
    titleRanges: _lit.of('ALBUM', '${album['title']}'),
    detailRanges: _lit.of('ARTIST', '${album['artist']}'),
  );

  Widget _albumGrid(String query, List<RowData> albums) {
    const widest = AppGeometry.carouselCover + 20;
    const across = AppSpace.l;
    return SliverPadding(
      padding: _pageEdges,
      sliver: SliverLayoutBuilder(
        builder: (context, constraints) {
          final extent = constraints.crossAxisExtent;
          final columns = (extent / (widest + across)).ceil().clamp(1, 12);
          final tile = (extent - across * (columns - 1)) / columns;
          return SliverGrid.builder(
            gridDelegate: SliverGridDelegateWithFixedCrossAxisCount(
              crossAxisCount: columns,
              mainAxisSpacing: 20,
              crossAxisSpacing: across,
              mainAxisExtent: tile + shelfCaptionHeight(context),
            ),
            itemCount: albums.length,
            itemBuilder: (context, i) => _albumCard(query, albums[i]),
          );
        },
      ),
    );
  }

  Widget _buildLibraryContent(
    BuildContext context,
    String query,
    List<RowData> tracks,
    List<RowData> albums,
    List<RowData> artists,
    List<String> reads,
    String? didYouMean,
    int trackCount,
  ) {
    if (query.isEmpty) {
      return ListView(
        padding: EdgeInsets.only(
          top: AppSpace.s,
          bottom: MediaQuery.paddingOf(context).bottom + AppGeometry.pageInset,
        ),
        children: [
          if (widget.model.recentSearches.isNotEmpty) ...[
            SectionTitle(
              'Recent searches',
              padding: const EdgeInsets.fromLTRB(
                AppGeometry.pageInset,
                AppSpace.s,
                AppSpace.s,
                AppSpace.xs,
              ),
              action: TextButton(
                onPressed: widget.model.forgetRecentSearches,
                child: const Text('Clear'),
              ),
            ),
            ...widget.model.recentSearches.map(
              (recent) => ListTile(
                leading: const Icon(Icons.history_rounded),
                title: Text(recent),
                trailing: const Icon(Icons.north_west_rounded, size: 18),
                onTap: () {
                  _controller.text = recent;
                  _onSearchChanged(recent);
                },
              ),
            ),
            const SizedBox(height: AppSpace.xl),
          ],
          const EmptyState(
            icon: Icons.search_rounded,
            title: 'Search your library and online',
            detail: 'Find songs, albums and artists you have, or choose Online to find music to download.',
          ),
        ],
      );
    }

    final matchedNothing =
        tracks.isEmpty && albums.isEmpty && artists.isEmpty && !_searching;
    final scopes = <(String, String)>[
      ('Top', 'Top'),
      (
        'Songs',
        trackCount > tracks.length
            ? 'Songs (${tracks.length} of $trackCount)'
            : 'Songs ($trackCount)',
      ),
      if (albums.isNotEmpty) ('Albums', 'Albums (${albums.length})'),
      if (artists.isNotEmpty) ('Artists', 'Artists (${artists.length})'),
    ];

    return CustomScrollView(
      slivers: [
        if (reads.isNotEmpty)
          SliverToBoxAdapter(
            child: SingleChildScrollView(
              scrollDirection: Axis.horizontal,
              padding: const EdgeInsets.fromLTRB(
                AppGeometry.pageInset,
                AppSpace.xs,
                AppGeometry.pageInset,
                AppSpace.xs,
              ),
              child: Row(
                children: [
                  for (final read in reads)
                    Padding(
                      padding: const EdgeInsets.only(right: AppSpace.s),
                      child: Pill(read, icon: Icons.tune_rounded),
                    ),
                ],
              ),
            ),
          ),
        if (didYouMean != null && didYouMean.isNotEmpty)
          SliverToBoxAdapter(
            child: Padding(
              padding: const EdgeInsets.fromLTRB(
                AppGeometry.pageInset,
                AppSpace.s,
                AppGeometry.pageInset,
                AppSpace.s,
              ),
              child: Align(
                alignment: AlignmentDirectional.centerStart,
                child: ActionChip(
                  avatar: Icon(
                    Icons.auto_fix_high_rounded,
                    color: Theme.of(context).colorScheme.primary,
                  ),
                  label: Text.rich(
                    TextSpan(
                      text: 'Did you mean ',
                      children: [
                        TextSpan(
                          text: didYouMean,
                          style: const TextStyle(fontWeight: FontWeight.w700),
                        ),
                        const TextSpan(text: '?'),
                      ],
                    ),
                  ),
                  onPressed: () {
                    _controller.text = didYouMean;
                    widget.model.keepRecentSearch(didYouMean);
                    _onSearchChanged(didYouMean);
                  },
                ),
              ),
            ),
          ),
        if (!matchedNothing)
          SliverToBoxAdapter(
            child: ChoiceRow(
              padding: const EdgeInsets.fromLTRB(
                AppGeometry.pageInset,
                AppSpace.s,
                AppGeometry.pageInset,
                AppSpace.xs,
              ),
              labels: [for (final (_, label) in scopes) label],
              selected: scopes
                  .firstWhere(
                    (scope) => scope.$1 == _libraryScope,
                    orElse: () => scopes.first,
                  )
                  .$2,
              onSelected: (label) => setState(
                () => _libraryScope = scopes
                    .firstWhere((scope) => scope.$2 == label)
                    .$1,
              ),
            ),
          ),
        if (matchedNothing)
          SliverFillRemaining(
            hasScrollBody: false,
            child: Column(
              mainAxisAlignment: MainAxisAlignment.center,
              children: [
                EmptyState(
                  icon: Icons.search_off_rounded,
                  title: 'No songs match “$query”',
                  detail: 'Check the spelling, or try searching online.',
                  action: FilledButton.icon(
                    icon: const Icon(Icons.public_rounded),
                    label: Text('Search online for “$query”'),
                    onPressed: () => _switchPlace('Online'),
                  ),
                ),
              ],
            ),
          )
        else ...[
          if (_libraryScope == 'Top' || _libraryScope == 'Songs') ...[
            SliverToBoxAdapter(
              child: SectionTitle(
                'Songs',
                padding: _sectionPadding,
                onSeeAll: _libraryScope == 'Top' && trackCount > 5
                    ? () => setState(() => _libraryScope = 'Songs')
                    : null,
              ),
            ),
            SliverList.builder(
              itemCount: _libraryScope == 'Top'
                  ? tracks.length.clamp(0, 5)
                  : tracks.length,
              itemBuilder: (context, i) => SongTile(
                model: widget.model,
                track: tracks[i],
                lit: _lit,
                onTap: () {
                  widget.model.keepRecentSearch(query);
                  widget.model.play(tracks, i);
                },
                open: widget.open,
              ),
            ),
            if (_libraryScope == 'Songs' && trackCount > tracks.length)
              SliverToBoxAdapter(
                child: Padding(
                  padding: const EdgeInsets.fromLTRB(
                    AppGeometry.pageInset,
                    AppSpace.m,
                    AppGeometry.pageInset,
                    AppSpace.s,
                  ),
                  child: Text(
                    'Showing the first ${tracks.length} of $trackCount songs. Narrow the search to see the rest.',
                    style: Theme.of(context).textTheme.bodySmall,
                  ),
                ),
              ),
          ],
          if ((_libraryScope == 'Top' || _libraryScope == 'Albums') &&
              albums.isNotEmpty) ...[
            SliverToBoxAdapter(
              child: SectionTitle(
                'Albums',
                padding: _sectionPadding,
                onSeeAll: _libraryScope == 'Top' && albums.length > 6
                    ? () => setState(() => _libraryScope = 'Albums')
                    : null,
              ),
            ),
            if (_libraryScope == 'Top')
              SliverToBoxAdapter(
                child: Shelf(
                  itemCount: albums.length.clamp(0, 6),
                  itemBuilder: (context, i) => _albumCard(query, albums[i]),
                ),
              )
            else
              _albumGrid(query, albums),
          ],
          if ((_libraryScope == 'Top' || _libraryScope == 'Artists') &&
              artists.isNotEmpty) ...[
            SliverToBoxAdapter(
              child: SectionTitle(
                'Artists',
                padding: _sectionPadding,
                onSeeAll: _libraryScope == 'Top' && artists.length > 6
                    ? () => setState(() => _libraryScope = 'Artists')
                    : null,
              ),
            ),
            if (_libraryScope == 'Top')
              SliverToBoxAdapter(
                child: Shelf(
                  itemCount: artists.length.clamp(0, 6),
                  itemWidth: AppGeometry.artistCircle,
                  coverHeight: AppGeometry.artistCircle,
                  itemBuilder: (context, i) => ArtistCard(
                    model: widget.model,
                    artist: artists[i],
                    nameRanges: _lit.of('ARTIST', '${artists[i]['name']}'),
                    onTap: () => _openLibraryArtist(query, artists[i]),
                  ),
                ),
              )
            else
              SliverList.builder(
                itemCount: artists.length,
                itemBuilder: (context, i) => CollectionRow(
                  leading: Artwork(
                    model: widget.model,
                    track: widget.model.trackById(artists[i]['artworkTrack']),
                    size: AppGeometry.rowArt,
                    radius: AppGeometry.rowArt / 2,
                  ),
                  title: LitText(
                    '${artists[i]['name']}',
                    ranges: _lit.of('ARTIST', '${artists[i]['name']}'),
                  ),
                  subtitle: Text('${artists[i]['songs']} songs'),
                  onTap: () => _openLibraryArtist(query, artists[i]),
                ),
              ),
          ],
          if (_libraryScope == 'Top' && trackCount < 5)
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
                      vertical: AppSpace.xs,
                    ),
                    child: CollectionRow(
                      leading: const IconTile(
                        icon: Icons.public_rounded,
                        colors: AppGradients.online,
                      ),
                      title: Text('Search online for “$query”'),
                      subtitle: const Text(
                        'Find releases, songs and downloads online',
                      ),
                      onTap: () => _switchPlace('Online'),
                    ),
                  ),
                ),
              ),
            ),
          SliverToBoxAdapter(child: _bottomSpace(context)),
        ],
      ],
    );
  }

  Widget _buildOnlineContent(BuildContext context, String query) {
    final model = widget.model;
    if (!model.online) {
      return EmptyState(
        icon: Icons.cloud_off_rounded,
        title: 'You’re listening offline',
        detail: 'Enable online features to find music, artwork and lyrics.',
        action: FilledButton(
          onPressed: () => model.call('online', {'enabled': true}),
          child: const Text('Enable online features'),
        ),
      );
    }

    return ListView(
      padding: const EdgeInsets.only(top: AppSpace.s),
      children: [
        if (_view == _OnlineView.results)
          Padding(
            padding: _pageEdges,
            child: Row(
              children: [
                SegmentedButton<String>(
                  showSelectedIcon: false,
                  segments: const [
                    ButtonSegment(value: 'Songs', label: Text('Songs')),
                    ButtonSegment(value: 'Albums', label: Text('Albums')),
                    ButtonSegment(value: 'Artists', label: Text('Artists')),
                  ],
                  selected: {_onlineType},
                  onSelectionChanged: (set) {
                    setState(() {
                      _onlineType = set.first;
                      _artistId = null;
                    });
                    if (query.isNotEmpty) _performOnlineSearch(query);
                  },
                ),
                const Spacer(),
                TextButton.icon(
                  icon: const Icon(Icons.link_rounded),
                  label: const Text('Link'),
                  onPressed: () => _openMusicLink(context),
                ),
              ],
            ),
          ),
        if (model.state['downloadStatus'] != null)
          Padding(
            padding: const EdgeInsets.fromLTRB(
              AppGeometry.pageInset,
              AppSpace.s,
              AppGeometry.pageInset,
              AppSpace.s,
            ),
            child: Card(
              child: ListTileTheme.merge(
                contentPadding: const EdgeInsets.symmetric(
                  horizontal: AppSpace.l,
                  vertical: AppSpace.xs,
                ),
                child: ListTile(
                  leading: Icon(
                    Icons.download_rounded,
                    color: Theme.of(context).colorScheme.primary,
                  ),
                  title: Text('${model.state['downloadStatus']}'),
                  subtitle: ValueListenableBuilder<Object?>(
                    valueListenable: model.hot('downloadedBytes'),
                    builder: (_, bytes, _) => Text(
                      '${(integer(bytes ?? model.state['downloadedBytes']) / 1048576).toStringAsFixed(1)} MB',
                      style: const TextStyle(fontFeatures: tabularFigures),
                    ),
                  ),
                  trailing: model.state['downloading'] == true
                      ? ActionIcon(
                          icon: Icons.close_rounded,
                          label: 'Cancel download',
                          onPressed: () => model.call('cancelDownload'),
                        )
                      : null,
                ),
              ),
            ),
          ),
        ...switch (_view) {
          _OnlineView.results => _onlineResults(context, query),
          _OnlineView.releases => _onlineReleases(context),
          _OnlineView.release => _onlineRelease(context),
          _OnlineView.link => _onlineLink(context),
        },
        _bottomSpace(context),
      ],
    );
  }

  Widget _onlineAlbum(RowData album, VoidCallback onTap, {String? detail}) =>
      CollectionRow(
        leading: const IconTile(icon: Icons.album_rounded),
        title: Text('${album['title']}'),
        subtitle: Text(detail ?? '${album['artist']}'),
        onTap: onTap,
      );

  Widget _onlineArtist(RowData artist) => CollectionRow(
    leading: const IconTile(icon: Icons.person_rounded, circle: true),
    title: Text('${artist['name']}'),
    onTap: () => _openArtist(artist),
  );

  List<Widget> _onlineResults(BuildContext context, String query) {
    final model = widget.model;
    final songs = rows(model.state['onlineSongs']);
    final albums = rows(model.state['onlineAlbums']);
    final artists = rows(model.state['onlineArtists']);
    if (query.isEmpty) {
      return const [
        SizedBox(height: AppSpace.l),
        EmptyState(
          icon: Icons.travel_explore_rounded,
          title: 'Explore the musical universe',
          detail: 'Search for any song, album or artist to preview and download to your library.',
        ),
      ];
    }
    final shown = switch (_onlineType) {
      'Albums' => albums.length,
      'Artists' => artists.length,
      _ => songs.length,
    };
    return [
      const SizedBox(height: AppSpace.s),
      if (_onlineType == 'Songs')
        ...songs.map((song) => _onlineSong(context, song)),
      if (_onlineType == 'Albums') ...[
        if (_artistId != null)
          SectionTitle(
            'Albums by $_artistName',
            padding: const EdgeInsets.fromLTRB(
              AppGeometry.pageInset,
              AppSpace.s,
              AppSpace.s,
              AppSpace.xs,
            ),
            action: ActionIcon(
              icon: Icons.close_rounded,
              label: 'Search albums again',
              onPressed: () {
                setState(() => _artistId = null);
                _performOnlineSearch(query);
              },
            ),
          ),
        ...albums.map(
          (album) => _onlineAlbum(album, () => _openEditions(album)),
        ),
        if (_artistId != null && model.state['artistAlbumsNext'] != null)
          Padding(
            padding: _pageEdges,
            child: Align(
              alignment: AlignmentDirectional.centerStart,
              child: TextButton(
                onPressed: () => model.call('artistAlbums', {
                  'id': _artistId,
                  'from': model.state['artistAlbumsNext'],
                }),
                child: const Text('More albums'),
              ),
            ),
          ),
      ],
      if (_onlineType == 'Artists') ...artists.map(_onlineArtist),
      if (shown == 0 && model.state['searching'] != true)
        const EmptyState(
          icon: Icons.search_off_rounded,
          title: 'Nothing found yet',
          detail: 'Try a different title or artist.',
        ),
    ];
  }

  List<Widget> _onlineReleases(BuildContext context) {
    final model = widget.model;
    final releases = rows(model.state['onlineReleases']);
    return [
      _onlineBack('Back to albums', _albumTitle, _backToResults),
      ...releases.map(
        (release) => _onlineAlbum(
          release,
          () => _openRelease(release),
          detail: [
            '${release['date'] ?? ''}',
            '${release['country'] ?? ''}',
          ].where((part) => part.isNotEmpty).join(' · '),
        ),
      ),
      if (releases.isEmpty && model.state['searching'] != true)
        const EmptyState(
          icon: Icons.album_outlined,
          title: 'No editions found',
          detail: 'This album has no releases to show.',
        ),
    ];
  }

  List<Widget> _onlineRelease(BuildContext context) {
    final model = widget.model;
    final songs = rows(model.state['onlineSongs']);
    return [
      _onlineBack('Back to editions', _releaseTitle, () {
        setState(() => _view = _OnlineView.releases);
      }),
      ...songs.map((song) => _onlineSong(context, song)),
      if (songs.isEmpty && model.state['searching'] != true)
        const EmptyState(
          icon: Icons.music_off_rounded,
          title: 'No songs listed',
          detail: 'This edition lists no songs.',
        ),
    ];
  }

  List<Widget> _onlineLink(BuildContext context) {
    final model = widget.model;
    final collection = row(model.state['onlineCollection']);
    final songs = rows(collection['songs']);
    final releases = rows(collection['releases']);
    final artists = rows(collection['artists']);
    return [
      _onlineBack(
        'Close link',
        '${collection['name'] ?? 'Shared music'}',
        () {
          model.call('clearCollection');
          _backToResults();
        },
        action: collection.isEmpty
            ? null
            : TextButton(
                onPressed: () => model.call('importPlaylist', {
                  'name': collection['name'] ?? 'Imported playlist',
                }),
                child: const Text('Import matches'),
              ),
      ),
      if (songs.isNotEmpty)
        const SectionTitle('Songs', padding: _sectionPadding),
      ...songs.map((song) => _onlineSong(context, song)),
      if (releases.isNotEmpty)
        const SectionTitle('Editions', padding: _sectionPadding),
      ...releases.map(
        (release) => _onlineAlbum(
          release,
          () => _openRelease(release),
          detail: '${release['date'] ?? ''}',
        ),
      ),
      if (artists.isNotEmpty)
        const SectionTitle('Artists', padding: _sectionPadding),
      ...artists.map(_onlineArtist),
      if (collection.isNotEmpty &&
          songs.isEmpty &&
          releases.isEmpty &&
          artists.isEmpty)
        const EmptyState(
          icon: Icons.link_off_rounded,
          title: 'Nothing to show',
          detail: 'No music could be read from this link.',
        ),
    ];
  }

  Widget _onlineBack(
    String tooltip,
    String title,
    VoidCallback onBack, {
    Widget? action,
  }) => ListTile(
    contentPadding: const EdgeInsets.symmetric(horizontal: AppSpace.m),
    leading: ActionIcon(
      icon: Icons.arrow_back_rounded,
      label: tooltip,
      onPressed: onBack,
    ),
    title: Text(
      title,
      maxLines: 1,
      overflow: TextOverflow.ellipsis,
      style: Theme.of(context).textTheme.titleMedium,
    ),
    trailing: action,
  );

  Widget _onlineSong(BuildContext context, RowData song) {
    final held = song['held'] == true;
    return CollectionRow(
      leading: IconTile(
        icon: held ? Icons.library_music_rounded : Icons.music_note_rounded,
        colors: held ? AppGradients.online : null,
      ),
      title: Text('${song['title']}'),
      subtitle: Text('${song['artist']} · ${song['album']}'),
      chevron: false,
      trailing: PopupMenuButton<String>(
        tooltip: 'More about ${song['title']}',
        icon: const Icon(Icons.more_horiz_rounded),
        onSelected: (v) => v == 'save'
            ? widget.model.call('saveWanted', {'recording': song['recording']})
            : chooseDownload(context, widget.model, {
                'recording': song['recording'],
              }),
        itemBuilder: (_) => [
          const PopupMenuItem(value: 'save', child: Text('Save for later')),
          const PopupMenuItem(value: 'download', child: Text('Download')),
        ],
      ),
    );
  }

  void _openEditions(RowData album) {
    setState(() {
      _view = _OnlineView.releases;
      _albumTitle = '${album['title']}';
    });
    widget.model.call('onlineReleases', {'id': album['id']});
  }

  void _openRelease(RowData release) {
    setState(() {
      _view = _OnlineView.release;
      _releaseTitle = '${release['title']}';
    });
    widget.model.call('releaseSongs', {'id': release['id']});
  }

  void _openArtist(RowData artist) {
    setState(() {
      _view = _OnlineView.results;
      _onlineType = 'Albums';
      _artistId = '${artist['id']}';
      _artistName = '${artist['name']}';
    });
    widget.model.call('artistAlbums', {'id': artist['id']});
  }

  void _backToResults() {
    setState(() => _view = _OnlineView.results);
    final query = _controller.text.trim();
    if (query.isNotEmpty && _artistId == null) _performOnlineSearch(query);
  }

  Future<void> _openMusicLink(BuildContext context) async {
    var draft = '';
    final link = await showDialog<String>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('Open music link'),
        content: TextFormField(
          autofocus: true,
          onChanged: (value) => draft = value,
          decoration: const InputDecoration(
            labelText: 'URL from TIDAL, Spotify or MusicBrainz',
          ),
          onFieldSubmitted: (value) => Navigator.pop(context, value.trim()),
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context),
            child: const Text('Cancel'),
          ),
          TextButton(
            onPressed: () => Navigator.pop(context, draft.trim()),
            child: const Text('Open'),
          ),
        ],
      ),
    );
    if (link != null && link.isNotEmpty) {
      setState(() {
        _place = 'Online';
        _view = _OnlineView.link;
      });
      widget.model.call('openLink', {'text': link});
    }
  }
}
