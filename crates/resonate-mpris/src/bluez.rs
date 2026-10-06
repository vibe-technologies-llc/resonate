use std::{mem::ManuallyDrop, sync::Arc, thread, time::Duration};

use zbus::{
    blocking::{
        Connection, connection,
        fdo::{DBusProxy, InterfacesAddedIterator, ObjectManagerProxy},
        object_server::InterfaceRef,
    },
    names::BusName,
    proxy::CacheProperties,
    zvariant::ObjectPath,
};

use crate::{
    BusOp, Error, Result,
    interfaces::{PlayerInterface, Shared},
    service::OBJECT_PATH,
};

const BLUEZ: &str = "org.bluez";
const BLUEZ_ROOT: &str = "/";
const MEDIA: &str = "org.bluez.Media1";
const REGISTER_PLAYER: &str = "RegisterPlayer";
const DEADLINE: Duration = Duration::from_secs(2);

pub(crate) struct Headsets {
    connection: Connection,
    player: InterfaceRef<PlayerInterface>,
}

impl Headsets {
    pub(crate) fn start(shared: &Arc<Shared>) -> Option<Self> {
        if !shared.host.answers_headsets() {
            return None;
        }
        match Self::new(shared) {
            Ok(headsets) => Some(headsets),
            Err(error) => {
                tracing::debug!(%error, "a headset's own controls reach the player only where the desktop hands them on");
                None
            }
        }
    }

    fn new(shared: &Arc<Shared>) -> Result<Self> {
        let connection = connection::Builder::system()
            .map_err(|source| Error::bus(BusOp::Connect, source))?
            .method_timeout(DEADLINE)
            .serve_at(
                OBJECT_PATH,
                PlayerInterface {
                    shared: Arc::clone(shared),
                },
            )
            .map_err(|source| Error::bus(BusOp::Serve, source))?
            .build()
            .map_err(|source| Error::bus(BusOp::Connect, source))?;
        let player = connection
            .object_server()
            .interface::<_, PlayerInterface>(OBJECT_PATH)
            .map_err(|source| Error::bus(BusOp::Serve, source))?;

        let adapters = ObjectManagerProxy::builder(&connection)
            .destination(BLUEZ)
            .and_then(|builder| builder.path(BLUEZ_ROOT))
            .and_then(|builder| builder.cache_properties(CacheProperties::No).build())
            .map_err(|source| Error::bus(BusOp::Find, source))?;
        let arriving = adapters
            .receive_interfaces_added()
            .map_err(|source| Error::bus(BusOp::Listen, source))?;

        let listening = connection.clone();
        thread::Builder::new()
            .name("resonate-headsets".to_owned())
            .spawn(move || register_with_every_adapter(&listening, &adapters, arriving))
            .map_err(|_| Error::ThreadStopped)?;

        Ok(Self { connection, player })
    }

    pub(crate) const fn player(&self) -> &InterfaceRef<PlayerInterface> {
        &self.player
    }

    pub(crate) fn rest(self) {
        if let Err(source) = self.connection.close() {
            let error = Error::bus(BusOp::Shutdown, source);
            tracing::debug!(%error, "BlueZ was left to notice the player gone on its own");
        }
    }
}

fn register_with_every_adapter(
    connection: &Connection,
    adapters: &ObjectManagerProxy<'_>,
    arriving: InterfacesAddedIterator,
) {
    let Ok(player) = connection
        .object_server()
        .interface::<_, PlayerInterface>(OBJECT_PATH)
    else {
        tracing::warn!("the player served to BlueZ vanished from the object server");
        return;
    };

    if bluez_is_running(connection) {
        match adapters.get_managed_objects() {
            Ok(standing) => {
                for (path, interfaces) in standing {
                    if interfaces
                        .keys()
                        .any(|interface| interface.as_str() == MEDIA)
                    {
                        register(connection, &path, &player);
                    }
                }
            }
            Err(source) => {
                let error = Error::bus(BusOp::Find, source.into());
                tracing::debug!(%error, "the Bluetooth adapters already standing could not be listed");
            }
        }
    }

    let mut arriving = ManuallyDrop::new(arriving);
    for arrived in arriving.by_ref() {
        let Ok(args) = arrived.args() else {
            continue;
        };
        if args
            .interfaces_and_properties()
            .keys()
            .any(|interface| interface.as_str() == MEDIA)
        {
            register(connection, args.object_path(), &player);
        }
    }
}

fn bluez_is_running(connection: &Connection) -> bool {
    let Ok(name) = BusName::try_from(BLUEZ) else {
        return false;
    };
    DBusProxy::new(connection)
        .ok()
        .and_then(|bus| bus.name_has_owner(name).ok())
        .unwrap_or(false)
}

fn register(
    connection: &Connection,
    adapter: &ObjectPath<'_>,
    player: &InterfaceRef<PlayerInterface>,
) {
    let registered = player.get().as_registered();
    let called = connection.call_method(
        Some(BLUEZ),
        adapter,
        Some(MEDIA),
        REGISTER_PLAYER,
        &(
            ObjectPath::from_static_str_unchecked(OBJECT_PATH),
            registered,
        ),
    );

    match called {
        Ok(_) => {
            tracing::info!(%adapter, "a headset's own controls reach the player through BlueZ")
        }
        Err(source) => {
            let error = Error::bus(BusOp::Register, source);
            tracing::debug!(%error, %adapter, "BlueZ did not take the player for a headset's controls");
        }
    }
}
