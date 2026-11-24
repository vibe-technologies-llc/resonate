use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use parking_lot::Mutex;
use resonate_codec::Sources;
use resonate_core::ZoneId;
use resonate_engine::{EngineConfig, Player};

use crate::{
    Documents, Error, MobilePlayer, PlaybackPreferences, Result, StoreOp,
    catalog::Catalog,
    documents::{GRANTED_SCHEMES, GrantedFiles},
    downloads::Downloads,
    enrichment::Enriching,
    features::Services,
    offering::Offered,
    output::{self, Routing},
    rendering::{Kinds, Rendering},
    room::Room,
    served::Served,
    sync::Syncing,
    tagging::Tagging,
    together::Together,
    watching,
    zoning::{self, Playing},
};

pub(crate) type Engine = dyn Fn(
        ZoneId,
        EngineConfig,
        &Arc<Sources>,
        &Arc<Routing>,
        Option<Kinds>,
    ) -> resonate_engine::Result<Player>
    + Send
    + Sync;

pub(crate) struct Launch {
    pub(crate) rendering: Rendering,
    pub(crate) offered: Offered,
    pub(crate) engine: Arc<Engine>,
}

impl Launch {
    pub(crate) fn on_the_phone(folder: &Path) -> Self {
        Self {
            rendering: Rendering::on_the_network(),
            offered: Offered::under(folder.to_path_buf()),
            engine: Arc::new(|_, config, sources, routing, kinds| {
                output::player(config, Arc::clone(sources), routing, kinds)
            }),
        }
    }
}

impl MobilePlayer {
    pub(crate) fn launched(
        folder: &str,
        documents: Arc<dyn Documents>,
        launch: Launch,
    ) -> Result<Arc<Self>> {
        std::fs::create_dir_all(folder).map_err(|source| Error::Folder {
            op: StoreOp::CreateFolder,
            source,
        })?;

        let Launch {
            rendering,
            offered,
            engine,
        } = launch;
        let catalog = Arc::new(Catalog::open(&Path::new(folder).join("documents.sqlite"))?);
        let files = Arc::clone(&documents);
        let served = Arc::new(Served::default());

        let sources = Arc::new(GRANTED_SCHEMES.iter().try_fold(
            served.sources(Sources::local().spooling_under(Path::new(folder).to_path_buf())),
            |sources, scheme| {
                Ok::<_, Error>(
                    sources.and(Arc::new(GrantedFiles::new(Arc::clone(&documents), scheme)?)),
                )
            },
        )?);
        let routing = Arc::new(Routing::new());
        let kept = catalog.kept_zones()?;

        rendering.taught_from(&catalog)?;
        if kept.iter().any(|zone| zone.renderer.is_some()) {
            rendering.look_behind();
        }

        let config = EngineConfig {
            volume: zoning::first_zones_volume(&kept),
            sink: zoning::first_zones_sink(&kept),
            ..EngineConfig::default()
        };
        let kinds = rendering.kinds();

        let player = engine(ZoneId::FIRST, config, &sources, &routing, kinds.clone())?;
        let engines: Box<zoning::Engines> = {
            let sources = Arc::clone(&sources);
            Box::new(
                move |zone: ZoneId, config: EngineConfig, routing: &Arc<Routing>| {
                    engine(zone, config, &sources, routing, kinds.clone())
                },
            )
        };

        let playing = Playing::start(
            player,
            routing,
            kept.iter().find(|zone| zone.id.is_first()),
            engines,
        );
        let this = Arc::new(Self {
            playing,
            catalog,
            sources,
            preferences: Mutex::new(PlaybackPreferences::default()),
            services: Services::default(),
            playlist_undo: Mutex::new(Vec::new()),
            downloads: Downloads::under(Path::new(folder)),
            syncing: Syncing::under(PathBuf::from(folder)),
            enriching: Enriching::default(),
            room: Room::under(PathBuf::from(folder)),
            tagging: Tagging::under(PathBuf::from(folder)),
            served,
            together: Together::default(),
            rendering,
            offered,
            releases: resonate_online::KeptRelease::in_folder(Path::new(folder)),
        });

        this.downloads.stall_by(this.services.downloads.clone());
        this.services.files.reach_through(files);

        let carried_over = this.catalog.carries_queues_over()?;

        if carried_over {
            this.resume_saved_in(ZoneId::FIRST)?;
        }
        watching::keep(&this, ZoneId::FIRST);
        this.restore_zones(&kept, carried_over)?;
        Ok(this)
    }
}
