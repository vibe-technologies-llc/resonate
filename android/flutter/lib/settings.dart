import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import 'library.dart';
import 'main.dart' show AppBottomBar, OpenPage;
import 'model.dart';
import 'player.dart';
import 'tools.dart' show WantedPane;
import 'visuals.dart';

const _settings =
    <({String group, String title, String detail, String page, IconData icon})>[
      (
        group: 'Sound & playback',
        title: 'Equaliser',
        detail: 'Shape your sound',
        page: 'equaliser',
        icon: Icons.equalizer_rounded,
      ),
      (
        group: 'Sound & playback',
        title: 'Sound processing',
        detail: 'ReplayGain, resampling and true peak',
        page: 'processing',
        icon: Icons.graphic_eq_rounded,
      ),
      (
        group: 'Sound & playback',
        title: 'Headphone correction',
        detail: 'AutoEq profiles for your headphones',
        page: 'correction',
        icon: Icons.headphones_rounded,
      ),
      (
        group: 'Sound & playback',
        title: 'Audio output',
        detail: 'Choose where your music plays',
        page: 'output',
        icon: Icons.speaker_outlined,
      ),
      (
        group: 'Sound & playback',
        title: 'Sleep timer',
        detail: 'Wind down with your music',
        page: 'sleep',
        icon: Icons.bedtime_outlined,
      ),
      (
        group: 'Your library',
        title: 'Music folders',
        detail: 'Sources and folder access',
        page: 'folders',
        icon: Icons.folder_open_rounded,
      ),
      (
        group: 'Connected services',
        title: 'Download providers',
        detail: 'Monochrome, hifi-api, Subsonic and TIDAL',
        page: 'providers',
        icon: Icons.cloud_download_outlined,
      ),
      (
        group: 'Connected services',
        title: 'Scrobbling',
        detail: 'ListenBrainz and Last.fm',
        page: 'scrobbling',
        icon: Icons.history_rounded,
      ),
      (
        group: 'Connected services',
        title: 'Device sync',
        detail: 'Your music, across your devices',
        page: 'sync',
        icon: Icons.devices_rounded,
      ),
    ];

class SettingsPage extends StatefulWidget {
  const SettingsPage({super.key, required this.model, required this.open});
  final PlayerModel model;
  final OpenPage open;
  @override
  State<SettingsPage> createState() => _SettingsPageState();
}

class _SettingsPageState extends State<SettingsPage> with RestorationMixin {
  final _query = RestorableTextEditingController();
  @override
  String get restorationId => 'settings';
  @override
  void restoreState(RestorationBucket? oldBucket, bool initialRestore) {
    registerForRestoration(_query, 'query');
  }

  @override
  void dispose() {
    _query.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final query = _query.value.text.toLowerCase();
    final showOnline = query.isEmpty || 'online features'.contains(query);
    final showArtwork =
        widget.model.online &&
        (query.isEmpty ||
            'artwork and details automatically portraits wi-fi missing covers lyrics'
                .contains(query));
    final entries = _settings
        .where((e) => '${e.title} ${e.detail}'.toLowerCase().contains(query))
        .toList();
    return ListView(
      padding: pageInsets(context),
      children: [
        Text('Settings', style: Theme.of(context).textTheme.headlineLarge),
        const SizedBox(height: 20),
        TextField(
          controller: _query.value,
          onChanged: (_) => setState(() {}),
          decoration: const InputDecoration(
            hintText: 'Search settings',
            prefixIcon: Icon(Icons.search_rounded),
          ),
        ),
        if (showOnline)
          Padding(
            padding: const EdgeInsets.only(top: 18),
            child: Card.filled(
              margin: EdgeInsets.zero,
              child: SwitchListTile(
                title: const Text('Online features'),
                subtitle: Text(
                  widget.model.online
                      ? 'Artwork, lyrics and music search are enabled'
                      : 'Artwork, lyrics and music search are offline',
                ),
                value: widget.model.online,
                onChanged: (value) =>
                    widget.model.call('online', {'enabled': value}),
              ),
            ),
          ),
        for (final group in [
          'Sound & playback',
          'Your library',
          'Connected services',
        ])
          if (entries.any((e) => e.group == group)) ...[
            SectionTitle(group),
            Card.filled(
              margin: EdgeInsets.zero,
              clipBehavior: Clip.antiAlias,
              child: Column(
                children: entries
                    .where((e) => e.group == group)
                    .map(
                      (e) => ListTile(
                        leading: Icon(
                          e.icon,
                          color: Theme.of(context).colorScheme.primary,
                        ),
                        title: Text(e.title),
                        subtitle: Text(e.detail),
                        trailing: const Icon(Icons.chevron_right_rounded),
                        onTap: () => e.page == 'sleep'
                            ? sleepTimer(context, widget.model)
                            : widget.open(e.page),
                      ),
                    )
                    .toList(),
              ),
            ),
          ],
        if (showArtwork) ArtworkDetailsSection(model: widget.model),
        if (entries.isEmpty && !showArtwork && !showOnline)
          const EmptyState(
            icon: Icons.search_off_rounded,
            title: 'No settings found',
            detail: 'Try sound, folders, or a service name.',
          ),
      ],
    );
  }
}

class ArtworkDetailsSection extends StatefulWidget {
  const ArtworkDetailsSection({super.key, required this.model});
  final PlayerModel model;

  @override
  State<ArtworkDetailsSection> createState() => _ArtworkDetailsSectionState();
}

class _ArtworkDetailsSectionState extends State<ArtworkDetailsSection> {
  @override
  void initState() {
    super.initState();
    widget.model.call('enrichmentStatus');
  }

  @override
  Widget build(BuildContext context) => ListenableBuilder(
    listenable: widget.model,
    builder: (context, _) {
      final model = widget.model;
      final automatic = model.state['enrichmentAutomatic'] != false;
      final status = row(model.state['enrichmentStatus']);
      final running = status['running'] == true;
      final due = integer(status['due']);
      final asked = integer(status['asked']);
      final covers = model.state['coversAskedAgain'];
      return Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          const SectionTitle('Artwork and details'),
          Card.filled(
            child: Column(
              children: [
                SwitchListTile(
                  title: const Text('Fetch artwork and details automatically'),
                  subtitle: const Text(
                    'Covers, artist portraits, lyrics and MusicBrainz details',
                  ),
                  value: automatic,
                  onChanged: model.online
                      ? (enabled) => model.call('enrichmentAutomatic', {
                          'enabled': enabled,
                        })
                      : null,
                ),
                SwitchListTile(
                  title: const Text('Only on Wi-Fi'),
                  subtitle: const Text(
                    'Wait for a network that is not metered',
                  ),
                  value: model.state['enrichmentUnmeteredOnly'] == true,
                  onChanged: automatic && model.online
                      ? (enabled) => model.call('enrichmentUnmeteredOnly', {
                          'enabled': enabled,
                        })
                      : null,
                ),
                ListTile(
                  leading: const Icon(Icons.image_search_rounded),
                  title: const Text('Look again for missing covers'),
                  subtitle: Text(
                    covers == null
                        ? 'Ask again for albums still without a cover'
                        : 'Asking again for $covers ${integer(covers) == 1 ? 'album' : 'albums'}',
                  ),
                  enabled: model.online,
                  onTap: model.online
                      ? () => model.call('retryMissingCovers')
                      : null,
                ),
                ListTile(
                  leading: running
                      ? const SizedBox(
                          width: 20,
                          height: 20,
                          child: CircularProgressIndicator(strokeWidth: 2),
                        )
                      : Icon(
                          status['stopped'] == 'FINISHED'
                              ? Icons.check_circle_outline_rounded
                              : Icons.info_outline_rounded,
                        ),
                  title: Text(
                    enrichmentLine(status),
                    style: Theme.of(context).textTheme.bodyMedium,
                  ),
                  trailing: running
                      ? TextButton(
                          onPressed: () => model.call('stopEnrichment'),
                          child: const Text('Stop'),
                        )
                      : null,
                ),
                if (running && due > 0)
                  Padding(
                    padding: const EdgeInsets.fromLTRB(16, 0, 16, 16),
                    child: LinearProgressIndicator(
                      value: (asked / due).clamp(0, 1),
                      semanticsLabel:
                          'Artwork lookup progress: $asked of $due songs',
                    ),
                  ),
              ],
            ),
          ),
        ],
      );
    },
  );
}

String enrichmentLine(RowData status) {
  if (status['running'] == true) {
    final due = integer(status['due']);
    return due > 0
        ? 'Finding artwork and details · ${integer(status['asked'])} of $due'
        : 'Finding artwork and details';
  }
  return switch (status['stopped']) {
    'FINISHED' => 'Artwork and details up to date',
    'CANCELLED' => 'Stopped',
    'OFFLINE' => 'Stopped: offline',
    'UNREACHABLE' => 'Stopped: MusicBrainz could not be reached',
    'REFUSED' => 'Stopped: the service refused too many requests',
    'FAILED' => 'Stopped: the library could not be read',
    _ => 'Not looked up yet',
  };
}

Future<void> refreshArtworkAndDetails(
  BuildContext context,
  PlayerModel model,
) async {
  final count = model.allTracks.length;
  final confirmed = await showDialog<bool>(
    context: context,
    builder: (context) => AlertDialog(
      title: const Text('Refresh artwork and details?'),
      content: Text(
        'Covers, artist portraits, details and lyrics will be looked up again for $count ${count == 1 ? 'song' : 'songs'}. This runs in the background and can take a while.',
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.pop(context, false),
          child: const Text('Cancel'),
        ),
        TextButton(
          onPressed: () => Navigator.pop(context, true),
          child: const Text('Refresh'),
        ),
      ],
    ),
  );
  if (confirmed == true && model.online) await model.call('refreshEnrichment');
}

class SettingsDetail extends StatefulWidget {
  const SettingsDetail({
    super.key,
    required this.model,
    required this.page,
    required this.open,
  });
  final PlayerModel model;
  final String page;
  final OpenPage open;
  @override
  State<SettingsDetail> createState() => _SettingsDetailState();
}

class _SettingsDetailState extends State<SettingsDetail> {
  @override
  void initState() {
    super.initState();
    final call = switch (widget.page) {
      'folders' => 'refreshFolders',
      'output' => 'outputs',
      'statistics' || 'suggestions' => 'statistics',
      'wanted' => 'loadWanted',
      _ => null,
    };
    if (call != null) widget.model.call(call);
  }

  String get title =>
      _settings.where((e) => e.page == widget.page).firstOrNull?.title ??
      switch (widget.page) {
        'tools' => 'Your library',
        'statistics' => 'Listening statistics',
        'suggestions' => 'Made for you',
        'wanted' => 'Saved for later',
        _ => widget.page,
      };
  @override
  Widget build(BuildContext context) => ListenableBuilder(
    listenable: widget.model,
    builder: (context, _) {
      final topInset = MediaQuery.paddingOf(context).top + kToolbarHeight;
      return ArtworkAtmosphere(
        model: widget.model,
        track: widget.model.current,
        subdued: true,
        child: Scaffold(
          backgroundColor: Colors.transparent,
          extendBody: true,
          extendBodyBehindAppBar: true,
          appBar: AppBar(
            backgroundColor: Colors.transparent,
            scrolledUnderElevation: 0,
            flexibleSpace: const EdgeTint(top: true, child: SizedBox.expand()),
            title: Text(title, maxLines: 1, overflow: TextOverflow.ellipsis),
          ),
          body: switch (widget.page) {
            'equaliser' => EqualiserPane(
              model: widget.model,
              topInset: topInset,
            ),
            'processing' => _processing(),
            'folders' => _folders(),
            'output' => _output(),
            'tools' => _tools(),
            'statistics' => _statistics(),
            'suggestions' => _suggestions(),
            'providers' => AccountPane(
              model: widget.model,
              kind: 'provider',
              topInset: topInset,
            ),
            'scrobbling' => AccountPane(
              model: widget.model,
              kind: 'scrobbling',
              topInset: topInset,
            ),
            'sync' => AccountPane(
              model: widget.model,
              kind: 'sync',
              topInset: topInset,
            ),
            'correction' => CorrectionPane(
              model: widget.model,
              topInset: topInset,
            ),
            'wanted' => Padding(
              padding: EdgeInsets.only(top: topInset),
              child: WantedPane(model: widget.model),
            ),
            _ => const SizedBox.shrink(),
          },
          bottomNavigationBar: AppBottomBar(
            model: widget.model,
            open: widget.open,
          ),
        ),
      );
    },
  );

  Widget _list(List<Widget> children) => Builder(
    builder: (context) => ListView(
      padding: pageInsets(
        context,
        top: MediaQuery.paddingOf(context).top + AppGeometry.pageInset,
      ),
      children: children,
    ),
  );

  Widget _processing() {
    final model = widget.model;
    final prefs = model.preferences;
    if (prefs.isEmpty) return const Center(child: CircularProgressIndicator());
    return _list([
      const Text(
        'Keep volume consistent and choose how your music is processed.',
      ),
      const SizedBox(height: 24),
      _choice('ReplayGain', 'replayGain', ['OFF', 'TRACK', 'ALBUM']),
      _choice('Resampling quality', 'quality', [
        'FAST',
        'BALANCED',
        'HIGH',
        'VERY_HIGH',
      ]),
      _choice('Resampling phase', 'phase', [
        'LINEAR',
        'INTERMEDIATE',
        'MINIMUM',
      ]),
      _choice('Lossy restoration', 'restoration', ['OFF', 'REPAIR', 'EXTEND']),
      for (final e in {
        'truePeak': 'True-peak clip prevention',
        'dsdLikePcm': 'Play DSD as PCM',
      }.entries)
        SwitchListTile(
          contentPadding: EdgeInsets.zero,
          title: Text(e.value),
          value: prefs[e.key] == true,
          onChanged: (value) => model.call('preferences', {e.key: value}),
        ),
      const SectionTitle('Playback buffer'),
      Text('${prefs['bufferMs']} ms'),
      CommitSlider(
        value: decimal(prefs['bufferMs']),
        min: 50,
        max: 2000,
        label: 'Playback buffer',
        onCommit: (value) =>
            model.call('preferences', {'bufferMs': value.round()}),
      ),
    ]);
  }

  Widget _choice(String label, String key, List<String> choices) => Padding(
    padding: const EdgeInsets.only(bottom: 18),
    child: DropdownButtonFormField<String>(
      key: ValueKey('$key:${widget.model.preferences[key]}'),
      initialValue: widget.model.preferences[key] as String?,
      decoration: InputDecoration(labelText: label),
      items: choices
          .map((v) => DropdownMenuItem(value: v, child: Text(humanize(v))))
          .toList(),
      onChanged: (v) => widget.model.call('preferences', {key: v}),
    ),
  );

  Widget _folders() => _list([
    const Text(
      'Add music folders or use Downloads. Android asks for music access to read existing songs in Downloads.',
    ),
    const SizedBox(height: 20),
    ...rows(widget.model.state['folders']).map(
      (folder) => Card.filled(
        child: ListTile(
          leading: const Icon(Icons.folder_outlined),
          title: Text('${folder['name']}'),
          subtitle: Text(
            folder['uri'] == 'content://media/external_primary/downloads'
                ? 'Tap to allow music access and scan existing songs'
                : folder['readable'] == true
                ? (folder['writable'] == true
                      ? 'Read and write access'
                      : 'Read access')
                : 'Access unavailable',
          ),
          onTap: folder['uri'] == 'content://media/external_primary/downloads'
              ? () => widget.model.call('useDownloads')
              : null,
          trailing: ActionIcon(
            icon: Icons.link_off_rounded,
            label: 'Remove access',
            onPressed: () async {
              final confirmed = await showDialog<bool>(
                context: context,
                builder: (context) => AlertDialog(
                  title: const Text('Remove folder access?'),
                  content: const Text(
                    'Your files and saved library entries will be kept.',
                  ),
                  actions: [
                    TextButton(
                      onPressed: () => Navigator.pop(context, false),
                      child: const Text('Cancel'),
                    ),
                    TextButton(
                      onPressed: () => Navigator.pop(context, true),
                      child: const Text('Remove access'),
                    ),
                  ],
                ),
              );
              if (confirmed == true) {
                widget.model.call('removeFolder', {'uri': folder['uri']});
              }
            },
          ),
        ),
      ),
    ),
    const SizedBox(height: 16),
    const Text(musicFolderGuidance),
    const SizedBox(height: 16),
    FilledButton.icon(
      onPressed: () => widget.model.call('useDownloads'),
      icon: const Icon(Icons.download_rounded),
      label: const Text('Use Downloads'),
    ),
    const SizedBox(height: 12),
    FilledButton.icon(
      onPressed: () => widget.model.call('chooseFolder'),
      icon: const Icon(Icons.add_rounded),
      label: const Text('Add music folder'),
    ),
    TextButton(
      onPressed: () => widget.model.call('rescan'),
      child: const Text('Rescan folders'),
    ),
    TextButton(
      onPressed: () => widget.model.call('notifications'),
      child: const Text('Allow playback notifications'),
    ),
  ]);

  Widget _output() => _list([
    const Text(
      'Android manages the physical audio route. Resonate outputs shared stereo float at 48 kHz.',
    ),
    const SizedBox(height: 16),
    Text(
      'Requested route: ${widget.model.state['selectedOutput'] ?? 0} · Opened route: ${widget.model.state['actualOutput'] ?? 0}',
    ),
    const SizedBox(height: 16),
    for (final output in [
      {'id': 0, 'name': 'System default'},
      ...rows(widget.model.state['outputs']),
    ])
      ListTile(
        leading: const Icon(Icons.speaker_outlined),
        title: Text('${output['name']}'),
        trailing:
            integer(output['id']) ==
                integer(widget.model.state['selectedOutput'])
            ? const Icon(Icons.check_rounded)
            : null,
        onTap: () => widget.model.call('output', {'id': output['id']}),
      ),
  ]);

  Widget _tools() => _list([
    const SectionTitle('A deeper listen'),
    for (final entry in <({String label, String page, IconData icon})>[
      (
        label: 'Suggestions',
        page: 'suggestions',
        icon: Icons.auto_awesome_rounded,
      ),
      (label: 'Statistics', page: 'statistics', icon: Icons.bar_chart_rounded),
      (
        label: 'Search online',
        page: 'online',
        icon: Icons.travel_explore_rounded,
      ),
      (
        label: 'Saved for later',
        page: 'wanted',
        icon: Icons.bookmark_border_rounded,
      ),
      (
        label: 'Music folders',
        page: 'folders',
        icon: Icons.folder_open_rounded,
      ),
      (label: 'Lyrics', page: 'lyrics', icon: Icons.lyrics_outlined),
      (label: 'Analysis', page: 'analysis', icon: Icons.insights_rounded),
      (label: 'Visualizer', page: 'visualizer', icon: Icons.waves_rounded),
    ])
      Card.filled(
        child: ListTile(
          leading: Icon(entry.icon),
          title: Text(entry.label),
          trailing: const Icon(Icons.chevron_right_rounded),
          onTap: () => widget.open(entry.page),
        ),
      ),
    const SectionTitle('Library care'),
    OutlinedButton(
      onPressed: () => widget.model.call('rescan'),
      child: const Text('Rescan folders'),
    ),
    TextButton(
      onPressed: !widget.model.online || widget.model.allTracks.isEmpty
          ? null
          : () => refreshArtworkAndDetails(context, widget.model),
      child: const Text('Refresh artwork and details'),
    ),
  ]);

  Widget _statistics() {
    final stats = row(widget.model.state['statistics']);
    return _list([
      Wrap(
        spacing: 8,
        children: {'All time': 0, 'Week': 7, 'Month': 30, 'Year': 365}.entries
            .map(
              (e) => ActionChip(
                label: Text(e.key),
                onPressed: () =>
                    widget.model.call('statistics', {'days': e.value}),
              ),
            )
            .toList(),
      ),
      const SizedBox(height: 24),
      Wrap(
        spacing: 12,
        runSpacing: 12,
        children:
            {
                  'Songs': stats['songs'],
                  'Albums': stats['albums'],
                  'Artists': stats['artists'],
                  'Listens': stats['plays'],
                  'Hours heard': integer(stats['listenedMs']) ~/ 3600000,
                }.entries
                .map(
                  (e) => SizedBox(
                    width: 145,
                    child: Card.filled(
                      child: Padding(
                        padding: const EdgeInsets.all(20),
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            Text(
                              '${e.value ?? 0}',
                              style: Theme.of(context).textTheme.headlineMedium
                                  ?.copyWith(fontWeight: FontWeight.bold),
                            ),
                            Text(e.key),
                          ],
                        ),
                      ),
                    ),
                  ),
                )
                .toList(),
      ),
      const SectionTitle('Your most played'),
      ...rows(stats['topTracks']).map(
        (track) => SongTile(
          model: widget.model,
          track: track,
          onTap: () => widget.model.play(
            rows(stats['topTracks']),
            rows(stats['topTracks']).indexOf(track),
          ),
          open: widget.open,
        ),
      ),
    ]);
  }

  Widget _suggestions() => _list([
    const Text('Mixes from your music and listening history.'),
    for (final entry in <({String title, String subtitle, RowData filter})>[
      (
        title: 'Never heard',
        subtitle: 'There is always something new to find.',
        filter: {'unplayed': true},
      ),
      (
        title: 'Your favourites',
        subtitle: 'The ones you come back to.',
        filter: {'favourites': true},
      ),
      (
        title: 'Most played',
        subtitle: 'The soundtrack to your days.',
        filter: {'order': 'PLAYS', 'descending': true},
      ),
      (
        title: 'Recently added',
        subtitle: 'Fresh arrivals in your library.',
        filter: {'order': 'ADDED', 'descending': true},
      ),
      (
        title: 'High resolution',
        subtitle: 'A little more detail.',
        filter: {'minimumRate': 88200},
      ),
    ])
      Padding(
        padding: const EdgeInsets.only(top: 16),
        child: Card.filled(
          child: ListTile(
            contentPadding: const EdgeInsets.all(20),
            leading: Icon(
              Icons.auto_awesome_rounded,
              color: Theme.of(context).colorScheme.primary,
            ),
            title: Text(entry.title),
            subtitle: Text(entry.subtitle),
            onTap: () async {
              await widget.model.call('filter', entry.filter);
              if (!mounted) return;
              Navigator.of(context).push(
                MaterialPageRoute<void>(
                  builder: (_) => Scaffold(
                    appBar: AppBar(title: Text(entry.title)),
                    body: ListenableBuilder(
                      listenable: widget.model,
                      builder: (_, _) =>
                          SongsPage(model: widget.model, open: widget.open),
                    ),
                  ),
                ),
              );
            },
          ),
        ),
      ),
  ]);
}

class EqualiserPane extends StatefulWidget {
  const EqualiserPane({super.key, required this.model, this.topInset = 0});
  final PlayerModel model;
  final double topInset;
  @override
  State<EqualiserPane> createState() => _EqualiserPaneState();
}

class _EqualiserPaneState extends State<EqualiserPane> {
  List<double>? _gains;
  RowData _limits = {};
  List<double> _frequencies = [];
  List<double> _response = [];
  String _revision = '';
  final _name = TextEditingController();
  final _text = TextEditingController();
  bool _import = false;
  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    final answer = row(await widget.model.call('equaliserInfo'));
    if (!mounted) return;
    setState(() {
      _limits = row(answer['limits']);
      _frequencies = (answer['frequencies'] as List? ?? [])
          .map(decimal)
          .toList();
      _gains = (answer['gains'] as List?)?.map(decimal).toList();
      _response = (answer['response'] as List? ?? [])
          .map((v) => (decimal(v) + 30) / 60)
          .toList();
    });
  }

  @override
  void dispose() {
    _name.dispose();
    _text.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final model = widget.model;
    final prefs = model.preferences;
    final curve = row(prefs['equaliser']);
    final revision = curve.toString();
    if (revision != _revision) {
      _revision = revision;
      unawaited(_load());
    }
    final preampLimit = decimal(_limits['preampAtMostDb']);
    final gainLimit = decimal(_limits['gainAtMostDb']);
    return ListView(
      padding: pageInsets(
        context,
        top: widget.topInset + AppGeometry.pageInset,
      ),
      children: [
        SwitchListTile(
          contentPadding: EdgeInsets.zero,
          title: const Text('Enable equaliser'),
          value: prefs['equaliserEnabled'] == true,
          onChanged: (v) => model.call('preferences', {'equaliserEnabled': v}),
        ),
        const Text('This curve applies to every Android audio route.'),
        if ('${curve['name'] ?? ''}'.isNotEmpty)
          SectionTitle('${curve['name']}'),
        const SizedBox(height: 20),
        Card.filled(
          child: Padding(
            padding: const EdgeInsets.all(18),
            child: Column(
              children: [
                SignalPlot(values: _response, height: 150),
                const SizedBox(height: 12),
                const Row(
                  mainAxisAlignment: MainAxisAlignment.spaceBetween,
                  children: [Text('20 Hz'), Text('1 kHz'), Text('20 kHz')],
                ),
              ],
            ),
          ),
        ),
        if (_gains == null && rows(curve['bands']).isNotEmpty)
          const Padding(
            padding: EdgeInsets.symmetric(vertical: 12),
            child: Text(
              'Your parametric profile is playing. Moving a band slider replaces it with ten octave bands.',
            ),
          ),
        const SectionTitle('Preamp'),
        if (preampLimit > 0)
          CommitSlider(
            value: decimal(curve['preampDb']),
            min: -preampLimit,
            max: preampLimit,
            label: 'Preamp',
            format: (v) => '${v.toStringAsFixed(1)} dB',
            onCommit: (v) => model.call('equaliser', {'preamp': v}),
          ),
        for (var i = 0; i < _frequencies.length && gainLimit > 0; i++) ...[
          Text(
            _frequencies[i] < 1000
                ? '${_frequencies[i]} Hz'
                : '${_frequencies[i] / 1000} kHz',
          ),
          CommitSlider(
            value: _gains?[i] ?? 0,
            min: -gainLimit,
            max: gainLimit,
            label: 'Band ${i + 1}',
            format: (v) => '${v.toStringAsFixed(1)} dB',
            onCommit: (v) {
              final gains =
                  _gains?.toList() ?? List.filled(_frequencies.length, 0.0);
              gains[i] = v;
              model.call('equaliser', {
                'preamp': decimal(curve['preampDb']),
                'gains': gains,
              });
            },
          ),
        ],
        TextButton(
          onPressed: () => model.call('equaliser', {'reset': true}),
          child: const Text('Reset curve'),
        ),
        OutlinedButton(
          onPressed: () => setState(() => _import = !_import),
          child: const Text('Import an equaliser profile'),
        ),
        if (_import) ...[
          const SizedBox(height: 16),
          TextField(
            controller: _name,
            decoration: const InputDecoration(labelText: 'Profile name'),
          ),
          const SizedBox(height: 12),
          TextField(
            controller: _text,
            minLines: 3,
            maxLines: 8,
            decoration: const InputDecoration(
              labelText: 'EqualizerAPO / GraphicEQ profile',
            ),
          ),
          TextButton(
            onPressed: () =>
                model.call('profile', {'name': _name.text, 'text': _text.text}),
            child: const Text('Apply profile'),
          ),
        ],
      ],
    );
  }
}

class CorrectionPane extends StatefulWidget {
  const CorrectionPane({super.key, required this.model, this.topInset = 0});
  final PlayerModel model;
  final double topInset;
  @override
  State<CorrectionPane> createState() => _CorrectionPaneState();
}

class _CorrectionPaneState extends State<CorrectionPane> {
  Timer? _timer;
  @override
  void dispose() {
    _timer?.cancel();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => Column(
    children: [
      Padding(
        padding: EdgeInsets.fromLTRB(24, widget.topInset + 24, 24, 24),
        child: TextField(
          decoration: const InputDecoration(
            labelText: 'AutoEq headphone correction',
            prefixIcon: Icon(Icons.search_rounded),
          ),
          onChanged: (v) {
            _timer?.cancel();
            _timer = Timer(
              const Duration(milliseconds: 350),
              () => widget.model.call('corrections', {'text': v}),
            );
          },
        ),
      ),
      if (widget.model.state['correctionBusy'] == true)
        const LinearProgressIndicator(),
      Expanded(
        child: ListView(
          padding: EdgeInsets.only(
            bottom:
                MediaQuery.paddingOf(context).bottom + AppGeometry.pageInset,
          ),
          children: rows(widget.model.state['corrections']).indexed
              .map(
                (entry) => ListTile(
                  title: Text('${entry.$2['name']}'),
                  subtitle: Text('${entry.$2['source'] ?? ''}'),
                  onTap: () async {
                    final confirm = await showDialog<bool>(
                      context: context,
                      builder: (context) => AlertDialog(
                        title: const Text('Replace equaliser curve?'),
                        content: Text('Apply ${entry.$2['name']}?'),
                        actions: [
                          TextButton(
                            onPressed: () => Navigator.pop(context, false),
                            child: const Text('Cancel'),
                          ),
                          TextButton(
                            onPressed: () => Navigator.pop(context, true),
                            child: const Text('Apply'),
                          ),
                        ],
                      ),
                    );
                    if (confirm == true) {
                      widget.model.call('applyCorrection', {'index': entry.$1});
                    }
                  },
                ),
              )
              .toList(),
        ),
      ),
    ],
  );
}

class AccountPane extends StatefulWidget {
  const AccountPane({
    super.key,
    required this.model,
    required this.kind,
    this.topInset = 0,
  });
  final PlayerModel model;
  final double topInset;
  final String kind;
  @override
  State<AccountPane> createState() => _AccountPaneState();
}

class _AccountPaneState extends State<AccountPane> {
  final Map<String, TextEditingController> _fields = {};
  final RowData _draft = {};
  String? _identity;
  bool _showSyncCode = false;
  String _text(String key, Object? initial) => _fields
      .putIfAbsent(key, () => TextEditingController(text: '$initial'))
      .text;
  Widget _field(
    String key,
    String label, {
    Object? initial = '',
    bool secret = false,
    int lines = 1,
  }) {
    _text(key, initial ?? '');
    return Padding(
      padding: const EdgeInsets.only(bottom: 16),
      child: TextField(
        controller: _fields[key],
        obscureText: secret,
        maxLines: secret ? 1 : lines,
        autocorrect: !secret,
        enableSuggestions: !secret,
        decoration: InputDecoration(labelText: label),
        onChanged: (value) => _draft[key] = value,
      ),
    );
  }

  String _syncStanding(
    Map<String, dynamic> status,
    Map<String, dynamic> config,
    bool online,
  ) {
    if (config['enabled'] != true) return 'Off';
    if (!online && config['nearbyOnly'] != true) {
      return 'Waiting for online features';
    }
    final devices = rows(status['devices']);
    final others = devices.where((d) => d['thisDevice'] != true).toList();
    final nearby = others.where((d) => d['nearby'] == true).length;
    final reached = others.where((d) => d['online'] == true).length;
    final standing = '${status['standing'] ?? ''}'.toLowerCase();
    switch (standing) {
      case 'connected':
        return reached == 0
            ? 'No other device online'
            : '$reached ${reached == 1 ? 'device' : 'devices'} online';
      case 'nearby':
        return nearby == 0
            ? 'Looking on this network'
            : '$nearby ${nearby == 1 ? 'device' : 'devices'} on this network';
      case 'unreachable':
        return nearby == 0
            ? "Can't reach the server"
            : 'Server unreachable · $nearby on this network';
      case 'refused':
        return 'The server refused this device';
      case 'connecting':
        return nearby == 0
            ? 'Connecting…'
            : 'Connecting · $nearby on this network';
      default:
        return 'Starting…';
    }
  }

  String _syncHeadline(
    Map<String, dynamic> status,
    Map<String, dynamic> config,
  ) {
    final standing = '${status['standing'] ?? ''}'.toLowerCase();
    final devices = rows(status['devices']);
    final hasNearby = devices.any(
      (d) => d['nearby'] == true && d['thisDevice'] != true,
    );
    switch (standing) {
      case 'connecting':
        return 'Connecting…';
      case 'refused':
        return 'Refused';
      case 'unreachable':
        return hasNearby ? 'Catching up' : 'Not syncing';
      case 'nearby':
        return hasNearby ? 'Catching up' : 'Looking for devices';
      case 'connected':
        final owed = integer(status['owed']);
        return owed > 0
            ? 'Sending $owed ${owed == 1 ? 'change' : 'changes'}'
            : 'Up to date';
      case 'off':
        return 'Off';
      default:
        return status['standing'] != null
            ? humanize('${status['standing']}')
            : 'Starting…';
    }
  }

  @override
  void dispose() {
    for (final field in _fields.values) {
      field.dispose();
    }
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final model = widget.model;
    final kind = widget.kind;
    final stored = row(
      model.state[kind == 'provider'
          ? 'providerAccount'
          : kind == 'scrobbling'
          ? 'scrobbleAccount'
          : 'syncConfig'],
    );
    if (stored.isEmpty) return const Center(child: CircularProgressIndicator());
    final identity = kind == 'provider'
        ? '${stored['provider']}:${stored['refreshToken']}'
        : kind == 'sync'
        ? '${stored['key']}'
        : 'scrobbling';
    if (_identity != identity) {
      _identity = identity;
      _showSyncCode = false;
      _draft.clear();
      _draft.addAll(stored);
      for (final key in _fields.keys) {
        _fields[key]!.text = '${stored[key] ?? ''}';
      }
    }
    return ListView(
      padding: pageInsets(
        context,
        top: widget.topInset + AppGeometry.pageInset,
      ),
      children: [
        if (kind == 'provider') ...[
          const Text(
            'Downloads are matched by recording codes, or by title, artist and duration. Choose the destination folder for each song.',
          ),
          const SizedBox(height: 24),
          DropdownButtonFormField<String>(
            initialValue: _draft['provider'] as String?,
            decoration: const InputDecoration(labelText: 'Provider'),
            items: ['MONOCHROME', 'HIFI_API', 'SUBSONIC', 'TIDAL']
                .map(
                  (v) => DropdownMenuItem(
                    value: v,
                    child: Text(v == 'HIFI_API' ? 'hifi-api' : humanize(v)),
                  ),
                )
                .toList(),
            onChanged: (v) => setState(() => _draft['provider'] = v),
          ),
          const SizedBox(height: 20),
          if (_draft['provider'] != 'TIDAL')
            _field(
              'server',
              'Server URL (blank uses the hosted service)',
              initial: _draft['server'],
            ),
          if (_draft['provider'] == 'SUBSONIC') ...[
            _field('user', 'Username', initial: stored['user']),
            _field(
              'password',
              'Password',
              initial: stored['password'],
              secret: true,
            ),
          ],
          if (_draft['provider'] == 'TIDAL') ...[
            _field('clientId', 'TIDAL client ID', initial: stored['clientId']),
            _field(
              'clientSecret',
              'Client secret (if required)',
              initial: stored['clientSecret'],
              secret: true,
            ),
            _field(
              'refreshToken',
              'Refresh token',
              initial: stored['refreshToken'],
              secret: true,
            ),
            TextButton(
              onPressed: model.state['tidalBusy'] == true
                  ? null
                  : () => model.call('authorizeTidal', _draft),
              child: const Text('Sign in with TIDAL'),
            ),
            if (row(model.state['tidalAuthorization']).isNotEmpty) ...[
              SelectableText(
                'Enter code: ${row(model.state['tidalAuthorization'])['userCode']}',
              ),
              TextButton(
                onPressed: () => model.call('openUrl', {
                  'url': row(model.state['tidalAuthorization'])['verifyAt'],
                }),
                child: const Text('Open TIDAL authorization'),
              ),
            ],
          ],
          FilledButton(
            onPressed: () => model.call('provider', _draft),
            child: const Text('Save provider'),
          ),
        ],
        if (kind == 'scrobbling') ...[
          const Text(
            'Completed listens are queued on this device and submitted when online features are enabled.',
          ),
          const SizedBox(height: 24),
          _field(
            'listenbrainzToken',
            'ListenBrainz token',
            initial: stored['listenbrainzToken'],
            secret: true,
          ),
          _field(
            'lastfmKey',
            'Last.fm API key',
            initial: stored['lastfmKey'],
            secret: true,
          ),
          _field(
            'lastfmSecret',
            'Last.fm shared secret',
            initial: stored['lastfmSecret'],
            secret: true,
          ),
          _field(
            'lastfmSession',
            'Last.fm session',
            initial: stored['lastfmSession'],
            secret: true,
          ),
          SwitchListTile(
            title: const Text('Include existing history'),
            value: _draft['includeHistory'] == true,
            onChanged: (v) => setState(() => _draft['includeHistory'] = v),
          ),
          FilledButton(
            onPressed: () => model.call('scrobbling', _draft),
            child: const Text('Save scrobbling'),
          ),
          TextButton(
            onPressed: () => model.call('validateScrobbling', _draft),
            child: const Text('Validate accounts'),
          ),
          ExpansionTile(
            title: const Text('Sign in to Last.fm'),
            children: [
              _field('user', 'Last.fm username'),
              _field('password', 'Last.fm password', secret: true),
              TextButton(
                onPressed: () async {
                  await model.call('lastFm', _draft);
                  _fields['password']?.clear();
                  _draft.remove('password');
                },
                child: const Text('Sign in'),
              ),
            ],
          ),
          TextButton(
            onPressed: () => model.call('sendFavourites'),
            child: const Text('Send identified favourites'),
          ),
          if (model.state['scrobbleStatus'] != null)
            Text('${model.state['scrobbleStatus']}'),
        ],
        if (kind == 'sync') ...[
          Card.filled(
            margin: EdgeInsets.zero,
            child: Column(
              children: [
                SwitchListTile(
                  title: const Text('Device sync'),
                  subtitle: Text(
                    _draft['enabled'] == true
                        ? '${_syncHeadline(row(model.state['syncStatus']), _draft)} · ${_syncStanding(row(model.state['syncStatus']), _draft, model.online)}'
                        : 'Playlists, favourites and history follow you across devices.',
                  ),
                  value: _draft['enabled'] == true,
                  onChanged: (v) => setState(() => _draft['enabled'] = v),
                ),
                Padding(
                  padding: const EdgeInsets.fromLTRB(16, 0, 16, 14),
                  child: Row(
                    children: [
                      if (!model.online && _draft['nearbyOnly'] != true)
                        OutlinedButton.icon(
                          onPressed: () {
                            setState(() => _draft['nearbyOnly'] = true);
                            model.call('sync', _draft);
                          },
                          icon: const Icon(Icons.wifi_rounded),
                          label: const Text('Sync only on this network'),
                        )
                      else
                        FilledButton.tonalIcon(
                          onPressed: () => model.forceSync(),
                          icon: const Icon(Icons.sync_rounded),
                          label: const Text('Sync now'),
                        ),
                    ],
                  ),
                ),
              ],
            ),
          ),
          const SizedBox(height: 16),
          Card.outlined(
            margin: EdgeInsets.zero,
            child: Padding(
              padding: const EdgeInsets.all(16),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  SwitchListTile(
                    contentPadding: EdgeInsets.zero,
                    title: const Text('Only this network'),
                    subtitle: const Text(
                      'Devices sync on the same network without a server',
                    ),
                    value: _draft['nearbyOnly'] == true,
                    onChanged: (v) => setState(() => _draft['nearbyOnly'] = v),
                  ),
                  _field('device', 'Device name', initial: stored['device']),
                  if (_draft['nearbyOnly'] == true)
                    const Padding(
                      padding: EdgeInsets.only(bottom: 12),
                      child: Text(
                        'Devices sync on the same network without a server.',
                      ),
                    )
                  else ...[
                    if (!model.online)
                      const Padding(
                        padding: EdgeInsets.only(bottom: 12),
                        child: Text(
                          'Turn on online features in Settings to sync.',
                        ),
                      ),
                    _field('server', 'Sync server', initial: stored['server']),
                  ],
                  FilledButton(
                    onPressed: () => model.call('sync', _draft),
                    child: const Text('Save sync settings'),
                  ),
                ],
              ),
            ),
          ),
          const SectionTitle('Your devices'),
          Card.outlined(
            margin: EdgeInsets.zero,
            child: Column(
              children: [
                Padding(
                  padding: const EdgeInsets.fromLTRB(16, 12, 16, 8),
                  child: Row(
                    children: [
                      TextButton.icon(
                        onPressed: () =>
                            setState(() => _showSyncCode = !_showSyncCode),
                        icon: Icon(
                          _showSyncCode
                              ? Icons.visibility_off_outlined
                              : Icons.visibility_outlined,
                        ),
                        label: Text(
                          _showSyncCode
                              ? 'Hide pairing code'
                              : 'Show pairing code',
                        ),
                      ),
                      const Spacer(),
                      TextButton.icon(
                        onPressed: () => Clipboard.setData(
                          ClipboardData(
                            text:
                                '${row(model.state['syncStatus'])['code'] ?? ''}',
                          ),
                        ),
                        icon: const Icon(Icons.copy_rounded, size: 18),
                        label: const Text('Copy pairing code'),
                      ),
                    ],
                  ),
                ),
                if (_showSyncCode) ...[
                  Padding(
                    padding: const EdgeInsets.symmetric(horizontal: 16),
                    child: Container(
                      width: double.infinity,
                      padding: const EdgeInsets.symmetric(
                        horizontal: 16,
                        vertical: 12,
                      ),
                      decoration: BoxDecoration(
                        color: Theme.of(context)
                            .colorScheme
                            .surfaceContainerHighest
                            .withValues(alpha: .6),
                        borderRadius: BorderRadius.circular(14),
                        border: Border.all(
                          color: Theme.of(context).colorScheme.outlineVariant
                              .withValues(alpha: .5),
                        ),
                      ),
                      child: SelectableText(
                        '${row(model.state['syncStatus'])['code'] ?? ''}',
                        textAlign: TextAlign.center,
                        style: const TextStyle(
                          fontFamily: 'monospace',
                          fontWeight: FontWeight.w700,
                          letterSpacing: 2,
                          fontSize: 16,
                        ),
                      ),
                    ),
                  ),
                  const SizedBox(height: 12),
                ],
                Padding(
                  padding: const EdgeInsets.symmetric(horizontal: 16),
                  child: Column(
                    children: [
                      _field('code', 'Pairing code', lines: 1),
                      Row(
                        children: [
                          Expanded(
                            child: OutlinedButton(
                              onPressed: () => model.call('joinSync', {
                                'code': _fields['code']?.text ?? '',
                              }),
                              child: const Text('Join a group'),
                            ),
                          ),
                          const SizedBox(width: 12),
                          Expanded(
                            child: OutlinedButton(
                              onPressed: () => model.call('newSyncGroup'),
                              child: const Text('Start a new group'),
                            ),
                          ),
                        ],
                      ),
                    ],
                  ),
                ),
                const SizedBox(height: 8),
                const Divider(height: 1),
                ...() {
                  final devList = rows(
                    row(model.state['syncStatus'])['devices'],
                  );
                  final thisDev = devList
                      .where((d) => d['thisDevice'] == true)
                      .toList();
                  final others =
                      devList.where((d) => d['thisDevice'] != true).toList()
                        ..sort((a, b) {
                          final aOn = a['online'] == true ? 1 : 0;
                          final bOn = b['online'] == true ? 1 : 0;
                          if (aOn != bOn) return bOn.compareTo(aOn);
                          return '${a['name']}'.compareTo('${b['name']}');
                        });
                  final allDevices = [...thisDev, ...others];
                  if (allDevices.isEmpty) {
                    return [
                      const Padding(
                        padding: EdgeInsets.all(16),
                        child: Text(
                          'No devices registered yet. Connect your devices using your pairing code.',
                          style: TextStyle(fontSize: 13),
                        ),
                      ),
                    ];
                  }
                  return allDevices.map((device) {
                    final isThis = device['thisDevice'] == true;
                    final isOnline = device['online'] == true;
                    final isNearby = device['nearby'] == true;
                    final platform = '${device['platform'] ?? ''}'
                        .toLowerCase();
                    final icon = isThis
                        ? Icons.phone_android_rounded
                        : platform.contains('iphone') ||
                              platform.contains('ios')
                        ? Icons.phone_iphone_rounded
                        : platform.contains('mac') ||
                              platform.contains('linux') ||
                              platform.contains('windows') ||
                              platform.contains('desktop')
                        ? Icons.computer_rounded
                        : Icons.devices_rounded;
                    final stateLabel = isThis
                        ? 'This device'
                        : isOnline
                        ? 'Online'
                        : isNearby
                        ? 'Nearby'
                        : 'Offline';
                    return ListTile(
                      leading: Icon(
                        icon,
                        color: isOnline || isThis
                            ? Theme.of(context).colorScheme.primary
                            : null,
                      ),
                      title: Text('${device['name']}'),
                      subtitle: Text(stateLabel),
                      trailing: Row(
                        mainAxisSize: MainAxisSize.min,
                        children: [
                          if (!isThis)
                            Container(
                              width: 8,
                              height: 8,
                              decoration: BoxDecoration(
                                shape: BoxShape.circle,
                                color: isOnline
                                    ? Colors.green
                                    : Theme.of(context).colorScheme.outline
                                          .withValues(alpha: .5),
                              ),
                            ),
                          if (!isThis && !isOnline)
                            IconButton(
                              icon: const Icon(
                                Icons.delete_outline_rounded,
                                size: 20,
                              ),
                              tooltip: 'Remove device',
                              onPressed: () =>
                                  model.forgetSyncDevice('${device['id']}'),
                            ),
                        ],
                      ),
                    );
                  }).toList();
                }(),
              ],
            ),
          ),
          const SectionTitle('Sync options'),
          Card.outlined(
            margin: EdgeInsets.zero,
            child: Column(
              children: [
                SwitchListTile(
                  title: const Text('Sync favourites'),
                  subtitle: const Text('Songs favoured on another device'),
                  value: _draft['favourites'] == true,
                  onChanged: (v) => setState(() => _draft['favourites'] = v),
                ),
                SwitchListTile(
                  title: const Text('Sync playlists'),
                  subtitle: const Text(
                    'Playlists set to keep their songs here',
                  ),
                  value: _draft['playlists'] == true,
                  onChanged: (v) => setState(() => _draft['playlists'] = v),
                ),
                SwitchListTile(
                  title: const Text('Keep all music here'),
                  subtitle: const Text('Every song your other devices hold'),
                  value: _draft['everything'] == true,
                  onChanged: (v) => setState(() => _draft['everything'] = v),
                ),
                SwitchListTile(
                  title: const Text('Transfer over mobile data'),
                  subtitle: const Text('Without it, songs move only on Wi-Fi'),
                  value: _draft['meteredAllowed'] == true,
                  onChanged: (v) =>
                      setState(() => _draft['meteredAllowed'] = v),
                ),
              ],
            ),
          ),
          if (rows(row(model.state['syncStatus'])['transfers']).isNotEmpty) ...[
            const SectionTitle('Transfers'),
            Card.outlined(
              margin: EdgeInsets.zero,
              child: Column(
                children: [
                  for (final transfer in rows(
                    row(model.state['syncStatus'])['transfers'],
                  )) ...[
                    () {
                      final artistName = '${transfer['artist'] ?? ''}';
                      final fromDevice = '${transfer['from'] ?? ''}';
                      final size = integer(transfer['size']);
                      final received = integer(transfer['received']);
                      final progress = size > 0
                          ? (received / size).clamp(0.0, 1.0)
                          : null;
                      final subtitleText = [
                        if (artistName.isNotEmpty) artistName,
                        if (fromDevice.isNotEmpty) 'from $fromDevice',
                        if (size > 0)
                          '${(received / 1048576).toStringAsFixed(1)} / ${(size / 1048576).toStringAsFixed(1)} MB',
                      ].join(' · ');
                      return ListTile(
                        leading: const Icon(Icons.download_rounded),
                        title: Text(
                          '${transfer['title'] ?? 'Transferring song'}',
                        ),
                        subtitle: subtitleText.isNotEmpty
                            ? Text(subtitleText)
                            : null,
                        trailing: progress != null
                            ? SizedBox(
                                width: 20,
                                height: 20,
                                child: CircularProgressIndicator(
                                  value: progress,
                                  strokeWidth: 2,
                                ),
                              )
                            : const SizedBox(
                                width: 20,
                                height: 20,
                                child: CircularProgressIndicator(
                                  strokeWidth: 2,
                                ),
                              ),
                      );
                    }(),
                  ],
                ],
              ),
            ),
          ],
          const SectionTitle('Songs received from your devices'),
          Card.outlined(
            margin: EdgeInsets.zero,
            child: Column(
              children: [
                const Padding(
                  padding: EdgeInsets.fromLTRB(16, 16, 16, 8),
                  child: Text(
                    'Songs from your other devices are saved in this folder and added to your library.',
                    style: TextStyle(fontSize: 13),
                  ),
                ),
                ...rows(model.state['folders'])
                    .where(
                      (folder) =>
                          folder['readable'] == true &&
                          folder['writable'] == true,
                    )
                    .map(
                      (folder) => ListTile(
                        leading: const Icon(Icons.folder_outlined),
                        title: Text('${folder['name']}'),
                        trailing: model.state['syncFolder'] == folder['uri']
                            ? const Icon(
                                Icons.check_rounded,
                                color: Colors.green,
                              )
                            : null,
                        onTap: () =>
                            model.call('syncFolder', {'uri': folder['uri']}),
                      ),
                    ),
              ],
            ),
          ),
          if (model.state['syncNotice'] != null)
            Padding(
              padding: const EdgeInsets.only(top: 16),
              child: Card.filled(
                color: Theme.of(context).colorScheme.errorContainer,
                child: Padding(
                  padding: const EdgeInsets.all(16),
                  child: Row(
                    children: [
                      Icon(
                        Icons.warning_rounded,
                        color: Theme.of(context).colorScheme.error,
                      ),
                      const SizedBox(width: 12),
                      Expanded(
                        child: Text(
                          '${model.state['syncNotice']}',
                          style: TextStyle(
                            color: Theme.of(context)
                                .colorScheme
                                .onErrorContainer,
                          ),
                        ),
                      ),
                    ],
                  ),
                ),
              ),
            ),
        ],
        const SizedBox(height: 24),
        const Text(
          'Credentials stay encrypted in Android Keystore.',
          style: TextStyle(fontSize: 12),
        ),
      ],
    );
  }
}
