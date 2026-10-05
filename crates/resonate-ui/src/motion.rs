use std::time::{Duration, Instant};

use gpui::{
    Animation, AnimationElement, AnimationExt as _, AnyElement, App, Element, ElementId,
    GlobalElementId, InspectorElementId, IntoElement, LayoutId, Pixels, Styled, Svg,
    Transformation, Window, px, size,
};

pub(crate) const ARRIVES_OVER: Duration = Duration::from_millis(150);

pub(crate) const FLIPS_OVER: Duration = Duration::from_millis(140);

pub(crate) const HANDS_OVER: Duration = Duration::from_millis(280);

pub(crate) const HINT_ARRIVES_OVER: Duration = Duration::from_millis(100);

pub(crate) const LIFT: Pixels = px(4.0);

const POPPED_FROM: f32 = 0.82;

const STARRED_FROM: f32 = 0.6;

const FADED_FROM: f32 = 0.4;

const LEAVING_GONE_BY: f32 = 0.4;

const ARRIVING_FROM: f32 = 0.3;

pub(crate) fn settling(share: f32) -> f32 {
    1.0 - (1.0 - share).powi(3)
}

pub(crate) fn smooth(share: f32) -> f32 {
    share * share * (3.0 - 2.0 * share)
}

pub(crate) fn overshooting(share: f32) -> f32 {
    const PULL: f32 = 1.7;

    let past = share - 1.0;
    1.0 + (PULL + 1.0) * past.powi(3) + PULL * past.powi(2)
}

pub(crate) fn between(from: f32, to: f32, share: f32) -> f32 {
    (to - from).mul_add(share, from)
}

pub(crate) fn turned(from: bool, to: bool, share: f32) -> f32 {
    let at = |on: bool| if on { 1.0 } else { 0.0 };
    between(at(from), at(to), share)
}

pub(crate) fn shown_while<E: IntoElement + Styled + 'static>(
    ground: E,
    id: impl Into<ElementId>,
    on: bool,
) -> Flip<E, bool> {
    flips(ground, id, on, |ground, from, to, share| {
        ground.opacity(turned(from, to, share))
    })
}

pub(crate) fn blended(from: u32, to: u32, share: f32) -> u32 {
    let channel = |shift: u32| {
        let one = ((from >> shift) & 0xff) as f32;
        let other = ((to >> shift) & 0xff) as f32;

        ((other - one).mul_add(share, one).round() as u32) << shift
    };

    channel(16) | channel(8) | channel(0)
}

pub(crate) fn leaving_opacity(share: f32) -> f32 {
    1.0 - smooth((share / LEAVING_GONE_BY).min(1.0))
}

pub(crate) fn arriving_opacity(share: f32) -> f32 {
    smooth(((share - ARRIVING_FROM) / (1.0 - ARRIVING_FROM)).clamp(0.0, 1.0))
}

pub(crate) fn lifted_in<E: IntoElement + Styled + 'static>(
    element: E,
    key: impl Into<ElementId>,
) -> AnimationElement<E> {
    element.with_animation(
        key,
        Animation::new(ARRIVES_OVER).with_easing(settling),
        |arriving, share| arriving.relative().top(LIFT * (1.0 - share)).opacity(share),
    )
}

pub(crate) fn risen_in<E: IntoElement + Styled + 'static>(
    element: E,
    key: impl Into<ElementId>,
    resting: Pixels,
) -> AnimationElement<E> {
    element.with_animation(
        key,
        Animation::new(ARRIVES_OVER).with_easing(settling),
        move |arriving, share| {
            arriving
                .bottom(resting - LIFT * (1.0 - share))
                .opacity(share)
        },
    )
}

pub(crate) fn faded_in<E: IntoElement + Styled + 'static>(
    element: E,
    key: impl Into<ElementId>,
    over: Duration,
) -> AnimationElement<E> {
    element.with_animation(
        key,
        Animation::new(over).with_easing(settling),
        |arriving, share| arriving.opacity(share),
    )
}

pub(crate) fn popped<T: Copy + PartialEq + 'static>(
    icon: Svg,
    id: impl Into<ElementId>,
    value: T,
) -> Flip<Svg, T> {
    flips(icon, id, value, |icon, _, _, share| {
        scaled(icon, between(POPPED_FROM, 1.0, share)).opacity(between(FADED_FROM, 1.0, share))
    })
}

pub(crate) fn starred(icon: Svg, id: impl Into<ElementId>, favoured: bool) -> Flip<Svg, bool> {
    flips(icon, id, favoured, |icon, from, to, share| {
        let grown = overshooting(share);
        match (from, to) {
            (false, true) => scaled(icon, between(STARRED_FROM, 1.0, grown))
                .opacity(between(FADED_FROM, 1.0, share)),
            _ => icon.opacity(between(FADED_FROM, 1.0, share)),
        }
    })
    .eased_by(linear)
}

fn scaled(icon: Svg, by: f32) -> Svg {
    icon.with_transformation(Transformation::scale(size(by, by)))
}

fn linear(share: f32) -> f32 {
    share
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Flipping<T> {
    shown: T,
    from: T,
    started: Option<Instant>,
}

impl<T: Copy + PartialEq> Flipping<T> {
    pub(crate) fn still(shown: T) -> Self {
        Self {
            shown,
            from: shown,
            started: None,
        }
    }

    pub(crate) fn towards(&mut self, to: T, now: Instant, over: Duration) {
        if to == self.shown {
            return;
        }
        let done = self.share(now, over);
        let turned_back = to == self.from && done < 1.0;
        let started = if turned_back {
            now.checked_sub(over.mul_f32(1.0 - done)).unwrap_or(now)
        } else {
            now
        };

        self.from = self.shown;
        self.shown = to;
        self.started = Some(started);
    }

    pub(crate) fn share(&self, now: Instant, over: Duration) -> f32 {
        self.started.map_or(1.0, |started| {
            (now.saturating_duration_since(started).as_secs_f32() / over.as_secs_f32()).min(1.0)
        })
    }

    pub(crate) fn at(&self, now: Instant, over: Duration) -> (T, T, f32) {
        (self.from, self.shown, self.share(now, over))
    }
}

type Animator<E, T> = Box<dyn FnOnce(E, T, T, f32) -> E>;

pub(crate) struct Flip<E, T> {
    id: ElementId,
    element: Option<E>,
    value: T,
    over: Duration,
    easing: fn(f32) -> f32,
    animator: Option<Animator<E, T>>,
}

pub(crate) fn flips<E, T>(
    element: E,
    id: impl Into<ElementId>,
    value: T,
    animator: impl FnOnce(E, T, T, f32) -> E + 'static,
) -> Flip<E, T> {
    Flip {
        id: id.into(),
        element: Some(element),
        value,
        over: FLIPS_OVER,
        easing: settling,
        animator: Some(Box::new(animator)),
    }
}

impl<E, T> Flip<E, T> {
    pub(crate) fn eased_by(mut self, easing: fn(f32) -> f32) -> Self {
        self.easing = easing;
        self
    }
}

impl<E: IntoElement + 'static, T: Copy + PartialEq + 'static> IntoElement for Flip<E, T> {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl<E: IntoElement + 'static, T: Copy + PartialEq + 'static> Element for Flip<E, T> {
    type RequestLayoutState = AnyElement;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        global_id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let Some(global_id) = global_id else {
            unreachable!("a flip always carries an id");
        };
        let value = self.value;
        let over = self.over;

        let (from, to, share) =
            window.with_element_state(global_id, |state: Option<Flipping<T>>, _| {
                let now = Instant::now();
                let mut flipping = state.unwrap_or_else(|| Flipping::still(value));
                flipping.towards(value, now, over);
                (flipping.at(now, over), flipping)
            });
        if share < 1.0 {
            window.request_animation_frame();
        }

        let (Some(element), Some(animator)) = (self.element.take(), self.animator.take()) else {
            unreachable!("a flip is laid out once");
        };
        let mut drawn = animator(element, from, to, (self.easing)(share)).into_any_element();

        (drawn.request_layout(window, cx), drawn)
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: gpui::Bounds<Pixels>,
        drawn: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) {
        drawn.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: gpui::Bounds<Pixels>,
        drawn: &mut Self::RequestLayoutState,
        _: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        drawn.paint(window, cx);
    }
}

pub(crate) struct Leaving<T> {
    pub(crate) what: T,
    since: Instant,
}

pub(crate) struct Handover<K, T> {
    shown: Option<(K, T)>,
    leaving: Option<Leaving<T>>,
    serial: u64,
}

impl<K, T> Default for Handover<K, T> {
    fn default() -> Self {
        Self {
            shown: None,
            leaving: None,
            serial: 0,
        }
    }
}

impl<K: PartialEq, T> Handover<K, T> {
    pub(crate) fn show(&mut self, key: K, what: T, now: Instant) {
        if self
            .leaving
            .as_ref()
            .is_some_and(|leaving| now.saturating_duration_since(leaving.since) >= HANDS_OVER)
        {
            self.leaving = None;
        }

        match self.shown.take() {
            Some((shown, _)) if shown == key => {}
            Some((_, before)) => {
                self.leaving = Some(Leaving {
                    what: before,
                    since: now,
                });
                self.serial += 1;
            }
            None => {}
        }
        self.shown = Some((key, what));
    }

    pub(crate) fn leaving(&self) -> Option<&Leaving<T>> {
        self.leaving.as_ref()
    }

    pub(crate) fn serial(&self) -> u64 {
        self.serial
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_song_drawn_again_hands_nothing_over() {
        let start = Instant::now();
        let mut handover = Handover::default();

        handover.show("echoes", 1, start);
        handover.show("echoes", 2, start + Duration::from_millis(40));

        assert!(handover.leaving().is_none());
        assert_eq!(handover.serial(), 0);
    }

    #[test]
    fn a_skip_keeps_the_last_song_beneath_until_the_handover_is_out() {
        let start = Instant::now();
        let mut handover = Handover::default();

        handover.show("echoes", "echoes", start);
        handover.show("time", "time", start);
        let leaving = handover.leaving().map(|leaving| leaving.what);
        let serial = handover.serial();

        assert_eq!(leaving, Some("echoes"));
        assert_eq!(serial, 1);

        handover.show("time", "time", start + HANDS_OVER);

        assert!(handover.leaving().is_none());
        assert_eq!(handover.serial(), 1);
    }

    #[test]
    fn a_skip_back_before_the_handover_ends_hands_over_again() {
        let start = Instant::now();
        let mut handover = Handover::default();

        handover.show("echoes", "echoes", start);
        handover.show("time", "time", start);
        handover.show("echoes", "echoes", start + HANDS_OVER / 3);
        let leaving = handover.leaving().map(|leaving| leaving.what);

        assert_eq!(leaving, Some("time"));
        assert_eq!(handover.serial(), 2);

        handover.show("echoes", "echoes", start + HANDS_OVER);

        assert!(handover.leaving().is_some());
    }

    #[test]
    fn a_flip_stands_still_until_its_value_changes() {
        let now = Instant::now();
        let mut flipping = Flipping::still(false);

        flipping.towards(false, now, FLIPS_OVER);

        assert_eq!(flipping.at(now, FLIPS_OVER), (false, false, 1.0));

        flipping.towards(true, now, FLIPS_OVER);

        assert_eq!(flipping.at(now, FLIPS_OVER), (false, true, 0.0));
        assert_eq!(
            flipping.at(now + FLIPS_OVER, FLIPS_OVER),
            (false, true, 1.0)
        );
    }

    #[test]
    fn a_flip_turned_back_midway_runs_home_from_where_it_stood() {
        let now = Instant::now();
        let quarter = now + FLIPS_OVER / 4;
        let mut flipping = Flipping::still(false);

        flipping.towards(true, now, FLIPS_OVER);
        flipping.towards(false, quarter, FLIPS_OVER);
        let (from, to, share) = flipping.at(quarter, FLIPS_OVER);

        assert_eq!((from, to), (true, false));
        assert!((share - 0.75).abs() < 1e-3);
    }

    #[test]
    fn the_curves_run_from_nothing_to_everything() {
        for curve in [settling, smooth, overshooting] {
            assert!(curve(0.0).abs() < 1e-6);
            assert!((curve(1.0) - 1.0).abs() < 1e-6);
        }

        assert!(overshooting(0.8) > 1.0);
        assert!(settling(0.5) > 0.5);
    }

    #[test]
    fn the_leaving_song_is_gone_before_the_arriving_one_is_whole() {
        assert!((leaving_opacity(0.0) - 1.0).abs() < 1e-6);
        assert!(leaving_opacity(LEAVING_GONE_BY).abs() < 1e-6);
        assert!(arriving_opacity(ARRIVING_FROM).abs() < 1e-6);
        assert!((arriving_opacity(1.0) - 1.0).abs() < 1e-6);
        assert!(leaving_opacity(0.5) + arriving_opacity(0.5) < 1.0);
    }

    #[test]
    fn a_blend_lands_on_each_end_and_passes_between() {
        assert_eq!(blended(0x000000, 0xffffff, 0.0), 0x000000);
        assert_eq!(blended(0x000000, 0xffffff, 1.0), 0xffffff);
        assert_eq!(blended(0x204060, 0x406080, 0.5), 0x305070);
    }
}
