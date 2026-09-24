use zbus::{blocking::connection, names::BusName};

use crate::{BusOp, Error, Result};

const ICON_LOADER_PATH: &str = "/KIconLoader";
const ICON_LOADER_INTERFACE: &str = "org.kde.KIconLoader";
const ICONS_CHANGED: &str = "iconChanged";
const EVERY_ICON_GROUP: i32 = 0;

pub fn tell_the_icons_changed() -> Result<()> {
    let connection = connection::Builder::session()
        .map_err(|source| Error::bus(BusOp::Connect, source))?
        .build()
        .map_err(|source| Error::bus(BusOp::Connect, source))?;

    connection
        .emit_signal(
            None::<BusName<'_>>,
            ICON_LOADER_PATH,
            ICON_LOADER_INTERFACE,
            ICONS_CHANGED,
            &(EVERY_ICON_GROUP,),
        )
        .map_err(|source| Error::bus(BusOp::Emit, source))
}
