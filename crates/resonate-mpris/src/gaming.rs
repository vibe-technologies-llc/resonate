use std::{thread, time::Duration};

use crate::{
    BusOp, Error, Result,
    blocking::{Connection, Proxy, connection, fdo::DBusProxy},
};

const SERVICE: &str = "com.feralinteractive.GameMode";
const PATH: &str = "/com/feralinteractive/GameMode";

pub fn watch_game_mode() -> Result<()> {
    thread::Builder::new()
        .name("resonate-game-mode".into())
        .spawn(|| {
            loop {
                let connected = connection::Builder::session()
                    .and_then(|builder| builder.method_timeout(Duration::from_secs(2)).build());
                match connected {
                    Ok(connection) => loop {
                        let active = active_on(&connection);
                        let Ok(active) = active else {
                            break;
                        };
                        resonate_core::gaming::report(active);
                        thread::sleep(Duration::from_secs(2));
                    },
                    Err(source) => {
                        let error = Error::bus(BusOp::Connect, source);
                        tracing::debug!(%error, "GameMode is not reachable");
                    }
                }
                resonate_core::gaming::report(false);
                thread::sleep(Duration::from_secs(10));
            }
        })
        .map(drop)
        .map_err(|_| Error::ThreadStopped)
}

fn active_on(connection: &Connection) -> zbus::Result<bool> {
    let bus = DBusProxy::new(connection)?;
    if !bus.name_has_owner(SERVICE.try_into()?)? {
        return Ok(false);
    }
    let game = Proxy::new(connection, SERVICE, PATH, SERVICE)?;
    let status: i32 = game.call("QueryStatus", &(std::process::id() as i32,))?;
    Ok(matches!(status, 1 | 2))
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicI32, Ordering},
    };

    use super::*;

    struct Game(Arc<AtomicI32>);

    #[zbus::interface(name = "com.feralinteractive.GameMode")]
    impl Game {
        fn query_status(&self, pid: i32) -> i32 {
            assert_eq!(pid, std::process::id() as i32);
            self.0.load(Ordering::Relaxed)
        }
    }

    #[test]
    fn an_absent_idle_active_or_registered_game_mode_is_read_from_the_bus() {
        if std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_none() {
            return;
        }
        let client = Connection::session().expect("the session bus connects");
        let bus = DBusProxy::new(&client).expect("the bus answers");
        if bus
            .name_has_owner(SERVICE.try_into().expect("a service name"))
            .expect("the owner reads")
        {
            return;
        }
        assert!(!active_on(&client).expect("an absent daemon is inactive"));
        let status = Arc::new(AtomicI32::new(0));
        let server = connection::Builder::session()
            .and_then(|builder| builder.name(SERVICE))
            .and_then(|builder| builder.serve_at(PATH, Game(Arc::clone(&status))))
            .and_then(|builder| builder.build())
            .expect("the test GameMode service starts");
        for (value, active) in [(0, false), (1, true), (2, true), (-1, false)] {
            status.store(value, Ordering::Relaxed);
            assert_eq!(active_on(&client).expect("the status reads"), active);
        }
        server.close().expect("the service stops");
        assert!(!active_on(&client).expect("a stopped daemon is inactive"));
    }
}
