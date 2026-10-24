pub const RESONATE_HOST: &str = "resonate.tecnio.dev";
pub const RESONATE_WEB_ADDRESS: &str = "https://resonate.tecnio.dev";
pub const RESONATE_SOCKET_ADDRESS: &str = "wss://resonate.tecnio.dev/";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_addresses_name_the_one_host() {
        assert_eq!(
            RESONATE_WEB_ADDRESS.strip_prefix("https://"),
            Some(RESONATE_HOST)
        );
        assert_eq!(
            RESONATE_SOCKET_ADDRESS
                .strip_prefix("wss://")
                .and_then(|rest| rest.strip_suffix('/')),
            Some(RESONATE_HOST)
        );
    }
}
