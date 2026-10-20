import 'dart:async';
import 'dart:math' as math;

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

  Widget _list(List<Widget> children, {double? top}) => Builder(
    builder: (context) => ListView(
      padding: pageInsets(
        context,
        top: top ?? MediaQuery.paddingOf(context).top + AppGeometry.pageInset,
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
    child: DropdownMenu<String>(
      key: ValueKey('$key:${widget.model.preferences[key]}'),
      initialSelection: widget.model.preferences[key] as String?,
      expandedInsets: EdgeInsets.zero,
      selectOnly: true,
      enableSearch: false,
      label: Text(label),
      inputDecorationTheme: Theme.of(context).inputDecorationTheme,
      menuStyle: selectionMenuStyle(context),
      dropdownMenuEntries: choices
          .map((v) => DropdownMenuEntry(value: v, label: _choiceLabel(v)))
          .toList(),
      onSelected: (v) {
        if (v != null) widget.model.call('preferences', {key: v});
      },
    ),
  );

  String _choiceLabel(String value) {
    final label = humanize(value);
    return '${label[0].toUpperCase()}${label.substring(1)}';
  }

  Widget _folders() => _list([
    const Text(
      'Add music folders or use Downloads. Android asks for music access to read existing songs in Downloads.',
    ),
    const SizedBox(height: 20),
    ...rows(widget.model.state['folders']).map(
      (folder) => Padding(
        padding: const EdgeInsets.only(bottom: 12),
        child: Card.filled(
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

  Widget _output() {
    final outputs = <RowData>[
      {'id': 0, 'name': 'System default', 'kind': 'Automatic'},
      ...rows(widget.model.state['outputs']),
    ];
    final selected = integer(widget.model.state['selectedOutput']);
    final actual = integer(widget.model.state['actualOutput']);
    final scheme = Theme.of(context).colorScheme;
    String titleFor(int id) {
      for (final output in outputs) {
        if (integer(output['id']) == id) return _outputTitle(output);
      }
      return 'Unavailable device';
    }

    return _list([
      Card.filled(
        child: Padding(
          padding: const EdgeInsets.all(18),
          child: Row(
            children: [
              Icon(Icons.volume_up_rounded, color: scheme.primary),
              const SizedBox(width: 16),
              Expanded(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      'Current output',
                      style: Theme.of(context).textTheme.labelMedium
                          ?.copyWith(color: scheme.onSurfaceVariant),
                    ),
                    Text(
                      titleFor(actual),
                      style: Theme.of(context).textTheme.titleMedium,
                    ),
                    if (selected != actual)
                      Text(
                        'Requested: ${titleFor(selected)}',
                        style: Theme.of(context).textTheme.bodySmall
                            ?.copyWith(color: scheme.onSurfaceVariant),
                      ),
                  ],
                ),
              ),
            ],
          ),
        ),
      ),
      const SectionTitle('Choose an output'),
      Card.filled(
        child: Column(
          children: [
            for (final output in outputs)
              ListTile(
                dense: true,
                minTileHeight: 64,
                leading: Icon(_outputIcon(output), color: scheme.primary),
                title: Text(_outputTitle(output)),
                subtitle: Text(_outputSubtitle(output)),
                trailing: integer(output['id']) == selected
                    ? Icon(Icons.check_circle_rounded, color: scheme.primary)
                    : null,
                onTap: () => widget.model.call('output', {'id': output['id']}),
              ),
          ],
        ),
      ),
      const SizedBox(height: 20),
      Text(
        'Android controls the physical route. Resonate plays shared stereo audio at 48 kHz.',
        style: Theme.of(context).textTheme.bodySmall
            ?.copyWith(color: scheme.onSurfaceVariant),
      ),
    ], top: MediaQuery.paddingOf(context).top + kToolbarHeight + 20);
  }

  String _outputTitle(RowData output) {
    final kind = '${output['kind'] ?? 'Audio device'}';
    return switch (kind) {
      'Built-in speaker' || 'Earpiece' => kind,
      _ => '${output['name']}',
    };
  }

  String _outputSubtitle(RowData output) {
    final kind = '${output['kind'] ?? 'Audio device'}';
    return switch (kind) {
      'Automatic' => 'Android chooses for you',
      'Built-in speaker' || 'Earpiece' => '${output['name']}',
      _ => kind,
    };
  }

  IconData _outputIcon(RowData output) => switch (output['kind']) {
    'Earpiece' => Icons.hearing_rounded,
    'Wired headphones' || 'Wired headset' => Icons.headphones_rounded,
    'Bluetooth audio' => Icons.bluetooth_audio_rounded,
    'USB audio' => Icons.usb_rounded,
    'HDMI audio' => Icons.tv_rounded,
    _ => Icons.speaker_rounded,
  };

  Widget _tools() => _list([
    const SectionTitle('A deeper listen'),
    Card.filled(
      margin: EdgeInsets.zero,
      child: Column(
        children:
            <({String label, String page, IconData icon})>[
                  (
                    label: 'Suggestions',
                    page: 'suggestions',
                    icon: Icons.auto_awesome_rounded,
                  ),
                  (
                    label: 'Statistics',
                    page: 'statistics',
                    icon: Icons.bar_chart_rounded,
                  ),
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
                  (
                    label: 'Lyrics',
                    page: 'lyrics',
                    icon: Icons.lyrics_outlined,
                  ),
                  (
                    label: 'Analysis',
                    page: 'analysis',
                    icon: Icons.insights_rounded,
                  ),
                  (
                    label: 'Visualizer',
                    page: 'visualizer',
                    icon: Icons.waves_rounded,
                  ),
                ]
                .map(
                  (entry) => ListTile(
                    dense: true,
                    visualDensity: VisualDensity.compact,
                    minTileHeight: 56,
                    minVerticalPadding: 0,
                    contentPadding: const EdgeInsets.symmetric(horizontal: 16),
                    leading: Icon(
                      entry.icon,
                      color: Theme.of(context).colorScheme.primary,
                    ),
                    title: Text(entry.label),
                    trailing: const Icon(Icons.chevron_right_rounded),
                    onTap: () => widget.open(entry.page),
                  ),
                )
                .toList(),
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
  ], top: MediaQuery.paddingOf(context).top + kToolbarHeight - 24);

  Widget _statistics() {
    final stats = row(widget.model.state['statistics']);
    final top = rows(stats['topTracks']);
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
      for (final (index, track) in top.indexed)
        SongTile(
          model: widget.model,
          track: track,
          onTap: () => widget.model.play(top, index),
          open: widget.open,
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
            onTap: () => widget.open('collection', {
              ...entry.filter,
              'kind': 'mix',
              'title': entry.title,
            }),
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

String _eqFilterValue(Object? raw) {
  final upper = '${raw ?? ''}'.toUpperCase();
  return switch (upper) {
    'PEAKING' => 'PEAKING',
    'LOW_SHELF' || 'LOWSHELF' || 'LOW-SHELF' => 'LOW_SHELF',
    'HIGH_SHELF' || 'HIGHSHELF' || 'HIGH-SHELF' => 'HIGH_SHELF',
    'LOW_PASS' || 'LOWPASS' || 'LOW-PASS' => 'LOW_PASS',
    'HIGH_PASS' || 'HIGHPASS' || 'HIGH-PASS' => 'HIGH_PASS',
    'NOTCH' => 'NOTCH',
    'BAND_PASS' || 'BANDPASS' || 'BAND-PASS' => 'BAND_PASS',
    'ALL_PASS' || 'ALLPASS' || 'ALL-PASS' => 'ALL_PASS',
    _ => 'PEAKING',
  };
}

bool _eqUsesGain(String filter) {
  final value = _eqFilterValue(filter);
  return value == 'PEAKING' || value == 'LOW_SHELF' || value == 'HIGH_SHELF';
}

String _eqFilterLabel(String filter) => switch (_eqFilterValue(filter)) {
  'LOW_SHELF' => 'Low shelf',
  'HIGH_SHELF' => 'High shelf',
  'LOW_PASS' => 'Low pass',
  'HIGH_PASS' => 'High pass',
  'NOTCH' => 'Notch',
  'BAND_PASS' => 'Band pass',
  'ALL_PASS' => 'All pass',
  _ => 'Peak',
};

double _eqTidyHertz(double hertz) {
  if (hertz < 100) return (hertz * 10).round() / 10;
  if (hertz < 1000) return hertz.roundToDouble();
  return (hertz / 10).round() * 10;
}

double _eqTidyDb(double decibels) => (decibels * 10).round() / 10;

double _eqTidyQ(double q) => (q * 100).round() / 100;

String _eqFormatHertz(double hertz) {
  if (hertz < 1000) {
    return hertz >= 100
        ? '${hertz.toStringAsFixed(0)} Hz'
        : '${hertz.toStringAsFixed(1)} Hz';
  }
  final khz = hertz / 1000;
  return khz >= 10
      ? '${khz.toStringAsFixed(1)} kHz'
      : '${khz.toStringAsFixed(2)} kHz';
}

List<RowData> _eqBands(RowData curve) => rows(curve['bands']);

double _eqLevel(RowData band, double preamp) {
  if (_eqUsesGain('${band['filter']}')) {
    return decimal(band['gainDb']) + preamp;
  }
  return preamp;
}

String _eqDb(double value) =>
    '${value > 0 ? '+' : ''}${value.toStringAsFixed(1)} dB';

String _eqRange(double hertz) => switch (hertz) {
  < 60 => 'Sub-bass',
  < 180 => 'Bass',
  < 400 => 'Warmth',
  < 900 => 'Low mids',
  < 1800 => 'Mids',
  < 3500 => 'Vocals',
  < 6000 => 'Presence',
  < 12000 => 'Treble',
  _ => 'Air',
};

const _presetIcons = {
  'Flat': Icons.horizontal_rule_rounded,
  'Bass boost': Icons.speaker_rounded,
  'Bass only': Icons.speaker_group_rounded,
  'Bass cut': Icons.waves_rounded,
  'Vocals': Icons.mic_none_rounded,
  'Warm': Icons.wb_sunny_outlined,
  'Bright': Icons.auto_awesome_outlined,
  'Rock': Icons.music_note_rounded,
  'Electronic': Icons.graphic_eq_rounded,
};

class _EqualiserPaneState extends State<EqualiserPane> {
  RowData _limits = {};
  List<double> _frequencies = [];
  List<double> _responseDb = [];
  List<RowData> _presets = [];
  RowData? _draft;
  int? _selected;
  bool _holding = false;
  String _prefsRevision = '';
  final _name = TextEditingController();
  final _text = TextEditingController();
  bool _import = false;
  bool _advanced = false;
  int _loadRequest = 0;
  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    final request = ++_loadRequest;
    final answer = row(await widget.model.call('equaliserInfo'));
    if (!mounted || request != _loadRequest) return;
    setState(() {
      _limits = row(answer['limits']);
      _frequencies = (answer['frequencies'] as List? ?? [])
          .map(decimal)
          .toList();
      _responseDb = (answer['response'] as List? ?? []).map(decimal).toList();
      _presets = rows(answer['presets']);
    });
  }

  @override
  void dispose() {
    _name.dispose();
    _text.dispose();
    super.dispose();
  }

  RowData _displayed(RowData prefsCurve) => _draft ?? prefsCurve;

  List<RowData> _wireBands(RowData curve) => _eqBands(curve)
      .map(
        (band) => {
          'filter': _eqFilterValue(band['filter']),
          'frequencyHz': decimal(band['frequencyHz']),
          'gainDb': decimal(band['gainDb']),
          'q': decimal(band['q']),
        },
      )
      .toList();

  void _commitCurve(RowData curve, {bool enableIfOff = false}) {
    widget.model.call('equaliser', {
      'name': '${curve['name'] ?? ''}',
      'preamp': decimal(curve['preampDb']),
      'bands': _wireBands(curve),
    });
    if (enableIfOff && widget.model.preferences['equaliserEnabled'] != true) {
      widget.model.call('preferences', {'equaliserEnabled': true});
    }
  }

  Future<void> _fitPreamp(RowData shown) async {
    final fitted = await widget.model.call('fittedPreamp', {
      'name': '${shown['name'] ?? ''}',
      'preamp': decimal(shown['preampDb']),
      'bands': _wireBands(shown),
    });
    if (!mounted || fitted is! num) return;
    _replaceCurve({...shown, 'name': '', 'preampDb': fitted.toDouble()});
  }

  void _addBand(RowData displayed, double hertz, double gainDb) {
    final bandsAtMost = integer(_limits['bandsAtMost']);
    final bands = _eqBands(displayed);
    if (bandsAtMost > 0 && bands.length >= bandsAtMost) return;
    final lowest = decimal(_limits['lowestHz']);
    final highest = decimal(_limits['highestHz']);
    final gainAtMost = decimal(_limits['gainAtMostDb']);
    final low = lowest > 0 ? lowest : 20.0;
    final high = highest > 0 ? highest : 20000.0;
    final gainCap = gainAtMost > 0 ? gainAtMost : 24.0;
    final next = {
      'filter': 'PEAKING',
      'frequencyHz': _eqTidyHertz(hertz.clamp(low, high)),
      'gainDb': _eqTidyDb(gainDb.clamp(-gainCap, gainCap)),
      'q': 1.0,
    };
    final nextBands = [...bands, row(next)];
    final nextCurve = {...displayed, 'name': '', 'bands': nextBands};
    setState(() {
      _draft = row(nextCurve);
      _selected = nextBands.length - 1;
      _holding = false;
    });
    _commitCurve(row(nextCurve), enableIfOff: true);
  }

  void _removeBand(RowData displayed, int index) {
    final bands = _eqBands(displayed).toList();
    if (index < 0 || index >= bands.length) return;
    bands.removeAt(index);
    final nextCurve = {...displayed, 'name': '', 'bands': bands};
    setState(() {
      _draft = row(nextCurve);
      _selected = null;
      _holding = false;
    });
    _commitCurve(row(nextCurve));
  }

  void _replaceCurve(RowData curve, {bool enableIfOff = false}) {
    setState(() {
      _draft = row(curve);
      _selected = null;
      _holding = false;
    });
    _commitCurve(row(curve), enableIfOff: enableIfOff);
  }

  RowData _tenBandCurve({
    List<double>? gains,
    String name = '',
    double preamp = 0,
  }) {
    final centres = _frequencies.length == 10
        ? _frequencies
        : const [
            31.25,
            62.5,
            125.0,
            250.0,
            500.0,
            1000.0,
            2000.0,
            4000.0,
            8000.0,
            16000.0,
          ];
    return {
      'name': name,
      'preampDb': preamp,
      'bands': centres.indexed
          .map(
            (entry) => {
              'filter': 'PEAKING',
              'frequencyHz': entry.$2,
              'gainDb': gains?[entry.$1] ?? 0.0,
              'q': 1.41421356,
            },
          )
          .toList(),
    };
  }

  void _applyPreset(RowData preset) {
    _replaceCurve(row(preset['curve']), enableIfOff: true);
  }

  void _changeBand(RowData curve, int index, RowData band) {
    final bands = _eqBands(curve).toList();
    bands[index] = band;
    final next = {...curve, 'name': '', 'bands': bands};
    setState(() {
      _draft = next;
      _holding = false;
    });
    _commitCurve(next, enableIfOff: true);
  }

  Future<void> _confirmReset() async {
    final reset = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('Reset equaliser?'),
        content: const Text(
          'Restore flat sound and return the output gain to 0 dB.',
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context, false),
            child: const Text('Cancel'),
          ),
          FilledButton(
            onPressed: () => Navigator.pop(context, true),
            child: const Text('Reset'),
          ),
        ],
      ),
    );
    if (reset == true && mounted) {
      _replaceCurve({'name': '', 'preampDb': 0.0, 'bands': []});
    }
  }

  @override
  Widget build(BuildContext context) {
    final model = widget.model;
    final prefs = model.preferences;
    final prefsCurve = row(prefs['equaliser']);
    final prefsStr = prefsCurve.toString();
    if (prefsStr != _prefsRevision) {
      if (!_holding) {
        _prefsRevision = prefsStr;
        _draft = null;
        unawaited(_load());
      }
    }
    if (prefs.isEmpty && _limits.isEmpty) {
      return ListView(
        padding: pageInsets(
          context,
          top: widget.topInset + AppGeometry.pageInset,
        ),
        children: const [Center(child: CircularProgressIndicator())],
      );
    }
    return ListenableBuilder(
      listenable: model,
      builder: (context, _) {
        final livePrefs = model.preferences;
        final liveCurve = row(livePrefs['equaliser']);
        final liveStr = liveCurve.toString();
        if (liveStr != _prefsRevision && !_holding) {
          _prefsRevision = liveStr;
          _draft = null;
          unawaited(_load());
        }
        final shown = _displayed(liveCurve);
        final shownBands = _eqBands(shown);
        final shownPreamp = decimal(shown['preampDb']);
        final shownSelected =
            _selected != null &&
                _selected! >= 0 &&
                _selected! < shownBands.length
            ? _selected
            : null;
        final preampLimit = decimal(_limits['preampAtMostDb']);
        final preampCap = preampLimit > 0 ? preampLimit : 24.0;
        final bandsAtMost = integer(_limits['bandsAtMost']);
        final canAdd = bandsAtMost <= 0 || shownBands.length < bandsAtMost;
        final editable = shownBands.isEmpty
            ? _tenBandCurve(preamp: shownPreamp)
            : shown;
        final editableBands = _eqBands(editable);
        final flat =
            shownPreamp == 0 &&
            shownBands.every(
              (band) =>
                  _eqUsesGain('${band['filter']}') &&
                  decimal(band['gainDb']) == 0,
            );
        final name = flat
            ? 'Flat'
            : '${shown['name'] ?? ''}'.isNotEmpty
            ? '${shown['name']}'
            : 'Custom';
        final preset = _presets
            .where((item) => item['name'] == name)
            .firstOrNull;
        final scheme = Theme.of(context).colorScheme;
        return ListView(
          padding: pageInsets(
            context,
            top: widget.topInset + AppGeometry.pageInset,
          ),
          children: [
            Card.filled(
              child: SwitchListTile(
                contentPadding: const EdgeInsets.symmetric(
                  horizontal: 20,
                  vertical: 8,
                ),
                secondary: Icon(Icons.equalizer_rounded, color: scheme.primary),
                title: const Text('Enable equaliser'),
                subtitle: Text(
                  livePrefs['equaliserEnabled'] == true
                      ? '$name · All audio outputs'
                      : 'Off · Original sound',
                ),
                value: livePrefs['equaliserEnabled'] == true,
                onChanged: (value) =>
                    model.call('preferences', {'equaliserEnabled': value}),
              ),
            ),
            const SectionTitle('Presets'),
            Wrap(
              spacing: 8,
              runSpacing: 8,
              children: [
                for (final item in _presets)
                  ChoiceChip(
                    avatar: Icon(
                      _presetIcons[item['name']] ?? Icons.equalizer_rounded,
                      size: 18,
                    ),
                    label: Text('${item['name']}'),
                    selected: preset == item,
                    showCheckmark: false,
                    tooltip: '${item['detail']}',
                    onSelected: (_) => _applyPreset(item),
                  ),
              ],
            ),
            const SizedBox(height: 12),
            Text(
              preset != null
                  ? '${preset['detail']}'
                  : 'Your own sound. Adjust the sliders to make it yours.',
              style: Theme.of(context).textTheme.bodyMedium
                  ?.copyWith(color: scheme.onSurfaceVariant),
            ),
            SectionTitle(
              'Adjust sound',
              action: TextButton.icon(
                onPressed: _confirmReset,
                icon: const Icon(Icons.restart_alt_rounded, size: 18),
                label: const Text('Reset'),
              ),
            ),
            SegmentedButton<bool>(
              segments: const [
                ButtonSegment(
                  value: false,
                  icon: Icon(Icons.graphic_eq_rounded),
                  label: Text('Sliders'),
                ),
                ButtonSegment(
                  value: true,
                  icon: Icon(Icons.tune_rounded),
                  label: Text('Advanced'),
                ),
              ],
              selected: {_advanced},
              onSelectionChanged: (values) =>
                  setState(() => _advanced = values.single),
            ),
            const SizedBox(height: 12),
            Text(
              _advanced
                  ? 'Select a band below or drag a point on the graph.'
                  : 'Slide left to reduce, right to boost.',
              style: Theme.of(context).textTheme.bodySmall
                  ?.copyWith(color: scheme.onSurfaceVariant),
            ),
            const SizedBox(height: 12),
            if (_advanced) ...[
              const SizedBox(height: 16),
              Card.filled(
                child: Padding(
                  padding: const EdgeInsets.all(18),
                  child: Column(
                    children: [
                      _EqualiserGraph(
                        curve: shown,
                        responseDb: _responseDb,
                        limits: _limits,
                        selected: shownSelected,
                        onSelect: (index) => setState(() => _selected = index),
                        onEdit: (next) => setState(() {
                          _draft = {...next, 'name': ''};
                          _holding = true;
                        }),
                        onCommit: (next) {
                          setState(() {
                            _draft = {...next, 'name': ''};
                            _holding = false;
                          });
                          _commitCurve({...next, 'name': ''});
                        },
                        onAdd: (hertz, gainDb) =>
                            _addBand(shown, hertz, gainDb),
                      ),
                      const SizedBox(height: 12),
                      const Row(
                        mainAxisAlignment: MainAxisAlignment.spaceBetween,
                        children: [
                          Text('20 Hz'),
                          Text('1 kHz'),
                          Text('20 kHz'),
                        ],
                      ),
                    ],
                  ),
                ),
              ),
              const SizedBox(height: 12),
              Wrap(
                spacing: 8,
                runSpacing: 4,
                children: [
                  for (var i = 0; i < shownBands.length; i++)
                    ChoiceChip(
                      label: Text(
                        '${i + 1} · ${_eqFormatHertz(decimal(shownBands[i]['frequencyHz']))}',
                      ),
                      selected: shownSelected == i,
                      onSelected: (_) => setState(() => _selected = i),
                    ),
                ],
              ),
              const SizedBox(height: 12),
              if (shownSelected != null) ...[
                const SizedBox(height: 12),
                _BandEditor(
                  band: shownBands[shownSelected],
                  index: shownSelected,
                  limits: _limits,
                  onChange: (nextBand) {
                    final nextBands = shownBands.toList();
                    nextBands[shownSelected] = nextBand;
                    final nextCurve = {
                      ...shown,
                      'name': '',
                      'bands': nextBands,
                    };
                    setState(() {
                      _draft = row(nextCurve);
                      _holding = false;
                    });
                    _commitCurve(row(nextCurve));
                  },
                  onDelete: () => _removeBand(shown, shownSelected),
                ),
              ],
              FilledButton.tonalIcon(
                onPressed: canAdd ? () => _addBand(shown, 1000, 0) : null,
                icon: const Icon(Icons.add_rounded),
                label: const Text('Add a band'),
              ),
              const SizedBox(height: 12),
            ],
            if (!_advanced)
              Card.filled(
                child: Padding(
                  padding: const EdgeInsets.symmetric(
                    horizontal: 16,
                    vertical: 8,
                  ),
                  child: Column(
                    children: [
                      for (var i = 0; i < editableBands.length; i++) ...[
                        if (i > 0) const Divider(height: 16),
                        if (_eqUsesGain('${editableBands[i]['filter']}'))
                          _EqualiserBandSlider(
                            key: ValueKey(
                              'eq-gain-$i-${editableBands[i]['frequencyHz']}',
                            ),
                            band: editableBands[i],
                            gainLimit: decimal(_limits['gainAtMostDb']),
                            onCommit: (gain) => _changeBand(editable, i, {
                              ...editableBands[i],
                              'gainDb': gain,
                            }),
                          )
                        else
                          ListTile(
                            contentPadding: EdgeInsets.zero,
                            title: Text(
                              _eqFormatHertz(
                                decimal(editableBands[i]['frequencyHz']),
                              ),
                            ),
                            subtitle: Text(
                              _eqFilterLabel('${editableBands[i]['filter']}'),
                            ),
                            trailing: TextButton(
                              onPressed: () => setState(() {
                                _advanced = true;
                                _selected = i;
                              }),
                              child: const Text('Edit'),
                            ),
                          ),
                      ],
                    ],
                  ),
                ),
              ),
            const SectionTitle('Output gain'),
            Card.filled(
              child: Padding(
                padding: const EdgeInsets.all(16),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Row(
                      children: [
                        const Expanded(child: Text('Preamp')),
                        Text(
                          _eqDb(shownPreamp),
                          style: Theme.of(context).textTheme.titleSmall,
                        ),
                      ],
                    ),
                    const SizedBox(height: 8),
                    Text(
                      'Sets the overall level before the frequency adjustments.',
                      style: Theme.of(context).textTheme.bodySmall
                          ?.copyWith(color: scheme.onSurfaceVariant),
                    ),
                    CommitSlider(
                      value: shownPreamp.clamp(-preampCap, preampCap),
                      min: -preampCap,
                      max: preampCap,
                      label: 'Preamp',
                      format: _eqDb,
                      onCommit: (value) => _replaceCurve({
                        ...shown,
                        'name': '',
                        'preampDb': _eqTidyDb(value),
                      }),
                    ),
                    TextButton.icon(
                      onPressed: shownBands.isEmpty
                          ? null
                          : () => _fitPreamp(shown),
                      icon: const Icon(
                        Icons.vertical_align_top_rounded,
                        size: 18,
                      ),
                      label: const Text('Fit the preamp'),
                    ),
                    Text(
                      "Set the preamp from the curve's own peak, so the loudest part of it reaches full scale and no further.",
                      style: Theme.of(context).textTheme.bodySmall
                          ?.copyWith(color: scheme.onSurfaceVariant),
                    ),
                  ],
                ),
              ),
            ),
            const SizedBox(height: 24),
            const SizedBox(height: 16),
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
                onPressed: () {
                  model.call('profile', {
                    'name': _name.text,
                    'text': _text.text,
                  });
                  setState(() {
                    _selected = null;
                    _import = false;
                  });
                },
                child: const Text('Apply profile'),
              ),
              Text(
                'EqualizerAPO or GraphicEQ text. Parametric filters load as bands; a GraphicEQ line is fitted onto bands.',
                style: Theme.of(context).textTheme.bodySmall?.copyWith(
                  color: Theme.of(context).colorScheme.onSurfaceVariant,
                ),
              ),
            ],
            const SizedBox(height: 24),
            RoomCorrectionSection(model: model),
          ],
        );
      },
    );
  }
}

String describeRoom(RowData room) {
  final channels = integer(room['channels']);
  if (channels == 0) return '';
  final label = switch (channels) {
    1 => 'Mono',
    2 => 'Stereo',
    _ => '$channels channels',
  };
  final kilohertz = (integer(room['rate']) / 1000).toStringAsFixed(1);
  final seconds = (integer(room['lengthMs']) / 1000).toStringAsFixed(2);
  return '$label · ${kilohertz.replaceFirst(RegExp(r'\.0$'), '')} kHz · $seconds s';
}

class RoomCorrectionSection extends StatelessWidget {
  const RoomCorrectionSection({super.key, required this.model});
  final PlayerModel model;

  Future<void> _forget(BuildContext context) async {
    final confirmed = await showDialog<bool>(
      context: context,
      builder: (dialog) => AlertDialog(
        title: const Text('Forget the room correction?'),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(dialog, false),
            child: const Text('Cancel'),
          ),
          TextButton(
            onPressed: () => Navigator.pop(dialog, true),
            child: const Text('Forget'),
          ),
        ],
      ),
    );
    if (confirmed == true) await model.call('forgetRoomCorrection');
  }

  @override
  Widget build(BuildContext context) => ListenableBuilder(
    listenable: model,
    builder: (context, _) {
      final room = row(model.state['room']);
      final named = room['name'] != null;
      final reading = room['reading'] == true;
      final scheme = Theme.of(context).colorScheme;
      return Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Text(
            'Room correction',
            style: Theme.of(context).textTheme.titleMedium,
          ),
          SwitchListTile(
            contentPadding: EdgeInsets.zero,
            title: const Text('Correct for the room'),
            subtitle: named
                ? Text('${room['name']} · ${describeRoom(room)}')
                : null,
            value: room['on'] == true,
            onChanged: named
                ? (value) => model.call('roomCorrection', {'enabled': value})
                : null,
          ),
          OutlinedButton.icon(
            onPressed: reading ? null : () => model.call('chooseRoomResponse'),
            icon: const Icon(Icons.graphic_eq_rounded),
            label: Text(
              reading
                  ? 'Reading…'
                  : named
                  ? 'Choose another response…'
                  : 'Choose a response…',
            ),
          ),
          if (named)
            TextButton.icon(
              onPressed: () => _forget(context),
              icon: const Icon(Icons.delete_outline_rounded),
              label: const Text('Forget the response'),
            ),
          const SizedBox(height: 8),
          Text(
            'A measured impulse response, up to ten seconds long, is convolved before the equaliser on every output. It is lowered where it would lift the music past full scale.',
            style: Theme.of(
              context,
            ).textTheme.bodySmall?.copyWith(color: scheme.onSurfaceVariant),
          ),
        ],
      );
    },
  );
}

class _EqualiserBandSlider extends StatefulWidget {
  const _EqualiserBandSlider({
    super.key,
    required this.band,
    required this.gainLimit,
    required this.onCommit,
  });
  final RowData band;
  final double gainLimit;
  final ValueChanged<double> onCommit;
  @override
  State<_EqualiserBandSlider> createState() => _EqualiserBandSliderState();
}

class _EqualiserBandSliderState extends State<_EqualiserBandSlider> {
  double? _draft;
  @override
  void didUpdateWidget(covariant _EqualiserBandSlider oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (decimal(oldWidget.band['gainDb']) != decimal(widget.band['gainDb'])) {
      _draft = null;
    }
  }

  @override
  Widget build(BuildContext context) {
    final frequency = decimal(widget.band['frequencyHz']);
    final gain = _draft ?? decimal(widget.band['gainDb']);
    final nativeLimit = widget.gainLimit > 0 ? widget.gainLimit : 24.0;
    final cap = math.min(
      nativeLimit,
      math.max(12.0, decimal(widget.band['gainDb']).abs()),
    );
    final label = '${_eqFormatHertz(frequency)} ${_eqRange(frequency)}';
    final filter = _eqFilterValue(widget.band['filter']);
    return Padding(
      padding: const EdgeInsets.only(top: 8),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Row(
            children: [
              Expanded(
                child: Text(
                  _eqRange(frequency),
                  style: Theme.of(context).textTheme.titleSmall,
                ),
              ),
              Text(
                _eqDb(gain),
                style: Theme.of(context).textTheme.titleSmall
                    ?.copyWith(color: Theme.of(context).colorScheme.primary),
              ),
            ],
          ),
          Text(
            '${_eqFormatHertz(frequency)}${filter == 'PEAKING' ? '' : ' · ${_eqFilterLabel(filter)}'}',
            style: Theme.of(context).textTheme.bodySmall?.copyWith(
              color: Theme.of(context).colorScheme.onSurfaceVariant,
            ),
          ),
          Slider(
            value: gain.clamp(-cap, cap),
            min: -cap,
            max: cap,
            divisions: (cap * 4).round(),
            label: label,
            semanticFormatterCallback: _eqDb,
            onChanged: (value) => setState(() => _draft = value),
            onChangeEnd: (value) {
              widget.onCommit(_eqTidyDb(value));
              setState(() => _draft = null);
            },
          ),
        ],
      ),
    );
  }
}

class _EqualiserGraph extends StatefulWidget {
  const _EqualiserGraph({
    required this.curve,
    required this.responseDb,
    required this.limits,
    required this.selected,
    required this.onSelect,
    required this.onEdit,
    required this.onCommit,
    required this.onAdd,
  });
  final RowData curve;
  final List<double> responseDb;
  final RowData limits;
  final int? selected;
  final ValueChanged<int?> onSelect;
  final ValueChanged<RowData> onEdit;
  final ValueChanged<RowData> onCommit;
  final void Function(double hertz, double gainDb) onAdd;
  @override
  State<_EqualiserGraph> createState() => _EqualiserGraphState();
}

class _EqualiserGraphState extends State<_EqualiserGraph> {
  int? _held;
  Offset _offset = Offset.zero;
  double? _span;
  bool _moved = false;
  Offset _start = Offset.zero;

  static const double _lowest = 20;
  static const double _highest = 20000;
  static const double _narrowest = 15;
  static const double _widest = 30;
  static const double _markedEvery = 5;
  static const double _reach = 32;
  static const double _tapTravel = 8;

  double _currentSpan() {
    final preamp = decimal(widget.curve['preampDb']);
    final bands = _eqBands(widget.curve);
    final handles = bands.map((band) => _eqLevel(band, preamp)).toList();
    final values = [...widget.responseDb, ...handles].where(
      (v) => v.isFinite,
    );
    double loudest = 0;
    for (final v in values) {
      final abs = v.abs();
      if (abs > loudest) loudest = abs;
    }
    final rounded = (loudest / _markedEvery).ceil() * _markedEvery;
    return rounded.clamp(_narrowest, _widest).toDouble();
  }

  double _x(double hertz, double width) {
    final held = hertz.clamp(_lowest, _highest);
    return math.log(held / _lowest) / math.ln10 / 3 * width;
  }

  double _hertzAtX(double x, double width) {
    final fraction = (x / math.max(width, 1)).clamp(0.0, 1.0);
    return _lowest * math.pow(1000, fraction).toDouble();
  }

  double _y(double decibels, double height, double span) {
    final held = decibels.clamp(-span, span);
    return (span - held) / (2 * span) * height;
  }

  double _dbAtY(double y, double height, double span) =>
      span - y / math.max(height, 1) * 2 * span;

  double _levelOf(RowData band) =>
      _eqLevel(band, decimal(widget.curve['preampDb']));

  int? _nearest(Offset point, Size size, double span) {
    final bands = _eqBands(widget.curve);
    double? best;
    int? at;
    for (var i = 0; i < bands.length; i++) {
      final centre = Offset(
        _x(decimal(bands[i]['frequencyHz']), size.width),
        _y(_levelOf(bands[i]), size.height, span),
      );
      final distance = (centre - point).distance;
      if (best == null || distance < best) {
        best = distance;
        at = i;
      }
    }
    if (at != null && best != null && best <= _reach) return at;
    return null;
  }

  RowData _placed(int held, Offset location, Size size, double span) {
    final bands = _eqBands(widget.curve).toList();
    if (held < 0 || held >= bands.length) return widget.curve;
    final pointed = Offset(
      location.dx + _offset.dx,
      location.dy + _offset.dy,
    );
    final lowest = decimal(widget.limits['lowestHz']);
    final highest = decimal(widget.limits['highestHz']);
    final gainAtMost = decimal(widget.limits['gainAtMostDb']);
    final low = lowest > 0 ? lowest : _lowest;
    final high = highest > 0 ? highest : _highest;
    final gainCap = gainAtMost > 0 ? gainAtMost : 24.0;
    final band = Map<String, dynamic>.from(bands[held]);
    final hertz = _eqTidyHertz(
      _hertzAtX(pointed.dx, size.width).clamp(low, high),
    );
    band['frequencyHz'] = hertz;
    if (_eqUsesGain('${band['filter']}')) {
      final preamp = decimal(widget.curve['preampDb']);
      final gain = _eqTidyDb(
        (_dbAtY(pointed.dy, size.height, span) - preamp).clamp(
          -gainCap,
          gainCap,
        ),
      );
      band['gainDb'] = gain;
    }
    bands[held] = row(band);
    return row({...widget.curve, 'bands': bands});
  }

  @override
  Widget build(BuildContext context) {
    final span = _span ?? _currentSpan();
    final scheme = Theme.of(context).colorScheme;
    return LayoutBuilder(
      builder: (context, constraints) {
        final width = constraints.maxWidth;
        const height = 240.0;
        final size = Size(width, height);
        final bands = _eqBands(widget.curve);
        return GestureDetector(
          behavior: HitTestBehavior.opaque,
          onPanStart: (details) {
            final point = details.localPosition;
            final current = _span ?? _currentSpan();
            final found = _nearest(point, size, current);
            if (found != null) {
              final centre = Offset(
                _x(decimal(bands[found]['frequencyHz']), size.width),
                _y(_levelOf(bands[found]), size.height, current),
              );
              setState(() {
                _held = found;
                _offset = centre - point;
                _span = current;
                _moved = false;
                _start = point;
              });
              widget.onSelect(found);
            } else {
              setState(() {
                _held = null;
                _span = current;
                _moved = false;
                _start = point;
              });
            }
          },
          onPanUpdate: (details) {
            if (_held == null || _span == null) return;
            final travel = (details.localPosition - _start).distance;
            if (!_moved && travel <= 2) return;
            if (!_moved) setState(() => _moved = true);
            widget.onEdit(_placed(_held!, details.localPosition, size, _span!));
          },
          onPanEnd: (details) {
            final held = _held;
            final frozen = _span;
            final moved = _moved;
            final start = _start;
            final end = details.localPosition;
            setState(() {
              _held = null;
              _span = null;
              _moved = false;
            });
            if (held != null && frozen != null) {
              if (moved) widget.onCommit(_placed(held, end, size, frozen));
            } else {
              final travel = (end - start).distance;
              if (travel <= _tapTravel) {
                final preamp = decimal(widget.curve['preampDb']);
                final hertz = _hertzAtX(start.dx, size.width);
                final gain = _dbAtY(start.dy, size.height, frozen ?? span) - preamp;
                widget.onAdd(hertz, gain);
              }
            }
          },
          onTapDown: (details) {
            final point = details.localPosition;
            final current = _span ?? _currentSpan();
            final found = _nearest(point, size, current);
            if (found != null) widget.onSelect(found);
          },
          child: SizedBox(
            height: height,
            width: double.infinity,
            child: Stack(
              children: [
                CustomPaint(
                  size: size,
                  painter: _EqualiserPainter(
                    responseDb: widget.responseDb,
                    span: span,
                    line: scheme.primary,
                    faint: scheme.onSurfaceVariant.withValues(alpha: 0.18),
                    marked: scheme.onSurfaceVariant.withValues(alpha: 0.45),
                  ),
                ),
                for (var i = 0; i < bands.length; i++)
                  Positioned(
                    left:
                        _x(
                          decimal(bands[i]['frequencyHz']).clamp(_lowest, _highest),
                          size.width,
                        ) -
                        13,
                    top:
                        _y(
                          _levelOf(bands[i]).clamp(-span, span),
                          size.height,
                          span,
                        ) -
                        13,
                    child: IgnorePointer(
                      child: Container(
                        width: 26,
                        height: 26,
                        decoration: BoxDecoration(
                          shape: BoxShape.circle,
                          color: widget.selected == i
                              ? scheme.primary
                              : scheme.surface,
                          border: Border.all(
                            color: scheme.primary,
                            width: 2,
                          ),
                          boxShadow: [
                            BoxShadow(
                              color: Colors.black.withValues(alpha: 0.15),
                              blurRadius: 2,
                              offset: const Offset(0, 1),
                            ),
                          ],
                        ),
                        child: Center(
                          child: Text(
                            '${i + 1}',
                            style: TextStyle(
                              fontSize: 11,
                              fontWeight: FontWeight.bold,
                              color: widget.selected == i
                                  ? scheme.onPrimary
                                  : scheme.primary,
                            ),
                          ),
                        ),
                      ),
                    ),
                  ),
              ],
            ),
          ),
        );
      },
    );
  }
}

class _EqualiserPainter extends CustomPainter {
  _EqualiserPainter({
    required this.responseDb,
    required this.span,
    required this.line,
    required this.faint,
    required this.marked,
  });
  final List<double> responseDb;
  final double span;
  final Color line;
  final Color faint;
  final Color marked;
  static const List<double> _faintFreqs = [
    30, 40, 50, 60, 70, 80, 90, 200, 300, 400, 500, 600, 700, 800, 900,
    2000, 3000, 4000, 5000, 6000, 7000, 8000, 9000,
  ];
  static const List<(double, String)> _labelledFreqs = [
    (100, '100'),
    (1000, '1k'),
    (10000, '10k'),
  ];
  double _x(double hertz, double width) {
    final held = hertz.clamp(20, 20000);
    return math.log(held / 20) / math.ln10 / 3 * width;
  }

  double _y(double decibels, double height) {
    final held = decibels.clamp(-span, span);
    return (span - held) / (2 * span) * height;
  }

  @override
  void paint(Canvas canvas, Size size) {
    final faintPaint = Paint()
      ..color = faint
      ..strokeWidth = 0.5;
    final markedPaint = Paint()
      ..color = marked
      ..strokeWidth = 0.75;
    for (final hertz in _faintFreqs) {
      final x = _x(hertz, size.width);
      canvas.drawLine(Offset(x, 0), Offset(x, size.height), faintPaint);
    }
    for (final entry in _labelledFreqs) {
      final x = _x(entry.$1, size.width);
      canvas.drawLine(Offset(x, 0), Offset(x, size.height), markedPaint);
      final label = TextPainter(
        text: TextSpan(
          text: entry.$2,
          style: const TextStyle(fontSize: 10, color: Colors.grey),
        ),
        textDirection: TextDirection.ltr,
      )..layout();
      label.paint(canvas, Offset(x + 3, size.height - 13));
    }
    var level = -span;
    while (level <= span + 0.001) {
      final y = _y(level, size.height);
      final zero = level.abs() < 0.001;
      if (zero) {
        canvas.drawLine(
          Offset(0, y),
          Offset(size.width, y),
          Paint()
            ..color = marked
            ..strokeWidth = 1,
        );
      } else {
        canvas.drawLine(Offset(0, y), Offset(size.width, y), faintPaint);
      }
      if (zero || (level - span).abs() < 0.001 || (level + span).abs() < 0.001) {
        final text = zero ? '0 dB' : level.toStringAsFixed(0);
        final label = TextPainter(
          text: TextSpan(
            text: text,
            style: const TextStyle(fontSize: 10, color: Colors.grey),
          ),
          textDirection: TextDirection.ltr,
        )..layout();
        label.paint(
          canvas,
          Offset(3, y + (level >= span - 0.001 ? 2 : -12)),
        );
      }
      level += 5;
    }
    if (responseDb.length < 2) return;
    final trace = Path();
    for (var i = 0; i < responseDb.length; i++) {
      final x = i / (responseDb.length - 1) * size.width;
      final y = _y(responseDb[i], size.height);
      if (i == 0) {
        trace.moveTo(x, y);
      } else {
        trace.lineTo(x, y);
      }
    }
    final zeroY = _y(0, size.height);
    final wash = Path.from(trace)
      ..lineTo(size.width, zeroY)
      ..lineTo(0, zeroY)
      ..close();
    canvas.drawPath(
      wash,
      Paint()..color = line.withValues(alpha: 0.18),
    );
    canvas.drawPath(
      trace,
      Paint()
        ..color = line
        ..strokeWidth = 2
        ..style = PaintingStyle.stroke
        ..strokeCap = StrokeCap.round
        ..strokeJoin = StrokeJoin.round,
    );
  }

  @override
  bool shouldRepaint(_EqualiserPainter old) =>
      responseDb != old.responseDb || span != old.span || line != old.line;
}

class _BandEditor extends StatelessWidget {
  const _BandEditor({
    required this.band,
    required this.index,
    required this.limits,
    required this.onChange,
    required this.onDelete,
  });
  final RowData band;
  final int index;
  final RowData limits;
  final ValueChanged<RowData> onChange;
  final VoidCallback onDelete;
  @override
  Widget build(BuildContext context) {
    final filter = _eqFilterValue(band['filter']);
    final freq = decimal(band['frequencyHz']);
    final gain = decimal(band['gainDb']);
    final q = decimal(band['q']);
    final lowest = decimal(limits['lowestHz']);
    final highest = decimal(limits['highestHz']);
    final gainAtMost = decimal(limits['gainAtMostDb']);
    final qLeast = decimal(limits['qAtLeast']);
    final qMost = decimal(limits['qAtMost']);
    final low = lowest > 0 ? lowest : 20.0;
    final high = highest > 0 ? highest : 20000.0;
    final gainCap = gainAtMost > 0 ? gainAtMost : 24.0;
    final qLow = qLeast > 0 ? qLeast : 0.1;
    final qHigh = qMost > 0 ? qMost : 40.0;
    final usesGain = _eqUsesGain(filter);
    return Card.filled(
      child: Padding(
        padding: const EdgeInsets.all(16),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                Expanded(child: Text('Band ${index + 1}', style: Theme.of(context).textTheme.titleMedium)),
                TextButton.icon(
                  onPressed: onDelete,
                  icon: const Icon(Icons.delete_outline_rounded, size: 18),
                  label: const Text('Delete'),
                ),
              ],
            ),
            const SizedBox(height: 8),
            DropdownMenu<String>(
              initialSelection: filter,
              expandedInsets: EdgeInsets.zero,
              requestFocusOnTap: false,
              label: const Text('Type'),
              menuStyle: selectionMenuStyle(context),
              dropdownMenuEntries: const [
                'PEAKING',
                'LOW_SHELF',
                'HIGH_SHELF',
                'LOW_PASS',
                'HIGH_PASS',
                'NOTCH',
                'BAND_PASS',
                'ALL_PASS',
              ]
                  .map(
                    (value) => DropdownMenuEntry(
                      value: value,
                      label: _eqFilterLabel(value),
                    ),
                  )
                  .toList(),
              onSelected: (value) {
                if (value == null) return;
                onChange(row({...band, 'filter': value}));
              },
            ),
            const SizedBox(height: 12),
            Text('${_eqFormatHertz(freq)} · ${_eqFilterLabel(filter)}'),
            _LogCommitSlider(
              value: freq.clamp(low, high),
              min: low,
              max: high,
              label: 'Band ${index + 1} frequency',
              format: _eqFormatHertz,
              onCommit: (chosen) => onChange(
                row({
                  ...band,
                  'frequencyHz': _eqTidyHertz(chosen.clamp(low, high)),
                }),
              ),
            ),
            if (usesGain) ...[
              Text('${gain.toStringAsFixed(1)} dB'),
              CommitSlider(
                value: gain.clamp(-gainCap, gainCap),
                min: -gainCap,
                max: gainCap,
                label: 'Band ${index + 1} gain',
                format: (v) => '${v.toStringAsFixed(1)} dB',
                onCommit: (chosen) => onChange(
                  row({
                    ...band,
                    'gainDb': _eqTidyDb(chosen.clamp(-gainCap, gainCap)),
                  }),
                ),
              ),
            ],
            Text('Q ${q.toStringAsFixed(2)}'),
            _LogCommitSlider(
              value: q.clamp(qLow, qHigh),
              min: qLow,
              max: qHigh,
              label: 'Band ${index + 1} Q',
              format: (v) => 'Q ${v.toStringAsFixed(2)}',
              onCommit: (chosen) => onChange(
                row({
                  ...band,
                  'q': _eqTidyQ(chosen.clamp(qLow, qHigh)),
                }),
              ),
            ),
          ],
        ),
      ),
    );
  }
}

class _LogCommitSlider extends StatefulWidget {
  const _LogCommitSlider({
    required this.value,
    required this.min,
    required this.max,
    required this.label,
    required this.onCommit,
    this.format,
  });
  final double value;
  final double min;
  final double max;
  final String label;
  final ValueChanged<double> onCommit;
  final String Function(double)? format;
  @override
  State<_LogCommitSlider> createState() => _LogCommitSliderState();
}

class _LogCommitSliderState extends State<_LogCommitSlider> {
  double? _draft;
  double _toSlider(double actual) {
    final low = math.max(widget.min, 1e-6);
    final high = math.max(widget.max, low * 1.001);
    final held = actual.clamp(low, high);
    return math.log(held / low) / math.log(high / low);
  }

  double _toActual(double slider) {
    final low = math.max(widget.min, 1e-6);
    final high = math.max(widget.max, low * 1.001);
    final held = slider.clamp(0.0, 1.0);
    return low * math.pow(high / low, held).toDouble();
  }

  @override
  Widget build(BuildContext context) {
    final shown = _draft ?? widget.value;
    final slider = _toSlider(shown).clamp(0.0, 1.0);
    return Semantics(
      label: widget.label,
      child: Slider(
        value: slider,
        min: 0,
        max: 1,
        label: widget.format?.call(shown) ?? shown.toStringAsFixed(1),
        onChanged: (t) => setState(() => _draft = _toActual(t)),
        onChangeEnd: (t) {
          final actual = _toActual(t);
          widget.onCommit(actual);
          setState(() => _draft = null);
        },
      ),
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
                      widget.model.call('applyCorrection', {
                        'id': entry.$2['id'],
                      });
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
        onChanged: (value) => setState(() => _draft[key] = value),
      ),
    );
  }

  void _commit(String key, bool value) {
    setState(() => _draft[key] = value);
    widget.model.call('sync', {key: value});
  }

  bool _unsaved(RowData stored) =>
      '${_draft['device'] ?? ''}' != '${stored['device'] ?? ''}' ||
      '${_draft['server'] ?? ''}' != '${stored['server'] ?? ''}';

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
          DropdownMenu<String>(
            initialSelection: _draft['provider'] as String?,
            expandedInsets: EdgeInsets.zero,
            selectOnly: true,
            enableSearch: false,
            label: const Text('Provider'),
            inputDecorationTheme: Theme.of(context).inputDecorationTheme,
            menuStyle: selectionMenuStyle(context),
            dropdownMenuEntries: ['MONOCHROME', 'HIFI_API', 'SUBSONIC', 'TIDAL']
                .map(
                  (v) => DropdownMenuEntry(
                    value: v,
                    label: v == 'HIFI_API' ? 'hifi-api' : humanize(v),
                  ),
                )
                .toList(),
            onSelected: (v) => setState(() => _draft['provider'] = v),
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
            'Completed listens are queued on this device and submitted when online features are enabled. '
            'Favourites with a recording are loved on each saved account and taken back there when removed.',
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
                  onChanged: (v) => _commit('enabled', v),
                ),
                Padding(
                  padding: const EdgeInsets.fromLTRB(16, 0, 16, 14),
                  child: Row(
                    children: [
                      if (!model.online && _draft['nearbyOnly'] != true)
                        OutlinedButton.icon(
                          onPressed: () => _commit('nearbyOnly', true),
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
                    onChanged: (v) => _commit('nearbyOnly', v),
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
                    onPressed: _unsaved(stored)
                        ? () => model.call('sync', {
                            'device': _draft['device'],
                            'server': _draft['server'],
                          })
                        : null,
                    child: Text(
                      _unsaved(stored)
                          ? 'Save device name and server'
                          : 'Device name and server saved',
                    ),
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
                  onChanged: (v) => _commit('favourites', v),
                ),
                SwitchListTile(
                  title: const Text('Sync playlists'),
                  subtitle: const Text(
                    'Playlists set to keep their songs here',
                  ),
                  value: _draft['playlists'] == true,
                  onChanged: (v) => _commit('playlists', v),
                ),
                SwitchListTile(
                  title: const Text('Keep all music here'),
                  subtitle: const Text('Every song your other devices hold'),
                  value: _draft['everything'] == true,
                  onChanged: (v) => _commit('everything', v),
                ),
                SwitchListTile(
                  title: const Text('Transfer over mobile data'),
                  subtitle: const Text('Without it, songs move only on Wi-Fi'),
                  value: _draft['meteredAllowed'] == true,
                  onChanged: (v) => _commit('meteredAllowed', v),
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
