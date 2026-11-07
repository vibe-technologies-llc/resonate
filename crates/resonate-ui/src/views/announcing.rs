use std::time::{Duration, Instant};

use gpui::{Context, Div, SharedString, Stateful, Task, div, prelude::*, px};

use crate::{
    downloads,
    views::{
        root::RootView,
        spoken::{Spoken as _, Urgency, listening},
    },
};

const ANNOUNCED_AT_MOST_EVERY: Duration = Duration::from_secs(5);

const THE_NODE: &str = "progress";

#[derive(Default)]
pub(crate) struct Announcer {
    said: Option<SharedString>,
    serial: u64,
    at: Option<Instant>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Heard {
    Said,
    Held(Duration),
    Quiet,
}

impl Announcer {
    pub(crate) fn hears(&mut self, line: Option<SharedString>, now: Instant) -> Heard {
        let Some(line) = line else {
            self.said = None;
            return Heard::Quiet;
        };
        if self.said.as_ref() == Some(&line) {
            return Heard::Quiet;
        }

        let waited = self.at.map(|at| now.saturating_duration_since(at));
        match waited {
            Some(waited) if waited < ANNOUNCED_AT_MOST_EVERY => {
                Heard::Held(ANNOUNCED_AT_MOST_EVERY - waited)
            }
            Some(_) | None => {
                self.said = Some(line);
                self.serial += 1;
                self.at = Some(now);
                Heard::Said
            }
        }
    }

    pub(crate) fn standing(&self) -> Option<(u64, &SharedString)> {
        self.said.as_ref().map(|said| (self.serial, said))
    }
}

impl RootView {
    fn progress_said(&self, cx: &mut Context<Self>) -> Option<SharedString> {
        let library = self.library.read(cx);
        if let Some(stats) = library.stats() {
            return Some(match stats.discovered {
                0 => SharedString::new_static("Scanning the library"),
                found => SharedString::from(format!(
                    "Scanning the library: {} of {found} files",
                    stats.processed
                )),
            });
        }
        if library.is_enriching() {
            return Some(SharedString::new_static("Looking up details online"));
        }
        if let Some(left) = library.keeps_left() {
            return Some(downloads::keeping_said(left));
        }
        let fetching: Vec<_> = library
            .downloads()
            .iter()
            .map(|download| library.fetching(download))
            .collect();
        if fetching
            .iter()
            .copied()
            .any(downloads::Fetching::is_underway)
        {
            let arriving = library.downloads().iter().any(|download| {
                library
                    .fetched(download)
                    .is_some_and(|fetched| fetched.is_arriving())
            });
            return Some(downloads::summed_up(&fetching, arriving));
        }

        self.matches_said(cx)
    }

    pub(crate) fn keep_the_announcements(&mut self, cx: &mut Context<Self>) {
        if !listening() {
            self.announced.said = None;
            return;
        }

        let line = self.progress_said(cx);
        if let Heard::Held(wait) = self.announced.hears(line, Instant::now()) {
            self.announcing_again = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(wait).await;
                this.update(cx, |_, cx| cx.notify()).ok();
            }));
        }
    }

    pub(crate) fn announcement(&self) -> Option<Stateful<Div>> {
        let (serial, said) = self.announced.standing()?;

        Some(
            div()
                .id((THE_NODE, serial as usize))
                .told(said.clone(), Urgency::Told)
                .absolute()
                .bottom_0()
                .left_0()
                .size(px(1.0))
                .overflow_hidden(),
        )
    }
}

pub(crate) type Waking = Option<Task<()>>;

#[cfg(test)]
mod tests {
    use super::*;

    fn line(text: &'static str) -> Option<SharedString> {
        Some(SharedString::new_static(text))
    }

    #[test]
    fn a_progress_line_is_said_once_and_the_next_waits_out_the_interval() {
        let mut announcer = Announcer::default();
        let start = Instant::now();

        assert_eq!(
            announcer.hears(line("Scanning: 10 of 90"), start),
            Heard::Said
        );
        assert_eq!(
            announcer.hears(line("Scanning: 10 of 90"), start),
            Heard::Quiet
        );
        assert_eq!(
            announcer.hears(line("Scanning: 20 of 90"), start + Duration::from_secs(2)),
            Heard::Held(Duration::from_secs(3))
        );
        assert_eq!(
            announcer
                .standing()
                .map(|(serial, said)| (serial, said.to_string())),
            Some((1, "Scanning: 10 of 90".to_owned()))
        );
        assert_eq!(
            announcer.hears(line("Scanning: 20 of 90"), start + Duration::from_secs(5)),
            Heard::Said
        );
        assert_eq!(
            announcer.standing().map(|(serial, _)| serial),
            Some(2),
            "each new line is a new node, so it is announced"
        );
    }

    #[test]
    fn a_progress_that_ends_leaves_nothing_standing() {
        let mut announcer = Announcer::default();
        let start = Instant::now();
        announcer.hears(line("Scanning"), start);

        assert_eq!(announcer.hears(None, start), Heard::Quiet);
        assert_eq!(announcer.standing(), None);
    }
}
