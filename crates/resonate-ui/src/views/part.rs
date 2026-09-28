use gpui::{
    AnyElement, AnyView, AppContext as _, Context, Entity, IntoElement, ParentElement, Render,
    StyleRefinement, Styled, WeakEntity, Window, div, px,
};

use crate::{theme, views::root::RootView};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Region {
    Header,
    Sidebar,
    Pane,
    Transport,
}

impl Region {
    fn laid_out(self) -> StyleRefinement {
        let style = StyleRefinement::default();
        match self {
            Self::Header => style.flex_none().w_full().h(px(theme::header_height())),
            Self::Sidebar => style.flex_none().h_full().w(px(theme::sidebar_width())),
            Self::Pane => style.flex_1().min_w(px(0.0)).h_full(),
            Self::Transport => style
                .flex_none()
                .w_full()
                .h(px(theme::transport_height() + TRANSPORT_RULE)),
        }
    }

    fn holding(self, drawn: AnyElement) -> AnyElement {
        let holder = div().flex().size_full();
        match self {
            Self::Header | Self::Transport => holder.flex_col(),
            Self::Sidebar | Self::Pane => holder,
        }
        .child(drawn)
        .into_any_element()
    }
}

const TRANSPORT_RULE: f32 = 1.0;

pub(crate) struct Part {
    root: WeakEntity<RootView>,
    region: Region,
}

impl Render for Part {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let region = self.region;
        let drawn = self
            .root
            .update(cx, |root, cx| root.region(region, window, cx))
            .unwrap_or_else(|_| div().into_any_element());

        region.holding(drawn)
    }
}

pub(crate) struct Parts {
    header: Entity<Part>,
    sidebar: Entity<Part>,
    pane: Entity<Part>,
    transport: Entity<Part>,
}

impl Parts {
    pub(crate) fn of(root: &Entity<RootView>, cx: &mut Context<RootView>) -> Self {
        let mut part = |region| {
            cx.new(|cx| {
                cx.observe(root, |_, _, cx| cx.notify()).detach();
                Part {
                    root: root.downgrade(),
                    region,
                }
            })
        };

        Self {
            header: part(Region::Header),
            sidebar: part(Region::Sidebar),
            pane: part(Region::Pane),
            transport: part(Region::Transport),
        }
    }

    pub(crate) fn header(&self) -> AnyView {
        cached(&self.header, Region::Header)
    }

    pub(crate) fn sidebar(&self) -> AnyView {
        cached(&self.sidebar, Region::Sidebar)
    }

    pub(crate) fn pane(&self) -> AnyView {
        cached(&self.pane, Region::Pane)
    }

    pub(crate) fn transport(&self) -> AnyView {
        cached(&self.transport, Region::Transport)
    }

    pub(crate) fn the_clock_moved(&self, pane_follows: bool, cx: &mut Context<RootView>) {
        self.transport.update(cx, |_, cx| cx.notify());
        if pane_follows {
            self.pane.update(cx, |_, cx| cx.notify());
        }
    }
}

fn cached(part: &Entity<Part>, region: Region) -> AnyView {
    AnyView::from(part.clone()).cached(region.laid_out())
}
