import 'dart:async';

import 'package:flutter/material.dart';

import 'library.dart';
import 'main.dart' show OpenPage;
import 'model.dart';
import 'tools.dart' show chooseDownload;
import 'visuals.dart';

class SearchPage extends StatefulWidget {
  const SearchPage({super.key, required this.model, required this.open});
  final PlayerModel model;
  final OpenPage open;
  @override
  State<SearchPage> createState() => _SearchPageState();
}

class _SearchPageState extends State<SearchPage> {
  final _controller = TextEditingController();
  final List<String> _recents = [];
  Timer? _debounce;
  String _place = 'Your library';
  String _libraryScope = 'Top';
  String _onlineType = 'Songs';
  RowData _libraryResult = {};
  bool _searching = false;

  @override
  void initState() {
    super.initState();
    _loadRecents();
  }

  Future<void> _loadRecents() async {
    final stored = widget.model.state['recentSearches'] as List? ?? [];
    if (stored.isNotEmpty && mounted) {
      setState(() {
        _recents.clear();
        _recents.addAll(stored.map((e) => '$e'));
      });
    }
  }

  void _saveRecent(String text) {
    final query = text.trim();
    if (query.isEmpty) return;
    setState(() {
      _recents.remove(query);
      _recents.insert(0, query);
      if (_recents.length > 10) _recents.removeLast();
    });
    widget.model.state['recentSearches'] = _recents;
  }

  void _clearRecents() {
    setState(() => _recents.clear());
    widget.model.state['recentSearches'] = [];
  }

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
        if (mounted && _controller.text.trim() == trimmed) {
          setState(() {
            _libraryResult = result;
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
  Widget build(BuildContext context) {
    final model = widget.model;
    final query = _controller.text.trim();
    final tracks = rows(_libraryResult['tracks']);
    final albums = rows(_libraryResult['albums']);
    final artists = rows(_libraryResult['artists']);
    final reads = (_libraryResult['reads'] as List? ?? []).map((e) => '$e').toList();
    final didYouMean = _libraryResult['didYouMean'] as String?;
    final trackCount = integer(_libraryResult['trackCount'] ?? tracks.length);

    return Scaffold(
      backgroundColor: Colors.transparent,
      body: SafeArea(
        bottom: false,
        child: Column(
          children: [
            Padding(
              padding: const EdgeInsets.fromLTRB(24, 16, 24, 8),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Row(
                    children: [
                      Expanded(
                        child: Text(
                          'Search',
                          style: Theme.of(context).textTheme.headlineLarge?.copyWith(
                            fontWeight: FontWeight.w800,
                            letterSpacing: -1,
                          ),
                        ),
                      ),
                      ActionIcon(
                        icon: Icons.hearing_rounded,
                        label: 'Listen & recognise',
                        onPressed: () => _openListening(context),
                      ),
                    ],
                  ),
                  const SizedBox(height: 16),
                  TextField(
                    controller: _controller,
                    onChanged: _onSearchChanged,
                    onSubmitted: (text) {
                      _saveRecent(text);
                      _onSearchChanged(text);
                    },
                    decoration: InputDecoration(
                      hintText: _place == 'Online'
                          ? 'Search songs, albums, artists…'
                          : 'Search your library',
                      prefixIcon: const Icon(Icons.search_rounded),
                      suffixIcon: query.isNotEmpty
                          ? IconButton(
                              icon: const Icon(Icons.close_rounded),
                              onPressed: () {
                                _controller.clear();
                                _onSearchChanged('');
                              },
                            )
                          : null,
                    ),
                  ),
                  const SizedBox(height: 12),
                  Row(
                    children: [
                      ChoiceChip(
                        label: const Text('Your library'),
                        selected: _place == 'Your library',
                        showCheckmark: false,
                        onSelected: (_) => _switchPlace('Your library'),
                      ),
                      const SizedBox(width: 8),
                      ChoiceChip(
                        label: const Text('Online'),
                        selected: _place == 'Online',
                        showCheckmark: false,
                        onSelected: (_) => _switchPlace('Online'),
                      ),
                      const Spacer(),
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
        padding: const EdgeInsets.fromLTRB(24, 8, 24, 180),
        children: [
          if (_recents.isNotEmpty) ...[
            Row(
              children: [
                Expanded(
                  child: Text(
                    'Recent searches',
                    style: Theme.of(context).textTheme.titleMedium?.copyWith(
                      fontWeight: FontWeight.w700,
                    ),
                  ),
                ),
                TextButton(
                  onPressed: _clearRecents,
                  child: const Text('Clear'),
                ),
              ],
            ),
            const SizedBox(height: 4),
            Card.filled(
              margin: EdgeInsets.zero,
              clipBehavior: Clip.antiAlias,
              child: Column(
                children: _recents
                    .map(
                      (recent) => ListTile(
                        leading: const Icon(Icons.history_rounded),
                        title: Text(recent),
                        trailing: const Icon(Icons.north_west_rounded, size: 18),
                        onTap: () {
                          _controller.text = recent;
                          _onSearchChanged(recent);
                        },
                      ),
                    )
                    .toList(),
              ),
            ),
            const SizedBox(height: 24),
          ],
          const EmptyState(
            icon: Icons.search_rounded,
            title: 'Search your library and online',
            detail: 'Find songs, albums and artists you have, or choose Online to find music to download.',
          ),
        ],
      );
    }

    final matchedNothing = tracks.isEmpty && albums.isEmpty && artists.isEmpty && !_searching;

    return CustomScrollView(
      slivers: [
        if (reads.isNotEmpty)
          SliverToBoxAdapter(
            child: SizedBox(
              height: 44,
              child: ListView.separated(
                padding: const EdgeInsets.symmetric(horizontal: 24),
                scrollDirection: Axis.horizontal,
                itemCount: reads.length,
                separatorBuilder: (_, _) => const SizedBox(width: 8),
                itemBuilder: (context, i) => Glass(
                  radius: 16,
                  padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 6),
                  child: Text(
                    reads[i],
                    style: TextStyle(
                      fontFamily: 'monospace',
                      fontSize: 12,
                      color: Theme.of(context).colorScheme.primary,
                    ),
                  ),
                ),
              ),
            ),
          ),
        if (didYouMean != null && didYouMean.isNotEmpty)
          SliverToBoxAdapter(
            child: Padding(
              padding: const EdgeInsets.fromLTRB(24, 8, 24, 8),
              child: InkWell(
                borderRadius: BorderRadius.circular(16),
                onTap: () {
                  _controller.text = didYouMean;
                  _saveRecent(didYouMean);
                  _onSearchChanged(didYouMean);
                },
                child: Glass(
                  radius: 16,
                  padding: const EdgeInsets.all(12),
                  child: Row(
                    children: [
                      Icon(Icons.auto_fix_high_rounded, color: Theme.of(context).colorScheme.primary),
                      const SizedBox(width: 12),
                      Expanded(
                        child: Text.rich(
                          TextSpan(
                            text: 'Did you mean ',
                            children: [
                              TextSpan(
                                text: didYouMean,
                                style: const TextStyle(fontWeight: FontWeight.bold),
                              ),
                              const TextSpan(text: '?'),
                            ],
                          ),
                        ),
                      ),
                    ],
                  ),
                ),
              ),
            ),
          ),
        if (!matchedNothing)
          SliverToBoxAdapter(
            child: SingleChildScrollView(
              scrollDirection: Axis.horizontal,
              padding: const EdgeInsets.fromLTRB(24, 8, 24, 12),
              child: Row(
                children: [
                  'Top',
                  'Songs ($trackCount)',
                  if (albums.isNotEmpty) 'Albums (${albums.length})',
                  if (artists.isNotEmpty) 'Artists (${artists.length})',
                ].map((scope) {
                  final key = scope.split(' ').first;
                  final selected = _libraryScope == key;
                  return Padding(
                    padding: const EdgeInsets.only(right: 8),
                    child: ChoiceChip(
                      label: Text(scope),
                      selected: selected,
                      showCheckmark: false,
                      onSelected: (_) => setState(() => _libraryScope = key),
                    ),
                  );
                }).toList(),
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
              child: Padding(
                padding: const EdgeInsets.fromLTRB(24, 8, 24, 4),
                child: Row(
                  children: [
                    Text(
                      'Songs',
                      style: Theme.of(context).textTheme.titleMedium?.copyWith(
                        fontWeight: FontWeight.bold,
                      ),
                    ),
                    const Spacer(),
                    if (_libraryScope == 'Top' && trackCount > 5)
                      TextButton(
                        onPressed: () => setState(() => _libraryScope = 'Songs'),
                        child: const Text('See all'),
                      ),
                  ],
                ),
              ),
            ),
            SliverList.builder(
              itemCount: _libraryScope == 'Top' ? tracks.length.clamp(0, 5) : tracks.length,
              itemBuilder: (context, i) => SongTile(
                model: widget.model,
                track: tracks[i],
                onTap: () {
                  _saveRecent(query);
                  widget.model.play(tracks, i);
                },
                open: widget.open,
              ),
            ),
          ],
          if ((_libraryScope == 'Top' || _libraryScope == 'Albums') && albums.isNotEmpty) ...[
            SliverToBoxAdapter(
              child: Padding(
                padding: const EdgeInsets.fromLTRB(24, 16, 24, 8),
                child: Row(
                  children: [
                    Text(
                      'Albums',
                      style: Theme.of(context).textTheme.titleMedium?.copyWith(
                        fontWeight: FontWeight.bold,
                      ),
                    ),
                    const Spacer(),
                    if (_libraryScope == 'Top' && albums.length > 6)
                      TextButton(
                        onPressed: () => setState(() => _libraryScope = 'Albums'),
                        child: const Text('See all'),
                      ),
                  ],
                ),
              ),
            ),
            if (_libraryScope == 'Top')
              SliverToBoxAdapter(
                child: SizedBox(
                  height: 200,
                  child: ListView.separated(
                    padding: const EdgeInsets.symmetric(horizontal: 24),
                    scrollDirection: Axis.horizontal,
                    itemCount: albums.length.clamp(0, 6),
                    separatorBuilder: (_, _) => const SizedBox(width: 14),
                    itemBuilder: (context, i) => SizedBox(
                      width: 136,
                      child: InkWell(
                        borderRadius: BorderRadius.circular(16),
                        onTap: () {
                          _saveRecent(query);
                          widget.open('collection', {...albums[i], 'kind': 'album'});
                        },
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            AspectRatio(
                              aspectRatio: 1,
                              child: Artwork(
                                model: widget.model,
                                track: widget.model.trackById(albums[i]['artworkTrack']),
                                radius: 16,
                              ),
                            ),
                            const SizedBox(height: 6),
                            Text(
                              '${albums[i]['title']}',
                              maxLines: 1,
                              overflow: TextOverflow.ellipsis,
                              style: const TextStyle(fontWeight: FontWeight.w600, fontSize: 13),
                            ),
                            Text(
                              '${albums[i]['artist']}',
                              maxLines: 1,
                              overflow: TextOverflow.ellipsis,
                              style: Theme.of(context).textTheme.bodySmall,
                            ),
                          ],
                        ),
                      ),
                    ),
                  ),
                ),
              )
            else
              SliverPadding(
                padding: const EdgeInsets.symmetric(horizontal: 24),
                sliver: SliverGrid.builder(
                  gridDelegate: const SliverGridDelegateWithMaxCrossAxisExtent(
                    maxCrossAxisExtent: 200,
                    mainAxisSpacing: 16,
                    crossAxisSpacing: 14,
                    childAspectRatio: .75,
                  ),
                  itemCount: albums.length,
                  itemBuilder: (context, i) => InkWell(
                    borderRadius: BorderRadius.circular(16),
                    onTap: () {
                      _saveRecent(query);
                      widget.open('collection', {...albums[i], 'kind': 'album'});
                    },
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        AspectRatio(
                          aspectRatio: 1,
                          child: Artwork(
                            model: widget.model,
                            track: widget.model.trackById(albums[i]['artworkTrack']),
                            radius: 16,
                          ),
                        ),
                        const SizedBox(height: 6),
                        Text(
                          '${albums[i]['title']}',
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: const TextStyle(fontWeight: FontWeight.w600),
                        ),
                        Text(
                          '${albums[i]['artist']}',
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: Theme.of(context).textTheme.bodySmall,
                        ),
                      ],
                    ),
                  ),
                ),
              ),
          ],
          if ((_libraryScope == 'Top' || _libraryScope == 'Artists') && artists.isNotEmpty) ...[
            SliverToBoxAdapter(
              child: Padding(
                padding: const EdgeInsets.fromLTRB(24, 16, 24, 4),
                child: Row(
                  children: [
                    Text(
                      'Artists',
                      style: Theme.of(context).textTheme.titleMedium?.copyWith(
                        fontWeight: FontWeight.bold,
                      ),
                    ),
                    const Spacer(),
                    if (_libraryScope == 'Top' && artists.length > 6)
                      TextButton(
                        onPressed: () => setState(() => _libraryScope = 'Artists'),
                        child: const Text('See all'),
                      ),
                  ],
                ),
              ),
            ),
            SliverList.builder(
              itemCount: _libraryScope == 'Top' ? artists.length.clamp(0, 6) : artists.length,
              itemBuilder: (context, i) => ListTile(
                contentPadding: const EdgeInsets.symmetric(horizontal: 24),
                leading: Artwork(
                  model: widget.model,
                  track: widget.model.trackById(artists[i]['artworkTrack']),
                  size: 48,
                  radius: 24,
                ),
                title: Text('${artists[i]['name']}'),
                subtitle: Text('${artists[i]['songs']} songs'),
                trailing: const Icon(Icons.chevron_right_rounded),
                onTap: () {
                  _saveRecent(query);
                  widget.open('collection', {
                    'kind': 'artist',
                    'key': artists[i]['name'],
                    'title': artists[i]['name'],
                    'artworkTrack': artists[i]['artworkTrack'],
                  });
                },
              ),
            ),
          ],
          if (_libraryScope == 'Top' && trackCount < 5)
            SliverToBoxAdapter(
              child: Padding(
                padding: const EdgeInsets.fromLTRB(24, 20, 24, 24),
                child: InkWell(
                  borderRadius: BorderRadius.circular(20),
                  onTap: () => _switchPlace('Online'),
                  child: Glass(
                    radius: 20,
                    padding: const EdgeInsets.all(16),
                    child: Row(
                      children: [
                        const CircleAvatar(
                          backgroundColor: Color(0xFF6D4C99),
                          child: Icon(Icons.public_rounded, color: Colors.white),
                        ),
                        const SizedBox(width: 16),
                        Expanded(
                          child: Column(
                            crossAxisAlignment: CrossAxisAlignment.start,
                            children: [
                              Text(
                                'Search online for “$query”',
                                style: const TextStyle(fontWeight: FontWeight.bold),
                              ),
                              const Text(
                                'Find releases, songs and downloads online',
                                style: TextStyle(fontSize: 12, color: Colors.white70),
                              ),
                            ],
                          ),
                        ),
                        const Icon(Icons.arrow_forward_rounded, size: 18),
                      ],
                    ),
                  ),
                ),
              ),
            ),
          const SliverToBoxAdapter(child: SizedBox(height: 180)),
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

    final songs = rows(model.state['onlineSongs']);
    final albums = rows(model.state['onlineAlbums']);
    final artists = rows(model.state['onlineArtists']);
    final collection = row(model.state['onlineCollection']);

    return ListView(
      padding: const EdgeInsets.fromLTRB(24, 8, 24, 180),
      children: [
        Row(
          children: [
            SegmentedButton<String>(
              segments: const [
                ButtonSegment(value: 'Songs', label: Text('Songs')),
                ButtonSegment(value: 'Albums', label: Text('Albums')),
                ButtonSegment(value: 'Artists', label: Text('Artists')),
              ],
              selected: {_onlineType},
              onSelectionChanged: (set) {
                setState(() => _onlineType = set.first);
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
        if (model.state['downloadStatus'] != null)
          Padding(
            padding: const EdgeInsets.symmetric(vertical: 8),
            child: Card.filled(
              margin: EdgeInsets.zero,
              child: ListTile(
                leading: const Icon(Icons.download_rounded),
                title: Text('${model.state['downloadStatus']}'),
                subtitle: Text(
                  '${(integer(model.state['downloadedBytes']) / 1048576).toStringAsFixed(1)} MB',
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
        if (collection.isNotEmpty)
          ListTile(
            title: Text(
              '${collection['title'] ?? collection['name'] ?? 'Shared music'}',
            ),
            trailing: TextButton(
              onPressed: () => model.call('importPlaylist', {
                'name': collection['name'] ?? collection['title'] ?? 'Imported playlist',
              }),
              child: const Text('Import matches'),
            ),
          ),
        if (query.isEmpty) ...[
          const SizedBox(height: 20),
          const EmptyState(
            icon: Icons.travel_explore_rounded,
            title: 'Explore the musical universe',
            detail: 'Search for any song, album or artist to preview and download to your library.',
          ),
        ] else if (_onlineType == 'Songs') ...[
          ...songs.map(
            (song) => ListTile(
              contentPadding: EdgeInsets.zero,
              leading: Icon(
                song['held'] == true ? Icons.library_music_rounded : Icons.music_note_rounded,
                color: song['held'] == true ? Theme.of(context).colorScheme.primary : null,
              ),
              title: Text('${song['title']}'),
              subtitle: Text('${song['artist']} · ${song['album']}'),
              trailing: PopupMenuButton<String>(
                onSelected: (v) => v == 'save'
                    ? model.call('saveWanted', {'recording': song['recording']})
                    : chooseDownload(context, model, {'recording': song['recording']}),
                itemBuilder: (_) => [
                  const PopupMenuItem(
                    value: 'save',
                    child: Text('Save for later'),
                  ),
                  const PopupMenuItem(
                    value: 'download',
                    child: Text('Download'),
                  ),
                ],
              ),
            ),
          ),
        ] else if (_onlineType == 'Albums') ...[
          ...albums.map(
            (album) => ListTile(
              contentPadding: EdgeInsets.zero,
              leading: const Icon(Icons.album_outlined),
              title: Text('${album['title']}'),
              subtitle: Text('${album['artist']}'),
              trailing: const Icon(Icons.chevron_right_rounded),
              onTap: () => model.call('onlineReleases', {'id': album['id']}),
            ),
          ),
        ] else if (_onlineType == 'Artists') ...[
          ...artists.map(
            (artist) => ListTile(
              contentPadding: EdgeInsets.zero,
              leading: const Icon(Icons.person_outline_rounded),
              title: Text('${artist['name']}'),
              trailing: const Icon(Icons.chevron_right_rounded),
              onTap: () {
                setState(() => _onlineType = 'Albums');
                model.call('artistAlbums', {'id': artist['id']});
              },
            ),
          ),
        ],
        const SizedBox(height: 180),
      ],
    );
  }

  Future<void> _openMusicLink(BuildContext context) async {
    final controller = TextEditingController();
    final link = await showDialog<String>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('Open music link'),
        content: TextField(
          controller: controller,
          decoration: const InputDecoration(labelText: 'URL from TIDAL, Spotify or MusicBrainz'),
        ),
        actions: [
          TextButton(onPressed: () => Navigator.pop(context), child: const Text('Cancel')),
          TextButton(
            onPressed: () => Navigator.pop(context, controller.text.trim()),
            child: const Text('Open'),
          ),
        ],
      ),
    );
    controller.dispose();
    if (link != null && link.isNotEmpty) {
      setState(() => _place = 'Online');
      widget.model.call('openLink', {'text': link});
    }
  }

  Future<void> _openListening(BuildContext context) async {
    await showModalBottomSheet<void>(
      context: context,
      showDragHandle: true,
      builder: (sheet) => SafeArea(
        child: Padding(
          padding: const EdgeInsets.all(24),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            children: [
              const Icon(Icons.hearing_rounded, size: 48),
              const SizedBox(height: 12),
              Text(
                'Music Recognition',
                style: Theme.of(context).textTheme.headlineSmall?.copyWith(fontWeight: FontWeight.bold),
              ),
              const SizedBox(height: 8),
              const Text(
                'Resonate uses AcoustID and audio fingerprinting to recognize tracks in your library or from audio playback.',
                textAlign: TextAlign.center,
              ),
              const SizedBox(height: 20),
              FilledButton(
                onPressed: () => Navigator.pop(sheet),
                child: const Text('Done'),
              ),
            ],
          ),
        ),
      ),
    );
  }
}
