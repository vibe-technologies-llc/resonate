use std::{path::Path, sync::Arc};

use resonate_core::{SampleRate, eq::Profile};
use resonate_engine::NodeName;
use resonate_eq::{Binding, Catalogue, Corrected, DeviceId, ProfileName, Store, search, suggest};
use resonate_pipewire::PipeWire;

use crate::{
    Cli, ConfigKey, Error, Result, SINK_TIMEOUT, cli::EqArgs, config, config::Config, online,
    select_sink, table::Table,
};

const DRAWN_AT: SampleRate = SampleRate::HZ_48000;

const EVERY_OTHER_DEVICE: &str = "every other device";
const ITS_OWN_CURVE: &str = "its own curve";

pub fn run(cli: &Cli, config: &Config, wanted: &EqArgs) -> Result<()> {
    let store = Store::at(config::equaliser_dir()?);
    let settings = crate::settings_path(cli)?;
    let bound = wanted.r#for.as_deref().map(NodeName::new);

    if let Some(text) = wanted.find.as_deref() {
        return find(config, text);
    }
    if wanted.suggest {
        return suggested(cli, config);
    }
    if wanted.list {
        return list(&store, config);
    }
    if let Some(name) = wanted.forget.as_deref() {
        return forget(&store, &settings, config, name);
    }
    if let Some(from) = wanted.import.as_deref() {
        return import(&store, &settings, bound.as_ref(), wanted, from);
    }
    if let Some(device) = wanted.fetch.as_deref() {
        return fetch(&store, &settings, config, bound.as_ref(), wanted, device);
    }
    if let Some(to) = wanted.export.as_deref() {
        return export(&store, config, bound.as_ref(), wanted, to);
    }
    if wanted.unbind {
        return unbind(&settings, bound.as_ref());
    }
    if let Some(name) = wanted.profile.as_deref() {
        return bind(&store, &settings, bound.as_ref(), name);
    }
    if wanted.on || wanted.off {
        config::store(&settings, ConfigKey::Equaliser, wanted.on)?;
        println!("equaliser: {}", switched(wanted.on));
        return Ok(());
    }

    print(&store, config, bound.as_ref())
}

const fn switched(on: bool) -> &'static str {
    if on { "on" } else { "off" }
}

fn named(text: &str) -> Result<ProfileName> {
    Ok(ProfileName::new(text)?)
}

fn spoken_of(sink: Option<&NodeName>) -> String {
    sink.map_or_else(
        || EVERY_OTHER_DEVICE.to_owned(),
        |sink| sink.as_str().to_owned(),
    )
}

fn spoken_binding(binding: &Binding) -> String {
    match binding {
        Binding::Profile(name) => name.to_string(),
        Binding::Own => ITS_OWN_CURVE.to_owned(),
    }
}

fn curve_of(store: &Store, owner: Option<&NodeName>, binding: &Binding) -> Result<Option<Profile>> {
    Ok(match binding {
        Binding::Profile(name) => store.read(name)?,
        Binding::Own => Some(store.own(owner.map(NodeName::as_str))?),
    })
}

fn bind(store: &Store, settings: &Path, sink: Option<&NodeName>, name: &str) -> Result<()> {
    let name = named(name)?;
    if store.read(&name)?.is_none() {
        return Err(Error::Eq(resonate_eq::Error::NoSuchProfile));
    }

    let binding = config::written(&Binding::Profile(name.clone()));
    match sink {
        Some(sink) => {
            config::store_in_table(settings, ConfigKey::EqualiserFor, sink.as_str(), binding)?;
        }
        None => config::store(settings, ConfigKey::EqualiserProfile, binding)?,
    }
    config::store(settings, ConfigKey::Equaliser, true)?;

    println!("bound {name} to {}", spoken_of(sink));
    Ok(())
}

fn unbind(settings: &Path, sink: Option<&NodeName>) -> Result<()> {
    match sink {
        Some(sink) => config::clear_in_table(settings, ConfigKey::EqualiserFor, sink.as_str())?,
        None => config::clear(settings, ConfigKey::EqualiserProfile)?,
    }
    println!("unbound {}", spoken_of(sink));
    Ok(())
}

fn import(
    store: &Store,
    settings: &Path,
    sink: Option<&NodeName>,
    wanted: &EqArgs,
    from: &Path,
) -> Result<()> {
    let called = wanted.profile.as_deref().map(named).transpose()?;
    let (name, kept) = store.import(from, called.as_ref())?;

    if kept.converted {
        println!(
            "kept a graphic curve as {name}, {} bands at {DRAWN_AT} and fitted again at the rate \
             a stream plays at",
            kept.profile.bands().len()
        );
    } else {
        println!("kept {name}, {} bands", kept.profile.bands().len());
    }
    if kept.passed_over > 0 {
        println!("{} lines were passed over", kept.passed_over);
    }

    if wanted.r#for.is_some() {
        return bind(store, settings, sink, name.as_str());
    }
    Ok(())
}

fn export(
    store: &Store,
    config: &Config,
    sink: Option<&NodeName>,
    wanted: &EqArgs,
    to: &Path,
) -> Result<()> {
    let (spoken, profile) = match wanted.profile.as_deref() {
        Some(name) => {
            let name = named(name)?;
            (name.to_string(), store.read(&name)?)
        }
        None => {
            let (owner, binding) =
                bound_to(config, sink).ok_or(Error::Eq(resonate_eq::Error::NoSuchProfile))?;
            (spoken_binding(binding), curve_of(store, owner, binding)?)
        }
    };
    let profile = profile.ok_or(Error::Eq(resonate_eq::Error::NoSuchProfile))?;

    store.export(&profile, to)?;
    println!("wrote {spoken} to {}", to.display());
    Ok(())
}

fn forget(store: &Store, settings: &Path, config: &Config, name: &str) -> Result<()> {
    let name = named(name)?;
    if !store.forget(&name)? {
        return Err(Error::Eq(resonate_eq::Error::NoSuchProfile));
    }

    let forgotten = Binding::Profile(name.clone());
    if let Some(bindings) = config.equaliser_for.as_ref() {
        if bindings.fallback() == Some(&forgotten) {
            config::clear(settings, ConfigKey::EqualiserProfile)?;
        }
        for (sink, bound) in bindings.by_sink() {
            if *bound == forgotten {
                config::clear_in_table(settings, ConfigKey::EqualiserFor, sink.as_str())?;
            }
        }
    }

    println!("forgot {name}");
    Ok(())
}

fn bound_to<'c>(
    config: &'c Config,
    sink: Option<&NodeName>,
) -> Option<(Option<&'c NodeName>, &'c Binding)> {
    config
        .equaliser_for
        .as_ref()
        .and_then(|bindings| bindings.for_sink(sink))
}

fn list(store: &Store, config: &Config) -> Result<()> {
    let names = store.names()?;
    if names.is_empty() {
        println!("no profiles are kept in {}", store.folder().display());
        return Ok(());
    }

    let mut table = Table::new(vec!["PROFILE", "BANDS", "PREAMP", "BOUND TO"]);
    for name in &names {
        let profile = store.read(name)?.unwrap_or_else(Profile::flat);
        let binding = Binding::Profile(name.clone());
        let bound = config.equaliser_for.as_ref().map_or_else(Vec::new, |held| {
            held.by_sink()
                .filter(|(_, to)| **to == binding)
                .map(|(sink, _)| sink.as_str().to_owned())
                .chain((held.fallback() == Some(&binding)).then(|| EVERY_OTHER_DEVICE.to_owned()))
                .collect()
        });

        table.push(vec![
            name.to_string(),
            profile.bands().len().to_string(),
            format!("{}", profile.preamp()),
            if bound.is_empty() {
                "—".to_owned()
            } else {
                bound.join(", ")
            },
        ]);
    }
    print!("{}", table.render());
    Ok(())
}

fn print(store: &Store, config: &Config, sink: Option<&NodeName>) -> Result<()> {
    println!("equaliser: {}", switched(config.equaliser_on()));

    match config
        .equaliser_for
        .as_ref()
        .filter(|held| !held.is_empty())
    {
        Some(bindings) => {
            let mut table = Table::new(vec!["DEVICE", "BOUND TO"]);
            if let Some(fallback) = bindings.fallback() {
                table.push(vec![
                    EVERY_OTHER_DEVICE.to_owned(),
                    spoken_binding(fallback),
                ]);
            }
            for (device, binding) in bindings.by_sink() {
                table.push(vec![device.as_str().to_owned(), spoken_binding(binding)]);
            }
            print!("{}", table.render());
        }
        None => println!("nothing is bound to any device"),
    }

    let Some((owner, binding)) = bound_to(config, sink) else {
        return Ok(());
    };
    let spoken = match (binding, owner) {
        (Binding::Own, None) if sink.is_some() => format!("the own curve of {EVERY_OTHER_DEVICE}"),
        _ => spoken_binding(binding),
    };
    let Some(profile) = curve_of(store, owner, binding)? else {
        println!("\n{spoken} is bound but no profile of that name is kept");
        return Ok(());
    };

    println!("\nfor {}: {spoken}", spoken_of(sink));
    println!("preamp: {}", profile.preamp());
    if let Some(target) = profile.target() {
        println!(
            "fitted to a graphic curve of {} points: the bands below are its fit at {DRAWN_AT}, \
             and a stream at another rate is fitted again at its own",
            target.points().len()
        );
    }
    if profile.bands().is_empty() {
        println!("it holds no bands");
        return Ok(());
    }

    let mut table = Table::new(vec!["#", "TYPE", "FREQUENCY", "GAIN", "Q", ""]);
    for (at, band) in profile.bands().iter().enumerate() {
        table.push(vec![
            (at + 1).to_string(),
            band.kind.label().to_owned(),
            format!("{}", band.frequency),
            format!("{}", band.gain),
            format!("{}", band.q),
            if band.on {
                String::new()
            } else {
                "off".to_owned()
            },
        ]);
    }
    print!("{}", table.render());
    println!("peak: {:+.2} dB", profile.peak_db(DRAWN_AT));
    Ok(())
}

fn asked(config: &Config) -> Result<(Corrected, Arc<Catalogue>)> {
    let corrections = online::corrections_asked_for(config, None)?;
    let catalogue = corrections.catalogue()?;
    Ok((corrections, catalogue))
}

fn find(config: &Config, text: &str) -> Result<()> {
    let (_, catalogue) = asked(config)?;
    let hits = search(&catalogue, text);
    if hits.is_empty() {
        println!("nothing measured answers to that");
        return Ok(());
    }

    let mut table = Table::new(vec!["DEVICE", "MEASURED BY", "RIG", "PATH"]);
    for device in hits.into_iter().filter_map(|found| catalogue.device(found)) {
        table.push(vec![
            device.label.clone(),
            device.measured_by.clone(),
            device.rig.clone().unwrap_or_else(|| "—".to_owned()),
            device.id.as_str().to_owned(),
        ]);
    }
    print!("{}", table.render());
    Ok(())
}

fn suggested(cli: &Cli, config: &Config) -> Result<()> {
    let pipewire = PipeWire::start("Resonate")?;
    let sink = select_sink(
        pipewire.enumerate_sinks(SINK_TIMEOUT)?,
        crate::wanted_sink(cli, config).as_ref(),
    )?;
    pipewire.shutdown()?;

    println!("sink:   {} ({})", sink.description, sink.name);

    let (_, catalogue) = asked(config)?;
    match suggest(&catalogue, &sink.description).and_then(|found| catalogue.device(found)) {
        Some(device) => {
            println!("looks like: {}", device.shown());
            println!(
                "fetch it:   resonate eq --fetch \"{}\" --for {}",
                device.id, sink.name
            );
        }
        None => {
            println!("nothing measured answers to that name clearly enough to be worth guessing at")
        }
    }
    Ok(())
}

fn fetch(
    store: &Store,
    settings: &Path,
    config: &Config,
    sink: Option<&NodeName>,
    wanted: &EqArgs,
    device: &str,
) -> Result<()> {
    let (corrections, _) = asked(config)?;
    let id = DeviceId::new(device)?;
    let profile = corrections
        .profile(&id)?
        .ok_or(Error::Eq(resonate_eq::Error::NoSuchProfile))?;

    let name = match wanted.profile.as_deref() {
        Some(name) => named(name)?,
        None => ProfileName::after(id.as_str().rsplit('/').next().unwrap_or(device)),
    };
    store.keep(&name, &profile)?;
    println!("kept {name}, {} bands", profile.bands().len());

    if wanted.r#for.is_some() {
        return bind(store, settings, sink, name.as_str());
    }
    Ok(())
}

#[cfg(feature = "ui")]
pub fn bound(config: &Config) -> resonate_ui::Bindings {
    let Some(bindings) = config.equaliser_for.as_ref() else {
        return resonate_ui::Bindings {
            enabled: config.equaliser_on(),
            ..resonate_ui::Bindings::default()
        };
    };

    resonate_ui::Bindings {
        enabled: config.equaliser_on(),
        fallback: bindings.fallback().cloned(),
        by_sink: bindings
            .by_sink()
            .map(|(sink, binding)| (sink.clone(), binding.clone()))
            .collect(),
    }
}

pub fn resolved(config: &Config) -> resonate_engine::Equalisation {
    let Ok(folder) = config::equaliser_dir() else {
        return resonate_engine::Equalisation::default();
    };
    let store = Store::at(folder);
    let Some(bindings) = config.equaliser_for.as_ref() else {
        return resonate_engine::Equalisation {
            enabled: config.equaliser_on(),
            ..resonate_engine::Equalisation::default()
        };
    };

    let held = |owner: Option<&NodeName>, binding: &Binding| -> Option<Arc<Profile>> {
        match curve_of(&store, owner, binding) {
            Ok(Some(profile)) => Some(Arc::new(profile)),
            Ok(None) => {
                tracing::warn!(
                    binding = %spoken_binding(binding),
                    "a binding names a profile that is not kept"
                );
                None
            }
            Err(error) => {
                tracing::warn!(
                    %error,
                    binding = %spoken_binding(binding),
                    "a bound curve could not be read"
                );
                None
            }
        }
    };

    resonate_engine::Equalisation {
        enabled: config.equaliser_on(),
        bound: bindings
            .by_sink()
            .filter_map(|(sink, binding)| {
                held(Some(sink), binding).map(|profile| (sink.clone(), profile))
            })
            .collect(),
        fallback: bindings.fallback().and_then(|binding| held(None, binding)),
    }
}
