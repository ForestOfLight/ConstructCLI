use construct_core::pack;

pub fn pack_phrase(kind: pack::HomeKind, world: Option<&str>) -> String {
    match (kind, world) {
        (pack::HomeKind::WorldConstruct, Some(w)) => {
            format!("{w}'s own copy of Construct — that world only")
        }
        (pack::HomeKind::WorldConstruct, None) => {
            "the world's own copy of Construct — that world only".to_string()
        }
        (pack::HomeKind::WorldStructuresPack, Some(w)) => {
            format!("{w}'s structures pack — that world only")
        }
        (pack::HomeKind::WorldStructuresPack, None) => {
            "the world's structures pack — that world only".to_string()
        }
        (pack::HomeKind::SharedConstruct, _) => {
            "the shared copy of Construct in development_behavior_packs — every world using it"
                .to_string()
        }
    }
}

pub fn pack_short(kind: pack::HomeKind, world: &str) -> String {
    match kind {
        pack::HomeKind::WorldConstruct => format!("{world}'s own copy of Construct"),
        pack::HomeKind::WorldStructuresPack => format!("{world}'s structures pack"),
        pack::HomeKind::SharedConstruct => "the shared copy of Construct".to_string(),
    }
}

pub fn target_field(kind: pack::HomeKind) -> &'static str {
    kind.source().as_str()
}
