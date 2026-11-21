use std::{thread, time::Duration};

use crossbeam_channel::{Receiver, Sender, unbounded};
use resonate_ui::AppIcon;
use resonate_win32::WindowIcons;

const SETTLES_AFTER: Duration = Duration::from_millis(1_500);
const BIG_SIDE: u32 = 64;
const SMALL_SIDE: u32 = 32;

pub(crate) struct Icons {
    wanted: Sender<AppIcon>,
}

impl Icons {
    pub(crate) fn start() -> Self {
        let (wanted, asked) = unbounded();

        let started = thread::Builder::new()
            .name("resonate-icon".to_owned())
            .spawn(move || follow(&asked));

        if let Err(error) = started {
            tracing::warn!(%error, "the window's icon will not follow the accent");
        }

        Self { wanted }
    }
}

impl resonate_ui::Launcher for Icons {
    fn show(&self, icon: AppIcon) {
        if self.wanted.send(icon).is_err() {
            tracing::debug!("nothing is placing the window's icon");
        }
    }
}

fn follow(asked: &Receiver<AppIcon>) {
    let mut worn = WindowIcons::new();
    while let Ok(mut icon) = asked.recv() {
        while let Ok(newer) = asked.recv_timeout(SETTLES_AFTER) {
            icon = newer;
        }
        worn_in(&mut worn, &icon);
    }
}

fn worn_in(worn: &mut WindowIcons, icon: &AppIcon) {
    let drawn = icon
        .png(BIG_SIDE)
        .and_then(|big| icon.png(SMALL_SIDE).map(|small| (big, small)));
    let (big, small) = match drawn {
        Ok(drawn) => drawn,
        Err(error) => {
            tracing::warn!(%error, "the window's icon could not be drawn in the accent");
            return;
        }
    };

    match worn.wear(&big, &small) {
        Ok(windows) => tracing::debug!(windows, "the window's icon follows the accent"),
        Err(error) => tracing::warn!(%error, "the window's icon did not follow the accent"),
    }
}
