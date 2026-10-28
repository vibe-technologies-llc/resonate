use std::sync::Arc;

use gpui::{
    AnyElement, Context, Entity, MouseButton, Pixels, Point, Role, SharedString, Subscription,
    Window, div, prelude::*, px, rgb,
};
use resonate_core::{KeptZone, Volume, ZoneId, ZoneName};
use resonate_engine::{Command, Error, Keeping, Listening, NodeName, PlaybackState, Zone, Zones};

use crate::{
    Notice, PlayerModel, ResonateApp,
    app::Moved,
    icons::{self, Icon},
    theme, toast,
    views::{
        hint::Names as _,
        kit::EndsInAnEllipsis as _,
        menu::{self, Menu},
        queue::{QueueNames, TakenBack},
        root::RootView,
        spoken::Spoken as _,
    },
};

pub(crate) const ZONE_NAME_PLACEHOLDER: &str = "Name this zone";
const SWITCHER: &str = "zones";
const SWITCHER_GROUP: &str = "zone-switcher";
const ADD_A_ZONE: &str = "Add a zone";
const RENAME_THE_ZONE: &str = "Rename this zone…";
const REMOVE_THE_ZONE: &str = "Remove this zone";
const NAME_SHOWN_AT_MOST: f32 = 120.0;
const NAMING_FIELD_WIDTH: f32 = 140.0;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Shown {
    pub(crate) id: ZoneId,
    pub(crate) name: ZoneName,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Switcher {
    shown: Vec<Shown>,
    selected: ZoneId,
}

impl Switcher {
    pub(crate) fn of(zones: impl IntoIterator<Item = (ZoneId, ZoneName)>) -> Self {
        let mut shown: Vec<Shown> = zones
            .into_iter()
            .map(|(id, name)| Shown { id, name })
            .collect();
        shown.sort_by_key(|zone| zone.id);

        Self {
            shown,
            selected: ZoneId::FIRST,
        }
    }

    pub(crate) const fn selected(&self) -> ZoneId {
        self.selected
    }

    pub(crate) fn shown(&self) -> &[Shown] {
        &self.shown
    }

    pub(crate) fn several(&self) -> bool {
        self.shown.len() > 1
    }

    pub(crate) fn name_of(&self, id: ZoneId) -> Option<&ZoneName> {
        self.shown
            .iter()
            .find(|zone| zone.id == id)
            .map(|zone| &zone.name)
    }

    pub(crate) fn select(&mut self, id: ZoneId) -> bool {
        if id == self.selected || self.name_of(id).is_none() {
            return false;
        }
        self.selected = id;
        true
    }

    pub(crate) fn added(&mut self, id: ZoneId, name: ZoneName) {
        if self.name_of(id).is_none() {
            self.shown.push(Shown { id, name });
            self.shown.sort_by_key(|zone| zone.id);
        }
        self.selected = id;
    }

    pub(crate) fn removed(&mut self, id: ZoneId) -> bool {
        if id.is_first() {
            return false;
        }
        let Some(at) = self.shown.iter().position(|zone| zone.id == id) else {
            return false;
        };
        self.shown.remove(at);
        if self.selected == id {
            self.selected = self
                .shown
                .get(at.saturating_sub(1))
                .map_or(ZoneId::FIRST, |zone| zone.id);
        }
        true
    }

    pub(crate) fn before(&self, id: ZoneId) -> ZoneId {
        self.shown
            .iter()
            .take_while(|zone| zone.id != id)
            .last()
            .map_or(ZoneId::FIRST, |zone| zone.id)
    }

    pub(crate) fn renamed(&mut self, id: ZoneId, name: ZoneName) -> bool {
        match self.shown.iter_mut().find(|zone| zone.id == id) {
            Some(zone) if zone.name != name => {
                zone.name = name;
                true
            }
            _ => false,
        }
    }

    pub(crate) const fn can_remove(&self) -> bool {
        !self.selected.is_first()
    }

    pub(crate) fn label(&self) -> Option<SharedString> {
        self.several()
            .then(|| self.name_of(self.selected))
            .flatten()
            .map(|name| SharedString::from(name.as_str().to_owned()))
    }

    pub(crate) fn saying(&self) -> String {
        let at = self
            .shown
            .iter()
            .position(|zone| zone.id == self.selected)
            .map_or(1, |at| at + 1);
        match self.name_of(self.selected) {
            Some(name) if self.several() => format!(
                "Zone: {name}, {at} of {}. Each zone plays its own queue to its own device",
                self.shown.len()
            ),
            _ => "Zones: add one to play another queue to another device".to_owned(),
        }
    }

    pub(crate) fn kept(
        &self,
        id: ZoneId,
        device: Option<&NodeName>,
        volume: Volume,
    ) -> Option<KeptZone> {
        let name = self.name_of(id)?.clone();
        Some(match id.is_first() {
            true => KeptZone {
                id,
                name,
                device: None,
                volume: None,
            },
            false => KeptZone {
                id,
                name,
                device: device.map(|device| device.as_str().into()),
                volume: Some(volume),
            },
        })
    }
}

pub(crate) struct ZoneShown {
    pub(crate) id: ZoneId,
    pub(crate) player: Entity<PlayerModel>,
    pub(crate) listening: Listening,
    pub(crate) resuming: Keeping,
    pub(crate) kept: Option<KeptZone>,
    _watched: Subscription,
}

pub(crate) fn refusal(error: &Error, switcher: &Switcher) -> String {
    match error {
        Error::DeviceHeld { by } => format!(
            "{} plays that device bit-perfect, so it can't be shared",
            switcher
                .name_of(*by)
                .map_or_else(|| "Another zone".to_owned(), ToString::to_string)
        ),
        other => toast::would_not_do(Command::Stop.kind(), other.cause()),
    }
}

impl RootView {
    pub(crate) fn zones(cx: &gpui::App) -> Arc<Zones> {
        Arc::clone(&cx.global::<ResonateApp>().zones)
    }

    pub(crate) fn watch_the_zone(zone: &Zone, cx: &mut Context<Self>) -> ZoneShown {
        let id = zone.id;
        let player = Arc::clone(&zone.player);
        let model = cx.new(|cx| PlayerModel::new(player, cx));
        let watched = cx.observe(&model, move |this, player, cx| {
            this.zone_moved(id, &player, cx);
        });

        ZoneShown {
            id,
            player: model,
            listening: Listening::default(),
            resuming: Keeping::default(),
            kept: None,
            _watched: watched,
        }
    }

    fn zone_moved(&mut self, id: ZoneId, player: &Entity<PlayerModel>, cx: &mut Context<Self>) {
        let playing = self
            .zones
            .iter()
            .any(|zone| zone.player.read(cx).state().playback == PlaybackState::Playing);
        self.library
            .update(cx, |library, _| library.heard_playing(playing));
        self.count_a_play(id, player, cx);
        self.keep_the_queue(id, player, cx);
        self.keep_the_zone(id, player, cx);
        if id != self.switcher.selected() {
            return;
        }

        self.follow_the_playing_row(player, cx);
        self.look_for_lyrics(cx);
        match player.read(cx).moved() {
            Moved::Clock => self
                .parts
                .the_clock_moved(self.pane.follows_the_clock(), cx),
            Moved::More => cx.notify(),
        }
    }

    fn keep_the_zone(&mut self, id: ZoneId, player: &Entity<PlayerModel>, cx: &mut Context<Self>) {
        if !self.library.read(cx).resumes() {
            return;
        }
        let kept = {
            let model = player.read(cx);
            self.switcher.kept(
                id,
                model.output_settings().sink.as_ref(),
                model.state().volume,
            )
        };
        let Some(shown) = self.zones.iter_mut().find(|zone| zone.id == id) else {
            return;
        };
        if kept.is_none() || shown.kept == kept {
            return;
        }
        shown.kept.clone_from(&kept);

        if let Some(kept) = kept {
            self.library
                .update(cx, |library, cx| library.keep_the_zone(kept, cx));
        }
    }

    pub(crate) fn show_zone(&mut self, id: ZoneId, cx: &mut Context<Self>) {
        let Some(player) = self
            .zones
            .iter()
            .find(|zone| zone.id == id)
            .map(|zone| zone.player.clone())
        else {
            return;
        };
        self.switcher.select(id);
        if player.entity_id() == self.player.entity_id() {
            return;
        }

        self.player.read(cx).listen_in(false);
        self.player = player;
        self.player.read(cx).listen_in(true);
        let shown = self.player.clone();
        self.visualiser
            .update(cx, |visualiser, cx| visualiser.follow(shown, cx));
        self.volume_aimed = None;
        self.muted_from = None;
        self.grabbed = None;
        self.seek_pointed = None;
        self.queue_names = QueueNames::default();
        self.took_out = TakenBack::default();
        cx.notify();
    }

    pub(crate) fn add_a_zone(&mut self, cx: &mut Context<Self>) {
        let zones = Self::zones(cx);
        let adding = cx.background_executor().spawn(async move { zones.add() });

        cx.spawn(async move |this, cx| {
            let added = adding.await;
            let _ = this.update(cx, |this, cx| match added {
                Ok(zone) => this.zone_added(&zone, cx),
                Err(error) => {
                    tracing::warn!(%error, "a zone could not be added");
                    let said = refusal(&error, &this.switcher);
                    toast::tell(Notice::Trouble(said), cx);
                }
            });
        })
        .detach();
    }

    fn zone_added(&mut self, zone: &Zone, cx: &mut Context<Self>) {
        let shown = Self::watch_the_zone(zone, cx);
        self.zones.push(shown);
        self.zones.sort_by_key(|zone| zone.id);
        self.switcher.added(zone.id, zone.name.clone());
        self.show_zone(zone.id, cx);
        let player = self.player.clone();
        self.keep_the_zone(zone.id, &player, cx);
        toast::tell(Notice::Done(format!("{} added", zone.name)), cx);
    }

    pub(crate) fn remove_the_zone(&mut self, id: ZoneId, cx: &mut Context<Self>) {
        if self.switcher.selected() == id {
            let back = self.switcher.before(id);
            self.show_zone(back, cx);
        }
        if !self.switcher.removed(id) {
            return;
        }
        self.zones.retain(|zone| zone.id != id);
        self.library
            .update(cx, |library, cx| library.forget_the_zone(id, cx));

        let zones = Self::zones(cx);
        cx.background_executor()
            .spawn(async move {
                if let Err(error) = zones.remove(id) {
                    tracing::warn!(%error, zone = %id, "a zone could not be taken away");
                }
            })
            .detach();
        cx.notify();
    }

    pub(crate) fn name_the_zone(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let selected = self.switcher.selected();
        let held = self
            .switcher
            .name_of(selected)
            .map(ToString::to_string)
            .unwrap_or_default();
        self.naming_the_zone = Some(selected);
        self.zone_name.update(cx, |field, cx| {
            field.set_text(held, cx);
            field.take_focus_and_select(window, cx);
        });
        cx.notify();
    }

    pub(crate) fn zone_named(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.naming_the_zone else {
            return;
        };
        let Some(name) = ZoneName::given(self.zone_name.read(cx).text()) else {
            self.stop_naming_the_zone(window, cx);
            return;
        };

        if self.switcher.renamed(id, name.clone())
            && let Err(error) = Self::zones(cx).rename(id, name)
        {
            tracing::warn!(%error, zone = %id, "a zone could not be renamed");
        }
        if let Some(player) = self
            .zones
            .iter()
            .find(|zone| zone.id == id)
            .map(|zone| zone.player.clone())
        {
            self.keep_the_zone(id, &player, cx);
        }
        self.stop_naming_the_zone(window, cx);
    }

    pub(crate) fn stop_naming_the_zone(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.naming_the_zone.take().is_none() {
            return;
        }
        self.zone_name.update(cx, |field, cx| field.clear(cx));
        window.focus(&self.focus, cx);
        cx.notify();
    }

    pub(crate) fn route_to(&mut self, sink: Option<NodeName>, cx: &mut Context<Self>) {
        let routed = Self::zones(cx).send(self.switcher.selected(), Command::SetSink(sink.clone()));
        match routed {
            Ok(()) => self.store(&crate::Setting::Sink(sink), cx),
            Err(error) => {
                tracing::warn!(%error, "a device was refused");
                let said = refusal(&error, &self.switcher);
                toast::tell(Notice::Trouble(said), cx);
            }
        }
    }

    pub(crate) fn zone_switcher(&self, cx: &mut Context<Self>) -> AnyElement {
        if self.naming_the_zone.is_some() {
            return div()
                .id("zone-naming")
                .flex_none()
                .w(px(NAMING_FIELD_WIDTH))
                .h(px(theme::search_height()))
                .px_2()
                .flex()
                .items_center()
                .rounded_md()
                .bg(rgb(theme::background()))
                .border_1()
                .border_color(theme::tinted(theme::accent(), 0x99))
                .text_size(px(theme::text_sm()))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_mouse_down_out(cx.listener(|this, _, window, cx| {
                    this.stop_naming_the_zone(window, cx);
                }))
                .child(self.zone_name.clone())
                .into_any_element();
        }

        let label = self.switcher.label();
        let saying = self.switcher.saying();
        let several = self.switcher.several();
        let control = div()
            .id(SWITCHER)
            .group(SWITCHER_GROUP)
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .gap_1p5()
            .h(px(theme::toggle_control()))
            .min_w(px(theme::toggle_control()))
            .px_2()
            .rounded_md()
            .cursor_pointer()
            .hover(|button| button.bg(rgb(theme::hover())))
            .when(several, |button| {
                button.bg(theme::tinted(theme::accent(), 0x1c))
            })
            .child(match several {
                true => icons::icon(Icon::Volume, theme::toggle_icon(), theme::accent()),
                false => icons::lit_on_hover(
                    icons::icon(Icon::Volume, theme::toggle_icon(), theme::muted()),
                    SWITCHER_GROUP,
                ),
            })
            .when_some(label, |button, label| {
                button.child(
                    div()
                        .max_w(px(NAME_SHOWN_AT_MOST))
                        .text_size(px(theme::text_sm()))
                        .text_color(rgb(theme::accent()))
                        .ends_in_an_ellipsis()
                        .child(label),
                )
            })
            .child(icons::icon(
                Icon::ChevronDown,
                theme::toggle_icon() * 0.75,
                theme::faint(),
            ))
            .said_as(Role::Button, &saying)
            .names(saying)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());

        let control = self.in_the_standing_ring_at(
            SWITCHER,
            control,
            |this, at, cx| {
                let menu = this.zone_menu(at);
                this.open_a_menu(menu, cx);
            },
            cx,
        );

        menu::opens_a_menu(control, |this, at, _| this.zone_menu(at), cx).into_any_element()
    }

    pub(crate) fn zone_menu(&self, at: Point<Pixels>) -> Menu {
        let mut menu = Menu::at(at);
        for zone in self.switcher.shown() {
            let id = zone.id;
            let icon = match id == self.switcher.selected() {
                true => Icon::Check,
                false => Icon::Volume,
            };
            menu = menu.does(icon, zone.name.to_string(), move |this, _, cx| {
                this.show_zone(id, cx);
            });
        }

        let selected = self.switcher.selected();
        menu = menu
            .apart()
            .does(Icon::Plus, ADD_A_ZONE, |this, _, cx| this.add_a_zone(cx))
            .does(Icon::Rename, RENAME_THE_ZONE, |this, window, cx| {
                this.name_the_zone(window, cx);
            });
        match self.switcher.can_remove() {
            true => menu.does(Icon::Delete, REMOVE_THE_ZONE, move |this, _, cx| {
                this.remove_the_zone(selected, cx);
            }),
            false => menu,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(name: &str) -> ZoneName {
        ZoneName::given(name).expect("a name")
    }

    fn zone(raw: u32) -> ZoneId {
        ZoneId::new(raw).expect("a zone number")
    }

    fn three() -> Switcher {
        Switcher::of([
            (zone(3), named("Study")),
            (ZoneId::FIRST, named("Zone 1")),
            (zone(2), named("Kitchen")),
        ])
    }

    #[test]
    fn the_switcher_opens_on_the_first_zone_with_every_zone_in_number_order() {
        let switcher = three();

        assert_eq!(switcher.selected(), ZoneId::FIRST);
        assert_eq!(
            switcher
                .shown()
                .iter()
                .map(|zone| zone.id.get())
                .collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        assert!(switcher.several());
        assert!(!switcher.can_remove());
    }

    #[test]
    fn selecting_a_zone_moves_the_switcher_and_an_unknown_or_the_same_one_does_nothing() {
        let mut switcher = three();

        assert!(switcher.select(zone(2)));
        assert!(!switcher.select(zone(2)));
        assert!(!switcher.select(zone(9)));
        assert_eq!(switcher.selected(), zone(2));
        assert!(switcher.can_remove());
        assert_eq!(switcher.label().as_deref(), Some("Kitchen"));
        assert_eq!(
            switcher.saying(),
            "Zone: Kitchen, 2 of 3. Each zone plays its own queue to its own device"
        );
    }

    #[test]
    fn a_zone_added_is_shown_in_its_place_and_selected() {
        let mut switcher = Switcher::of([(ZoneId::FIRST, named("Zone 1"))]);

        assert_eq!(switcher.label(), None);
        switcher.added(zone(2), ZoneName::first_given_to(zone(2)));

        assert_eq!(switcher.selected(), zone(2));
        assert_eq!(switcher.label().as_deref(), Some("Zone 2"));
    }

    #[test]
    fn removing_the_selected_zone_falls_back_to_the_one_before_and_the_first_is_never_removed() {
        let mut switcher = three();
        switcher.select(zone(3));

        assert_eq!(switcher.before(zone(3)), zone(2));
        assert_eq!(switcher.before(ZoneId::FIRST), ZoneId::FIRST);
        assert!(!switcher.removed(ZoneId::FIRST));
        assert!(switcher.removed(zone(3)));
        assert_eq!(switcher.selected(), zone(2));
        assert!(!switcher.removed(zone(3)));
        assert!(switcher.removed(zone(2)));
        assert_eq!(switcher.selected(), ZoneId::FIRST);
        assert!(!switcher.several());
    }

    #[test]
    fn renaming_holds_the_new_name_and_a_rename_to_the_same_name_is_nothing() {
        let mut switcher = three();

        assert!(switcher.renamed(zone(2), named("Living room")));
        assert!(!switcher.renamed(zone(2), named("Living room")));
        assert!(!switcher.renamed(zone(9), named("Nowhere")));
        assert_eq!(
            switcher.name_of(zone(2)).map(ZoneName::as_str),
            Some("Living room")
        );
    }

    #[test]
    fn the_first_zone_keeps_only_its_name_and_another_its_device_and_volume_too() {
        let switcher = three();
        let dac = NodeName::new("alsa_output.usb");
        let half = Volume::new(0.5).expect("half way");

        assert_eq!(
            switcher.kept(ZoneId::FIRST, Some(&dac), half),
            Some(KeptZone {
                id: ZoneId::FIRST,
                name: named("Zone 1"),
                device: None,
                volume: None,
            })
        );
        assert_eq!(
            switcher.kept(zone(3), Some(&dac), half),
            Some(KeptZone {
                id: zone(3),
                name: named("Study"),
                device: Some("alsa_output.usb".into()),
                volume: Some(half),
            })
        );
        assert_eq!(switcher.kept(zone(9), None, half), None);
    }

    #[test]
    fn a_device_held_by_another_zone_is_refused_in_that_zones_name() {
        let switcher = three();

        assert_eq!(
            refusal(&Error::DeviceHeld { by: zone(3) }, &switcher),
            "Study plays that device bit-perfect, so it can't be shared"
        );
    }
}
