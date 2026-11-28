use resonate_engine::Watched;

#[uniffi::export]
pub fn watch_closely(closely: bool) {
    let watched = if closely {
        Watched::Closely
    } else {
        Watched::Loosely
    };

    watched.set();
}

#[cfg(test)]
mod tests {
    use resonate_engine::Watched;

    use super::watch_closely;

    #[test]
    fn a_phone_leaving_the_foreground_lets_its_players_be_watched_loosely_until_it_comes_back() {
        watch_closely(false);

        assert_eq!(Watched::now(), Watched::Loosely);

        watch_closely(true);

        assert_eq!(Watched::now(), Watched::Closely);
    }
}
