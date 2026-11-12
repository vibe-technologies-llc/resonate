use std::{
    fs,
    num::NonZeroU32,
    path::Path,
    sync::Arc,
    time::{Instant, SystemTime},
};

use gpui::{App, Context, Image, Pixels, RenderImage, px};
use resonate_core::{AlbumId, ArtistId};
use resonate_library::{CoverArt, Drawing, Mbid, Raster};

use super::{
    A_FAILED_COVER_IS_ASKED_AGAIN_AFTER, DECODES_AT_ONCE, DECODES_KEPT_FOR_WHAT_IS_SHOWN,
    FETCHES_AT_ONCE, LibraryModel, side,
    watched::{drawn_cover, drawn_portrait, picture_or_trouble, whole_cover_of},
};
use crate::{drawing::Drawer, recent::Leaving, theme, words};

impl LibraryModel {
    pub fn scaled_by(&mut self, scale: Scale) {
        self.scale = scale;
    }

    pub fn cover_is_absent(&self, id: AlbumId, drawn: Drawn) -> bool {
        let wanted = AtSide {
            key: id,
            side: drawn.side(self.scale),
        };
        matches!(self.covers.peek(&wanted), Some(None))
    }

    pub fn cover_ahead(&mut self, id: AlbumId, drawn: Drawn, cx: &mut Context<Self>) -> bool {
        if self.decoding.len() + DECODES_KEPT_FOR_WHAT_IS_SHOWN >= DECODES_AT_ONCE {
            return false;
        }
        let _ = self.cover(id, drawn, cx);
        true
    }

    pub fn cover(&mut self, id: AlbumId, drawn: Drawn, cx: &mut Context<Self>) -> Option<Picture> {
        let wanted = AtSide {
            key: id,
            side: drawn.side(self.scale),
        };
        if let Some(held) = self.covers.get(&wanted) {
            return held.clone();
        }
        if self.decoding.contains(&wanted) || self.decoding.len() >= DECODES_AT_ONCE {
            return None;
        }
        self.decoding.insert(wanted.clone());

        let library = Arc::clone(&self.library);
        let side = wanted.side;
        let drawing = cx
            .global::<Drawer>()
            .draw(move || drawn_cover(&library, id, side));
        cx.spawn(async move |this, cx| {
            let read = drawing.await;
            let landed = this.update(cx, |this, cx| {
                let decoded = picture_or_trouble(read, words::models::read_a_cover(), cx);
                this.decoding.remove(&wanted);
                this.covers.insert(wanted, decoded).forget(cx);
                cx.notify();
            });
            let _ = landed;
        })
        .detach();
        None
    }

    pub fn whole_cover(&mut self, id: AlbumId, cx: &mut Context<Self>) -> Option<Picture> {
        if let Some(magnified) = &self.magnified
            && magnified.names(&id)
        {
            return magnified.whole();
        }
        self.magnified.replace(Magnifying::Reading(id)).forget(cx);

        let library = Arc::clone(&self.library);
        let drawing = cx
            .global::<Drawer>()
            .draw(move || whole_cover_of(&library, id));
        cx.spawn(async move |this, cx| {
            let read = drawing.await;
            let landed = this.update(cx, |this, cx| {
                let read = picture_or_trouble(read, words::models::read_a_cover(), cx);
                if this.magnified.as_ref().is_some_and(|held| held.names(&id)) {
                    this.magnified
                        .replace(Magnifying::Read(id, read))
                        .forget(cx);
                    cx.notify();
                } else {
                    read.forget(cx);
                }
            });
            let _ = landed;
        })
        .detach();
        None
    }

    pub fn released_cover(
        &mut self,
        release: &Mbid,
        drawn: Drawn,
        cx: &mut Context<Self>,
    ) -> Option<Picture> {
        self.released_cover_with_group(release, None, drawn, cx)
    }

    pub fn released_cover_with_group(
        &mut self,
        release: &Mbid,
        group: Option<&Mbid>,
        drawn: Drawn,
        cx: &mut Context<Self>,
    ) -> Option<Picture> {
        let wanted = AtSide {
            key: release.clone(),
            side: drawn.side(self.scale),
        };
        if let Some(held) = self.released_covers.get(&wanted) {
            return held.clone();
        }
        let reference = self.reference.clone().filter(|_| self.online);
        if reference.is_none() && group.is_none() {
            return None;
        }
        if self.fetching_covers.contains(release)
            || self.fetching_covers.len() >= FETCHES_AT_ONCE
            || self
                .covers_unfetched
                .get(release)
                .is_some_and(|failed| failed.elapsed() < A_FAILED_COVER_IS_ASKED_AGAIN_AFTER)
        {
            return None;
        }
        self.fetching_covers.insert(release.clone());

        let asked = release.clone();
        let group = group.cloned();
        let library = Arc::clone(&self.library);
        let fetched = cx.background_executor().spawn(async move {
            if let Some(group) = &group {
                match library.unheld_cover(group) {
                    Ok(Some(kept)) => return (Ok(Some(kept)), true),
                    Ok(None) => {}
                    Err(error) => {
                        tracing::warn!(%error, "a kept cover for a release not held could not be read");
                    }
                }
            }
            match reference {
                Some(reference) => (reference.cover(&asked, group.as_ref()), true),
                None => (Ok(None), false),
            }
        });
        let side = wanted.side;
        cx.spawn(async move |this, cx| {
            let (art, answered) = fetched.await;
            let (art, answered) = match art {
                Ok(art) => (art, answered),
                Err(error) => {
                    tracing::warn!(%error, "a found song's cover could not be fetched");
                    (None, false)
                }
            };
            let drawing = this.update(cx, |_, cx| {
                cx.global::<Drawer>()
                    .draw(move || art.as_ref().and_then(|art| drawn_within(art, side)))
            });
            let Ok(drawing) = drawing else {
                return;
            };
            let decoded = drawing.await.flatten();
            let landed = this.update(cx, |this, cx| {
                this.fetching_covers.remove(&wanted.key);
                if answered || decoded.is_some() {
                    this.covers_unfetched.remove(&wanted.key);
                    this.released_covers.insert(wanted, decoded).forget(cx);
                    cx.notify();
                } else {
                    this.covers_unfetched.insert(wanted.key, Instant::now());
                }
            });
            let _ = landed;
        })
        .detach();
        None
    }

    pub fn portrait(
        &mut self,
        id: ArtistId,
        portrayed: Portrayed,
        cx: &mut Context<Self>,
    ) -> Option<Picture> {
        let wanted = AtSide {
            key: id,
            side: portrayed.drawn().side(self.scale),
        };
        if let Some(held) = self.portraits.get(&wanted) {
            return held.clone();
        }
        if self.decoding_portraits.contains(&wanted)
            || self.decoding_portraits.len() >= DECODES_AT_ONCE
        {
            return None;
        }
        self.decoding_portraits.insert(wanted.clone());

        let library = Arc::clone(&self.library);
        let side = wanted.side;
        let drawing = cx
            .global::<Drawer>()
            .draw(move || drawn_portrait(&library, id, side));
        cx.spawn(async move |this, cx| {
            let read = drawing.await;
            let landed = this.update(cx, |this, cx| {
                let decoded = picture_or_trouble(read, words::models::read_a_portrait(), cx);
                this.decoding_portraits.remove(&wanted);
                this.portraits.insert(wanted, decoded).forget(cx);
                cx.notify();
            });
            let _ = landed;
        })
        .detach();
        None
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Drawn {
    InARow,
    NowPlaying,
    InAGrid,
    OnThePage,
}

impl Drawn {
    pub(super) fn cell(self) -> f32 {
        match self {
            Self::InARow => theme::row_cover(),
            Self::NowPlaying => theme::now_playing_cover(),
            Self::InAGrid => theme::grid_cover(),
            Self::OnThePage => theme::scope_cover(),
        }
    }

    pub(crate) fn side(self, scale: Scale) -> NonZeroU32 {
        scale.texels_for(self.cell())
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Scale(f32);

impl Scale {
    pub const ONE: Self = Self(1.0);

    pub fn of(factor: f32) -> Self {
        if factor.is_finite() && factor > 0.0 {
            Self(factor)
        } else {
            Self::ONE
        }
    }

    pub fn snapped(self, length: Pixels) -> Pixels {
        px((f32::from(length) * self.0).round() / self.0)
    }

    fn texels_for(self, cell: f32) -> NonZeroU32 {
        let averaged = if self.0 < DRAWN_ONE_TO_ONE_FROM {
            TEXELS_A_PIXEL_BELOW_TWICE
        } else {
            1.0
        };
        side((cell * self.0 * averaged).ceil() as u32)
    }
}

pub(super) const TEXELS_A_PIXEL_BELOW_TWICE: f32 = 2.0;

pub(super) const DRAWN_ONE_TO_ONE_FROM: f32 = 2.0;

pub(super) const MAGNIFIED_AT_MOST: NonZeroU32 = side(4_096);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Portrayed {
    InARow,
    InAGrid,
    OnThePage,
}

impl Portrayed {
    pub(crate) const fn drawn(self) -> Drawn {
        match self {
            Self::InARow => Drawn::InARow,
            Self::InAGrid => Drawn::InAGrid,
            Self::OnThePage => Drawn::OnThePage,
        }
    }
}

pub(crate) type Picture = Arc<RenderImage>;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct AtSide<K> {
    pub(crate) key: K,
    pub(crate) side: NonZeroU32,
}

pub(super) fn renewed<T: PartialEq>(held: &mut Arc<[T]>, read: Vec<T>) -> bool {
    if **held == *read {
        return false;
    }
    *held = read.into();
    true
}

pub(crate) fn picture_of(raster: Raster) -> Option<Picture> {
    let Raster {
        width,
        height,
        bgra,
    } = raster;
    let held = image::RgbaImage::from_raw(width, height, bgra)?;
    Some(Arc::new(RenderImage::new(vec![image::Frame::new(held)])))
}

pub(crate) fn drawn_within(art: &CoverArt, side: NonZeroU32) -> Option<Picture> {
    picture_of(Drawing::of(art)?.no_larger_than(side)?)
}

pub(crate) fn drawn_square(art: &CoverArt, side: NonZeroU32) -> Option<Picture> {
    picture_of(Drawing::of(art)?.squared(side)?)
}

pub(crate) fn magnified_of(art: &CoverArt) -> Option<Picture> {
    drawn_within(art, MAGNIFIED_AT_MOST)
}

pub(crate) enum Magnifying<K> {
    Reading(K),
    Read(K, Option<Picture>),
}

impl<K: PartialEq> Magnifying<K> {
    pub(crate) fn names(&self, wanted: &K) -> bool {
        match self {
            Self::Reading(named) | Self::Read(named, _) => named == wanted,
        }
    }

    pub(crate) fn whole(&self) -> Option<Picture> {
        match self {
            Self::Reading(_) => None,
            Self::Read(_, whole) => whole.clone(),
        }
    }
}

pub(crate) trait Forget {
    fn forget(self, cx: &mut App);
}

impl Forget for Picture {
    fn forget(self, cx: &mut App) {
        cx.drop_image(self, None);
    }
}

impl Forget for Arc<Image> {
    fn forget(self, cx: &mut App) {
        self.remove_asset(cx);
    }
}

impl<T: Forget> Forget for Option<T> {
    fn forget(self, cx: &mut App) {
        if let Some(held) = self {
            held.forget(cx);
        }
    }
}

impl<T: Forget> Forget for Leaving<T> {
    fn forget(self, cx: &mut App) {
        for held in self {
            held.forget(cx);
        }
    }
}

impl<K> Forget for Magnifying<K> {
    fn forget(self, cx: &mut App) {
        match self {
            Self::Reading(_) => {}
            Self::Read(_, whole) => whole.forget(cx),
        }
    }
}

pub(super) fn landed_since(folder: &Path, tried: SystemTime) -> bool {
    let Ok(listed) = fs::read_dir(folder) else {
        return false;
    };
    listed.flatten().any(|entry| {
        entry
            .metadata()
            .ok()
            .filter(fs::Metadata::is_file)
            .and_then(|held| {
                held.modified()
                    .ok()
                    .max(resonate_core::file_facts::changed_at(&held))
            })
            .is_some_and(|touched| touched > tried)
    })
}
