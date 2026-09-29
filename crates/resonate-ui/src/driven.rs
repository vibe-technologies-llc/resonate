use std::{
    env, fs,
    num::NonZeroUsize,
    path::{Path, PathBuf},
    process,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use crossbeam_channel::{Receiver, Sender, unbounded};
use gpui::{
    Entity, Modifiers, MouseButton, Pixels, Point, ScrollDelta, ScrollWheelEvent, TestAppContext,
    TouchPhase, VisualTestContext, point, px, size,
};
use resonate_core::{Appearance, Frames, Gain, MediaLocation, Resumable, Resumption, TrackId};
use resonate_engine::{
    AudioSource, Backend, Command, EngineConfig, Player, ProfileIndex, SinkChange, SinkError,
    SinkId, SinkInfo, SinkResult, SinkStream, StreamRequest, Surveyor,
};
use resonate_eq::Corrected;
use resonate_library::{Fingerprinters, Library, ScanOptions};
use resonate_listen::{Listener, Listening, Recognisers};
use resonate_lyrics::Lyricists;
use resonate_providers::Providers;

use crate::{
    AppIcon, Bindings, Ephemeral, Launcher, Listens, Online, Places, Present, ResonateApp,
    RootView, SettingsCategory, Sourcing, Tabs, WindowButtons, app, drawing::Drawer, theme,
};

pub(crate) const WIDE: f32 = 1_400.0;
pub(crate) const TALL: f32 = 1_600.0;
const FRAME: Duration = Duration::from_millis(16);
const PATIENCE: Duration = Duration::from_secs(10);
const RATE: u32 = 44_100;
const CHANNELS: u16 = 2;

pub(crate) struct Folder {
    root: PathBuf,
}

impl Folder {
    pub(crate) fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = env::temp_dir().join(format!(
            "resonate-driven-{}-{}",
            process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).expect("a writable temporary folder");
        Self { root }
    }

    pub(crate) fn path(&self) -> &Path {
        &self.root
    }

    pub(crate) fn tone(&self, name: &str, seconds: u32) -> PathBuf {
        self.tagged(name, seconds, &[])
    }

    pub(crate) fn tagged(&self, name: &str, seconds: u32, tags: &[(&[u8; 4], &str)]) -> PathBuf {
        let frames = RATE * seconds;
        let block = u32::from(CHANNELS) * 2;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + frames * block).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&CHANNELS.to_le_bytes());
        bytes.extend_from_slice(&RATE.to_le_bytes());
        bytes.extend_from_slice(&(RATE * block).to_le_bytes());
        bytes.extend_from_slice(&(block as u16).to_le_bytes());
        bytes.extend_from_slice(&16_u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&(frames * block).to_le_bytes());
        for frame in 0..frames {
            let level = ((frame % 100) as i16 - 50) * 200;
            for _ in 0..CHANNELS {
                bytes.extend_from_slice(&level.to_le_bytes());
            }
        }
        if !tags.is_empty() {
            let mut listed = b"INFO".to_vec();
            for (id, text) in tags {
                let mut value = text.as_bytes().to_vec();
                value.push(0);
                if value.len() % 2 == 1 {
                    value.push(0);
                }
                listed.extend_from_slice(*id);
                listed.extend_from_slice(&(value.len() as u32).to_le_bytes());
                listed.extend_from_slice(&value);
            }
            bytes.extend_from_slice(b"LIST");
            bytes.extend_from_slice(&(listed.len() as u32).to_le_bytes());
            bytes.extend_from_slice(&listed);
            let riff = (bytes.len() - 8) as u32;
            bytes[4..8].copy_from_slice(&riff.to_le_bytes());
        }
        let path = self.root.join(name);
        fs::write(&path, bytes).expect("a writable temporary file");
        path
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

struct Unplugged {
    changes: Receiver<SinkChange>,
    _announcing: Sender<SinkChange>,
}

struct NoSinks;

impl Surveyor for NoSinks {
    fn enumerate_sinks(&self, _timeout: Duration) -> SinkResult<Vec<SinkInfo>> {
        Ok(Vec::new())
    }
}

impl Backend for Unplugged {
    fn subscribe_sinks(&self) -> Receiver<SinkChange> {
        self.changes.clone()
    }

    fn surveyor(&self) -> Arc<dyn Surveyor> {
        Arc::new(NoSinks)
    }

    fn open(&self, _: &StreamRequest, _: Box<dyn AudioSource>) -> SinkResult<SinkStream> {
        Err(SinkError::NoSink)
    }

    fn set_device_volume(&self, _: SinkId, _: Gain) -> SinkResult<()> {
        Ok(())
    }

    fn set_device_mute(&self, _: SinkId, _: bool) -> SinkResult<()> {
        Ok(())
    }

    fn set_card_profile(&self, _: SinkId, _: ProfileIndex) -> SinkResult<()> {
        Ok(())
    }

    fn shutdown(self: Box<Self>) -> SinkResult<()> {
        Ok(())
    }
}

struct Unseen;

impl Present for Unseen {
    fn follow(&self, _: &resonate_core::Presence) {}
}

impl Launcher for Unseen {
    fn show(&self, _: AppIcon) {}
}

fn unplugged_player() -> Arc<Player> {
    let (announcing, changes) = unbounded();
    Arc::new(
        Player::with_backend(EngineConfig::default(), move |_| {
            Ok(Box::new(Unplugged {
                changes,
                _announcing: announcing,
            }))
        })
        .expect("an engine over no graph starts"),
    )
}

fn nobody_listening() -> Listens {
    Listens {
        listener: Listener::new("resonate-tests"),
        recognisers: Arc::new(Recognisers::none()),
        tell: Arc::new(|_| {}),
        from: Listening::Desktop,
        length: Duration::from_secs(12),
    }
}

pub(crate) struct Driven {
    pub(crate) root: Entity<RootView>,
    pub(crate) cx: VisualTestContext,
}

impl Driven {
    pub(crate) fn open(cx: &mut TestAppContext, library: Arc<Library>) -> Self {
        Self::opened_in(cx, library, &Folder::new())
    }

    pub(crate) fn opened_in(
        cx: &mut TestAppContext,
        library: Arc<Library>,
        folder: &Folder,
    ) -> Self {
        Self::corrected_by(cx, library, folder, Corrected::uncorrected())
    }

    pub(crate) fn corrected_by(
        cx: &mut TestAppContext,
        library: Arc<Library>,
        folder: &Folder,
        corrections: Corrected,
    ) -> Self {
        theme::wear(Appearance::default());
        let (attention, _) = unbounded();
        let unseen = Arc::new(Unseen);
        cx.update(|cx| {
            cx.set_global(Drawer::new());
            cx.set_global(ResonateApp {
                player: unplugged_player(),
                library,
                lyricists: Arc::new(Lyricists::unsourced()),
                fingerprinters: Arc::new(Fingerprinters::none()),
                corrections: Arc::new(corrections),
                settings: Arc::new(Ephemeral),
                online: Online::default(),
                bindings: Bindings::default(),
                reference: None,
                attention,
                places: Places {
                    config: folder.path().join("config.toml"),
                    library: folder.path().join("library.db"),
                    equaliser: folder.path().join("equaliser"),
                },
                resume: false,
                history_kept: resonate_library::HistoryKept::Forever,
                organise_as: String::new(),
                notify: Arc::new(AtomicBool::new(false)),
                by_sound: Arc::new(AtomicBool::new(false)),
                convolution: None,
                window_buttons: WindowButtons::SHOWN,
                scroll_volume: true,
                caret: crate::CaretBlink::as_built(),
                scrollbars: resonate_core::ScrollbarMode::default(),
                tabs: Tabs::AS_BUILT,
                remember_tab: false,
                last_tab: None,
                remember_window_size: false,
                window_size: None,
                remember_settings_category: false,
                last_settings_category: SettingsCategory::default(),
                presence: resonate_core::Presence::default(),
                present: unseen.clone(),
                launcher: unseen,
                sourcing: Sourcing {
                    inbox: None,
                    register: Arc::new(|_: Option<&Path>| Providers::none()),
                },
                listens: nobody_listening(),
                first_read: None,
            });
            cx.bind_keys(app::bindings());
        });
        let (root, cx) = cx.add_window_view(RootView::new);
        let mut cx = cx.clone();
        cx.update(|window, _| window.activate_window());
        cx.simulate_resize(size(px(WIDE), px(TALL)));
        cx.run_until_parked();
        Self { root, cx }
    }

    pub(crate) fn settle(&mut self) {
        self.cx.run_until_parked();
        self.cx.executor().advance_clock(FRAME);
        self.cx.run_until_parked();
        self.cx.update(|window, _| window.refresh());
        self.cx.run_until_parked();
    }

    pub(crate) fn until(&mut self, mut holds: impl FnMut(&RootView, &gpui::App) -> bool) {
        let began = Instant::now();
        loop {
            self.settle();
            if self.read(&mut holds) {
                return;
            }
            assert!(
                began.elapsed() < PATIENCE,
                "the window never came to the state asked for"
            );
            thread::sleep(FRAME);
        }
    }

    pub(crate) fn play(&mut self, files: &[PathBuf]) {
        let rows = files
            .iter()
            .map(|path| Resumable {
                location: MediaLocation::local(path),
                span: None,
                held: None,
            })
            .collect();
        let player = self
            .cx
            .update(|_, cx| Arc::clone(&cx.global::<ResonateApp>().player));
        player
            .send(Command::Resume(Resumption {
                rows,
                order: (0..files.len()).collect(),
                row: 0,
                at: Frames::ZERO,
                shuffle: false,
                next: None,
            }))
            .expect("the engine takes a resumption");
        self.until(|root, cx| root.player.read(cx).state().current.is_some());
    }

    pub(crate) fn scroll(&mut self, selector: &'static str, down: f32) {
        let at = self.centre_of(selector);
        self.scroll_at(at, down);
    }

    pub(crate) fn scroll_at(&mut self, at: Point<Pixels>, down: f32) {
        self.cx.simulate_event(ScrollWheelEvent {
            position: at,
            delta: ScrollDelta::Pixels(point(px(0.0), px(-down))),
            modifiers: Modifiers::none(),
            touch_phase: TouchPhase::Moved,
        });
        self.settle();
    }

    pub(crate) fn bounds_of(&mut self, selector: &'static str) -> gpui::Bounds<Pixels> {
        self.settle();
        self.cx
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("nothing drawn answers to {selector}"))
    }

    pub(crate) fn centre_of(&mut self, selector: &'static str) -> Point<Pixels> {
        self.bounds_of(selector).center()
    }

    pub(crate) fn click(&mut self, selector: &'static str) {
        let at = self.centre_of(selector);
        self.click_at(at);
    }

    pub(crate) fn click_at(&mut self, at: Point<Pixels>) {
        self.hover(at);
        self.cx.simulate_click(at, Modifiers::none());
        self.settle();
    }

    pub(crate) fn right_click(&mut self, selector: &'static str) {
        let at = self.centre_of(selector);
        self.right_click_at(at);
    }

    pub(crate) fn right_click_at(&mut self, at: Point<Pixels>) {
        self.hover(at);
        self.cx
            .simulate_mouse_down(at, MouseButton::Right, Modifiers::none());
        self.cx
            .simulate_mouse_up(at, MouseButton::Right, Modifiers::none());
        self.settle();
    }

    pub(crate) fn drag(&mut self, from: Point<Pixels>, to: Point<Pixels>) {
        self.hover(from);
        self.cx
            .simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
        self.settle();
        let steps = 8;
        for step in 1..=steps {
            let along = step as f32 / steps as f32;
            let x = from.x + (to.x - from.x) * along;
            let y = from.y + (to.y - from.y) * along;
            self.cx
                .simulate_mouse_move(point(x, y), Some(MouseButton::Left), Modifiers::none());
            self.settle();
        }
        self.cx
            .simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
        self.settle();
    }

    pub(crate) fn hover(&mut self, at: Point<Pixels>) {
        self.cx.simulate_mouse_move(at, None, Modifiers::none());
        self.settle();
    }

    pub(crate) fn scanned(library: &Library, folder: &Folder) {
        library
            .scan(ScanOptions {
                roots: vec![folder.path().to_path_buf()],
                incremental: true,
                follow_symlinks: false,
                extract_cover_art: true,
                workers: NonZeroUsize::MIN,
            })
            .expect("a scan starts")
            .join()
            .expect("a scan ends");
    }

    pub(crate) fn queue(&mut self) -> Vec<TrackId> {
        self.settle();
        self.read(|root, cx| {
            root.player
                .read(cx)
                .queue()
                .iter()
                .map(|item| item.id)
                .collect()
        })
    }

    pub(crate) fn read<R>(&mut self, read: impl FnOnce(&RootView, &gpui::App) -> R) -> R {
        let root = self.root.clone();
        self.cx.update(|_, cx| read(root.read(cx), cx))
    }

    pub(crate) fn focus(
        &mut self,
        field: impl FnOnce(&RootView) -> &Entity<crate::views::field::Field>,
    ) {
        let root = self.root.clone();
        self.cx.update(|window, cx| {
            let field = field(root.read(cx)).clone();
            field.read(cx).take_focus(window);
        });
        self.settle();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Pane,
        views::{settings::curve::Plot, visualiser::Showing},
    };

    fn catalog() -> Arc<Library> {
        Arc::new(Library::open_in_memory().expect("a catalog in memory"))
    }

    #[gpui::test]
    fn a_press_on_a_sidebar_tab_opens_its_pane(cx: &mut TestAppContext) {
        let mut driven = Driven::open(cx, catalog());
        assert_eq!(driven.read(|root, _| root.pane), Pane::default());

        driven.click("tab-visualiser");

        assert_eq!(driven.read(|root, _| root.pane), Pane::Visualiser);
    }

    #[gpui::test]
    fn a_press_on_the_scope_segment_turns_the_visualiser_to_the_scope(cx: &mut TestAppContext) {
        let folder = Folder::new();
        let mut driven = Driven::open(cx, catalog());
        driven.play(&[folder.tone("tone.wav", 2)]);
        driven.click("tab-visualiser");
        assert_eq!(
            driven.read(|root, cx| root.visualiser.read(cx).showing()),
            Showing::Spectrum
        );

        driven.click("visualiser-showing-1");

        assert_eq!(
            driven.read(|root, cx| root.visualiser.read(cx).showing()),
            Showing::Scope
        );
    }

    #[gpui::test]
    fn a_press_on_the_equaliser_curve_adds_a_band_a_drag_moves_it_and_a_right_press_takes_it_out(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let mut driven = Driven::opened_in(cx, catalog(), &folder);
        driven.click("tab-settings");
        driven.click("category-Equaliser");
        let curve = driven.bounds_of("equaliser-curve");
        let pressed = point(
            curve.origin.x + curve.size.width * 0.3,
            curve.origin.y + curve.size.height * 0.3,
        );

        driven.click_at(pressed);
        assert_eq!(
            driven.bounds_of("equaliser-curve"),
            curve,
            "the curve moved out from under the press that shaped it"
        );
        let added = driven.read(|root, cx| root.equaliser.read(cx).shown_bands());
        let dragged_to = point(
            curve.origin.x + curve.size.width * 0.6,
            curve.origin.y + curve.size.height * 0.7,
        );
        driven.drag(pressed, dragged_to);
        let moved = driven.read(|root, cx| root.equaliser.read(cx).shown_bands());
        assert_eq!(moved.len(), 1);
        assert!(
            moved[0].frequency > added[0].frequency && moved[0].gain < added[0].gain,
            "the drag did not carry the band right and down: {added:?} to {moved:?}"
        );

        let (x, y) = driven.read(|root, _| Plot::of(root.curve_plotted.get()).handle(moved[0]));
        driven.right_click_at(point(px(x), px(y)));
        assert!(
            driven
                .read(|root, cx| root.equaliser.read(cx).shown_bands())
                .is_empty(),
            "a right press on the band left it standing"
        );
    }

    fn named(prefix: &str, id: impl std::fmt::Display) -> &'static str {
        Box::leak(format!("{prefix}-{id}").into_boxed_str())
    }

    fn scanned_catalog(folder: &Folder, titles: &[&str]) -> Arc<Library> {
        for title in titles {
            folder.tone(&format!("{title}.wav"), 1);
        }
        let library = catalog();
        Driven::scanned(&library, folder);
        library
    }

    #[gpui::test]
    fn a_right_press_on_a_track_opens_its_menu_and_an_entry_does_what_it_says(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let library = scanned_catalog(&folder, &["Echoes", "Time"]);
        let first = library
            .tracks(&resonate_library::TrackQuery::default())
            .expect("the scanned tracks")[0]
            .id;
        let mut driven = Driven::opened_in(cx, library, &folder);
        driven.click("tab-tracks");
        driven.until(|root, cx| root.library.read(cx).tracks_counted() == 2);

        driven.right_click(named("track", first.get()));
        assert!(
            driven.read(|root, _| root.menu.is_some()),
            "a right press opened no menu"
        );

        driven.click("menu-entry-2");
        assert!(
            driven.read(|root, _| root.menu.is_none()),
            "the menu stayed open"
        );
        driven.until(|root, cx| root.player.read(cx).queue().len() == 1);
    }

    #[gpui::test]
    fn a_row_played_from_a_listing_read_in_part_queues_the_whole_listing_from_that_row(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let library = scanned_catalog(&folder, &["Echoes", "Money", "Time"]);
        let mut every: Vec<TrackId> = library
            .tracks(&resonate_library::TrackQuery::default())
            .expect("the scanned tracks")
            .iter()
            .map(|track| track.id)
            .collect();
        let mut driven = Driven::opened_in(cx, library, &folder);
        driven.click("tab-tracks");
        driven.until(|root, cx| root.library.read(cx).tracks_counted() == 3);
        let root = driven.root.clone();
        let first = driven.cx.update(|window, cx| {
            root.update(cx, |root, cx| {
                let listing = root.library.read(cx).listing();
                root.play_the_listing_from(&listing[1..2], 0, window, cx);
                listing[1].id
            })
        });
        driven.until(|root, cx| root.player.read(cx).queue().len() == 3);

        let mut queued = driven.queue();
        let at = driven.read(|root, cx| root.player.read(cx).state().queue_position);

        assert_eq!(at.and_then(|at| queued.get(at)), Some(&first));
        queued.sort_unstable();
        every.sort_unstable();
        assert_eq!(queued, every);
    }

    #[gpui::test]
    fn the_playlist_picker_puts_a_track_into_the_playlist_pressed(cx: &mut TestAppContext) {
        let folder = Folder::new();
        let library = scanned_catalog(&folder, &["Echoes"]);
        let track = library
            .tracks(&resonate_library::TrackQuery::default())
            .expect("the scanned tracks")[0]
            .id;
        let playlist = library.create_playlist("Meddle").expect("a playlist");
        let mut driven = Driven::opened_in(cx, Arc::clone(&library), &folder);
        driven.click("tab-tracks");
        driven.until(|root, cx| root.library.read(cx).tracks_counted() == 1);

        driven.right_click(named("track", track.get()));
        driven.click("menu-entry-3");
        assert!(
            driven.read(|root, _| root.adding.is_some()),
            "Add to playlist opened no picker"
        );

        driven.click(named("picker", playlist.get()));
        assert!(driven.read(|root, _| root.adding.is_none()));
        let began = Instant::now();
        while library
            .playlist_entries(playlist, None)
            .expect("the playlist's rows")
            .is_empty()
        {
            assert!(began.elapsed() < PATIENCE, "the row never landed");
            driven.settle();
            thread::sleep(FRAME);
        }
    }

    #[gpui::test]
    fn a_queued_row_dragged_below_another_is_played_after_it(cx: &mut TestAppContext) {
        let folder = Folder::new();
        let files = [
            folder.tone("one.wav", 1),
            folder.tone("two.wav", 1),
            folder.tone("three.wav", 1),
        ];
        let mut driven = Driven::opened_in(cx, catalog(), &folder);
        driven.play(&files);
        driven.click("queue");
        let before = driven.queue();
        assert_eq!(before.len(), 3);

        let from = driven.centre_of(named("queued", before[1].get()));
        let onto = driven.bounds_of(named("queued", before[2].get()));
        driven.drag(
            from,
            point(onto.center().x, onto.origin.y + onto.size.height * 0.75),
        );

        driven.until(|root, cx| {
            let now: Vec<TrackId> = root
                .player
                .read(cx)
                .queue()
                .iter()
                .map(|item| item.id)
                .collect();
            now == [before[0], before[2], before[1]]
        });
    }

    #[gpui::test]
    fn a_press_on_a_suggestion_card_opens_the_rows_it_would_hold(cx: &mut TestAppContext) {
        let folder = Folder::new();
        let titles: Vec<String> = (1..=12).map(|nth| format!("Track {nth}")).collect();
        let named: Vec<&str> = titles.iter().map(String::as_str).collect();
        let library = scanned_catalog(&folder, &named);
        let mut driven = Driven::opened_in(cx, library, &folder);
        driven.until(|root, cx| !root.library.read(cx).suggestions().is_empty());
        driven.click("tab-suggestions");

        driven.click("suggestion-open-0");

        driven.until(|root, cx| {
            root.library
                .read(cx)
                .opened_suggestion()
                .is_some_and(|opened| !opened.tracks.is_empty())
        });
    }

    #[gpui::test]
    fn the_caret_blinks_as_the_desktop_says_and_holds_still_where_it_says_not_to(
        cx: &mut TestAppContext,
    ) {
        let mut driven = Driven::open(cx, catalog());
        driven.cx.simulate_keystrokes("ctrl-f");
        driven.settle();
        let lit = |driven: &mut Driven| driven.read(|root, cx| root.search.read(cx).caret_is_lit());
        let mut seen_dark = false;
        for _ in 0..40 {
            driven.cx.executor().advance_clock(FRAME * 4);
            driven.settle();
            seen_dark |= !lit(&mut driven);
        }
        assert!(seen_dark, "the caret never blinked");

        let caret = driven
            .cx
            .update(|_, cx| cx.global::<ResonateApp>().caret.clone());
        caret.steady();
        driven.cx.executor().advance_clock(Duration::from_secs(2));
        driven.settle();
        for _ in 0..40 {
            driven.cx.executor().advance_clock(FRAME * 4);
            driven.settle();
            assert!(
                lit(&mut driven),
                "a caret the desktop holds still went dark"
            );
        }
    }

    #[gpui::test]
    fn the_way_back_lands_on_the_pixel_the_list_was_left_at(cx: &mut TestAppContext) {
        let folder = Folder::new();
        for nth in 0..80 {
            folder.tagged(
                &format!("{nth:02}.wav"),
                1,
                &[(b"INAM", &format!("Track {nth}")), (b"IPRD", "Meddle")],
            );
        }
        let library = catalog();
        Driven::scanned(&library, &folder);
        let album = library
            .tracks(&resonate_library::TrackQuery::default())
            .expect("the scanned tracks")[0]
            .album_id
            .expect("an album the tags named");
        let mut driven = Driven::opened_in(cx, library, &folder);
        driven.click("tab-tracks");
        driven.until(|root, cx| root.library.read(cx).tracks_counted() == 80);
        let middle = point(px(WIDE * 0.6), px(TALL * 0.5));
        driven.scroll_at(middle, 1_000.0);
        driven.scroll_at(middle, 37.0);
        let left_at = driven.read(|root, _| root.track_rows.0.borrow().base_handle.offset().y);
        assert!(left_at < px(-500.0), "the list did not scroll: {left_at:?}");

        let root = driven.root.clone();
        driven.cx.update(|_, cx| {
            root.update(cx, |root, cx| {
                root.opened(crate::Selection::Album(album), cx);
            });
        });
        driven.settle();
        driven
            .cx
            .update(|_, cx| root.update(cx, |root, cx| root.go_back(cx)));
        driven.until(|root, _| root.landing_on.is_none());
        driven.settle();

        let landed = driven.read(|root, _| root.track_rows.0.borrow().base_handle.offset().y);
        assert!(
            (f32::from(landed) - f32::from(left_at)).abs() < 1.0,
            "the way back landed at {landed:?} where the list was left at {left_at:?}"
        );
    }

    struct Measuring {
        source: resonate_core::SourceId,
    }

    impl resonate_eq::Corrections for Measuring {
        fn source(&self) -> &resonate_core::SourceId {
            &self.source
        }

        fn catalogue(&self) -> resonate_eq::Result<Arc<resonate_eq::Catalogue>> {
            Ok(Arc::new(resonate_eq::Catalogue::empty()))
        }

        fn profile(
            &self,
            _: &resonate_eq::DeviceId,
        ) -> resonate_eq::Result<Option<resonate_core::eq::Profile>> {
            Ok(None)
        }
    }

    fn texts(driven: &mut Driven) -> (String, String, String) {
        driven.read(|root, cx| {
            (
                root.search.read(cx).text().to_owned(),
                root.listenbrainz.read(cx).text().to_owned(),
                root.looking.read(cx).text().to_owned(),
            )
        })
    }

    #[gpui::test]
    fn what_is_typed_into_the_listenbrainz_token_stays_out_of_the_library_search(
        cx: &mut TestAppContext,
    ) {
        let mut driven = Driven::open(cx, catalog());
        driven.click("tab-settings");
        driven.click("category-Online");
        driven.focus(|root| &root.listenbrainz);

        driven.cx.simulate_input("token");
        driven.settle();

        let (searched, token, _) = texts(&mut driven);
        assert_eq!(token, "token");
        assert_eq!(searched, "", "the token was typed into the search as well");
    }

    #[gpui::test]
    fn escape_in_the_autoeq_search_clears_it_and_leaves_the_pane_where_it_was(
        cx: &mut TestAppContext,
    ) {
        let measuring = Measuring {
            source: resonate_core::SourceId::new("measured").expect("a usable source name"),
        };
        let mut driven = Driven::corrected_by(
            cx,
            catalog(),
            &Folder::new(),
            Corrected::uncorrected().and(Arc::new(measuring)),
        );
        driven.click("tab-settings");
        driven.click("category-Equaliser");
        driven.focus(|root| &root.looking);
        driven.cx.simulate_input("hd 650");
        driven.settle();
        let before = texts(&mut driven);

        driven.cx.simulate_keystrokes("escape");
        driven.settle();

        let (searched, _, looked_for) = texts(&mut driven);
        let pane = driven.read(|root, _| root.pane);
        assert_eq!(before.2, "hd 650");
        assert_eq!(looked_for, "", "escape left the AutoEq search as it was");
        assert_eq!(searched, "");
        assert_eq!(
            pane,
            Pane::Settings,
            "escape stepped back out of the settings"
        );
    }

    #[gpui::test]
    fn a_shorter_history_is_armed_by_the_first_press_and_kept_by_the_second(
        cx: &mut TestAppContext,
    ) {
        let six_months = resonate_library::HistoryKept::for_days(182).expect("a span of days");
        let kept = |driven: &mut Driven| {
            driven.read(|root, cx| {
                (
                    cx.global::<ResonateApp>().history_kept,
                    root.aging_the_history,
                )
            })
        };
        let mut driven = Driven::open(cx, catalog());
        driven.click("tab-settings");
        driven.focus(|root| &root.finding);
        driven.cx.simulate_input("listened");
        driven.settle();

        driven.click("history-kept-4");

        assert_eq!(
            kept(&mut driven),
            (resonate_library::HistoryKept::Forever, Some(six_months)),
            "the first press forgot listens rather than asking again"
        );

        driven.click("history-kept-4");

        assert_eq!(kept(&mut driven), (six_months, None));

        driven.click("history-kept-0");

        assert_eq!(
            kept(&mut driven),
            (resonate_library::HistoryKept::Forever, None),
            "keeping more was asked about twice"
        );
    }

    #[gpui::test]
    fn the_wheel_scrolls_the_settings_body(cx: &mut TestAppContext) {
        let mut driven = Driven::open(cx, catalog());
        driven.click("tab-settings");
        let before = driven.read(|root, _| root.settings_scroll.offset().y);

        driven.scroll("settings", 400.0);

        let after = driven.read(|root, _| root.settings_scroll.offset().y);
        assert!(
            after < before,
            "the body did not scroll: {before:?} to {after:?}"
        );
    }

    #[gpui::test]
    fn a_press_on_the_listen_button_opens_the_sheet_and_its_chips_choose_a_microphone(
        cx: &mut TestAppContext,
    ) {
        let mut driven = Driven::open(cx, catalog());
        assert!(!driven.read(|root, _| root.listening_open));

        driven.click("listen");
        assert!(driven.read(|root, _| root.listening_open));

        driven.click("listen-microphone");
        assert_eq!(
            driven.read(|root, cx| root.listen.read(cx).from().clone()),
            Listening::Microphone(None)
        );

        let listen = driven.read(|root, _| root.listen.clone());
        driven.cx.update(|_, cx| {
            listen.update(cx, |listen, cx| {
                listen.hearing_of(vec![
                    ("alsa_input.desk".to_owned(), "Desk microphone".to_owned()),
                    ("alsa_input.headset".to_owned(), "Headset".to_owned()),
                ]);
                cx.notify();
            });
        });
        driven.click("listen-microphone-choice-1");
        assert_eq!(
            driven.read(|root, cx| root.listen.read(cx).from().clone()),
            Listening::Microphone(Some(resonate_engine::NodeName::new("alsa_input.headset")))
        );

        driven.click("listen-desktop");
        assert_eq!(
            driven.read(|root, cx| root.listen.read(cx).from().clone()),
            Listening::Desktop
        );
    }
}
