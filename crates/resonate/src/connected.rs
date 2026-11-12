#[cfg(feature = "sync")]
use std::sync::Arc;

#[cfg(feature = "sync")]
use resonate_mpris::{Reloadable, ZoneAsked};

#[cfg(feature = "sync")]
use crate::{
    cli::TransportArgs,
    lasting, open_library, party,
    playing::{Opening, play_queue},
    reloading, settings_path, sync, together,
};
use crate::{
    cli::{self, Cli, SharedArgs, SyncArgs},
    config::Config,
    error::{Error, Result},
};

#[cfg(feature = "sync")]
pub(crate) fn synced(cli: &Cli, config: &Config, args: &SyncArgs) -> Result<()> {
    let settings = settings_path(cli)?;
    let asked = match (
        &args.join,
        &args.forget,
        &args.fetch,
        &args.from,
        &args.handoff,
    ) {
        _ if args.control.is_some() => sync::Asked::Control {
            device: args.control.as_deref().unwrap_or_default(),
            zone: args.zone.as_deref().map(ZoneAsked::read),
            command: remote_command(args)?,
        },
        (Some(code), ..) => sync::Asked::Join(code),
        _ if args.leave => sync::Asked::Leave,
        (_, Some(device), ..) => sync::Asked::Forget(device),
        (_, _, Some(playlist), Some(from), _) => sync::Asked::Fetch { playlist, from },
        (.., Some(from)) => sync::Asked::Handoff { from },
        _ => sync::Asked::Standing,
    };
    let library = match asked {
        sync::Asked::Join(_) | sync::Asked::Leave => None,
        _ => Some(Arc::new(open_library(cli, config)?)),
    };
    let Some(handed) = reloading::delivering(&settings, Some(Reloadable::Sync), || {
        sync::asked(config, &settings, library.clone(), asked)
    })?
    else {
        return Ok(());
    };
    let transport = TransportArgs {
        shuffle: false,
        repeat: None,
        volume: None,
    };
    play_queue(
        cli,
        config,
        handed.items,
        library,
        Opening {
            place: handed.at,
            ..Opening::default()
        },
        &transport,
    )
}

#[cfg(feature = "sync")]
fn remote_command(args: &SyncArgs) -> Result<Option<resonate_sync::RemoteCommand>> {
    use resonate_sync::{Permille, RemoteCommand};

    let row = |given: u32| usize::try_from(given.saturating_sub(1)).unwrap_or(usize::MAX);
    if let Some(key) = args.press {
        return Ok(Some(match key {
            cli::PressArg::Play => RemoteCommand::Play,
            cli::PressArg::Pause => RemoteCommand::Pause,
            cli::PressArg::Toggle => RemoteCommand::Toggle,
            cli::PressArg::Next => RemoteCommand::Next,
            cli::PressArg::Previous => RemoteCommand::Previous,
        }));
    }
    if let Some(given) = args.seek.as_deref() {
        let at =
            lasting::lasting(given, lasting::Bare::Seconds).ok_or_else(|| Error::NotAPlace {
                given: given.trim().into(),
            })?;
        return Ok(Some(RemoteCommand::Seek(at)));
    }
    if let Some(percent) = args.set_volume {
        return Ok(Some(RemoteCommand::Volume(Permille::new(
            u16::from(percent) * 10,
        ))));
    }
    Ok(args
        .jump
        .map(|given| RemoteCommand::Jump(row(given)))
        .or_else(|| args.remove.map(|given| RemoteCommand::Remove(row(given))))
        .or_else(|| {
            args.move_row
                .zip(args.to)
                .map(|(from, to)| RemoteCommand::Move {
                    from: row(from),
                    to: row(to),
                })
        }))
}

#[cfg(not(feature = "sync"))]
pub(crate) fn synced(_cli: &Cli, _config: &Config, _args: &SyncArgs) -> Result<()> {
    Err(Error::SyncOff)
}

#[cfg(feature = "sync")]
pub(crate) fn shared(cli: &Cli, config: &Config, args: &SharedArgs) -> Result<()> {
    together::run(
        config,
        &settings_path(cli)?,
        Arc::new(open_library(cli, config)?),
        args,
    )
}

#[cfg(not(feature = "sync"))]
pub(crate) fn shared(_cli: &Cli, _config: &Config, _args: &SharedArgs) -> Result<()> {
    Err(Error::SyncOff)
}

#[cfg(feature = "sync")]
pub(crate) fn party_asked(cli: &Cli, config: &Config, wanted: &cli::PartyWanted) -> Result<()> {
    party::run(cli, config, wanted)
}

#[cfg(not(feature = "sync"))]
pub(crate) fn party_asked(_cli: &Cli, _config: &Config, _wanted: &cli::PartyWanted) -> Result<()> {
    Err(Error::SyncOff)
}

#[cfg(all(test, feature = "sync"))]
mod tests {
    use std::time::Duration;

    use clap::Parser as _;

    use super::*;
    use crate::cli::Sub;

    fn driving(arguments: &[&str]) -> Option<resonate_sync::RemoteCommand> {
        let parsed = Cli::try_parse_from(
            ["resonate", "sync", "--control", "Desk"]
                .iter()
                .chain(arguments)
                .copied(),
        );
        let Ok(Cli {
            command: Some(Sub::Sync(args)),
            ..
        }) = parsed
        else {
            panic!("{arguments:?} was refused: {parsed:?}");
        };
        remote_command(&args).expect("a command")
    }

    #[test]
    fn another_device_is_driven_by_one_flag_at_a_time_and_rows_are_counted_from_one() {
        use resonate_sync::{Permille, RemoteCommand};

        let two_at_once = Cli::try_parse_from([
            "resonate",
            "sync",
            "--control",
            "Desk",
            "--jump",
            "2",
            "--press",
            "next",
        ]);
        let a_zone_without_a_device = Cli::try_parse_from(["resonate", "sync", "--zone", "2"]);

        assert_eq!(driving(&[]), None);
        assert_eq!(driving(&["--press", "toggle"]), Some(RemoteCommand::Toggle));
        assert_eq!(
            driving(&["--seek", "1:30"]),
            Some(RemoteCommand::Seek(Duration::from_secs(90)))
        );
        assert_eq!(
            driving(&["--set-volume", "40"]),
            Some(RemoteCommand::Volume(Permille::new(400)))
        );
        assert_eq!(driving(&["--jump", "1"]), Some(RemoteCommand::Jump(0)));
        assert_eq!(driving(&["--remove", "3"]), Some(RemoteCommand::Remove(2)));
        assert_eq!(
            driving(&["--move", "4", "--to", "1"]),
            Some(RemoteCommand::Move { from: 3, to: 0 })
        );
        assert!(two_at_once.is_err());
        assert!(a_zone_without_a_device.is_err());
    }
}
