use crate::output::Out;
use construct_core::Result;
use construct_core::catalog::{self, Entry, Source};
use construct_core::discovery::{Installation, World, installation};
use construct_core::error::CoreError;
use construct_core::pack;
use construct_core::store::{self, OpenedStore};

pub struct Loaded {
    pub entries: Vec<Entry>,
    pub store: Option<OpenedStore>,
}

pub fn for_world(
    world: &World,
    installations: &[Installation],
    source: Option<Source>,
    out: &mut Out,
) -> Result<Loaded> {
    let (world_entries, store) = if source.is_some_and(|s| s.is_pack()) {
        (Vec::new(), None)
    } else {
        let store = store::open_world_store(world)?;
        let entries = catalog::from_world(&store)?;
        (entries, Some(store))
    };

    let (pack_entries, _packs) = packs_for_world(world, installations, source, out)?;

    let entries = catalog::unify(world_entries, pack_entries);
    Ok(Loaded { entries, store })
}

pub fn world_scoped(entries: Vec<Entry>) -> Vec<Entry> {
    entries
        .into_iter()
        .filter(|e| e.source != Source::SharedPack)
        .collect()
}

pub fn explain_miss(err: CoreError, world: Option<&World>) -> CoreError {
    match (err, world) {
        (CoreError::StructureNotFound { name, near }, Some(w)) => CoreError::StructureNotFound {
            name: format!("{name} in {}", w.display_name),
            near,
        },
        (other, _) => other,
    }
}

pub fn packs_for_world(
    world: &World,
    installations: &[Installation],
    source: Option<Source>,
    out: &mut Out,
) -> Result<(Vec<Entry>, Vec<pack::Home>)> {
    let mut packs = Vec::new();
    let pack_entries = if source == Some(Source::WorldDb) {
        Vec::new()
    } else {
        match installation::for_world(installations, world).and_then(|i| {
            let serving = pack::serving(world, i);
            if serving.is_empty() {
                Err(CoreError::ConstructNotInstalled {
                    searched: pack::searched_roots(world, i),
                })
            } else {
                Ok(serving)
            }
        }) {
            Ok(serving) => {
                let mut entries = Vec::new();
                for home in &serving {
                    entries.extend(catalog::from_pack(&home.dir, home.kind.source()));
                }
                packs = serving;
                entries
            }
            Err(e) if source.is_some_and(|s| s.is_pack()) => return Err(e),
            Err(e) => {
                out.warn(format!("{e}; showing world structures only"));
                Vec::new()
            }
        }
    };

    Ok((pack_entries, packs))
}
