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
    Entity, Modifiers, MouseButton, Pixels, Point, ScrollDelta, ScrollEvent, ScrollSource,
    TestAppContext, TouchPhase, VisualTestContext, point, px, size,
};
use resonate_core::{
    Appearance, Frames, Gain, MediaLocation, QueueStamp, Resumable, Resumption, SpeakerAngle,
    SpeakerElevation, TrackId, ZoneId, ZoneName,
};
use resonate_engine::{
    AudioSource, Backend, Command, EngineConfig, Opener, Opening, Opens, Player, SinkChange,
    SinkError, SinkId, SinkInfo, SinkResult, SinkStream, StreamRequest, Surveyor, Zones,
};
use resonate_eq::Corrected;
use resonate_library::{Fingerprinters, Library, Playing, Reference, ScanOptions};
use resonate_listen::{Listener, Listening, Recognisers};
use resonate_lyrics::Lyricists;
use resonate_providers::Providers;

use crate::{
    AppIcon, Bindings, Consulted, Ephemeral, Launcher, Listens, Measurers, Online, Places, Present,
    ResonateApp, RootView, SettingsCategory, Sourcing, Tabs, WindowButtons, app,
    drawing::Drawer,
    settings::{Registering, Supplying},
    theme,
    views::dropping::Copying,
};

pub(crate) const WIDE: f32 = 1_400.0;
pub(crate) const TALL: f32 = 1_600.0;
const FRAME: Duration = Duration::from_millis(16);
const PATIENCE: Duration = Duration::from_secs(10);
const RATE: u32 = 44_100;
const CHANNELS: u16 = 2;

pub(crate) struct Reaching {
    pub(crate) reference: Arc<dyn Reference>,
    pub(crate) register: Registering,
}

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
    sinks: Vec<SinkInfo>,
}

struct Listed(Vec<SinkInfo>);

impl Surveyor for Listed {
    fn enumerate_sinks(&self, _timeout: Duration) -> SinkResult<Vec<SinkInfo>> {
        Ok(self.0.clone())
    }
}

impl Opener for Listed {
    fn open(&self, _: &StreamRequest, _: Box<dyn AudioSource>) -> SinkResult<SinkStream> {
        Err(SinkError::NoSink)
    }
}

impl Backend for Unplugged {
    fn subscribe_sinks(&self) -> Receiver<SinkChange> {
        self.changes.clone()
    }

    fn surveyor(&self) -> Arc<dyn Surveyor> {
        Arc::new(Listed(self.sinks.clone()))
    }

    fn opener(&self) -> Arc<dyn Opener> {
        Arc::new(Listed(self.sinks.clone()))
    }

    fn set_device_volume(&self, _: SinkId, _: Gain) -> SinkResult<()> {
        Ok(())
    }

    fn set_device_mute(&self, _: SinkId, _: bool) -> SinkResult<()> {
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

pub(crate) fn unplugged_player() -> Arc<Player> {
    player_over(Vec::new())
}

fn player_over(sinks: Vec<SinkInfo>) -> Arc<Player> {
    let (announcing, changes) = unbounded();
    Arc::new(
        Player::with_backend(EngineConfig::default(), move |_| {
            Ok(Box::new(Unplugged {
                changes,
                _announcing: announcing,
                sinks,
            }))
        })
        .expect("an engine over no graph starts"),
    )
}

struct OverTheSame(Vec<SinkInfo>);

impl Opens for OverTheSame {
    fn open(&self, _: &Opening, _: &Player) -> resonate_engine::Result<Arc<Player>> {
        Ok(player_over(self.0.clone()))
    }
}

pub(crate) fn zones_over(sinks: Vec<SinkInfo>) -> Arc<Zones> {
    Zones::start(
        player_over(sinks.clone()),
        ZoneName::first_given_to(ZoneId::FIRST),
        Arc::new(OverTheSame(sinks)),
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
        Self::built(
            cx,
            library,
            folder,
            corrections,
            None,
            None,
            Lyricists::unsourced(),
        )
    }

    pub(crate) fn reaching(
        cx: &mut TestAppContext,
        library: Arc<Library>,
        folder: &Folder,
        reaching: Reaching,
    ) -> Self {
        Self::built(
            cx,
            library,
            folder,
            Corrected::uncorrected(),
            Some(reaching),
            None,
            Lyricists::unsourced(),
        )
    }

    pub(crate) fn singing(cx: &mut TestAppContext, library: Arc<Library>, folder: &Folder) -> Self {
        Self::built(
            cx,
            library,
            folder,
            Corrected::uncorrected(),
            None,
            None,
            Lyricists::local(),
        )
    }

    pub(crate) fn recognising(
        cx: &mut TestAppContext,
        library: Arc<Library>,
        folder: &Folder,
        fingerprinters: Fingerprinters,
    ) -> Self {
        Self::built(
            cx,
            library,
            folder,
            Corrected::uncorrected(),
            None,
            Some(Arc::new(fingerprinters)),
            Lyricists::unsourced(),
        )
    }

    pub(crate) fn hearing(
        cx: &mut TestAppContext,
        library: Arc<Library>,
        sinks: Vec<SinkInfo>,
    ) -> Self {
        Self::built_over(
            cx,
            library,
            &Folder::new(),
            Corrected::uncorrected(),
            None,
            None,
            Lyricists::unsourced(),
            sinks,
        )
    }

    fn built(
        cx: &mut TestAppContext,
        library: Arc<Library>,
        folder: &Folder,
        corrections: Corrected,
        reaching: Option<Reaching>,
        recognising: Option<Arc<Fingerprinters>>,
        lyricists: Lyricists,
    ) -> Self {
        Self::built_over(
            cx,
            library,
            folder,
            corrections,
            reaching,
            recognising,
            lyricists,
            Vec::new(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn built_over(
        cx: &mut TestAppContext,
        library: Arc<Library>,
        folder: &Folder,
        corrections: Corrected,
        reaching: Option<Reaching>,
        recognising: Option<Arc<Fingerprinters>>,
        lyricists: Lyricists,
        sinks: Vec<SinkInfo>,
    ) -> Self {
        cx.executor().allow_parking();

        let (reference, register) = match reaching {
            Some(Reaching {
                reference,
                register,
            }) => (Some(reference), register),
            None => {
                let register: Registering = Arc::new(|_: &Supplying<'_>| Providers::none());
                (None, register)
            }
        };
        let online = Online {
            enabled: reference.is_some() || recognising.is_some(),
            ..Online::default()
        };
        let fingerprinters = recognising.unwrap_or_else(|| Arc::new(Fingerprinters::none()));
        theme::wear(Appearance::default());
        let (attention, _) = unbounded();
        let unseen = Arc::new(Unseen);
        cx.update(|cx| {
            cx.set_global(Drawer::new());
            cx.set_global(ResonateApp {
                zones: zones_over(sinks),
                library,
                lyricists: Arc::new(lyricists),
                fingerprinters: Arc::clone(&fingerprinters),
                corrections: Arc::new(corrections),
                settings: Arc::new(Ephemeral),
                online,
                bindings: Bindings::default(),
                measurers: Measurers::default(),
                measures: None,
                for_the_pass: Consulted {
                    reference: reference.clone(),
                    fingerprinters,
                },
                reference,
                scrobblers: None,
                signs_in: None,
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
                lyrics_by_the_locale: Arc::new(AtomicBool::new(false)),
                convolution: None,
                speaker_angle: SpeakerAngle::STANDARD,
                speaker_elevation: SpeakerElevation::LEVEL,
                music_folder: None,
                file_dropped: true,
                window_buttons: WindowButtons::SHOWN,
                scroll_volume: true,
                mouse_navigation: true,
                reduced_motion: false,
                caret: crate::CaretBlink::as_built(),
                scrollbars: resonate_core::ScrollbarMode::default(),
                spectral: resonate_core::Spectral::default(),
                tabs: Tabs::AS_BUILT,
                remember_tab: false,
                last_tab: None,
                artists_drawn: resonate_core::ArtistsDrawn::default(),
                remember_window_size: false,
                window_size: None,
                remember_settings_category: false,
                last_settings_category: SettingsCategory::default(),
                presence: resonate_core::Presence::default(),
                present: unseen.clone(),
                synchronised: crate::Synchronised::default(),
                syncing: None,
                reads_as_a_code: None,
                launcher: unseen,
                sourcing: Sourcing {
                    inbox: None,
                    register,
                    unkept: Arc::new(|| false),
                },
                listens: nobody_listening(),
                first_read: None,
                copying: Copying::default(),
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
            .update(|_, cx| cx.global::<ResonateApp>().zones.first().player);
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
        self.cx.simulate_event(ScrollEvent {
            position: at,
            delta: ScrollDelta::Pixels(point(px(0.0), px(-down))),
            modifiers: Modifiers::none(),
            touch_phase: TouchPhase::Moved,
            source: ScrollSource::Pointer,
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

    pub(crate) fn click_with(&mut self, selector: &'static str, modifiers: Modifiers) {
        let at = self.centre_of(selector);
        self.hover(at);
        self.cx.simulate_click(at, modifiers);
        self.settle();
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
            let focus = gpui::Focusable::focus_handle(field(root.read(cx)).read(cx), cx);
            focus.focus(window, cx);
        });
        self.settle();
    }
}

#[cfg(test)]
mod tests {
    use gpui::BorrowAppContext as _;
    use resonate_core::Span;
    use resonate_engine::Placement;
    use resonate_library::{Direction, Maker, SortOrder};

    use super::*;
    use crate::{
        Pane, Selection,
        equaliser::{Cell as BandCell, Editing},
        views::{
            menu::{self, Menu},
            reorder::{Listed, Shift},
            settings::curve::Plot,
            visualiser::Showing,
        },
    };

    fn catalog() -> Arc<Library> {
        Arc::new(Library::open_in_memory().expect("a catalog in memory"))
    }

    #[gpui::test]
    fn an_edit_a_model_made_beside_the_window_is_taken_onto_its_undo_and_put_back_there(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let database = folder.path().join("library.db");
        let window = Arc::new(Library::open(&database).expect("a catalog on disc"));
        let model = Library::open(&database).expect("a second handle on the catalog");
        model.offer_edits_as_a_model();
        let mut driven = Driven::opened_in(cx, window, &folder);

        let evening = model
            .create_playlist("Evening")
            .expect("the model starts a playlist");
        driven.root.update(&mut driven.cx, |root, cx| {
            root.library
                .update(cx, |library, cx| library.take_what_a_model_offered(cx));
        });
        driven.until(|root, cx| {
            root.library
                .read(cx)
                .undoable()
                .is_some_and(|step| step.made_by == Maker::AModel && step.playlist == evening)
        });

        driven
            .root
            .update(&mut driven.cx, |root, cx| root.undo_edit(cx));
        driven.until(|_, _| {
            model
                .playlist(evening)
                .is_ok_and(|standing| standing.is_none())
        });
        assert!(
            driven.read(|root, cx| root.library.read(cx).undoable().is_none()),
            "the model's edit was still offered once put back"
        );
    }

    #[gpui::test]
    fn a_press_on_a_sidebar_tab_opens_its_pane(cx: &mut TestAppContext) {
        let mut driven = Driven::open(cx, catalog());
        assert_eq!(driven.read(|root, _| root.pane), Pane::default());

        driven.click("tab-visualiser");

        assert_eq!(driven.read(|root, _| root.pane), Pane::Visualiser);
    }

    #[gpui::test]
    fn the_signal_path_is_as_wide_as_what_it_says_rather_than_the_panel(cx: &mut TestAppContext) {
        let folder = Folder::new();
        let mut driven = Driven::open(cx, catalog());
        driven.play(&[folder.tone("tone.wav", 2)]);

        let path = driven.bounds_of("signal-path");
        let room = driven.read(|root, _| root.playing_room.get());

        assert!(path.size.width > px(0.0));
        assert!(
            path.size.width < room / 2.0,
            "the signal path took {:?} of {room:?}",
            path.size.width
        );
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
    fn bindings_and_a_tidal_account_changed_from_the_command_line_are_followed_by_the_window(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let mut driven = Driven::opened_in(cx, catalog(), &folder);
        let bindings = crate::Bindings {
            enabled: true,
            fallback: Some(resonate_eq::Binding::Own),
            by_sink: Vec::new(),
        };
        let account = crate::TidalSignIn {
            client_id: "client".to_owned(),
            client_secret: "secret".to_owned(),
            refresh_token: "token".to_owned(),
        };

        driven.root.update(&mut driven.cx, |root, cx| {
            root.follow(crate::Followed::Bindings(bindings.clone()), cx);
            root.follow(crate::Followed::Tidal(account), cx);
        });
        driven.settle();

        assert_eq!(
            driven.read(|root, cx| root.equaliser.read(cx).bindings().clone()),
            bindings
        );
        let online = driven.read(|_, cx| cx.global::<ResonateApp>().online.clone());
        assert_eq!(online.tidal_client_id, "client");
        assert_eq!(online.tidal_client_secret, "secret");
        assert_eq!(online.tidal_refresh_token, "token");
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

    fn with_a_band_at(driven: &mut Driven, hertz: f64) {
        driven.click("tab-settings");
        driven.click("category-Equaliser");
        let curve = driven.bounds_of("equaliser-curve");
        driven.click_at(point(
            curve.origin.x + curve.size.width * 0.3,
            curve.origin.y + curve.size.height * 0.3,
        ));
        let taken = driven.root.update(&mut driven.cx, |root, cx| {
            let editing = Editing {
                row: 0,
                cell: BandCell::Frequency,
            };
            root.equaliser
                .update(cx, |model, cx| model.take(editing, &format!("{hertz}"), cx))
        });
        assert!(taken, "the band refused {hertz} Hz");
        driven.settle();
    }

    fn the_bands(driven: &mut Driven) -> Vec<resonate_core::eq::Band> {
        driven.read(|root, cx| root.equaliser.read(cx).shown_bands())
    }

    #[gpui::test]
    fn enter_on_an_equaliser_cell_nothing_was_typed_into_keeps_the_band_exactly(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let mut driven = Driven::opened_in(cx, catalog(), &folder);
        with_a_band_at(&mut driven, 1059.46);
        let before = the_bands(&mut driven);

        driven.click("band-frequency-0");
        driven.cx.simulate_keystrokes("enter");
        driven.settle();

        assert_eq!(the_bands(&mut driven), before);
        assert!(driven.read(|root, cx| root.equaliser.read(cx).editing().is_none()));
    }

    #[gpui::test]
    fn undo_in_an_equaliser_cell_cannot_reach_the_value_of_the_cell_before(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let mut driven = Driven::opened_in(cx, catalog(), &folder);
        with_a_band_at(&mut driven, 1059.46);
        driven.click("band-frequency-0");
        driven.cx.simulate_keystrokes("enter");
        driven.settle();

        driven.click("band-gain-0");
        driven.cx.simulate_keystrokes("ctrl-z");
        driven.settle();

        let (held, written) = driven.read(|root, cx| {
            let editing = Editing {
                row: 0,
                cell: BandCell::Gain,
            };
            (
                root.figure.read(cx).text().to_owned(),
                root.equaliser.read(cx).written(editing),
            )
        });
        assert_eq!(held, written);
    }

    #[gpui::test]
    fn tab_out_of_an_equaliser_cell_takes_what_was_typed_and_closes_it(cx: &mut TestAppContext) {
        let folder = Folder::new();
        let mut driven = Driven::opened_in(cx, catalog(), &folder);
        with_a_band_at(&mut driven, 1059.46);

        driven.click("band-frequency-0");
        driven.cx.simulate_keystrokes("ctrl-a");
        driven.cx.simulate_input("2000");
        driven.cx.simulate_keystrokes("tab");
        driven.settle();

        let frequency = the_bands(&mut driven)[0].frequency;
        assert_eq!(
            frequency,
            resonate_core::eq::Frequency::from_hertz(2000.0).expect("a frequency")
        );
        assert!(
            driven.read(|root, cx| root.equaliser.read(cx).editing().is_none()),
            "the cell stayed open after tab left it"
        );
    }

    #[gpui::test]
    fn a_letter_typed_over_everything_in_a_field_is_undone_to_the_text_it_replaced(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let mut driven = Driven::opened_in(cx, catalog(), &folder);
        driven.focus(|root| &root.search);
        driven.cx.simulate_input("dark");
        driven.cx.simulate_keystrokes("ctrl-a");
        driven.cx.simulate_input("x");
        assert_eq!(
            driven.read(|root, cx| root.search.read(cx).text().to_owned()),
            "x"
        );

        driven.cx.simulate_keystrokes("ctrl-z");

        assert_eq!(
            driven.read(|root, cx| root.search.read(cx).text().to_owned()),
            "dark"
        );
    }

    fn named_by(name: &str) -> &'static str {
        Box::leak(name.to_owned().into_boxed_str())
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
    fn a_zone_added_is_shown_with_a_queue_of_its_own_and_switching_back_touches_neither(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let library = scanned_catalog(&folder, &["Echoes"]);
        let mut driven = Driven::opened_in(cx, library, &folder);
        let root = driven.root.clone();
        let second = ZoneId::new(2).expect("a second zone");
        let row = resonate_engine::QueueItem {
            id: TrackId::new(7).expect("a track id"),
            location: MediaLocation::local(folder.path().join("elsewhere.flac")),
            span: None,
        };

        driven
            .cx
            .update(|_, cx| root.update(cx, |root, cx| root.add_a_zone(cx)));
        driven.until(|root, _| root.zones.len() == 2);
        let shown_second = driven.read(|root, _| root.switcher.selected());
        driven.cx.update(|_, cx| {
            root.update(cx, |root, cx| {
                root.send(
                    Command::Insert {
                        items: vec![row.clone()],
                        at: Placement::Queued,
                        play: false,
                    },
                    cx,
                );
            });
        });
        driven.until(|root, cx| root.player.read(cx).queue().len() == 1);
        driven
            .cx
            .update(|_, cx| root.update(cx, |root, cx| root.show_zone(ZoneId::FIRST, cx)));
        driven.settle();
        let first_queue = driven.read(|root, cx| root.player.read(cx).queue().len());
        let label = driven.read(|root, _| root.switcher.label());
        let menu = driven.read(|root, _| root.zone_menu(Point::default()).labels());

        driven
            .cx
            .update(|_, cx| root.update(cx, |root, cx| root.remove_the_zone(second, cx)));
        driven.until(|_, cx| cx.global::<ResonateApp>().zones.count() == 1);

        assert_eq!(shown_second, second);
        assert_eq!(first_queue, 0, "a row queued in one zone landed in another");
        assert_eq!(label.as_deref(), Some("Zone 1"));
        assert_eq!(
            menu,
            vec!["Zone 1", "Zone 2", "", "Add a zone", "Rename this zone…"]
        );
        assert!(driven.read(|root, _| root.zones.len() == 1 && !root.switcher.several()));
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
    fn the_row_whose_menu_is_open_stays_lit_so_its_controls_do_not_vanish_under_the_scrim(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let library = scanned_catalog(&folder, &["Echoes", "Time"]);
        let tracks = library
            .tracks(&resonate_library::TrackQuery::default())
            .expect("the scanned tracks");
        let (first, second) = (tracks[0].id, tracks[1].id);
        let mut driven = Driven::opened_in(cx, library, &folder);
        driven.click("tab-tracks");
        driven.until(|root, cx| root.library.read(cx).tracks_counted() == 2);
        let lit = |driven: &mut Driven, id: resonate_core::TrackId| {
            driven.read(|_, cx| crate::views::menu::lit_by_its_menu(("track", id.get()), cx))
        };

        let before = lit(&mut driven, first);
        driven.right_click(named("track", first.get()));
        let while_open = (lit(&mut driven, first), lit(&mut driven, second));
        driven.click("menu-entry-2");
        let after = lit(&mut driven, first);

        assert!(!before);
        assert_eq!(while_open, (true, false));
        assert!(!after);
    }

    #[gpui::test]
    fn the_play_in_a_tracks_menu_plays_the_listing_from_it_as_enter_does(cx: &mut TestAppContext) {
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
        driven.click("menu-entry-0");

        driven.until(|root, cx| root.player.read(cx).queue().len() == 2);
    }

    #[gpui::test]
    fn the_play_in_a_playlists_menu_turns_shuffle_off_as_every_other_play_does(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let library = scanned_catalog(&folder, &["Echoes"]);
        let playlist = library.create_playlist("Meddle").expect("a playlist");
        let mut driven = Driven::opened_in(cx, library, &folder);
        driven.root.update(&mut driven.cx, |root, cx| {
            root.send(Command::SetShuffle(true), cx);
        });
        driven.until(|root, cx| root.player.read(cx).state().shuffle);

        driven.root.update(&mut driven.cx, |root, cx| {
            let menu = crate::views::playlists::playlist_menu(
                point(px(200.0), px(200.0)),
                playlist,
                false,
                cx,
            );
            root.open_a_menu(menu, cx);
        });
        driven.click("menu-entry-0");

        driven.until(|root, cx| !root.player.read(cx).state().shuffle);
    }

    fn playlist_holding_the_first_song(library: &Library) -> resonate_core::PlaylistId {
        let track = library
            .tracks(&resonate_library::TrackQuery::default())
            .expect("the scanned tracks")
            .remove(0);
        let playlist = library.create_playlist("Meddle").expect("a playlist");
        library
            .add_to_playlist(playlist, &[resonate_library::Cut::of(&track)])
            .expect("a row put in");
        playlist
    }

    #[gpui::test]
    fn the_play_in_a_playlists_menu_queues_what_the_playlist_holds(cx: &mut TestAppContext) {
        let folder = Folder::new();
        let library = scanned_catalog(&folder, &["Echoes"]);
        let playlist = playlist_holding_the_first_song(&library);
        let mut driven = Driven::opened_in(cx, library, &folder);

        driven.root.update(&mut driven.cx, |root, cx| {
            let menu = crate::views::playlists::playlist_menu(
                point(px(200.0), px(200.0)),
                playlist,
                false,
                cx,
            );
            root.open_a_menu(menu, cx);
        });
        driven.click("menu-entry-0");

        driven.until(|root, cx| root.player.read(cx).queue().len() == 1);
    }

    #[gpui::test]
    fn a_playlist_that_is_not_opened_is_read_off_the_windows_thread_and_an_opened_one_is_at_hand(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let library = scanned_catalog(&folder, &["Echoes"]);
        let playlist = playlist_holding_the_first_song(&library);
        let mut driven = Driven::opened_in(cx, library, &folder);
        let seen = std::rc::Rc::new(std::cell::Cell::new(None));
        let ask = |driven: &mut Driven| {
            let seen = std::rc::Rc::clone(&seen);
            driven.root.update(&mut driven.cx, |root, cx| {
                root.with_playlist_entries(playlist, cx, move |_, entries, _| {
                    seen.set(Some(entries.len()));
                });
            });
        };

        ask(&mut driven);
        assert_eq!(
            seen.get(),
            None,
            "the playlist was read on the window's thread"
        );
        let waiting = std::rc::Rc::clone(&seen);
        driven.until(move |_, _| waiting.get() == Some(1));

        seen.set(None);
        driven.root.update(&mut driven.cx, |root, cx| {
            root.set_pane(Pane::Playlists, cx);
            root.show_playlist(Some(playlist), cx);
        });
        driven.until(|root, cx| root.library.read(cx).opened() == Some(playlist));
        ask(&mut driven);

        assert_eq!(seen.get(), Some(1), "an opened playlist was not at hand");
    }

    #[gpui::test]
    fn an_album_played_from_its_menu_is_queued_whole_whatever_the_search_listed(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        folder.tagged("echoes.wav", 1, &[(b"INAM", "Echoes"), (b"IPRD", "Meddle")]);
        folder.tagged("one.wav", 1, &[(b"INAM", "One"), (b"IPRD", "Meddle")]);
        let library = catalog();
        Driven::scanned(&library, &folder);
        let album = library
            .albums(&resonate_library::AlbumQuery::default())
            .expect("the albums read")[0]
            .id;
        let mut driven = Driven::opened_in(cx, library, &folder);
        driven.click("tab-tracks");
        driven.until(|root, cx| root.library.read(cx).tracks_counted() == 2);
        driven.focus(|root| &root.search);
        driven.cx.simulate_input("echoes");
        driven.until(|root, cx| root.library.read(cx).tracks_counted() == 1);

        let root = driven.root.clone();
        driven.cx.update(|window, cx| {
            root.update(cx, |root, cx| {
                root.plays_everything_in(
                    Selection::Album(album),
                    Placement::Queued,
                    true,
                    window,
                    cx,
                );
            });
        });

        driven.until(|root, cx| root.player.read(cx).queue().len() == 2);
    }

    #[gpui::test]
    fn showing_another_playlist_forgets_the_order_kept_for_the_one_before(cx: &mut TestAppContext) {
        let folder = Folder::new();
        let library = scanned_catalog(&folder, &["Echoes"]);
        let kept = library.create_playlist("Kept").expect("a playlist");
        let other = library.create_playlist("Other").expect("a playlist");
        let mut driven = Driven::opened_in(cx, library, &folder);

        driven.root.update(&mut driven.cx, |root, cx| {
            root.show_playlist(Some(kept), cx);
            root.keep_in_order(kept, true, cx);
        });
        driven.settle();

        assert!(driven.read(|root, _| root.keeping));

        driven.root.update(&mut driven.cx, |root, cx| {
            root.show_playlist(Some(other), cx);
        });
        driven.settle();

        assert!(!driven.read(|root, _| root.keeping));
    }

    #[gpui::test]
    fn the_menu_key_opens_the_reached_rows_menu_and_the_arrows_and_enter_press_an_entry(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let library = scanned_catalog(&folder, &["Echoes", "Time"]);
        let mut driven = Driven::opened_in(cx, library, &folder);
        driven.click("tab-tracks");
        driven.until(|root, cx| root.library.read(cx).tracks_counted() == 2);

        driven.cx.simulate_keystrokes("shift-f10");
        assert!(
            driven.read(|root, _| root.menu.is_none()),
            "a menu opened with no row reached"
        );

        driven.cx.simulate_keystrokes("down menu");
        assert!(
            driven.read(|root, _| root.menu.is_some()),
            "the menu key opened no menu on the reached row"
        );

        driven.cx.simulate_keystrokes("down down enter");
        assert!(
            driven.read(|root, _| root.menu.is_none()),
            "the menu stayed open"
        );
        driven.until(|root, cx| root.player.read(cx).queue().len() == 1);

        driven.cx.simulate_keystrokes("shift-f10");
        assert!(
            driven.read(|root, _| root.menu.is_some()),
            "shift-f10 opened no menu on the reached row"
        );
    }

    fn tabbed_onto(driven: &mut Driven, named: &'static str) -> bool {
        const TAB_STOPS_AT_MOST: usize = 64;
        let root = driven.root.clone();
        let on_it = |driven: &mut Driven| {
            driven.cx.update(|window, cx| {
                root.read(cx)
                    .standing_controls
                    .at(named, cx)
                    .is_focused(window)
            })
        };
        (0..TAB_STOPS_AT_MOST).any(|_| {
            driven.cx.simulate_keystrokes("tab");
            on_it(driven)
        })
    }

    fn tabbed_onto_the_pane(driven: &mut Driven, named: &'static str) -> bool {
        const TAB_STOPS_AT_MOST: usize = 64;
        let root = driven.root.clone();
        let on_it = |driven: &mut Driven| {
            driven
                .cx
                .update(|window, cx| root.read(cx).controls.at(named, cx).is_focused(window))
        };
        (0..TAB_STOPS_AT_MOST).any(|_| {
            driven.cx.simulate_keystrokes("tab");
            on_it(driven)
        })
    }

    #[gpui::test]
    fn tab_reaches_a_headings_play_and_a_rows_star_and_enter_presses_them(cx: &mut TestAppContext) {
        let folder = Folder::new();
        let library = scanned_catalog(&folder, &["Echoes"]);
        let track = library
            .tracks(&resonate_library::TrackQuery::default())
            .expect("the scanned tracks")[0]
            .id;
        let mut driven = Driven::opened_in(cx, library, &folder);
        driven.click("tab-tracks");
        driven.until(|root, cx| root.library.read(cx).tracks_counted() == 1);

        assert!(
            tabbed_onto_the_pane(&mut driven, "play-all"),
            "tab never reached the heading's Play"
        );
        driven.cx.simulate_keystrokes("enter");
        driven.until(|root, cx| root.player.read(cx).queue().len() == 1);

        assert!(
            tabbed_onto_the_pane(&mut driven, "track-favourite-0"),
            "tab never reached the row's star"
        );
        driven.cx.simulate_keystrokes("enter");
        driven.until(|root, cx| {
            let library = root.library.read(cx);
            library
                .tracks()
                .iter()
                .any(|row| row.id == track && library.favours_track(row))
        });
    }

    #[gpui::test]
    fn a_mark_opening_a_menu_where_pressed_opens_it_from_the_keyboard_too(cx: &mut TestAppContext) {
        let folder = Folder::new();
        let library = scanned_catalog(&folder, &["Echoes"]);
        let playlist = library.create_playlist("Meddle").expect("a playlist");
        let mut driven = Driven::opened_in(cx, library, &folder);
        let root = driven.root.clone();
        driven.cx.update(|_, cx| {
            root.update(cx, |root, cx| {
                root.set_pane(Pane::Playlists, cx);
                root.show_playlist(Some(playlist), cx);
            });
        });
        driven.until(|root, cx| root.library.read(cx).opened() == Some(playlist));

        assert!(
            tabbed_onto_the_pane(&mut driven, "more-of-the-playlist"),
            "tab never reached the playlist's menu mark"
        );
        driven.cx.simulate_keystrokes("enter");
        driven.settle();
        assert!(
            driven.read(|root, _| root.menu.is_some()),
            "enter opened no menu"
        );
    }

    fn pressed_and_released(driven: &mut Driven, key: &str) {
        let keystroke = gpui::Keystroke::parse(key).expect("a key to press");
        driven.cx.simulate_keystrokes(key);
        driven.cx.simulate_event(gpui::KeyUpEvent { keystroke });
        driven.settle();
    }

    #[gpui::test]
    fn enter_pressed_and_released_on_next_skips_one_song_not_two(cx: &mut TestAppContext) {
        let folder = Folder::new();
        let files = [
            folder.tone("one.wav", 5),
            folder.tone("two.wav", 5),
            folder.tone("three.wav", 5),
        ];
        let mut driven = Driven::opened_in(cx, catalog(), &folder);
        driven.play(&files);
        let queue = driven.queue();

        assert!(
            tabbed_onto(&mut driven, "next"),
            "tab never reached the next button"
        );
        pressed_and_released(&mut driven, "enter");
        driven.until(|root, cx| {
            root.player
                .read(cx)
                .state()
                .current
                .is_some_and(|current| current.id != queue[0])
        });

        let current = driven.read(|root, cx| root.player.read(cx).state().current.map(|c| c.id));
        assert_eq!(current, Some(queue[1]), "one press of next skipped two");
    }

    #[gpui::test]
    fn a_rail_holding_the_caret_moves_with_the_arrows(cx: &mut TestAppContext) {
        let folder = Folder::new();
        let file = folder.tone("tone.wav", 1);
        let mut driven = Driven::opened_in(cx, catalog(), &folder);
        driven.play(&[file]);
        let before = driven.read(|root, cx| root.player.read(cx).state().volume);

        assert!(
            tabbed_onto(&mut driven, "volume"),
            "tab never reached the volume rail"
        );
        driven.cx.simulate_keystrokes("left");
        driven.until(|root, cx| root.player.read(cx).state().volume < before);
    }

    #[gpui::test]
    fn tab_reaches_the_transport_buttons_and_the_sidebar_and_enter_presses_them(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let library = scanned_catalog(&folder, &["Echoes"]);
        let mut driven = Driven::opened_in(cx, library, &folder);
        driven.click("tab-tracks");
        driven.until(|root, cx| root.library.read(cx).tracks_counted() == 1);

        assert!(
            tabbed_onto(&mut driven, "queue"),
            "tab never reached the queue button"
        );
        driven.cx.simulate_keystrokes("enter");
        assert!(
            driven.read(|root, _| root.pane == Pane::Queue),
            "enter on the queue button did not open the queue"
        );

        assert!(
            tabbed_onto(&mut driven, Pane::Albums.label()),
            "tab never reached the sidebar's albums"
        );
        driven.cx.simulate_keystrokes("enter");
        assert!(
            driven.read(|root, _| root.pane == Pane::Albums),
            "enter on the sidebar's albums did not open them"
        );
    }

    #[gpui::test]
    fn tab_reaches_the_sleep_timer_and_enter_opens_its_menu_and_sets_a_timer(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let files = [folder.tone("one.wav", 5)];
        let mut driven = Driven::opened_in(cx, catalog(), &folder);
        driven.play(&files);

        assert!(
            tabbed_onto(&mut driven, "sleep"),
            "tab never reached the sleep timer"
        );
        driven.cx.simulate_keystrokes("enter");
        driven.settle();
        assert!(
            driven.read(|root, _| root.menu.is_some()),
            "enter opened no menu on the sleep timer"
        );

        driven.cx.simulate_keystrokes("down enter");
        driven.until(|root, cx| root.player.read(cx).state().sleeping.is_some());
        assert!(driven.read(|root, _| root.menu.is_none()));
    }

    #[gpui::test]
    fn tab_reaches_a_lyric_line_and_enter_opens_a_menu_to_copy_it_or_play_from_it(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let files = [folder.tone("one.wav", 5)];
        std::fs::write(
            folder.path().join("one.lrc"),
            "[00:00.50]first words\n[00:03.00]second words\n",
        )
        .expect("a sidecar beside the track");
        let mut driven = Driven::singing(cx, catalog(), &folder);
        driven.play(&files);
        let root = driven.root.clone();
        driven
            .cx
            .update(|_, cx| root.update(cx, |root, cx| root.set_pane(Pane::Lyrics, cx)));
        driven.until(|root, cx| root.lyrics.read(cx).text().len() == 2);

        assert!(
            tabbed_onto_the_pane(&mut driven, "lyric-1"),
            "tab never reached the second lyric line"
        );
        driven.cx.simulate_keystrokes("enter");
        driven.settle();
        assert!(
            driven.read(|root, _| root.menu.is_some()),
            "enter opened no menu on the lyric line"
        );

        let sought = driven.read(|root, cx| root.player.read(cx).state().seeks);

        driven.cx.simulate_keystrokes("down down down enter");
        driven.settle();
        assert!(driven.read(|root, _| root.menu.is_none()));
        driven.until(|root, cx| root.player.read(cx).state().seeks != sought);
    }

    #[gpui::test]
    fn a_lyric_line_clicked_seeks_without_keeping_the_caret_that_would_hold_it_lit(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let files = [folder.tone("one.wav", 5)];
        std::fs::write(
            folder.path().join("one.lrc"),
            "[00:00.50]first words\n[00:03.00]second words\n",
        )
        .expect("a sidecar beside the track");
        let mut driven = Driven::singing(cx, catalog(), &folder);
        driven.play(&files);
        let root = driven.root.clone();
        driven
            .cx
            .update(|_, cx| root.update(cx, |root, cx| root.set_pane(Pane::Lyrics, cx)));
        driven.until(|root, cx| root.lyrics.read(cx).text().len() == 2);
        driven.settle();
        let sought = driven.read(|root, cx| root.player.read(cx).state().seeks);

        driven.click("lyric-0");
        driven.until(|root, cx| root.player.read(cx).state().seeks != sought);

        let held = driven.cx.update(|window, cx| {
            let root = root.read(cx);
            (0..2).any(|line| {
                root.controls
                    .at(format!("lyric-{line}"), cx)
                    .is_focused(window)
            })
        });
        assert!(
            !held,
            "a clicked lyric line kept the caret and stays lit once sung"
        );
    }

    #[gpui::test]
    fn tabbing_down_an_unsynced_sheet_keeps_the_line_reached_in_view(cx: &mut TestAppContext) {
        const LINES: usize = 40;
        const TABS: usize = 90;
        let folder = Folder::new();
        let files = [folder.tone("one.wav", 5)];
        let sheet: String = (0..LINES).map(|n| format!("words of line {n}\n")).collect();
        std::fs::write(folder.path().join("one.lrc"), sheet).expect("a sidecar beside the track");
        let mut driven = Driven::singing(cx, catalog(), &folder);
        driven.play(&files);
        let root = driven.root.clone();
        driven
            .cx
            .update(|_, cx| root.update(cx, |root, cx| root.set_pane(Pane::Lyrics, cx)));
        driven.until(|root, cx| root.lyrics.read(cx).text().len() == LINES);
        driven.settle();

        let mut deepest = 0;
        for _ in 0..TABS {
            driven.cx.simulate_keystrokes("tab");
            driven.settle();

            let reached = driven.cx.update(|window, cx| {
                let root = root.read(cx);
                (0..LINES).find(|line| {
                    root.controls
                        .at(format!("lyric-{line}"), cx)
                        .is_focused(window)
                })
            });
            let Some(line) = reached else {
                continue;
            };
            deepest = deepest.max(line);

            let (seen, drawn) = driven.read(|root, cx| {
                let model = root.lyrics.read(cx);
                let pane = model.scroll().bounds();
                let drawn = model
                    .scroll()
                    .bounds_for_item(line)
                    .expect("a line is drawn");
                let top = drawn.top() + model.scroll().offset().y;
                let bottom = drawn.bottom() + model.scroll().offset().y;
                (pane, (top, bottom))
            });
            assert!(
                drawn.0 >= seen.top() && drawn.1 <= seen.bottom(),
                "line {line} was reached by tab but stands outside the pane"
            );
        }

        assert!(
            deepest > LINES / 2,
            "tab only got as far as line {deepest} of {LINES}"
        );
    }

    #[gpui::test]
    fn a_queued_track_is_taken_out_of_the_queue_from_its_menu_wherever_it_is_listed(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let library = scanned_catalog(&folder, &["Echoes", "Time"]);
        let mut driven = Driven::opened_in(cx, library, &folder);
        driven.click("tab-tracks");
        driven.until(|root, cx| root.library.read(cx).tracks_counted() == 2);
        let root = driven.root.clone();
        let (kept, taken) = driven.cx.update(|window, cx| {
            root.update(cx, |root, cx| {
                let listing = root.library.read(cx).listing();
                root.play_the_listing_from(&listing, 0, window, cx);
                (listing[0].id, listing[1].id)
            })
        });
        driven.until(|root, cx| root.player.read(cx).queue().len() == 2);

        driven.right_click(named("track", taken.get()));
        driven.click("menu-entry-3");
        driven.until(|root, cx| root.player.read(cx).queue().len() == 1);

        assert_eq!(driven.queue(), vec![kept]);
    }

    #[gpui::test]
    fn a_track_queued_twice_is_taken_out_of_both_places_at_once_and_put_back_in_two_steps(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let one = folder.tone("one.wav", 1);
        let two = folder.tone("two.wav", 1);
        let three = folder.tone("three.wav", 1);
        let mut driven = Driven::opened_in(cx, catalog(), &folder);
        driven.play(&[one, two.clone(), three, two.clone()]);
        let before = driven.queue();
        assert_eq!(before.len(), 4);

        let root = driven.root.clone();
        let location = MediaLocation::local(&two);
        driven.cx.update(|_, cx| {
            root.update(cx, |root, cx| {
                root.take_out_of_the_queue(&location, None, cx);
            })
        });
        driven.until(|root, cx| root.player.read(cx).queue().len() == 2);

        assert_eq!(driven.queue(), vec![before[0], before[2]]);

        for _ in 0..2 {
            let put = driven.cx.update(|_, cx| {
                root.update(cx, |root, cx| {
                    let queued = root.player.read(cx).queue().len();
                    root.put_the_queue_back(cx).then_some(queued)
                })
            });
            let queued = put.expect("a step was left to put back");
            driven.until(|root, cx| root.player.read(cx).queue().len() > queued);
        }

        let restored: Vec<TrackId> = driven.queue();
        assert_eq!(restored.len(), 4);
        assert_eq!(restored[0], before[0]);
        assert_eq!(restored[2], before[2]);
    }

    #[gpui::test]
    fn playing_a_track_row_takes_a_playlists_mark_off_the_queue(cx: &mut TestAppContext) {
        let folder = Folder::new();
        let library = scanned_catalog(&folder, &["Echoes", "Time"]);
        let playlist = library.create_playlist("Mix").expect("a playlist is made");
        let mut driven = Driven::opened_in(cx, Arc::clone(&library), &folder);
        driven.click("tab-tracks");
        driven.until(|root, cx| root.library.read(cx).tracks_counted() == 2);
        let root = driven.root.clone();
        let marked = QueueStamp::default();

        let held = driven.cx.update(|window, cx| {
            root.update(cx, |root, cx| {
                root.library.update(cx, |library, _| {
                    library.set_playing_playlist(Some(Playing {
                        playlist,
                        queue: marked,
                    }));
                });
                let before = root.library.read(cx).playing_playlist(marked);
                let listing = root.library.read(cx).listing();
                root.play_the_listing_from(&listing, 0, window, cx);
                (before, root.library.read(cx).playing_playlist(marked))
            })
        });

        assert_eq!(held, (Some(playlist), None));
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
    fn a_track_dragged_onto_the_queue_button_is_queued_and_an_album_onto_a_pinned_playlist_fills_it(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        folder.tagged("echoes.wav", 1, &[(b"INAM", "Echoes"), (b"IPRD", "Meddle")]);
        folder.tagged(
            "one.wav",
            1,
            &[(b"INAM", "One of These Days"), (b"IPRD", "Meddle")],
        );
        let library = catalog();
        Driven::scanned(&library, &folder);
        let tracks = library
            .tracks(&resonate_library::TrackQuery::default())
            .expect("the scanned tracks");
        let album = library
            .albums(&resonate_library::AlbumQuery::default())
            .expect("the albums read")[0]
            .id;
        let playlist = library.create_playlist("Drives").expect("a playlist");
        library
            .pin_playlist(playlist, true)
            .expect("the playlist pinned");
        let mut driven = Driven::opened_in(cx, Arc::clone(&library), &folder);
        driven.click("tab-tracks");
        driven.until(|root, cx| root.library.read(cx).tracks_counted() == 2);

        let from = driven.centre_of(named("track", tracks[1].id.get()));
        let onto = driven.centre_of("queue");
        driven.drag(from, onto);
        driven.until(|root, cx| root.player.read(cx).queue().len() == 1);
        assert_eq!(
            driven.read(|root, cx| root.player.read(cx).queue()[0].location.clone()),
            tracks[1].location
        );

        driven.click("tab-albums");
        let from = driven.centre_of(named("album", album.get()));
        let onto = driven.centre_of(named("pinned-playlist", playlist.get()));
        driven.drag(from, onto);
        let began = Instant::now();
        while library
            .playlist_entries(playlist, None)
            .expect("the playlist's rows")
            .len()
            < 2
        {
            assert!(began.elapsed() < PATIENCE, "the album never landed");
            driven.settle();
            thread::sleep(FRAME);
        }
    }

    #[gpui::test]
    fn a_track_dragged_out_of_the_reach_it_stands_in_carries_the_whole_reach(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let library = scanned_catalog(&folder, &["Echoes", "Money", "Time"]);
        let mut driven = Driven::opened_in(cx, library, &folder);
        driven.click("tab-tracks");
        driven.until(|root, cx| root.library.read(cx).tracks_counted() == 3);
        let listing = driven.read(|root, cx| root.library.read(cx).listing());

        driven.cx.simulate_keystrokes("down shift-down");
        driven.settle();
        let from = driven.centre_of(named("track", listing[0].id.get()));
        let onto = driven.centre_of("queue");
        driven.drag(from, onto);
        driven.until(|root, cx| root.player.read(cx).queue().len() == 2);

        let queued: Vec<_> = driven.read(|root, cx| {
            root.player
                .read(cx)
                .queue()
                .iter()
                .map(|item| item.location.clone())
                .collect()
        });
        assert_eq!(
            queued,
            [listing[0].location.clone(), listing[1].location.clone()]
        );
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

    fn with_music_folder(driven: &mut Driven, folder: Option<PathBuf>) {
        driven.cx.update(|_, cx| {
            cx.update_global::<ResonateApp, _>(|global, _| global.music_folder = folder);
        });
    }

    #[gpui::test]
    fn songs_dropped_on_the_window_are_copied_into_the_music_folder_and_join_the_library(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let music = folder.path().join("music");
        fs::create_dir_all(&music).expect("a writable temporary folder");
        let music = music.canonicalize().expect("a folder that is there");
        let from = Folder::new();
        let song = from.tagged("dropped.wav", 1, &[(b"INAM", "Dropped Song")]);
        let mut driven = Driven::opened_in(cx, catalog(), &folder);
        with_music_folder(&mut driven, Some(music.clone()));

        driven.root.update(&mut driven.cx, |root, cx| {
            root.dropped(vec![song.clone()], cx);
        });

        driven.until(|root, cx| root.library.read(cx).tracks_counted() == 1);
        assert!(
            music.join("dropped.wav").is_file(),
            "the song was not copied"
        );
        assert!(song.is_file(), "the original was taken away");
        assert!(driven.read(|root, _| root.taking_in.is_none()));
    }

    #[gpui::test]
    fn songs_dropped_with_filing_on_are_filed_by_the_layout_once_scanned(cx: &mut TestAppContext) {
        let folder = Folder::new();
        let music = folder.path().join("music");
        fs::create_dir_all(&music).expect("a writable temporary folder");
        let music = music.canonicalize().expect("a folder that is there");
        let from = Folder::new();
        let song = from.tagged(
            "dropped.wav",
            1,
            &[
                (b"INAM", "Echoes"),
                (b"IART", "Pink Floyd"),
                (b"IPRD", "Meddle"),
            ],
        );
        let cover = from.path().join("cover.jpg");
        fs::write(&cover, b"picture").expect("a writable temporary file");
        let mut driven = Driven::opened_in(cx, catalog(), &folder);
        with_music_folder(&mut driven, Some(music.clone()));
        driven.cx.update(|_, cx| {
            cx.update_global::<ResonateApp, _>(|global, _| {
                global.organise_as = "{artist}/{album}/{title}".to_owned();
            });
        });

        driven.root.update(&mut driven.cx, |root, cx| {
            root.dropped(vec![song, cover], cx);
        });

        let filed = music.join("Pink Floyd/Meddle");
        driven.until(|_, _| filed.join("Echoes.wav").is_file());
        driven.until(|root, cx| !root.library.read(cx).is_busy());
        assert!(!music.join("dropped.wav").exists());
        assert_eq!(
            fs::read(filed.join("cover.jpg")).expect("the cover followed its song"),
            b"picture"
        );
    }

    #[gpui::test]
    fn a_drop_with_no_music_folder_chosen_copies_nothing_and_opens_the_setting(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let from = Folder::new();
        let song = from.tone("dropped.wav", 1);
        let mut driven = Driven::opened_in(cx, catalog(), &folder);

        driven.root.update(&mut driven.cx, |root, cx| {
            root.dropped(vec![song], cx);
        });
        driven.settle();

        assert_eq!(driven.read(|root, _| root.pane), Pane::Settings);
        assert_eq!(
            driven.read(|root, _| root.settings_category),
            SettingsCategory::Library
        );
        assert!(driven.read(|root, _| root.taking_in.is_none()));
    }

    #[gpui::test]
    fn the_sidebar_scrolls_only_once_the_window_is_too_short_for_its_tabs(cx: &mut TestAppContext) {
        let mut driven = Driven::open(cx, catalog());
        driven.settle();

        let roomy = driven.read(|root, _| root.sidebar_scroll.max_offset().y);

        driven.cx.simulate_resize(size(px(WIDE), px(300.0)));
        driven.settle();

        let squeezed = driven.read(|root, _| root.sidebar_scroll.max_offset().y);

        let settings = driven.bounds_of("tab-settings");

        assert_eq!(f32::from(roomy), 0.0);
        assert!(f32::from(squeezed) > 0.0);
        assert!(
            settings.bottom() <= px(300.0),
            "the Settings tab fell below the window: {settings:?}"
        );
    }

    #[gpui::test]
    fn a_drag_over_the_window_is_weighed_and_drawn_until_it_leaves(cx: &mut TestAppContext) {
        let folder = Folder::new();
        let from = Folder::new();
        let song = from.tone("held.wav", 1);
        let note = from.path().join("notes.txt");
        fs::write(&note, b"words").expect("a writable temporary file");
        let mut driven = Driven::opened_in(cx, catalog(), &folder);
        with_music_folder(&mut driven, Some(folder.path().to_path_buf()));

        driven.root.update(&mut driven.cx, |root, cx| {
            root.dragged_over(&[song, note], cx);
        });
        driven.settle();

        assert!(driven.read(|root, _| root.incoming.is_some()));

        driven.until(|root, _| root.incoming.is_none());
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

    fn a_png() -> Vec<u8> {
        let mut bytes = Vec::new();
        image::RgbaImage::from_pixel(8, 8, image::Rgba([200, 40, 40, 255]))
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .expect("a picture in memory");
        bytes
    }

    #[gpui::test]
    fn a_cover_found_after_it_was_first_drawn_is_drawn_once_the_catalog_reloads(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        folder.tagged("echoes.wav", 1, &[(b"INAM", "Echoes"), (b"IPRD", "Meddle")]);
        let library = catalog();
        Driven::scanned(&library, &folder);
        let album = library
            .albums(&resonate_library::AlbumQuery::default())
            .expect("the albums read")
            .first()
            .expect("the album scanned")
            .id;
        let mut driven = Driven::opened_in(cx, Arc::clone(&library), &folder);
        let drawn = |driven: &mut Driven| {
            driven.root.update(&mut driven.cx, |root, cx| {
                root.library
                    .update(cx, |model, cx| {
                        model.cover(album, crate::Drawn::InAGrid, cx)
                    })
                    .is_some()
            })
        };

        assert!(!drawn(&mut driven));
        driven.settle();
        assert!(!drawn(&mut driven), "an album with no cover drew one");

        library
            .land_archive_cover(
                album,
                &resonate_library::CoverArt {
                    format: resonate_library::ImageFormat::Png,
                    bytes: a_png(),
                },
            )
            .expect("the cover lands");
        driven.root.update(&mut driven.cx, |root, cx| {
            root.library.update(cx, |model, cx| model.reload(cx));
        });
        drawn(&mut driven);
        driven.settle();

        assert!(
            drawn(&mut driven),
            "the miss drawn before the cover existed was kept for the run"
        );
    }

    #[gpui::test]
    fn a_queued_row_is_read_again_once_the_catalog_changes_under_it(cx: &mut TestAppContext) {
        let folder = Folder::new();
        folder.tagged("echoes.wav", 1, &[(b"INAM", "Echos"), (b"IPRD", "Meddle")]);
        let library = catalog();
        Driven::scanned(&library, &folder);
        let track = library
            .tracks(&resonate_library::TrackQuery::default())
            .expect("the tracks read")
            .remove(0);
        let item = resonate_engine::QueueItem {
            id: track.id,
            location: track.location.clone(),
            span: track.span,
        };
        let mut driven = Driven::opened_in(cx, Arc::clone(&library), &folder);
        let titled = |driven: &mut Driven| {
            driven.root.update(&mut driven.cx, |root, cx| {
                root.library
                    .update(cx, |model, _| model.track_of(&item))
                    .map(|track| track.title)
            })
        };
        assert_eq!(titled(&mut driven).as_deref(), Some("Echos"));

        library
            .land_recording(
                track.id,
                &resonate_library::Recording {
                    id: resonate_library::Mbid::new("b1a9c0de-1111-4222-8333-444455556666")
                        .expect("an id"),
                    title: "Echoes".to_owned(),
                    credit: Vec::new(),
                    length: None,
                    isrcs: Vec::new(),
                    releases: Vec::new(),
                },
                resonate_library::Certainty::Exactly,
                None,
            )
            .expect("the lookup lands");
        driven.root.update(&mut driven.cx, |root, cx| {
            root.library.update(cx, |model, cx| model.reload(cx));
        });
        let held = driven.read(|root, cx| root.library.read(cx).names_held());
        assert_eq!(held, 1, "a reload emptied the names the queue draws from");

        let began = Instant::now();
        while titled(&mut driven).as_deref() != Some("Echoes") {
            assert!(
                began.elapsed() < Duration::from_secs(10),
                "the queue kept the row as it was first read"
            );
            driven.settle();
            thread::sleep(FRAME);
        }
    }

    #[gpui::test]
    fn what_was_heard_most_is_read_back_in_the_order_it_is_listed(cx: &mut TestAppContext) {
        let folder = Folder::new();
        let library = scanned_catalog(&folder, &["Echoes", "Money", "Time"]);
        let ids: Vec<_> = library
            .tracks(&resonate_library::TrackQuery::default())
            .expect("the scanned tracks")
            .iter()
            .map(|track| track.id)
            .rev()
            .collect();
        let driven = Driven::opened_in(cx, library, &folder);

        let read: Vec<_> = driven
            .root
            .read_with(&driven.cx, |root, cx| {
                root.library.read(cx).held_tracks(&ids)
            })
            .iter()
            .map(|track| track.id)
            .collect();

        assert_eq!(read, ids);
    }

    #[gpui::test]
    fn typing_at_the_window_searches_with_every_letter_and_its_spaces_rather_than_steering(
        cx: &mut TestAppContext,
    ) {
        let mut driven = Driven::open(cx, catalog());

        driven
            .cx
            .simulate_keystrokes("r->r u->u s->s h->h space o->o f->f");
        driven.settle();

        assert_eq!(
            driven.read(|root, cx| root.search.read(cx).text().to_owned()),
            "rush of"
        );
        assert!(
            !driven.read(|root, cx| root.player.read(cx).state().shuffle),
            "an h typed into a search shuffled the queue"
        );
        let search_focused = |driven: &mut Driven| {
            let root = driven.root.clone();
            driven
                .cx
                .update(|window, cx| root.read(cx).search.read(cx).is_focused(window))
        };
        assert!(
            search_focused(&mut driven),
            "a search typed at the window left the field without the caret"
        );

        driven.cx.simulate_keystrokes("escape");
        driven.settle();
        assert_eq!(
            driven.read(|root, cx| root.search.read(cx).text().to_owned()),
            "",
            "escape left a search typed at the window standing"
        );
        assert!(
            !search_focused(&mut driven),
            "escape left the caret in the search"
        );
    }

    #[gpui::test]
    fn an_input_method_composing_at_the_window_begins_a_search_and_its_commit_lands_in_it(
        cx: &mut TestAppContext,
    ) {
        use gpui::InputHandler as _;

        use crate::views::field::Composing;

        let mut driven = Driven::open(cx, catalog());
        let search = driven.read(|root, _| root.search.clone());
        let text =
            |driven: &mut Driven| driven.read(|root, cx| root.search.read(cx).text().to_owned());

        driven.cx.update(|window, cx| {
            Composing::at_the_window(search.clone(), true)
                .replace_and_mark_text_in_range(None, "ka", None, window, cx);
        });
        driven.settle();
        assert_eq!(text(&mut driven), "ka");
        assert!(driven.read(|root, cx| root.search.read(cx).holds_a_preedit()));

        driven.cx.update(|window, cx| {
            Composing::at_the_window(search.clone(), true)
                .replace_and_mark_text_in_range(None, "\u{304b}", None, window, cx);
        });
        assert_eq!(text(&mut driven), "\u{304b}");

        driven.cx.update(|window, cx| {
            Composing::at_the_window(search.clone(), true)
                .replace_text_in_range(None, "\u{304b}", window, cx);
        });
        driven.settle();
        assert_eq!(text(&mut driven), "\u{304b}");
        assert!(!driven.read(|root, cx| root.search.read(cx).holds_a_preedit()));

        driven.cx.update(|window, cx| {
            Composing::at_the_window(search.clone(), true)
                .replace_text_in_range(None, "x", window, cx);
        });
        assert_eq!(
            text(&mut driven),
            "\u{304b}",
            "a key the window already heard was typed a second time"
        );
    }

    #[gpui::test]
    fn an_input_method_composing_over_a_sheet_is_not_taken_into_the_search(
        cx: &mut TestAppContext,
    ) {
        use gpui::InputHandler as _;

        use crate::views::field::Composing;

        let mut driven = Driven::open(cx, catalog());
        let search = driven.read(|root, _| root.search.clone());

        driven.cx.update(|window, cx| {
            Composing::at_the_window(search, false)
                .replace_and_mark_text_in_range(None, "ka", None, window, cx);
        });
        driven.settle();

        assert_eq!(
            driven.read(|root, cx| root.search.read(cx).text().to_owned()),
            ""
        );
    }

    #[gpui::test]
    fn a_preedit_is_one_line_and_its_mark_stops_where_the_field_stops_holding_it(
        cx: &mut TestAppContext,
    ) {
        use gpui::EntityInputHandler as _;

        use crate::edit::LONGEST_CONTENT;

        let mut driven = Driven::open(cx, catalog());
        driven.focus(|root| &root.search);
        let search = driven.read(|root, _| root.search.clone());

        driven.cx.update(|window, cx| {
            search.update(cx, |field, cx| {
                field.replace_and_mark_text_in_range(None, "a\nb\rc", None, window, cx);
            });
        });
        assert_eq!(
            driven.read(|root, cx| root.search.read(cx).text().to_owned()),
            "a b c"
        );

        driven.cx.update(|window, cx| {
            search.update(cx, |field, cx| {
                field.replace_text_in_range(None, "", window, cx);
                field.hold("a".repeat(LONGEST_CONTENT - 2), cx);
                field.replace_and_mark_text_in_range(None, "xyz", None, window, cx);
            });
        });
        let marked = driven.cx.update(|window, cx| {
            search.update(cx, |field, cx| field.marked_text_range(window, cx))
        });
        assert_eq!(
            driven.read(|root, cx| root.search.read(cx).text().len()),
            LONGEST_CONTENT
        );
        assert_eq!(marked, Some(LONGEST_CONTENT - 2..LONGEST_CONTENT));
    }

    #[gpui::test]
    fn a_contact_typed_and_left_with_the_pointer_is_kept_and_one_left_with_escape_is_put_back(
        cx: &mut TestAppContext,
    ) {
        let mut driven = Driven::open(cx, catalog());
        let contact = |driven: &mut Driven| {
            driven.read(|_, cx| cx.global::<ResonateApp>().online.contact.clone())
        };
        let typed = |driven: &mut Driven, text: &str| {
            let root = driven.root.clone();
            driven.cx.update(|_, cx| {
                let field = root.read(cx).contact.clone();
                field.update(cx, |field, cx| field.hold(text.to_owned(), cx));
            });
        };

        driven.focus(|root| &root.contact);
        typed(&mut driven, "me@example.test");
        driven.cx.update(|window, cx| {
            let root = driven.root.clone();
            root.update(cx, |root, cx| root.leave_contact(window, cx));
        });
        driven.settle();
        assert_eq!(contact(&mut driven), "me@example.test");

        driven.focus(|root| &root.contact);
        typed(&mut driven, "someone@else.test");
        driven.cx.update(|window, cx| {
            let root = driven.root.clone();
            root.update(cx, |root, cx| root.put_back_contact(window, cx));
        });
        driven.settle();
        assert_eq!(contact(&mut driven), "me@example.test");
        assert_eq!(
            driven.read(|root, cx| root.contact.read(cx).text().to_owned()),
            "me@example.test"
        );
    }

    fn showing_settings(driven: &mut Driven, category: SettingsCategory) {
        driven.root.update(&mut driven.cx, |root, cx| {
            root.set_pane(Pane::Settings, cx);
            root.settings_category = category;
            cx.notify();
        });
        driven.settle();
    }

    fn typed_into(
        driven: &mut Driven,
        field: fn(&RootView) -> &Entity<crate::views::field::Field>,
        text: &str,
    ) {
        driven.focus(field);

        let root = driven.root.clone();
        driven.cx.update(|_, cx| {
            let field = field(root.read(cx)).clone();
            field.update(cx, |field, cx| field.hold(text.to_owned(), cx));
        });
    }

    #[gpui::test]
    fn tab_out_of_an_online_field_gives_what_was_typed_without_taking_the_caret_back(
        cx: &mut TestAppContext,
    ) {
        let mut driven = Driven::open(cx, catalog());
        showing_settings(&mut driven, SettingsCategory::Online);

        typed_into(&mut driven, |root| &root.contact, "me@example.test");
        driven.cx.simulate_keystrokes("tab");
        driven.settle();

        assert_eq!(
            driven.read(|_, cx| cx.global::<ResonateApp>().online.contact.clone()),
            "me@example.test"
        );
        assert!(
            !driven.cx.update(|window, cx| {
                let root = driven.root.read(cx);
                root.contact.read(cx).is_focused(window)
            }),
            "the contact kept the caret"
        );
        assert!(
            !driven.cx.update(|window, cx| {
                let root = driven.root.read(cx);
                window.focused(cx).is_some_and(|held| held == root.focus)
            }),
            "the window took the caret back from the next stop"
        );

        showing_settings(&mut driven, SettingsCategory::Recognition);

        typed_into(&mut driven, |root| &root.acoustid, "a-key");
        driven.cx.simulate_keystrokes("tab");
        driven.settle();

        assert_eq!(
            driven.read(|_, cx| cx.global::<ResonateApp>().online.acoustid_key.clone()),
            "a-key"
        );

        typed_into(&mut driven, |root| &root.audd, "an-audd");
        driven.cx.simulate_keystrokes("tab");
        driven.settle();

        assert_eq!(
            driven.read(|_, cx| cx.global::<ResonateApp>().online.audd_token.clone()),
            "an-audd"
        );

        showing_settings(&mut driven, SettingsCategory::Scrobbling);

        typed_into(&mut driven, |root| &root.listenbrainz, "a-token");
        driven.cx.simulate_keystrokes("tab");
        driven.settle();

        assert_eq!(
            driven.read(|_, cx| cx.global::<ResonateApp>().online.listenbrainz_token.clone()),
            "a-token"
        );
    }

    #[gpui::test]
    fn tab_out_of_the_filing_layout_keeps_a_valid_one_and_escape_puts_it_back(
        cx: &mut TestAppContext,
    ) {
        let mut driven = Driven::open(cx, catalog());
        showing_settings(&mut driven, SettingsCategory::Library);
        let filed_as = |driven: &mut Driven| {
            driven.read(|_, cx| cx.global::<ResonateApp>().organise_as.clone())
        };
        let before = filed_as(&mut driven);

        typed_into(&mut driven, |root| &root.organising, "{artist}/{title}");
        driven.cx.simulate_keystrokes("tab");
        driven.settle();

        assert_eq!(filed_as(&mut driven), "{artist}/{title}");

        typed_into(&mut driven, |root| &root.organising, "{album}/{title}");
        driven.cx.simulate_keystrokes("escape");
        driven.settle();

        assert_eq!(filed_as(&mut driven), "{artist}/{title}");
        assert_ne!(before, "{artist}/{title}");
    }

    #[gpui::test]
    fn tab_out_of_the_typed_folder_adds_it_and_escape_discards_it(cx: &mut TestAppContext) {
        let folder = Folder::new();
        let kept = Folder::new();
        let dropped = Folder::new();
        let mut driven = Driven::opened_in(cx, catalog(), &folder);
        showing_settings(&mut driven, SettingsCategory::Library);

        typed_into(
            &mut driven,
            |root| &root.typed_root,
            &dropped.path().display().to_string(),
        );
        driven.cx.simulate_keystrokes("escape");
        driven.settle();

        assert_eq!(
            driven.read(|root, cx| root.typed_root.read(cx).text().to_owned()),
            ""
        );

        typed_into(
            &mut driven,
            |root| &root.typed_root,
            &kept.path().display().to_string(),
        );
        driven.cx.simulate_keystrokes("tab");
        driven.until(|root, cx| root.library.read(cx).roots().len() == 1);

        assert_eq!(
            driven.read(|root, cx| root.library.read(cx).roots().to_vec()),
            vec![kept.path().to_path_buf()]
        );
    }

    fn reached_in_the_tracks(driven: &mut Driven, row: usize) {
        let root = driven.root.clone();
        driven.cx.update(|_, cx| {
            root.update(cx, |root, cx| {
                root.reach_at(Shift::Listing(Listed::Tracks), row, false, cx);
            });
        });
        driven.settle();
    }

    fn reach_in_the_tracks(driven: &mut Driven) -> Option<Span> {
        driven.read(|root, _| root.reaching(Shift::Listing(Listed::Tracks)))
    }

    const CONTROL: Modifiers = Modifiers {
        control: true,
        alt: false,
        shift: false,
        platform: false,
        function: false,
    };

    fn chosen_rows(driven: &mut Driven) -> Vec<usize> {
        driven.read(|root, _| {
            root.reached()
                .map(|reach| reach.selected().rows().collect())
                .unwrap_or_default()
        })
    }

    fn listed_ids(driven: &mut Driven) -> Vec<TrackId> {
        driven.read(|root, cx| {
            root.library
                .read(cx)
                .listing()
                .iter()
                .map(|track| track.id)
                .collect()
        })
    }

    #[gpui::test]
    fn a_control_click_chooses_a_track_apart_from_the_others_and_plays_nothing(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let library = scanned_catalog(&folder, &["Echoes", "Money", "Time", "Us"]);
        let mut driven = Driven::opened_in(cx, library, &folder);
        driven.click("tab-tracks");
        driven.until(|root, cx| root.library.read(cx).tracks_counted() == 4);
        let ids = listed_ids(&mut driven);

        driven.click_with(named("track", ids[0].get()), CONTROL);
        driven.click_with(named("track", ids[2].get()), CONTROL);

        assert_eq!(chosen_rows(&mut driven), [0, 2]);
        assert_eq!(
            driven.read(|root, cx| root.player.read(cx).queue().len()),
            0
        );

        driven.click_with(named("track", ids[0].get()), CONTROL);

        assert_eq!(chosen_rows(&mut driven), [2]);
        assert_eq!(
            driven.read(|root, _| root.reached().map(|reach| reach.row)),
            Some(0)
        );
    }

    #[gpui::test]
    fn a_shift_click_chooses_every_track_between_and_plays_nothing(cx: &mut TestAppContext) {
        let folder = Folder::new();
        let library = scanned_catalog(&folder, &["Echoes", "Money", "Time", "Us"]);
        let mut driven = Driven::opened_in(cx, library, &folder);
        driven.click("tab-tracks");
        driven.until(|root, cx| root.library.read(cx).tracks_counted() == 4);
        let ids = listed_ids(&mut driven);

        driven.click_with(named("track", ids[0].get()), CONTROL);
        driven.click_with(
            named("track", ids[3].get()),
            Modifiers {
                shift: true,
                ..Modifiers::none()
            },
        );

        assert_eq!(chosen_rows(&mut driven), [0, 1, 2, 3]);
        assert_eq!(
            driven.read(|root, cx| root.player.read(cx).queue().len()),
            0
        );
    }

    #[gpui::test]
    fn the_keys_hop_over_rows_and_choose_them_and_enter_plays_every_one_chosen_in_order(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let library = scanned_catalog(&folder, &["Echoes", "Money", "Time", "Us"]);
        let mut driven = Driven::opened_in(cx, library, &folder);
        driven.click("tab-tracks");
        driven.until(|root, cx| root.library.read(cx).tracks_counted() == 4);
        let ids = listed_ids(&mut driven);

        driven
            .cx
            .simulate_keystrokes("down ctrl-shift-down ctrl-shift-down");

        assert_eq!(chosen_rows(&mut driven), [0]);
        assert_eq!(
            driven.read(|root, _| root.reached().map(|reach| reach.row)),
            Some(2)
        );

        driven.cx.simulate_keystrokes("ctrl-space");

        assert_eq!(chosen_rows(&mut driven), [0, 2]);

        driven.cx.simulate_keystrokes("ctrl-space");

        assert_eq!(chosen_rows(&mut driven), [0]);

        driven.cx.simulate_keystrokes("ctrl-space enter");
        driven.until(|root, cx| root.player.read(cx).queue().len() == 2);

        assert_eq!(driven.queue(), [ids[0], ids[2]]);
    }

    #[gpui::test]
    fn dragging_a_row_chosen_apart_carries_every_chosen_row_to_the_queue(cx: &mut TestAppContext) {
        let folder = Folder::new();
        let library = scanned_catalog(&folder, &["Echoes", "Money", "Time", "Us"]);
        let mut driven = Driven::opened_in(cx, library, &folder);
        driven.click("tab-tracks");
        driven.until(|root, cx| root.library.read(cx).tracks_counted() == 4);
        let ids = listed_ids(&mut driven);

        driven.click_with(named("track", ids[3].get()), CONTROL);
        driven.click_with(named("track", ids[1].get()), CONTROL);
        let from = driven.centre_of(named("track", ids[1].get()));
        let onto = driven.centre_of("queue");
        driven.drag(from, onto);
        driven.until(|root, cx| root.player.read(cx).queue().len() == 2);

        assert_eq!(driven.queue(), [ids[1], ids[3]]);
    }

    #[gpui::test]
    fn dragging_a_row_that_is_not_chosen_carries_that_row_alone(cx: &mut TestAppContext) {
        let folder = Folder::new();
        let library = scanned_catalog(&folder, &["Echoes", "Money", "Time", "Us"]);
        let mut driven = Driven::opened_in(cx, library, &folder);
        driven.click("tab-tracks");
        driven.until(|root, cx| root.library.read(cx).tracks_counted() == 4);
        let ids = listed_ids(&mut driven);

        driven.click_with(named("track", ids[0].get()), CONTROL);
        driven.click_with(named("track", ids[1].get()), CONTROL);
        let from = driven.centre_of(named("track", ids[3].get()));
        let onto = driven.centre_of("queue");
        driven.drag(from, onto);
        driven.until(|root, cx| root.player.read(cx).queue().len() == 1);

        assert_eq!(driven.queue(), [ids[3]]);
    }

    #[gpui::test]
    fn an_arrow_gives_up_the_rows_chosen_apart(cx: &mut TestAppContext) {
        let folder = Folder::new();
        let library = scanned_catalog(&folder, &["Echoes", "Money", "Time", "Us"]);
        let mut driven = Driven::opened_in(cx, library, &folder);
        driven.click("tab-tracks");
        driven.until(|root, cx| root.library.read(cx).tracks_counted() == 4);

        driven
            .cx
            .simulate_keystrokes("down ctrl-shift-down ctrl-space down");

        assert_eq!(chosen_rows(&mut driven), [2]);
    }

    #[gpui::test]
    fn the_menu_of_rows_chosen_apart_queues_every_one_of_them(cx: &mut TestAppContext) {
        let folder = Folder::new();
        let library = scanned_catalog(&folder, &["Echoes", "Money", "Time", "Us"]);
        let mut driven = Driven::opened_in(cx, library, &folder);
        driven.click("tab-tracks");
        driven.until(|root, cx| root.library.read(cx).tracks_counted() == 4);
        let ids = listed_ids(&mut driven);

        driven
            .cx
            .simulate_keystrokes("down ctrl-shift-down ctrl-shift-down ctrl-space menu");
        assert!(driven.read(|root, _| root.menu.is_some()));

        driven.cx.simulate_keystrokes("down down enter");
        driven.until(|root, cx| root.player.read(cx).queue().len() == 2);

        assert_eq!(driven.queue(), [ids[0], ids[2]]);
    }

    #[gpui::test]
    fn a_control_click_chooses_albums_apart_and_enter_plays_every_one_chosen(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        folder.tagged("one.wav", 1, &[(b"INAM", "One"), (b"IPRD", "Meddle")]);
        folder.tagged("two.wav", 1, &[(b"INAM", "Two"), (b"IPRD", "Animals")]);
        folder.tagged("three.wav", 1, &[(b"INAM", "Three"), (b"IPRD", "Wish")]);
        let library = catalog();
        Driven::scanned(&library, &folder);
        let mut driven = Driven::opened_in(cx, Arc::clone(&library), &folder);
        driven.click("tab-albums");
        driven.until(|root, cx| root.library.read(cx).albums_listed() == 3);
        let albums = driven.read(|root, cx| root.library.read(cx).albums());

        driven.click_with(named("album", albums[0].id.get()), CONTROL);
        driven.click_with(named("album", albums[2].id.get()), CONTROL);

        assert_eq!(chosen_rows(&mut driven), [0, 2]);
        assert_eq!(driven.read(|root, _| root.pane), Pane::Albums);

        driven.cx.simulate_keystrokes("enter");
        driven.until(|root, cx| root.player.read(cx).queue().len() == 2);

        let of_album = |album: resonate_core::AlbumId| -> TrackId {
            library
                .tracks(&resonate_library::TrackQuery {
                    album: Some(album),
                    ..resonate_library::TrackQuery::default()
                })
                .expect("the album's tracks")[0]
                .id
        };
        assert_eq!(
            driven.queue(),
            [of_album(albums[0].id), of_album(albums[2].id)]
        );
        assert_eq!(driven.read(|root, _| root.pane), Pane::Albums);
    }

    fn favourite_everything(library: &Library) {
        for track in library
            .tracks(&resonate_library::TrackQuery::default())
            .expect("the scanned tracks")
        {
            library
                .favour(resonate_library::Favoured::Track(track.id), true)
                .expect("a favourite kept");
        }
    }

    fn favourites_listed(driven: &mut Driven) -> Vec<TrackId> {
        driven.read(|root, cx| {
            root.library
                .read(cx)
                .favourite_tracks()
                .iter()
                .map(|track| track.id)
                .collect()
        })
    }

    fn menu_labels(driven: &mut Driven) -> Vec<String> {
        driven.read(|root, _| root.menu.as_ref().map(Menu::labels).unwrap_or_default())
    }

    fn pressed_in_the_menu(driven: &mut Driven, label: &str) {
        let at = menu_labels(driven)
            .iter()
            .position(|held| held == label)
            .unwrap_or_else(|| panic!("the menu offers no {label}"));
        driven.click(named("menu-entry", at));
    }

    fn deleting_asked_about(driven: &mut Driven) -> Option<Vec<TrackId>> {
        driven.read(|root, _| {
            root.deleting
                .as_ref()
                .map(|deleting| deleting.tracks.clone())
        })
    }

    fn favourites_open_with_two_chosen(
        cx: &mut TestAppContext,
        folder: &Folder,
    ) -> (Driven, Arc<Library>, Vec<TrackId>) {
        let library = scanned_catalog(folder, &["Echoes", "Money", "Time", "Us"]);
        favourite_everything(&library);
        let mut driven = Driven::opened_in(cx, Arc::clone(&library), folder);
        driven.click("tab-favourites");
        driven.until(|root, cx| root.library.read(cx).favourite_tracks().len() == 4);
        let ids = favourites_listed(&mut driven);

        driven.click_with(named("track", ids[3].get()), CONTROL);
        driven.click_with(named("track", ids[1].get()), CONTROL);

        (driven, library, ids)
    }

    #[gpui::test]
    fn a_control_click_chooses_favourites_apart_and_enter_plays_every_one_chosen(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let (mut driven, _library, ids) = favourites_open_with_two_chosen(cx, &folder);

        assert_eq!(chosen_rows(&mut driven), [1, 3]);
        assert_eq!(driven.read(|root, _| root.pane), Pane::Favourites);
        assert_eq!(
            driven.read(|root, cx| root.player.read(cx).queue().len()),
            0
        );

        driven.cx.simulate_keystrokes("enter");
        driven.until(|root, cx| root.player.read(cx).queue().len() == 2);

        assert_eq!(driven.queue(), [ids[1], ids[3]]);
    }

    #[gpui::test]
    fn the_keys_hop_over_favourites_and_choose_them(cx: &mut TestAppContext) {
        let folder = Folder::new();
        let library = scanned_catalog(&folder, &["Echoes", "Money", "Time"]);
        favourite_everything(&library);
        let mut driven = Driven::opened_in(cx, library, &folder);
        driven.click("tab-favourites");
        driven.until(|root, cx| root.library.read(cx).favourite_tracks().len() == 3);

        driven
            .cx
            .simulate_keystrokes("down ctrl-shift-down ctrl-shift-down ctrl-space");

        assert_eq!(chosen_rows(&mut driven), [0, 2]);
    }

    #[gpui::test]
    fn the_menu_of_favourites_chosen_apart_offers_to_hide_delete_copy_show_and_share_them(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let (mut driven, _library, ids) = favourites_open_with_two_chosen(cx, &folder);

        driven.right_click(named("track", ids[1].get()));
        let labels = menu_labels(&mut driven);

        for offered in [
            menu::HIDE_EACH,
            menu::DELETE_FROM_DISK,
            menu::COPY_PATHS,
            menu::SHOW_IN_FOLDER,
            menu::SHARE_EACH,
            menu::PLAY,
            menu::UNFAVOUR,
        ] {
            assert!(labels.iter().any(|label| label == offered), "{offered}");
        }
        for withheld in [menu::SHARE, menu::GO_TO_ALBUM] {
            assert!(!labels.iter().any(|label| label == withheld), "{withheld}");
        }
    }

    #[gpui::test]
    fn hiding_the_rows_chosen_apart_hides_each_of_them_and_no_other(cx: &mut TestAppContext) {
        let folder = Folder::new();
        let (mut driven, library, ids) = favourites_open_with_two_chosen(cx, &folder);

        driven.right_click(named("track", ids[1].get()));
        pressed_in_the_menu(&mut driven, menu::HIDE_EACH);
        driven.until(|root, cx| root.library.read(cx).favourite_tracks().len() == 2);

        let left: Vec<TrackId> = library
            .tracks(&resonate_library::TrackQuery::default())
            .expect("the tracks still shown")
            .into_iter()
            .map(|track| track.id)
            .collect();
        assert_eq!(left.len(), 2);
        assert!(left.contains(&ids[0]) && left.contains(&ids[2]));
    }

    #[gpui::test]
    fn deleting_the_rows_chosen_apart_asks_once_for_all_of_them(cx: &mut TestAppContext) {
        let folder = Folder::new();
        let (mut driven, _library, ids) = favourites_open_with_two_chosen(cx, &folder);

        driven.right_click(named("track", ids[1].get()));
        pressed_in_the_menu(&mut driven, menu::DELETE_FROM_DISK);

        assert_eq!(
            deleting_asked_about(&mut driven),
            Some(vec![ids[1], ids[3]])
        );
    }

    #[gpui::test]
    fn the_menu_of_tracks_chosen_apart_offers_to_hide_and_delete_them(cx: &mut TestAppContext) {
        let folder = Folder::new();
        let library = scanned_catalog(&folder, &["Echoes", "Money", "Time", "Us"]);
        let mut driven = Driven::opened_in(cx, library, &folder);
        driven.click("tab-tracks");
        driven.until(|root, cx| root.library.read(cx).tracks_counted() == 4);
        let ids = listed_ids(&mut driven);

        driven.click_with(named("track", ids[0].get()), CONTROL);
        driven.click_with(named("track", ids[2].get()), CONTROL);
        driven.right_click(named("track", ids[0].get()));
        let labels = menu_labels(&mut driven);

        for offered in [menu::HIDE_EACH, menu::DELETE_FROM_DISK, menu::COPY_PATHS] {
            assert!(labels.iter().any(|label| label == offered), "{offered}");
        }

        pressed_in_the_menu(&mut driven, menu::DELETE_FROM_DISK);

        assert_eq!(
            deleting_asked_about(&mut driven),
            Some(vec![ids[0], ids[2]])
        );
    }

    #[gpui::test]
    fn a_control_click_chooses_a_suggestions_rows_apart_and_enter_plays_them(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let titles: Vec<String> = (1..=12).map(|nth| format!("Track {nth}")).collect();
        let named_titles: Vec<&str> = titles.iter().map(String::as_str).collect();
        let library = scanned_catalog(&folder, &named_titles);
        let mut driven = Driven::opened_in(cx, library, &folder);
        driven.until(|root, cx| !root.library.read(cx).suggestions().is_empty());
        driven.click("tab-suggestions");
        driven.click("suggestion-open-0");
        driven.until(|root, cx| {
            root.library
                .read(cx)
                .opened_suggestion()
                .is_some_and(|opened| opened.tracks.len() > 3)
        });
        let ids: Vec<TrackId> = driven.read(|root, cx| {
            root.library
                .read(cx)
                .opened_suggestion()
                .map(|opened| opened.tracks.iter().map(|track| track.id).collect())
                .unwrap_or_default()
        });

        driven.click_with(named("track", ids[2].get()), CONTROL);
        driven.click_with(named("track", ids[0].get()), CONTROL);

        assert_eq!(chosen_rows(&mut driven), [0, 2]);
        assert_eq!(driven.read(|root, _| root.pane), Pane::Suggestions);

        driven.cx.simulate_keystrokes("enter");
        driven.until(|root, cx| root.player.read(cx).queue().len() == 2);

        assert_eq!(driven.queue(), [ids[0], ids[2]]);
    }

    fn three_artists(folder: &Folder) -> Arc<Library> {
        for (file, title, artist) in [
            ("one.wav", "One", "Air"),
            ("two.wav", "Two", "Beck"),
            ("three.wav", "Three", "Cake"),
        ] {
            folder.tagged(file, 1, &[(b"INAM", title), (b"IART", artist)]);
        }
        let library = catalog();
        Driven::scanned(&library, folder);
        library
    }

    #[gpui::test]
    fn a_control_click_chooses_artists_apart_and_enter_queues_every_one_chosen(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let library = three_artists(&folder);
        let mut driven = Driven::opened_in(cx, Arc::clone(&library), &folder);
        driven.click("tab-artists");
        driven.until(|root, cx| root.library.read(cx).artists_listed() == 3);
        let artists = driven.read(|root, cx| root.library.read(cx).artists());

        driven.click_with(named("artist", artists[0].id.get()), CONTROL);
        driven.click_with(named("artist", artists[2].id.get()), CONTROL);

        assert_eq!(chosen_rows(&mut driven), [0, 2]);
        assert_eq!(driven.read(|root, _| root.pane), Pane::Artists);
        assert_eq!(
            driven.read(|root, cx| root.player.read(cx).queue().len()),
            0
        );

        driven.cx.simulate_keystrokes("enter");
        driven.until(|root, cx| root.player.read(cx).queue().len() == 2);

        let of_artist = |artist: resonate_core::ArtistId| -> TrackId {
            library
                .tracks(&resonate_library::TrackQuery {
                    artist: Some(artist),
                    ..resonate_library::TrackQuery::default()
                })
                .expect("the artist's tracks")[0]
                .id
        };
        assert_eq!(
            driven.queue(),
            [of_artist(artists[0].id), of_artist(artists[2].id)]
        );
        assert_eq!(driven.read(|root, _| root.pane), Pane::Artists);
    }

    #[gpui::test]
    fn a_plain_click_on_an_artist_still_opens_it_and_chooses_nothing(cx: &mut TestAppContext) {
        let folder = Folder::new();
        let library = three_artists(&folder);
        let mut driven = Driven::opened_in(cx, library, &folder);
        driven.click("tab-artists");
        driven.until(|root, cx| root.library.read(cx).artists_listed() == 3);
        let artists = driven.read(|root, cx| root.library.read(cx).artists());

        driven.click(named("artist", artists[1].id.get()));

        assert_eq!(
            driven.read(|root, cx| root.library.read(cx).selection()),
            Selection::Artist(artists[1].id)
        );
        assert!(chosen_rows(&mut driven).is_empty());
    }

    #[gpui::test]
    fn the_menu_of_artists_chosen_apart_plays_queues_and_favourites_them_all(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let library = three_artists(&folder);
        let mut driven = Driven::opened_in(cx, library, &folder);
        driven.click("tab-artists");
        driven.until(|root, cx| root.library.read(cx).artists_listed() == 3);
        let artists = driven.read(|root, cx| root.library.read(cx).artists());

        driven.click_with(named("artist", artists[0].id.get()), CONTROL);
        driven.click_with(named("artist", artists[1].id.get()), CONTROL);
        driven.right_click(named("artist", artists[0].id.get()));

        assert_eq!(
            menu_labels(&mut driven),
            [
                menu::PLAY,
                menu::PLAY_NEXT,
                menu::ADD_TO_QUEUE,
                "",
                menu::FAVOUR
            ]
        );

        pressed_in_the_menu(&mut driven, menu::FAVOUR);
        driven.until(|root, cx| {
            let library = root.library.read(cx);
            library.favoured_artist(artists[0].id) && library.favoured_artist(artists[1].id)
        });

        assert!(!driven.read(|root, cx| root.library.read(cx).favoured_artist(artists[2].id)));
    }

    fn three_records_all_favourite(folder: &Folder) -> Arc<Library> {
        for (file, title, artist, album) in [
            ("one.wav", "One", "Air", "Meddle"),
            ("two.wav", "Two", "Beck", "Animals"),
            ("three.wav", "Three", "Cake", "Wish"),
        ] {
            folder.tagged(
                file,
                1,
                &[(b"INAM", title), (b"IART", artist), (b"IPRD", album)],
            );
        }
        let library = catalog();
        Driven::scanned(&library, folder);
        favourite_everything(&library);
        for album in library
            .albums(&resonate_library::AlbumQuery::default())
            .expect("the scanned albums")
        {
            library
                .favour(resonate_library::Favoured::Album(album.id), true)
                .expect("a favourite kept");
        }
        for artist in library
            .artists(&resonate_library::ArtistQuery::default())
            .expect("the scanned artists")
        {
            library
                .favour(resonate_library::Favoured::Artist(artist.id), true)
                .expect("a favourite kept");
        }
        library
    }

    fn favourites_with_shelves(cx: &mut TestAppContext, folder: &Folder) -> (Driven, Arc<Library>) {
        let library = three_records_all_favourite(folder);
        let mut driven = Driven::opened_in(cx, Arc::clone(&library), folder);
        driven.click("tab-favourites");
        driven.until(|root, cx| {
            let held = root.library.read(cx).favourited();
            held.artists == 3 && held.albums == 3
        });

        (driven, library)
    }

    fn the_tracks_of_an_artist(library: &Library, artist: resonate_core::ArtistId) -> TrackId {
        library
            .tracks(&resonate_library::TrackQuery {
                artist: Some(artist),
                ..resonate_library::TrackQuery::default()
            })
            .expect("the artist's tracks")[0]
            .id
    }

    fn the_tracks_of_an_album(library: &Library, album: resonate_core::AlbumId) -> TrackId {
        library
            .tracks(&resonate_library::TrackQuery {
                album: Some(album),
                ..resonate_library::TrackQuery::default()
            })
            .expect("the album's tracks")[0]
            .id
    }

    #[gpui::test]
    fn a_control_click_chooses_favourite_artists_apart_and_the_menu_queues_each_in_turn(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let (mut driven, library) = favourites_with_shelves(cx, &folder);
        let artists = driven.read(|root, cx| root.library.read(cx).favourite_artists());

        driven.click_with(named("artist", artists[2].id.get()), CONTROL);
        driven.click_with(named("artist", artists[0].id.get()), CONTROL);

        assert_eq!(chosen_rows(&mut driven), [0, 2]);
        assert_eq!(
            driven.read(|root, _| root.reached().map(|reach| reach.shift)),
            Some(Shift::Listing(Listed::FavouriteArtists))
        );
        assert_eq!(driven.read(|root, _| root.pane), Pane::Favourites);
        assert_eq!(
            driven.read(|root, cx| root.player.read(cx).queue().len()),
            0
        );

        driven.right_click(named("artist", artists[0].id.get()));

        assert_eq!(
            menu_labels(&mut driven),
            [
                menu::PLAY,
                menu::PLAY_NEXT,
                menu::ADD_TO_QUEUE,
                "",
                menu::UNFAVOUR
            ]
        );

        pressed_in_the_menu(&mut driven, menu::ADD_TO_QUEUE);
        driven.until(|root, cx| root.player.read(cx).queue().len() == 2);

        assert_eq!(
            driven.queue(),
            [
                the_tracks_of_an_artist(&library, artists[0].id),
                the_tracks_of_an_artist(&library, artists[2].id)
            ]
        );
    }

    #[gpui::test]
    fn a_control_click_chooses_favourite_albums_apart_and_the_menu_plays_each_in_turn(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let (mut driven, library) = favourites_with_shelves(cx, &folder);
        let albums = driven.read(|root, cx| root.library.read(cx).favourite_albums());

        driven.click_with(named("album", albums[1].id.get()), CONTROL);
        driven.click_with(named("album", albums[2].id.get()), CONTROL);

        assert_eq!(chosen_rows(&mut driven), [1, 2]);
        assert_eq!(
            driven.read(|root, _| root.reached().map(|reach| reach.shift)),
            Some(Shift::Listing(Listed::FavouriteAlbums))
        );

        driven.right_click(named("album", albums[1].id.get()));
        pressed_in_the_menu(&mut driven, menu::PLAY);
        driven.until(|root, cx| root.player.read(cx).queue().len() == 2);

        assert_eq!(
            driven.queue(),
            [
                the_tracks_of_an_album(&library, albums[1].id),
                the_tracks_of_an_album(&library, albums[2].id)
            ]
        );
    }

    #[gpui::test]
    fn a_shift_click_widens_the_choice_along_a_favourites_shelf(cx: &mut TestAppContext) {
        let folder = Folder::new();
        let (mut driven, _library) = favourites_with_shelves(cx, &folder);
        let artists = driven.read(|root, cx| root.library.read(cx).favourite_artists());

        driven.click_with(named("artist", artists[0].id.get()), CONTROL);
        driven.click_with(
            named("artist", artists[2].id.get()),
            Modifiers {
                shift: true,
                ..Modifiers::none()
            },
        );

        assert_eq!(chosen_rows(&mut driven), [0, 1, 2]);
    }

    #[gpui::test]
    fn a_plain_click_on_a_favourite_artist_still_opens_it_and_chooses_nothing(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let (mut driven, _library) = favourites_with_shelves(cx, &folder);
        let artists = driven.read(|root, cx| root.library.read(cx).favourite_artists());

        driven.click(named("artist", artists[1].id.get()));

        assert_eq!(
            driven.read(|root, cx| root.library.read(cx).selection()),
            Selection::Artist(artists[1].id)
        );
        assert!(chosen_rows(&mut driven).is_empty());
    }

    fn reached_at(driven: &mut Driven) -> Option<(Shift, usize)> {
        driven.read(|root, _| root.reached().map(|reach| (reach.shift, reach.row)))
    }

    #[gpui::test]
    fn the_arrows_walk_the_favourites_from_the_tracks_up_into_the_shelves_and_back_down(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let (mut driven, _library) = favourites_with_shelves(cx, &folder);
        let tracks = Shift::Listing(Listed::Favourites);
        let albums = Shift::Listing(Listed::FavouriteAlbums);
        let artists = Shift::Listing(Listed::FavouriteArtists);

        driven.cx.simulate_keystrokes("down");
        assert_eq!(reached_at(&mut driven), Some((tracks, 0)));

        driven.cx.simulate_keystrokes("up");
        assert_eq!(reached_at(&mut driven), Some((albums, 0)));

        driven.cx.simulate_keystrokes("up");
        assert_eq!(reached_at(&mut driven), Some((artists, 0)));

        driven.cx.simulate_keystrokes("up");
        assert_eq!(reached_at(&mut driven), Some((artists, 0)));

        driven.cx.simulate_keystrokes("right right right");
        assert_eq!(reached_at(&mut driven), Some((artists, 2)));

        driven.cx.simulate_keystrokes("left");
        assert_eq!(reached_at(&mut driven), Some((artists, 1)));

        driven.cx.simulate_keystrokes("down");
        assert_eq!(reached_at(&mut driven), Some((albums, 0)));

        driven.cx.simulate_keystrokes("down");
        assert_eq!(reached_at(&mut driven), Some((tracks, 0)));

        driven.cx.simulate_keystrokes("down down down");
        assert_eq!(reached_at(&mut driven), Some((tracks, 2)));
    }

    #[gpui::test]
    fn the_arrows_in_the_favourites_pass_over_a_shelf_that_is_empty(cx: &mut TestAppContext) {
        let folder = Folder::new();
        let library = three_records_all_favourite(&folder);
        for album in library
            .albums(&resonate_library::AlbumQuery::default())
            .expect("the scanned albums")
        {
            library
                .favour(resonate_library::Favoured::Album(album.id), false)
                .expect("a favourite taken back");
        }
        let mut driven = Driven::opened_in(cx, Arc::clone(&library), &folder);
        driven.click("tab-favourites");
        driven.until(|root, cx| {
            let held = root.library.read(cx).favourited();
            held.artists == 3 && held.albums == 0
        });

        driven.cx.simulate_keystrokes("down up");

        assert_eq!(
            reached_at(&mut driven),
            Some((Shift::Listing(Listed::FavouriteArtists), 0))
        );
    }

    #[gpui::test]
    fn a_shelf_alone_is_walked_by_the_arrows_where_no_song_is_a_favourite(cx: &mut TestAppContext) {
        let folder = Folder::new();
        let library = three_records_all_favourite(&folder);
        for track in library
            .tracks(&resonate_library::TrackQuery::default())
            .expect("the scanned tracks")
        {
            library
                .favour(resonate_library::Favoured::Track(track.id), false)
                .expect("a favourite taken back");
        }
        let mut driven = Driven::opened_in(cx, Arc::clone(&library), &folder);
        driven.click("tab-favourites");
        driven.until(|root, cx| {
            let held = root.library.read(cx).favourited();
            held.artists == 3 && held.albums == 3 && held.tracks == 0
        });

        driven.cx.simulate_keystrokes("down down");

        assert_eq!(
            reached_at(&mut driven),
            Some((Shift::Listing(Listed::FavouriteAlbums), 0))
        );
    }

    #[gpui::test]
    fn ctrl_space_chooses_favourite_artists_apart_and_enter_queues_each_in_turn(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let (mut driven, library) = favourites_with_shelves(cx, &folder);
        let artists = driven.read(|root, cx| root.library.read(cx).favourite_artists());

        driven
            .cx
            .simulate_keystrokes("down up up ctrl-shift-down ctrl-shift-down ctrl-space");

        assert_eq!(chosen_rows(&mut driven), [0, 2]);
        assert_eq!(
            driven.read(|root, _| root.reached().map(|reach| reach.shift)),
            Some(Shift::Listing(Listed::FavouriteArtists))
        );

        driven.cx.simulate_keystrokes("ctrl-space");

        assert_eq!(chosen_rows(&mut driven), [0]);

        driven.cx.simulate_keystrokes("ctrl-space enter");
        driven.until(|root, cx| root.player.read(cx).queue().len() == 2);

        assert_eq!(
            driven.queue(),
            [
                the_tracks_of_an_artist(&library, artists[0].id),
                the_tracks_of_an_artist(&library, artists[2].id)
            ]
        );
        assert_eq!(driven.read(|root, _| root.pane), Pane::Favourites);
    }

    #[gpui::test]
    fn the_menu_key_on_favourite_albums_chosen_apart_plays_each_in_turn(cx: &mut TestAppContext) {
        let folder = Folder::new();
        let (mut driven, library) = favourites_with_shelves(cx, &folder);
        let albums = driven.read(|root, cx| root.library.read(cx).favourite_albums());

        driven
            .cx
            .simulate_keystrokes("down up ctrl-shift-down ctrl-space menu");
        driven.settle();

        assert_eq!(chosen_rows(&mut driven), [0, 1]);
        assert!(driven.read(|root, _| root.menu.is_some()));

        driven.cx.simulate_keystrokes("enter");
        driven.until(|root, cx| root.player.read(cx).queue().len() == 2);

        assert_eq!(
            driven.queue(),
            [
                the_tracks_of_an_album(&library, albums[0].id),
                the_tracks_of_an_album(&library, albums[1].id)
            ]
        );
    }

    #[gpui::test]
    fn the_menu_key_on_one_favourite_artist_offers_that_artists_menu(cx: &mut TestAppContext) {
        let folder = Folder::new();
        let (mut driven, _library) = favourites_with_shelves(cx, &folder);

        driven.cx.simulate_keystrokes("down up up menu");
        driven.settle();

        assert_eq!(
            menu_labels(&mut driven)[..3],
            [menu::PLAY, menu::PLAY_NEXT, menu::ADD_TO_QUEUE]
        );
    }

    #[gpui::test]
    fn enter_on_one_favourite_artist_opens_it(cx: &mut TestAppContext) {
        let folder = Folder::new();
        let (mut driven, _library) = favourites_with_shelves(cx, &folder);
        let artists = driven.read(|root, cx| root.library.read(cx).favourite_artists());

        driven.cx.simulate_keystrokes("down up up right enter");
        driven.settle();

        assert_eq!(
            driven.read(|root, cx| root.library.read(cx).selection()),
            Selection::Artist(artists[1].id)
        );
    }

    #[gpui::test]
    fn enter_on_one_favourite_album_opens_it(cx: &mut TestAppContext) {
        let folder = Folder::new();
        let (mut driven, _library) = favourites_with_shelves(cx, &folder);
        let albums = driven.read(|root, cx| root.library.read(cx).favourite_albums());

        driven.cx.simulate_keystrokes("down up right right enter");
        driven.settle();

        assert_eq!(
            driven.read(|root, cx| root.library.read(cx).selection()),
            Selection::Album(albums[2].id)
        );
    }

    #[gpui::test]
    fn an_arrow_gives_up_the_artists_chosen_apart_on_a_shelf(cx: &mut TestAppContext) {
        let folder = Folder::new();
        let (mut driven, _library) = favourites_with_shelves(cx, &folder);
        let artists = driven.read(|root, cx| root.library.read(cx).favourite_artists());

        driven.click_with(named("artist", artists[0].id.get()), CONTROL);
        driven.click_with(named("artist", artists[1].id.get()), CONTROL);
        driven.cx.simulate_keystrokes("right");

        assert_eq!(chosen_rows(&mut driven), [2]);
    }

    #[gpui::test]
    fn unfavouriting_the_artists_chosen_on_the_shelf_lets_go_of_the_choice(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let (mut driven, _library) = favourites_with_shelves(cx, &folder);
        let artists = driven.read(|root, cx| root.library.read(cx).favourite_artists());

        driven.click_with(named("artist", artists[0].id.get()), CONTROL);
        driven.click_with(named("artist", artists[1].id.get()), CONTROL);
        driven.right_click(named("artist", artists[0].id.get()));
        pressed_in_the_menu(&mut driven, menu::UNFAVOUR);
        driven.until(|root, cx| root.library.read(cx).favourited().artists == 1);

        assert!(chosen_rows(&mut driven).is_empty());
    }

    fn three_playlists_of_a_song_each(
        folder: &Folder,
    ) -> (Arc<Library>, Vec<(resonate_core::PlaylistId, TrackId)>) {
        let library = scanned_catalog(folder, &["Echoes", "Money", "Time"]);
        let tracks = library
            .tracks(&resonate_library::TrackQuery::default())
            .expect("the scanned tracks");
        let held = ["Aa", "Bb", "Cc"]
            .into_iter()
            .zip(&tracks)
            .map(|(name, track)| {
                let playlist = library.create_playlist(name).expect("a playlist");
                library
                    .add_to_playlist(playlist, &[resonate_library::Cut::of(track)])
                    .expect("a row put in");
                (playlist, track.id)
            })
            .collect();

        (library, held)
    }

    fn song_of(
        held: &[(resonate_core::PlaylistId, TrackId)],
        playlist: resonate_core::PlaylistId,
    ) -> TrackId {
        held.iter()
            .find(|(held, _)| *held == playlist)
            .map(|(_, track)| *track)
            .expect("a playlist that was made")
    }

    #[gpui::test]
    fn a_control_click_chooses_playlists_apart_and_enter_plays_every_one_chosen(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let (library, held) = three_playlists_of_a_song_each(&folder);
        let mut driven = Driven::opened_in(cx, library, &folder);
        driven.click("tab-playlists");
        driven.until(|root, cx| root.library.read(cx).playlists().len() == 3);
        let listed = driven.read(|root, cx| root.library.read(cx).saved_playlists());

        driven.click_with(named("playlist", listed[0].id.get()), CONTROL);
        driven.click_with(named("playlist", listed[2].id.get()), CONTROL);

        assert_eq!(chosen_rows(&mut driven), [0, 2]);
        assert_eq!(driven.read(|root, cx| root.library.read(cx).opened()), None);
        assert_eq!(
            driven.read(|root, cx| root.player.read(cx).queue().len()),
            0
        );

        driven.cx.simulate_keystrokes("enter");
        driven.until(|root, cx| root.player.read(cx).queue().len() == 2);

        assert_eq!(
            driven.queue(),
            [song_of(&held, listed[0].id), song_of(&held, listed[2].id)]
        );
    }

    #[gpui::test]
    fn a_plain_click_on_a_playlist_still_opens_it_and_chooses_nothing(cx: &mut TestAppContext) {
        let folder = Folder::new();
        let (library, _) = three_playlists_of_a_song_each(&folder);
        let mut driven = Driven::opened_in(cx, library, &folder);
        driven.click("tab-playlists");
        driven.until(|root, cx| root.library.read(cx).playlists().len() == 3);
        let listed = driven.read(|root, cx| root.library.read(cx).saved_playlists());

        driven.click(named("playlist", listed[1].id.get()));

        assert_eq!(
            driven.read(|root, cx| root.library.read(cx).opened()),
            Some(listed[1].id)
        );
        assert!(chosen_rows(&mut driven).is_empty());
    }

    fn a_playlist_of_five_songs_opened(
        cx: &mut TestAppContext,
        folder: &Folder,
    ) -> (Driven, Vec<TrackId>) {
        let library = scanned_catalog(folder, &["Aa", "Bb", "Cc", "Dd", "Ee"]);
        let tracks = library
            .tracks(&resonate_library::TrackQuery::default())
            .expect("the scanned tracks");
        let playlist = library.create_playlist("Evening").expect("a playlist");
        let cuts: Vec<_> = tracks.iter().map(resonate_library::Cut::of).collect();
        library
            .add_to_playlist(playlist, &cuts)
            .expect("rows put in");
        let mut driven = Driven::opened_in(cx, library, folder);
        driven.click("tab-playlists");
        driven.until(|root, cx| root.library.read(cx).playlists().len() == 1);
        driven.click(named("playlist", playlist.get()));
        driven.until(|root, cx| root.library.read(cx).entries().len() == 5);

        (driven, tracks.iter().map(|track| track.id).collect())
    }

    fn entries_held(driven: &mut Driven) -> Vec<TrackId> {
        driven.read(|root, cx| {
            root.library
                .read(cx)
                .entries()
                .iter()
                .filter_map(|entry| entry.track.as_ref().map(|track| track.id))
                .collect()
        })
    }

    #[gpui::test]
    fn a_control_click_chooses_playlist_rows_apart_and_a_plain_click_still_plays(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let (mut driven, ids) = a_playlist_of_five_songs_opened(cx, &folder);

        driven.click_with(named("entry", 0), CONTROL);
        driven.click_with(named("entry", 2), CONTROL);

        assert_eq!(chosen_rows(&mut driven), [0, 2]);
        assert_eq!(
            driven.read(|root, cx| root.player.read(cx).queue().len()),
            0
        );

        driven.click_with(
            named("entry", 4),
            Modifiers {
                shift: true,
                ..Modifiers::none()
            },
        );

        assert_eq!(chosen_rows(&mut driven), [0, 2, 3, 4]);

        driven.click(named("entry", 1));
        driven.until(|root, cx| root.player.read(cx).queue().len() == 5);

        assert_eq!(driven.queue(), ids);
        assert_eq!(chosen_rows(&mut driven), [1]);
    }

    #[gpui::test]
    fn taking_out_playlist_rows_chosen_apart_is_one_edit_and_one_undo(cx: &mut TestAppContext) {
        let folder = Folder::new();
        let (mut driven, ids) = a_playlist_of_five_songs_opened(cx, &folder);

        driven.click_with(named("entry", 0), CONTROL);
        driven.click_with(named("entry", 2), CONTROL);
        driven.cx.simulate_keystrokes("delete");
        driven.until(|root, cx| root.library.read(cx).entries().len() == 3);

        assert_eq!(entries_held(&mut driven), [ids[1], ids[3], ids[4]]);
        assert_eq!(chosen_rows(&mut driven), [0]);

        driven
            .root
            .update(&mut driven.cx, |root, cx| root.undo_edit(cx));
        driven.until(|root, cx| root.library.read(cx).entries().len() == 5);

        assert_eq!(entries_held(&mut driven), ids);
    }

    #[gpui::test]
    fn dragging_a_playlist_row_chosen_apart_gathers_every_chosen_row_where_it_drops(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let (mut driven, ids) = a_playlist_of_five_songs_opened(cx, &folder);

        driven.click_with(named("entry", 0), CONTROL);
        driven.click_with(named("entry", 2), CONTROL);
        let from = driven.centre_of(named("entry", 0));
        let onto = driven.centre_of(named("entry", 4));
        driven.drag(from, onto);
        driven.until(|root, cx| {
            root.library
                .read(cx)
                .entries()
                .first()
                .and_then(|entry| entry.track.as_ref())
                .is_some_and(|track| track.title != "Aa")
        });

        assert_eq!(
            entries_held(&mut driven),
            [ids[1], ids[3], ids[4], ids[0], ids[2]]
        );
        assert_eq!(chosen_rows(&mut driven), [3, 4]);

        driven
            .root
            .update(&mut driven.cx, |root, cx| root.undo_edit(cx));
        driven.until(|root, cx| {
            root.library
                .read(cx)
                .entries()
                .first()
                .and_then(|entry| entry.track.as_ref())
                .is_some_and(|track| track.title == "Aa")
        });

        assert_eq!(entries_held(&mut driven), ids);
    }

    #[gpui::test]
    fn the_move_keys_gather_playlist_rows_chosen_apart_one_place_down(cx: &mut TestAppContext) {
        let folder = Folder::new();
        let (mut driven, ids) = a_playlist_of_five_songs_opened(cx, &folder);

        driven.click_with(named("entry", 0), CONTROL);
        driven.click_with(named("entry", 2), CONTROL);
        driven.cx.simulate_keystrokes("alt-down");
        driven.until(|root, cx| {
            root.library
                .read(cx)
                .entries()
                .first()
                .and_then(|entry| entry.track.as_ref())
                .is_some_and(|track| track.title != "Aa")
        });

        assert_eq!(
            entries_held(&mut driven),
            [ids[1], ids[3], ids[0], ids[2], ids[4]]
        );
        assert_eq!(chosen_rows(&mut driven), [2, 3]);
    }

    fn searched_for_echoes(driven: &mut Driven) {
        driven.click("tab-tracks");
        let model = driven.read(|root, _| root.library.clone());
        driven.cx.update(|_, cx| {
            model.update(cx, |library, cx| library.set_query("echoes".to_owned(), cx));
        });
    }

    #[gpui::test]
    fn a_control_click_chooses_top_results_songs_apart_and_enter_plays_them(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        for title in ["Echoes One", "Echoes Two", "Echoes Three"] {
            folder.tagged(&format!("{title}.wav"), 1, &[(b"INAM", title)]);
        }
        let library = catalog();
        Driven::scanned(&library, &folder);
        let mut driven = Driven::opened_in(cx, library, &folder);
        searched_for_echoes(&mut driven);
        driven.until(|root, cx| root.library.read(cx).listing().len() == 3);
        let ids = listed_ids(&mut driven);

        driven.click_with(named("track", ids[0].get()), CONTROL);
        driven.click_with(named("track", ids[2].get()), CONTROL);

        assert_eq!(chosen_rows(&mut driven), [0, 2]);
        assert_eq!(
            driven.read(|root, cx| root.player.read(cx).queue().len()),
            0
        );

        driven.cx.simulate_keystrokes("enter");
        driven.until(|root, cx| root.player.read(cx).queue().len() == 2);

        assert_eq!(driven.queue(), [ids[0], ids[2]]);
    }

    #[gpui::test]
    fn the_keys_choose_top_results_songs_and_never_an_artist_or_an_album(cx: &mut TestAppContext) {
        let folder = Folder::new();
        for title in ["Echoes One", "Echoes Two"] {
            folder.tagged(
                &format!("{title}.wav"),
                1,
                &[(b"INAM", title), (b"IART", "Echoes"), (b"IPRD", "Echoes")],
            );
        }
        let library = catalog();
        Driven::scanned(&library, &folder);
        let mut driven = Driven::opened_in(cx, library, &folder);
        searched_for_echoes(&mut driven);
        driven.until(|root, cx| {
            let library = root.library.read(cx);
            library.listing().len() == 2
                && !library.artists().is_empty()
                && !library.albums().is_empty()
        });

        driven.focus(|root| &root.search);
        for key in ["down", "ctrl-shift-down", "ctrl-shift-down", "ctrl-space"] {
            driven.cx.simulate_keystrokes(key);
            driven.settle();
        }

        assert_eq!(chosen_rows(&mut driven), [0, 1]);
        assert_eq!(
            driven.read(|root, _| root.reached().map(|reach| reach.row)),
            Some(1)
        );
    }

    #[gpui::test]
    fn the_menu_of_playlists_chosen_apart_queues_the_songs_of_each_and_nothing_else(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let (library, held) = three_playlists_of_a_song_each(&folder);
        let mut driven = Driven::opened_in(cx, library, &folder);
        driven.click("tab-playlists");
        driven.until(|root, cx| root.library.read(cx).playlists().len() == 3);
        let listed = driven.read(|root, cx| root.library.read(cx).saved_playlists());

        driven.click_with(named("playlist", listed[0].id.get()), CONTROL);
        driven.click_with(named("playlist", listed[1].id.get()), CONTROL);
        driven.right_click(named("playlist", listed[0].id.get()));

        assert_eq!(
            menu_labels(&mut driven),
            [menu::PLAY, menu::PLAY_NEXT, menu::ADD_TO_QUEUE]
        );

        pressed_in_the_menu(&mut driven, menu::ADD_TO_QUEUE);
        driven.until(|root, cx| root.player.read(cx).queue().len() == 2);

        assert_eq!(
            driven.queue(),
            [song_of(&held, listed[0].id), song_of(&held, listed[1].id)]
        );
    }

    #[gpui::test]
    fn opening_an_album_lets_go_of_the_row_reached_in_the_listing_before(cx: &mut TestAppContext) {
        let folder = Folder::new();
        folder.tagged("one.wav", 1, &[(b"INAM", "One"), (b"IPRD", "Meddle")]);
        folder.tagged("two.wav", 1, &[(b"INAM", "Two"), (b"IPRD", "Meddle")]);
        folder.tagged("three.wav", 1, &[(b"INAM", "Three"), (b"IPRD", "Other")]);
        let library = catalog();
        Driven::scanned(&library, &folder);
        let album = library
            .tracks(&resonate_library::TrackQuery::default())
            .expect("the scanned tracks")
            .into_iter()
            .find_map(|track| track.album_id)
            .expect("an album the tags named");
        let mut driven = Driven::opened_in(cx, library, &folder);
        driven.click("tab-tracks");
        driven.until(|root, cx| root.library.read(cx).tracks_counted() == 3);

        reached_in_the_tracks(&mut driven, 2);
        assert_eq!(reach_in_the_tracks(&mut driven), Some(Span::one(2)));

        let root = driven.root.clone();
        driven.cx.update(|_, cx| {
            root.update(cx, |root, cx| root.opened(Selection::Album(album), cx));
        });
        driven.settle();

        assert_eq!(reach_in_the_tracks(&mut driven), None);
    }

    #[gpui::test]
    fn an_order_or_reading_change_lets_go_of_the_row_reached_so_it_names_no_other_song(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let library = scanned_catalog(&folder, &["Echoes", "Money", "Time"]);
        let mut driven = Driven::opened_in(cx, library, &folder);
        driven.click("tab-tracks");
        driven.until(|root, cx| root.library.read(cx).tracks_counted() == 3);

        reached_in_the_tracks(&mut driven, 1);
        let root = driven.root.clone();
        driven.cx.update(|_, cx| {
            root.update(cx, |root, cx| {
                root.library
                    .update(cx, |library, cx| library.order_tracks(SortOrder::Title, cx));
            });
        });
        driven.settle();
        assert_eq!(reach_in_the_tracks(&mut driven), None);

        reached_in_the_tracks(&mut driven, 1);
        driven.cx.update(|_, cx| {
            root.update(cx, |root, cx| {
                root.library.update(cx, |library, cx| {
                    library.read_tracks(Direction::Descending, cx)
                });
            });
        });
        driven.settle();
        assert_eq!(reach_in_the_tracks(&mut driven), None);

        reached_in_the_tracks(&mut driven, 1);
        driven.settle();
        assert_eq!(reach_in_the_tracks(&mut driven), Some(Span::one(1)));
    }

    #[gpui::test]
    fn pasting_at_the_window_puts_the_words_in_the_search(cx: &mut TestAppContext) {
        let mut driven = Driven::open(cx, catalog());

        driven.cx.update(|_, cx| {
            cx.write_to_clipboard(gpui::ClipboardItem::new_string("rush of".to_owned()));
        });
        driven.cx.simulate_keystrokes("ctrl-v");
        driven.settle();

        assert_eq!(
            driven.read(|root, cx| root.search.read(cx).text().to_owned()),
            "rush of"
        );
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
    fn the_way_back_and_forward_crosses_from_one_category_to_another(cx: &mut TestAppContext) {
        let folder = Folder::new();
        folder.tagged("01.wav", 1, &[(b"INAM", "Echoes"), (b"IPRD", "Meddle")]);
        let library = catalog();
        Driven::scanned(&library, &folder);
        let mut driven = Driven::opened_in(cx, library, &folder);
        driven.click("tab-tracks");
        driven.click("tab-albums");
        driven.click("tab-artists");
        let root = driven.root.clone();

        driven
            .cx
            .update(|_, cx| root.update(cx, |root, cx| root.go_back(cx)));
        driven.settle();
        assert_eq!(driven.read(|root, _| root.pane), Pane::Albums);

        driven
            .cx
            .update(|_, cx| root.update(cx, |root, cx| root.go_back(cx)));
        driven.settle();
        assert_eq!(driven.read(|root, _| root.pane), Pane::Tracks);

        driven
            .cx
            .update(|_, cx| root.update(cx, |root, cx| root.go_forward(cx)));
        driven.settle();
        assert_eq!(driven.read(|root, _| root.pane), Pane::Albums);
        assert_eq!(
            driven.read(|root, _| root.titled.as_ref().map(ToString::to_string)),
            Some("Resonate".to_owned()),
            "a window playing nothing was not titled by the app"
        );
    }

    #[gpui::test]
    fn the_way_back_on_the_page_keeps_to_the_tab_pressed_and_follows_a_link_across(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        folder.tagged(
            "01.wav",
            1,
            &[
                (b"INAM", "Echoes"),
                (b"IPRD", "Meddle"),
                (b"IART", "Pink Floyd"),
            ],
        );
        let library = catalog();
        Driven::scanned(&library, &folder);
        let track = library
            .tracks(&resonate_library::TrackQuery::default())
            .expect("the scanned tracks")[0]
            .clone();
        let album = crate::Selection::Album(track.album_id.expect("an album the tags named"));
        let artist = crate::Selection::Artist(track.artist_id.expect("an artist the tags named"));
        let mut driven = Driven::opened_in(cx, library, &folder);
        let root = driven.root.clone();
        let standing = |driven: &mut Driven| {
            driven.read(|root, cx| (root.in_front(cx), root.library.read(cx).selection()))
        };
        let stepped_back = |driven: &mut Driven| {
            driven
                .cx
                .update(|_, cx| root.update(cx, |root, cx| root.step_back(cx)));
            driven.settle();
        };
        driven.click("tab-albums");
        driven.cx.update(|_, cx| {
            root.update(cx, |root, cx| root.opened(album, cx));
        });
        driven.settle();
        driven.cx.update(|_, cx| {
            root.update(cx, |root, cx| root.opened(artist, cx));
        });
        driven.settle();

        stepped_back(&mut driven);
        let followed_back = standing(&mut driven);
        driven.cx.update(|_, cx| {
            root.update(cx, |root, cx| root.opened(artist, cx));
        });
        driven.settle();
        driven.click("tab-albums");
        let pressed = standing(&mut driven);
        stepped_back(&mut driven);
        let within_the_tab = standing(&mut driven);
        stepped_back(&mut driven);
        let at_its_top = standing(&mut driven);
        stepped_back(&mut driven);
        let stays = standing(&mut driven);

        assert_eq!(
            followed_back,
            (Pane::Albums, album),
            "a link was not followed back"
        );
        assert_eq!(pressed.0, Pane::Albums);
        assert_eq!(within_the_tab, (Pane::Albums, album));
        assert_eq!(at_its_top, (Pane::Albums, crate::Selection::Everything));
        assert_eq!(stays.0, Pane::Albums, "the way back left the tab pressed");

        driven
            .cx
            .update(|_, cx| root.update(cx, |root, cx| root.go_back(cx)));
        driven.settle();
        assert_ne!(
            standing(&mut driven).0,
            Pane::Albums,
            "the mouse's back button no longer crosses to the tab left"
        );
    }

    fn eighty_tracks_scrolled(cx: &mut TestAppContext) -> (Driven, Pixels) {
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
        let mut driven = Driven::opened_in(cx, library, &folder);
        driven.click("tab-tracks");
        driven.until(|root, cx| root.library.read(cx).tracks_counted() == 80);
        let middle = point(px(WIDE * 0.6), px(TALL * 0.5));
        driven.scroll_at(middle, 1_000.0);
        driven.scroll_at(middle, 37.0);
        let left_at = driven.read(|root, _| root.track_rows.0.borrow().base_handle.offset().y);
        assert!(left_at < px(-500.0), "the list did not scroll: {left_at:?}");

        (driven, left_at)
    }

    #[gpui::test]
    fn clearing_a_search_lands_the_tracks_where_they_were_scrolled(cx: &mut TestAppContext) {
        let (mut driven, left_at) = eighty_tracks_scrolled(cx);

        driven.focus(|root| &root.search);
        driven.cx.simulate_input("track 7");
        driven.settle();
        driven.click("search-songs");
        driven.read(|root, _| {
            let base = root.track_rows.0.borrow().base_handle.clone();
            base.set_offset(point(px(0.0), px(0.0)));
        });
        driven.cx.simulate_keystrokes("escape");
        driven.until(|root, _| root.landing_on.is_none());
        driven.settle();

        let landed = driven.read(|root, _| root.track_rows.0.borrow().base_handle.offset().y);
        assert!(
            (f32::from(landed) - f32::from(left_at)).abs() < 1.0,
            "clearing the search landed at {landed:?} where the list was left at {left_at:?}"
        );
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

    #[gpui::test]
    fn an_album_opened_and_then_forgotten_is_left_rather_than_headed_by_its_number(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        folder.tagged("one.wav", 1, &[(b"INAM", "One"), (b"IPRD", "Meddle")]);
        let library = catalog();
        Driven::scanned(&library, &folder);
        let album = library
            .tracks(&resonate_library::TrackQuery::default())
            .expect("the scanned tracks")[0]
            .album_id
            .expect("an album the tags named");
        let mut driven = Driven::opened_in(cx, Arc::clone(&library), &folder);

        let root = driven.root.clone();
        driven.cx.update(|_, cx| {
            root.update(cx, |root, cx| {
                root.opened(crate::Selection::Album(album), cx);
            });
        });
        driven.settle();
        assert_eq!(
            driven.read(|root, cx| root.library.read(cx).selection()),
            crate::Selection::Album(album)
        );

        let roots = library.roots().expect("the roots read");
        for held in roots {
            library.remove_root(&held).expect("the root is forgotten");
        }
        driven.cx.update(|_, cx| {
            root.update(cx, |root, cx| {
                root.library.update(cx, |model, cx| model.reload(cx))
            });
        });
        driven.until(|root, cx| root.library.read(cx).selection() == crate::Selection::Everything);
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
        driven.click("category-Scrobbling");
        driven.focus(|root| &root.listenbrainz);

        driven.cx.simulate_input("token");
        driven.settle();

        let (searched, token, _) = texts(&mut driven);
        assert_eq!(token, "token");
        assert_eq!(searched, "", "the token was typed into the search as well");
    }

    struct SignsInWithOnePassword;

    impl resonate_library::Scrobblers for SignsInWithOnePassword {
        fn under(&self, _: String) -> Arc<dyn resonate_library::Scrobbler> {
            unreachable!("no ListenBrainz token is given here")
        }

        fn signed_in_to_lastfm(
            &self,
            asked: &resonate_library::LastfmSignIn,
        ) -> resonate_library::Result<resonate_library::LastfmSession> {
            let right = asked.key == "a key"
                && asked.secret == "a secret"
                && asked.user == "listener"
                && asked.password == "right";
            if !right {
                return Err(resonate_library::Error::Refused {
                    op: resonate_library::LookupOp::Token,
                    status: 401,
                });
            }
            Ok(resonate_library::LastfmSession {
                name: "listener".to_owned(),
                key: "a session key".to_owned(),
            })
        }
    }

    fn type_into(driven: &mut Driven, nth: usize, typed: &str) {
        driven.focus(move |root| &root.lastfm[nth]);
        driven.cx.simulate_input(typed);
        driven.settle();
    }

    #[gpui::test]
    fn last_fm_is_signed_in_to_from_its_group_and_the_password_is_never_kept(
        cx: &mut TestAppContext,
    ) {
        let mut driven = Driven::open(cx, catalog());
        driven.cx.update(|_, cx| {
            cx.update_global::<ResonateApp, _>(|global, _| {
                global.scrobblers = Some(Arc::new(SignsInWithOnePassword));
                global.online.enabled = true;
            });
        });
        let session =
            |_: &RootView, cx: &gpui::App| cx.global::<ResonateApp>().online.lastfm_session.clone();
        driven.click("tab-settings");
        driven.focus(|root| &root.finding);
        driven.cx.simulate_input("last.fm password");
        driven.settle();

        type_into(&mut driven, 0, "a key");
        driven.cx.simulate_keystrokes("enter");
        type_into(&mut driven, 1, "a secret");
        type_into(&mut driven, 2, "listener");
        type_into(&mut driven, 3, "wrong");
        driven.cx.simulate_keystrokes("enter");
        driven.until(|root, cx| {
            !root.lastfm_signing.is_asking() && root.lastfm[3].read(cx).text().is_empty()
        });

        assert_eq!(driven.read(session), "");
        assert_eq!(
            driven.read(|_, cx| {
                let online = &cx.global::<ResonateApp>().online;
                (online.lastfm_key.clone(), online.lastfm_secret.clone())
            }),
            ("a key".to_owned(), "a secret".to_owned())
        );

        type_into(&mut driven, 3, "right");
        driven.click("lastfm-sign-in");
        driven.until(|root, cx| !session(root, cx).is_empty());

        assert_eq!(driven.read(session), "a session key");
        assert_eq!(
            driven.read(|root, cx| root.lastfm[3].read(cx).text().to_owned()),
            ""
        );

        driven.click("lastfm-sign-out");

        assert_eq!(driven.read(session), "");
        assert_eq!(
            driven.read(|_, cx| cx.global::<ResonateApp>().online.lastfm_key.clone()),
            "a key"
        );
    }

    #[gpui::test]
    fn the_earlier_plays_are_asked_for_by_the_second_press_alone(cx: &mut TestAppContext) {
        let library = catalog();
        let owed = |library: &Library| {
            library
                .earlier_listens_owed(resonate_library::ListeningService::ListenBrainz)
                .expect("the catalog reads")
        };
        let mut driven = Driven::open(cx, Arc::clone(&library));
        driven.click("tab-settings");
        driven.focus(|root| &root.finding);
        driven.cx.simulate_input("listenbrainz token");
        driven.settle();
        driven.focus(|root| &root.listenbrainz);
        driven.cx.simulate_input("token");
        driven.cx.simulate_keystrokes("enter");
        driven.settle();

        driven.click("send-the-earlier-plays");

        assert!(driven.read(|root, _| root.telling_the_earlier_plays));
        assert!(!owed(&library), "the first press sent the earlier plays");

        driven.click("send-the-earlier-plays");

        assert!(owed(&library));
        assert!(
            driven.read(|root, _| { !root.telling_the_earlier_plays && root.earlier_plays_asked })
        );
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
    fn a_preferred_measurer_is_chosen_from_the_equaliser_pane_and_worn_at_once(
        cx: &mut TestAppContext,
    ) {
        let worn = |driven: &mut Driven| {
            driven.read(|root, cx| {
                (
                    cx.global::<ResonateApp>().measurers.fallback.clone(),
                    root.equaliser
                        .read(cx)
                        .measurer_for(None)
                        .map(str::to_owned),
                )
            })
        };
        let mut driven = Driven::open(cx, catalog());
        driven.click("tab-settings");
        driven.click("category-Equaliser");

        driven.click("measurer-rtings");

        assert_eq!(
            worn(&mut driven),
            (Some("rtings".to_owned()), Some("rtings".to_owned()))
        );

        driven.click("measurer-best-ranked");

        assert_eq!(worn(&mut driven), (None, None));
    }

    #[gpui::test]
    fn a_measurer_is_chosen_for_the_device_in_use_alone_and_worn_at_once(cx: &mut TestAppContext) {
        use resonate_engine::NodeName;

        let desk = NodeName::new("alsa_output.desk");
        let worn = |driven: &mut Driven| {
            let desk = desk.clone();
            driven.read(move |root, cx| {
                (
                    cx.global::<ResonateApp>().measurers.clone(),
                    root.equaliser
                        .read(cx)
                        .measurer_for(Some(&desk))
                        .map(str::to_owned),
                )
            })
        };
        let mut driven = Driven::hearing(
            cx,
            catalog(),
            vec![
                sink_named(1, "alsa_output.desk", "Desk speakers", true),
                sink_named(2, "alsa_output.usb", "USB DAC", false),
            ],
        );
        driven.until(|root, cx| root.player.read(cx).sinks().len() == 2);
        driven.click("tab-settings");
        driven.click("category-Equaliser");

        driven.click("measurer-rtings");
        driven.click("device-measurer-crinacle");

        let (measurers, for_the_desk) = worn(&mut driven);
        assert_eq!(measurers.fallback.as_deref(), Some("rtings"));
        assert_eq!(measurers.of_the_device(&desk), Some("crinacle"));
        assert_eq!(measurers.by_sink.len(), 1);
        assert_eq!(for_the_desk.as_deref(), Some("crinacle"));

        driven.click("device-measurer-best-ranked");

        let (measurers, for_the_desk) = worn(&mut driven);
        assert_eq!(measurers.fallback.as_deref(), Some("rtings"));
        assert!(measurers.by_sink.is_empty());
        assert_eq!(for_the_desk.as_deref(), Some("rtings"));
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
    fn forgetting_a_folder_is_armed_by_the_first_press_and_done_by_the_second(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let library = catalog();
        let music = folder.path().join("music");
        fs::create_dir_all(&music).expect("a folder to forget");
        library.add_root(&music).expect("the root is added");
        let mut driven = Driven::opened_in(cx, Arc::clone(&library), &folder);
        let forget: &'static str =
            Box::leak(format!("{}-forget", music.display()).into_boxed_str());
        driven.click("tab-settings");
        driven.click("category-Library");

        driven.click(forget);

        assert_eq!(
            driven.read(|root, _| root.forgetting_the_folder.clone()),
            Some(music.clone())
        );
        assert_eq!(
            library.roots().expect("the roots read"),
            vec![music.clone()]
        );

        driven.click(forget);

        driven.until(|_, _| library.roots().expect("the roots read").is_empty());
        assert_eq!(
            driven.read(|root, _| root.forgetting_the_folder.clone()),
            None
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
    fn the_reduce_motion_switch_reaches_gpui_and_the_app_global(cx: &mut TestAppContext) {
        let mut driven = Driven::open(cx, catalog());
        showing_settings(&mut driven, SettingsCategory::Appearance);
        assert!(!driven.read(|_, cx| cx.reduce_motion()));

        let track = named("reduce-motion", "track");
        driven.click(track);
        driven.until(|_, cx| cx.reduce_motion());

        assert!(driven.read(|_, cx| cx.global::<ResonateApp>().reduced_motion));

        driven.click(track);
        driven.until(|_, cx| !cx.reduce_motion());

        assert!(!driven.read(|_, cx| cx.global::<ResonateApp>().reduced_motion));
    }

    #[gpui::test]
    fn tabbing_onto_an_icon_only_control_names_it_where_a_pointer_would_have_shown_a_hint(
        cx: &mut TestAppContext,
    ) {
        let folder = Folder::new();
        let files = [folder.tone("one.wav", 5)];
        let mut driven = Driven::opened_in(cx, catalog(), &folder);
        driven.play(&files);
        let root = driven.root.clone();
        let named_now = |driven: &mut Driven| {
            driven
                .cx
                .update(|window, cx| root.read(cx).focused_control_named(window))
        };

        assert_eq!(named_now(&mut driven), None);
        assert!(
            tabbed_onto(&mut driven, "next"),
            "tab never reached the next button"
        );

        let caption = named_now(&mut driven).expect("the focused button is named");
        assert!(caption.contains("Next"), "the caption reads {caption:?}");

        driven.click("tab-tracks");
        driven.settle();

        assert_eq!(named_now(&mut driven), None);
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

        driven.click("listen-keep-track");
        assert!(driven.read(|root, cx| root.listen.read(cx).keeps_listening()));
        driven.click("listen-keep-track");
        assert!(!driven.read(|root, cx| root.listen.read(cx).keeps_listening()));
    }

    fn pressed_until(
        driven: &mut Driven,
        id: &'static str,
        index: usize,
        holds: impl Fn(&resonate_engine::OutputSettings) -> bool,
    ) {
        driven.click(named(id, index));
        driven.until(|root, cx| holds(root.player.read(cx).output_settings()));
    }

    fn flipped_until(
        driven: &mut Driven,
        id: &'static str,
        read: impl Fn(&resonate_engine::OutputSettings) -> bool,
    ) {
        let before = driven.read(|root, cx| read(root.player.read(cx).output_settings()));

        let track = named(id, "track");
        driven.click(track);
        driven.until(|root, cx| read(root.player.read(cx).output_settings()) != before);
        driven.click(track);
        driven.until(|root, cx| read(root.player.read(cx).output_settings()) == before);
    }

    #[gpui::test]
    fn every_choice_and_switch_of_the_processing_category_reaches_the_engine(
        cx: &mut TestAppContext,
    ) {
        use resonate_core::Trim;
        use resonate_engine::{
            Crossfeed, DitherKind, FilterPhase, Levelling, NoiseShaping, Quality, ReplayGainMode,
            Restoration,
        };

        let trim = |millibels| Trim::from_millibels(millibels).expect("a trim");
        let mut driven = Driven::open(cx, catalog());
        showing_settings(&mut driven, SettingsCategory::Processing);

        for (index, held) in [
            Quality::Fast,
            Quality::Balanced,
            Quality::High,
            Quality::VeryHigh,
        ]
        .into_iter()
        .enumerate()
        {
            pressed_until(&mut driven, "quality", index, |settings| {
                settings.quality == held
            });
        }
        for (index, held) in [
            FilterPhase::Linear,
            FilterPhase::Intermediate,
            FilterPhase::Minimum,
        ]
        .into_iter()
        .enumerate()
        {
            pressed_until(&mut driven, "filter-phase", index, |settings| {
                settings.filter_phase == held
            });
        }
        for (index, held) in [Restoration::Off, Restoration::Repair, Restoration::Extend]
            .into_iter()
            .enumerate()
        {
            pressed_until(&mut driven, "restore-lossy", index, |settings| {
                settings.restoration == held
            });
        }
        for (index, held) in [
            DitherKind::None,
            DitherKind::Rectangular,
            DitherKind::Triangular,
        ]
        .into_iter()
        .enumerate()
        {
            pressed_until(&mut driven, "dither", index, |settings| {
                settings.dither == held
            });
        }
        for (index, held) in [
            NoiseShaping::None,
            NoiseShaping::Lipshitz,
            NoiseShaping::Threshold,
        ]
        .into_iter()
        .enumerate()
        {
            pressed_until(&mut driven, "noise-shaping", index, |settings| {
                settings.noise_shaping == held
            });
        }
        for (index, held) in [
            ReplayGainMode::Off,
            ReplayGainMode::Track,
            ReplayGainMode::Album,
        ]
        .into_iter()
        .enumerate()
        {
            pressed_until(&mut driven, "replay-gain", index, |settings| {
                settings.replay_gain == held
            });
        }
        for (index, millibels) in [-600, -300, 0, 300, 600].into_iter().enumerate() {
            pressed_until(&mut driven, "pre-amp", index, |settings| {
                settings.levelling.pre_amp == trim(millibels)
            });
        }
        for (index, millibels) in [0, -300, -600, -900].into_iter().enumerate() {
            pressed_until(&mut driven, "untagged", index, |settings| {
                settings.levelling.untagged == trim(millibels)
            });
        }
        assert_eq!(
            driven.read(|root, cx| root.player.read(cx).output_settings().levelling),
            Levelling {
                pre_amp: trim(600),
                untagged: trim(-900),
            },
            "one level setting undid the other"
        );

        flipped_until(&mut driven, "true-peak", |settings| settings.true_peak);
        driven.scroll("settings", 600.0);
        for (index, held) in [
            Crossfeed::Off,
            Crossfeed::Light,
            Crossfeed::Moderate,
            Crossfeed::Strong,
        ]
        .into_iter()
        .enumerate()
        {
            pressed_until(&mut driven, "crossfeed", index, |settings| {
                settings.crossfeed == held
            });
        }
    }

    #[gpui::test]
    fn a_speaker_angle_chosen_in_the_room_correction_group_is_worn_at_once(
        cx: &mut TestAppContext,
    ) {
        let mut driven = Driven::open(cx, catalog());
        showing_settings(&mut driven, SettingsCategory::Equaliser);
        for _ in 0..60 {
            let in_reach = driven
                .cx
                .debug_bounds(named("speaker-angle", 45))
                .is_some_and(|bounds| {
                    bounds.center().y > px(150.0) && bounds.center().y < px(500.0)
                });
            if in_reach {
                break;
            }
            driven.scroll("settings", 200.0);
        }

        driven.click(named("speaker-angle", 45));

        driven.until(|_, cx| cx.global::<ResonateApp>().speaker_angle.degrees() == 45);
    }

    fn scrolled_to(driven: &mut Driven, id: &'static str) {
        for _ in 0..80 {
            let in_reach = driven.cx.debug_bounds(id).is_some_and(|bounds| {
                bounds.center().y > px(150.0) && bounds.center().y < px(500.0)
            });
            if in_reach {
                return;
            }
            driven.scroll("settings", 200.0);
        }
    }

    #[gpui::test]
    fn any_speaker_angle_and_elevation_is_reached_by_stepping_from_the_room_correction_group(
        cx: &mut TestAppContext,
    ) {
        let mut driven = Driven::open(cx, catalog());
        showing_settings(&mut driven, SettingsCategory::Equaliser);
        scrolled_to(&mut driven, "speaker-angle-more");

        driven.click("speaker-angle-more");
        driven.until(|_, cx| cx.global::<ResonateApp>().speaker_angle.degrees() == 31);
        driven.click("speaker-angle-less");
        driven.click("speaker-angle-less");
        driven.until(|_, cx| cx.global::<ResonateApp>().speaker_angle.degrees() == 29);

        scrolled_to(&mut driven, "speaker-elevation-more");
        driven.click("speaker-elevation-more");
        driven.until(|_, cx| cx.global::<ResonateApp>().speaker_elevation.degrees() == 1);
        driven.click(named("speaker-elevation", 30));
        driven.until(|_, cx| cx.global::<ResonateApp>().speaker_elevation.degrees() == 30);
        driven.click("speaker-elevation-less");
        driven.until(|_, cx| cx.global::<ResonateApp>().speaker_elevation.degrees() == 29);
    }

    #[gpui::test]
    fn measure_in_the_equaliser_pane_plays_the_sweep_and_keeps_the_fit_as_a_profile(
        cx: &mut TestAppContext,
    ) {
        struct Flat;

        impl crate::Measures for Flat {
            fn measure(
                &self,
                _through: Option<&resonate_engine::NodeName>,
                _from: &resonate_listen::Listening,
            ) -> std::result::Result<resonate_eq::Response, crate::Unmeasured> {
                let points = (0..60)
                    .map(|step| resonate_eq::Point {
                        hertz: 20.0 * 1.12_f64.powi(step),
                        decibels: if step < 20 { 6.0 } else { 0.0 },
                    })
                    .collect();
                Ok(resonate_eq::Response::from_points(points).expect("a response"))
            }
        }

        let mut driven = Driven::open(cx, catalog());
        driven.cx.update(|_, cx| {
            cx.update_global::<ResonateApp, _>(|app, _| app.measures = Some(Arc::new(Flat)));
        });
        showing_settings(&mut driven, SettingsCategory::Equaliser);
        scrolled_to(&mut driven, "measure-the-room");

        driven.click("measure-the-room");

        driven.until(|root, cx| {
            root.equaliser
                .read(cx)
                .kept()
                .iter()
                .any(|name| name.as_str() == "measured")
        });
    }

    #[gpui::test]
    fn every_choice_and_switch_of_the_output_category_reaches_the_engine(cx: &mut TestAppContext) {
        let mut driven = Driven::open(cx, catalog());
        showing_settings(&mut driven, SettingsCategory::Output);

        pressed_until(&mut driven, "sample-rate", 1, |settings| {
            !settings.prefer_bit_perfect
        });
        pressed_until(&mut driven, "sample-rate", 0, |settings| {
            settings.prefer_bit_perfect
        });
        pressed_until(&mut driven, "graph-rate", 0, |settings| {
            settings.force_graph_rate
        });
        pressed_until(&mut driven, "graph-rate", 1, |settings| {
            !settings.force_graph_rate
        });
        for (index, millis) in [100, 250, 500, 1_000].into_iter().enumerate() {
            pressed_until(&mut driven, "buffer", index, |settings| {
                settings.buffer == Duration::from_millis(millis)
            });
        }

        flipped_until(&mut driven, "device-volume", |settings| {
            settings.device_volume
        });
        flipped_until(&mut driven, "dop", |settings| settings.dop);
        flipped_until(&mut driven, "dsd-like-pcm", |settings| {
            settings.dsd_like_pcm
        });
    }

    fn sink_named(id: u32, name: &str, description: &str, is_default: bool) -> SinkInfo {
        use resonate_core::{ChannelLayout, SampleFormat, SampleRate};
        use resonate_engine::{NodeName, SinkFormats, Words};

        SinkInfo {
            id: SinkId::new(id),
            name: NodeName::new(name),
            description: description.to_owned(),
            is_default,
            is_hardware: true,
            port: None,
            profile: None,
            formats: vec![SinkFormats {
                format: SampleFormat::S32,
                words: Words::offered_for(SampleFormat::S32),
                rates: vec![SampleRate::HZ_48000],
                channels: vec![ChannelLayout::Stereo],
            }],
            allowed_rates: vec![SampleRate::HZ_48000],
            current_rate: Some(SampleRate::HZ_48000),
        }
    }

    #[gpui::test]
    fn every_device_row_is_pressed_to_choose_its_sink_and_the_default_row_lets_go(
        cx: &mut TestAppContext,
    ) {
        use resonate_engine::NodeName;

        let mut driven = Driven::hearing(
            cx,
            catalog(),
            vec![
                sink_named(1, "alsa_output.desk", "Desk speakers", true),
                sink_named(2, "alsa_output.usb", "USB DAC", false),
                sink_named(3, "bluez_output.buds", "Earbuds", false),
            ],
        );
        driven.until(|root, cx| root.player.read(cx).sinks().len() == 3);
        showing_settings(&mut driven, SettingsCategory::Output);

        let chosen = |driven: &mut Driven| {
            driven.read(|root, cx| root.player.read(cx).output_settings().sink.clone())
        };
        assert_eq!(chosen(&mut driven), None);

        for name in ["alsa_output.usb", "bluez_output.buds", "alsa_output.desk"] {
            driven.click(named_by(name));
            driven.until(|root, cx| {
                root.player.read(cx).output_settings().sink == Some(NodeName::new(name))
            });
        }

        driven.click("follow-the-default");
        driven.until(|root, cx| root.player.read(cx).output_settings().sink.is_none());
    }

    #[gpui::test]
    fn the_bluetooth_choices_appear_with_the_switch_and_each_reaches_the_engine(
        cx: &mut TestAppContext,
    ) {
        let mut driven = Driven::open(cx, catalog());
        showing_settings(&mut driven, SettingsCategory::Output);
        let wake = |driven: &mut Driven| {
            driven.read(|root, cx| root.player.read(cx).output_settings().bluetooth)
        };
        assert!(!wake(&mut driven).on);

        driven.click("bluetooth-wake-track");
        driven.until(|root, cx| root.player.read(cx).output_settings().bluetooth.on);
        driven.scroll("settings", 600.0);

        for (index, millis) in [250, 500, 750, 1_000, 1_500].into_iter().enumerate() {
            pressed_until(&mut driven, "bluetooth-lead", index, |settings| {
                settings.bluetooth.lead == Duration::from_millis(millis)
            });
        }
        for (index, seconds) in [60, 300, 900, 3_600].into_iter().enumerate() {
            pressed_until(&mut driven, "bluetooth-awake", index, |settings| {
                settings.bluetooth.awake_for == Duration::from_secs(seconds)
            });
        }
        assert!(
            wake(&mut driven).on,
            "a choice under the switch turned it off"
        );
        assert_eq!(wake(&mut driven).lead, Duration::from_millis(1_500));

        driven.scroll("settings", -600.0);
        driven.click("bluetooth-wake-track");
        driven.until(|root, cx| !root.player.read(cx).output_settings().bluetooth.on);
    }
}
