pub fn watch() {
    #[cfg(target_os = "linux")]
    if let Err(error) = resonate_mpris::watch_game_mode() {
        tracing::debug!(%error, "GameMode is not watched");
    }
    #[cfg(windows)]
    if let Err(error) = std::thread::Builder::new()
        .name("resonate-game-mode".into())
        .spawn(|| match resonate_win32::GameModeWatch::start() {
            Ok(watch) => loop {
                resonate_core::gaming::report(watch.active());
                std::thread::sleep(std::time::Duration::from_millis(100));
            },
            Err(error) => tracing::debug!(%error, "Game Mode is not watched"),
        })
    {
        tracing::debug!(%error, "Game Mode watcher did not start");
    }
}
