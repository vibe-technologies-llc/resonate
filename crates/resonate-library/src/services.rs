use std::fmt;

#[derive(Clone, PartialEq, Eq)]
pub enum ServiceAccount {
    Subsonic {
        server: String,
        user: String,
        password: String,
    },
    Jellyfin {
        server: String,
        user: String,
        password: String,
        token: String,
        user_id: String,
        device: String,
    },
    Tidal {
        client_id: String,
        client_secret: String,
        refresh_token: String,
    },
    HifiApi {
        server: String,
        key: String,
    },
    Monochrome {
        server: String,
    },
    YoutubeMusic {
        cookies: String,
        program: String,
    },
}

impl fmt::Debug for ServiceAccount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Subsonic { .. } => "ServiceAccount::Subsonic",
            Self::Jellyfin { .. } => "ServiceAccount::Jellyfin",
            Self::Tidal { .. } => "ServiceAccount::Tidal",
            Self::HifiApi { .. } => "ServiceAccount::HifiApi",
            Self::Monochrome { .. } => "ServiceAccount::Monochrome",
            Self::YoutubeMusic { .. } => "ServiceAccount::YoutubeMusic",
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostedServices {
    Allowed,
    Declined,
}

impl HostedServices {
    #[must_use]
    pub const fn allowed(allowed: bool) -> Self {
        if allowed {
            Self::Allowed
        } else {
            Self::Declined
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Hosting {
    Hosted,
    Custom(String),
}

#[derive(Clone, PartialEq, Eq)]
pub struct SubsonicLogin {
    pub url: String,
    pub user: String,
    pub password: String,
}

#[derive(Clone, PartialEq, Eq)]
pub struct JellyfinSession {
    pub token: String,
    pub user: String,
    pub device: String,
}

#[derive(Clone, PartialEq, Eq)]
pub struct JellyfinLogin {
    pub url: String,
    pub user: String,
    pub password: String,
    pub session: Option<JellyfinSession>,
}

#[derive(Clone, PartialEq, Eq)]
pub struct TidalLogin {
    pub client_id: String,
    pub client_secret: Option<String>,
    pub refresh_token: String,
}

#[derive(Clone, PartialEq, Eq)]
pub struct Key(String);

impl Key {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct YoutubeMusicLogin {
    pub cookies: String,
    pub program: Option<String>,
}

macro_rules! withheld {
    ($($login:ident),*) => {
        $(impl fmt::Debug for $login {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(concat!(stringify!($login), "(<withheld>)"))
            }
        })*
    };
}

withheld!(
    SubsonicLogin,
    JellyfinSession,
    JellyfinLogin,
    TidalLogin,
    Key,
    YoutubeMusicLogin
);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reachable {
    Subsonic(SubsonicLogin),
    Jellyfin(JellyfinLogin),
    Tidal(TidalLogin),
    Resonate(Key),
    HifiApi(Hosting),
    Monochrome(Hosting),
    YoutubeMusic(YoutubeMusicLogin),
}

impl Reachable {
    #[must_use]
    pub const fn scheme(&self) -> &'static str {
        match self {
            Self::Subsonic(_) => "subsonic",
            Self::Jellyfin(_) => "jellyfin",
            Self::Tidal(_) => "tidal",
            Self::Resonate(_) => "resonate",
            Self::HifiApi(_) => "hifi-api",
            Self::Monochrome(_) => "monochrome",
            Self::YoutubeMusic(_) => "ytmusic",
        }
    }

    #[must_use]
    pub const fn fetches_for_wants(&self) -> bool {
        !matches!(self, Self::Jellyfin(_))
    }

    const fn rank(&self) -> u8 {
        match self {
            Self::Subsonic(_) => 0,
            Self::Jellyfin(_) => 1,
            Self::Tidal(_) => 2,
            Self::Resonate(_) => 3,
            Self::HifiApi(_) => 4,
            Self::Monochrome(_) => 5,
            Self::YoutubeMusic(_) => 6,
        }
    }
}

fn given(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

fn typed(value: &str) -> Option<String> {
    (!value.trim().is_empty()).then(|| value.to_owned())
}

fn hosting(server: &str, hosted: HostedServices) -> Option<Hosting> {
    match given(server) {
        Some(server) => Some(Hosting::Custom(server)),
        None => (hosted == HostedServices::Allowed).then_some(Hosting::Hosted),
    }
}

impl ServiceAccount {
    #[must_use]
    pub fn services(&self, hosted: HostedServices) -> Vec<Reachable> {
        match self {
            Self::Subsonic {
                server,
                user,
                password,
            } => subsonic(server, user, password).into_iter().collect(),
            Self::Jellyfin {
                server,
                user,
                password,
                token,
                user_id,
                device,
            } => jellyfin(server, user, password, [token, user_id, device])
                .into_iter()
                .collect(),
            Self::Tidal {
                client_id,
                client_secret,
                refresh_token,
            } => given(client_id)
                .zip(given(refresh_token))
                .map(|(client_id, refresh_token)| {
                    Reachable::Tidal(TidalLogin {
                        client_id,
                        client_secret: given(client_secret),
                        refresh_token,
                    })
                })
                .into_iter()
                .collect(),
            Self::HifiApi { server, key } => given(key)
                .map(|key| Reachable::Resonate(Key(key)))
                .into_iter()
                .chain(hosting(server, hosted).map(Reachable::HifiApi))
                .collect(),
            Self::Monochrome { server } => hosting(server, hosted)
                .map(Reachable::Monochrome)
                .into_iter()
                .collect(),
            Self::YoutubeMusic { cookies, program } => {
                vec![Reachable::YoutubeMusic(YoutubeMusicLogin {
                    cookies: cookies.trim().to_owned(),
                    program: given(program),
                })]
            }
        }
    }
}

fn subsonic(server: &str, user: &str, password: &str) -> Option<Reachable> {
    Some(Reachable::Subsonic(SubsonicLogin {
        url: given(server)?,
        user: given(user)?,
        password: typed(password)?,
    }))
}

fn jellyfin(
    server: &str,
    user: &str,
    password: &str,
    [token, user_id, device]: [&str; 3],
) -> Option<Reachable> {
    let url = given(server)?;
    let session =
        given(token)
            .zip(given(user_id))
            .zip(given(device))
            .map(|((token, user), device)| JellyfinSession {
                token,
                user,
                device,
            });
    if session.is_some() {
        return Some(Reachable::Jellyfin(JellyfinLogin {
            url,
            user: given(user).unwrap_or_default(),
            password: String::new(),
            session,
        }));
    }
    Some(Reachable::Jellyfin(JellyfinLogin {
        url,
        user: given(user)?,
        password: typed(password)?,
        session: None,
    }))
}

#[must_use]
pub fn planned<T>(
    accounts: impl IntoIterator<Item = (T, ServiceAccount)>,
    hosted: HostedServices,
) -> Vec<(T, Reachable)>
where
    T: Clone,
{
    let mut planned: Vec<(T, Reachable)> = Vec::new();
    for (tag, account) in accounts {
        for service in account.services(hosted) {
            if planned
                .iter()
                .all(|(_, held)| held.scheme() != service.scheme())
            {
                planned.push((tag.clone(), service));
            }
        }
    }
    planned.sort_by_key(|(_, service)| service.rank());
    planned
}

#[must_use]
pub fn reachable(
    accounts: impl IntoIterator<Item = ServiceAccount>,
    hosted: HostedServices,
) -> Vec<Reachable> {
    planned(accounts.into_iter().map(|account| ((), account)), hosted)
        .into_iter()
        .map(|((), service)| service)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schemes(services: &[Reachable]) -> Vec<&'static str> {
        services.iter().map(Reachable::scheme).collect()
    }

    fn hifi(server: &str, key: &str) -> ServiceAccount {
        ServiceAccount::HifiApi {
            server: server.to_owned(),
            key: key.to_owned(),
        }
    }

    #[test]
    fn a_key_is_the_resonate_server_beside_whatever_hifi_api_the_account_names() {
        let named = reachable(
            [hifi("https://hifi.home.arpa", " key ")],
            HostedServices::Declined,
        );
        let hosted = reachable([hifi("", "key")], HostedServices::Allowed);
        let declined = reachable([hifi(" ", "key")], HostedServices::Declined);
        let unkeyed = reachable([hifi("", "  ")], HostedServices::Allowed);

        assert_eq!(
            named,
            vec![
                Reachable::Resonate(Key("key".to_owned())),
                Reachable::HifiApi(Hosting::Custom("https://hifi.home.arpa".to_owned())),
            ]
        );
        assert_eq!(schemes(&hosted), ["resonate", "hifi-api"]);
        assert_eq!(hosted[1], Reachable::HifiApi(Hosting::Hosted));
        assert_eq!(schemes(&declined), ["resonate"]);
        assert_eq!(unkeyed, vec![Reachable::HifiApi(Hosting::Hosted)]);
    }

    #[test]
    fn services_are_planned_in_one_order_whatever_order_the_accounts_were_given_in() {
        let planned = reachable(
            [
                ServiceAccount::Monochrome {
                    server: String::new(),
                },
                hifi("", "key"),
                ServiceAccount::Tidal {
                    client_id: "client".to_owned(),
                    client_secret: String::new(),
                    refresh_token: "token".to_owned(),
                },
                ServiceAccount::Subsonic {
                    server: "https://music.home.arpa".to_owned(),
                    user: "ada".to_owned(),
                    password: " pass ".to_owned(),
                },
            ],
            HostedServices::Allowed,
        );

        assert_eq!(
            schemes(&planned),
            ["subsonic", "tidal", "resonate", "hifi-api", "monochrome"]
        );
        assert_eq!(
            planned[0],
            Reachable::Subsonic(SubsonicLogin {
                url: "https://music.home.arpa".to_owned(),
                user: "ada".to_owned(),
                password: " pass ".to_owned(),
            })
        );
    }

    #[test]
    fn an_account_missing_what_it_needs_plans_nothing_and_the_first_of_a_scheme_stands() {
        let first = ServiceAccount::Monochrome {
            server: "https://first.home.arpa".to_owned(),
        };
        let second = ServiceAccount::Monochrome {
            server: "https://second.home.arpa".to_owned(),
        };

        let planned = planned(
            [
                (1, first),
                (2, second),
                (
                    3,
                    ServiceAccount::Subsonic {
                        server: "https://music.home.arpa".to_owned(),
                        user: "ada".to_owned(),
                        password: "  ".to_owned(),
                    },
                ),
                (
                    4,
                    ServiceAccount::Tidal {
                        client_id: "client".to_owned(),
                        client_secret: String::new(),
                        refresh_token: String::new(),
                    },
                ),
            ],
            HostedServices::Allowed,
        );

        assert_eq!(planned.len(), 1);
        assert_eq!(planned[0].0, 1);
    }

    #[test]
    fn a_jellyfin_account_is_ready_with_a_password_or_a_whole_kept_session_and_fetches_nothing() {
        let jellyfin = |user: &str, password: &str, session: [&str; 3]| ServiceAccount::Jellyfin {
            server: "https://jellyfin.home.arpa".to_owned(),
            user: user.to_owned(),
            password: password.to_owned(),
            token: session[0].to_owned(),
            user_id: session[1].to_owned(),
            device: session[2].to_owned(),
        };

        let by_password = reachable(
            [jellyfin("ada", "pass", ["", "", ""])],
            HostedServices::Declined,
        );
        let by_session = reachable(
            [jellyfin("", "", ["token", "user", "device"])],
            HostedServices::Declined,
        );
        let half = reachable(
            [jellyfin("", "", ["token", "", "device"])],
            HostedServices::Declined,
        );

        assert_eq!(schemes(&by_password), ["jellyfin"]);
        assert!(!by_password[0].fetches_for_wants());
        assert!(matches!(
            &by_session[0],
            Reachable::Jellyfin(JellyfinLogin { session: Some(_), password, .. }) if password.is_empty()
        ));
        assert!(half.is_empty());
    }

    #[test]
    fn no_login_prints_what_it_holds() {
        let printed = format!(
            "{:?}",
            reachable([hifi("", "hush-secret")], HostedServices::Declined)
        );

        assert!(!printed.contains("hush-secret"));
    }
}
