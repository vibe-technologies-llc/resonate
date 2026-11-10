use resonate_ui::AppIcon;

pub(crate) struct Icons;

impl Icons {
    pub(crate) const fn start() -> Self {
        Self
    }
}

impl resonate_ui::Launcher for Icons {
    fn show(&self, _: AppIcon) {}
}
