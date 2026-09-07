//! The `structuretemplate_` key grammar.
//!
//! A structure saved in-game is stored under `structuretemplate_` followed by a
//! namespaced id, e.g. `structuretemplate_mystructure:copy`. A bare name means
//! the `mystructure` namespace, which is what the structure block and
//! `/structure` use by default.

pub const PREFIX: &[u8] = b"structuretemplate_";
pub const DEFAULT_NAMESPACE: &str = "mystructure";

/// Adds the default namespace to a bare name, leaving qualified names alone.
pub fn qualify(name: &str) -> String {
    if name.contains(':') {
        name.to_string()
    } else {
        format!("{DEFAULT_NAMESPACE}:{name}")
    }
}

/// The leveldb key for a structure name, bare or qualified.
pub fn encode(name: &str) -> Vec<u8> {
    let mut out = PREFIX.to_vec();
    out.extend_from_slice(qualify(name).as_bytes());
    out
}

/// The leveldb key for an id exactly as given, with no namespace added.
pub fn encode_exact(id: &str) -> Vec<u8> {
    let mut out = PREFIX.to_vec();
    out.extend_from_slice(id.as_bytes());
    out
}

/// Every key a user-supplied name could refer to, most likely first.
///
/// A bare name normally means the `mystructure` namespace, but a world that
/// Minecraft did not write can hold a key with no namespace at all — and stage
/// 1's `structures` shows those, so `export` has to be able to fetch them.
pub fn candidates(id: &str) -> Vec<Vec<u8>> {
    let qualified = encode(id);
    let exact = encode_exact(id);
    if qualified == exact {
        vec![qualified]
    } else {
        vec![qualified, exact]
    }
}

/// The qualified id in a leveldb key, or `None` if it is not a structure key.
pub fn decode(key: &[u8]) -> Option<String> {
    let body = key.strip_prefix(PREFIX)?;
    std::str::from_utf8(body)
        .ok()
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// How an id is shown to a user: the default namespace is noise, any other
/// namespace is information.
pub fn display_name(id: &str) -> &str {
    id.strip_prefix(DEFAULT_NAMESPACE)
        .and_then(|r| r.strip_prefix(':'))
        .unwrap_or(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qualifies_a_bare_name_with_the_default_namespace() {
        assert_eq!(qualify("house"), "mystructure:house");
    }

    #[test]
    fn leaves_an_explicit_namespace_alone() {
        assert_eq!(qualify("understudy:players"), "understudy:players");
    }

    #[test]
    fn encodes_the_key_measured_from_a_real_world() {
        // Ssu8ww1SFbM= ("construct show") really contains this key.
        assert_eq!(
            encode("copy"),
            b"structuretemplate_mystructure:copy".to_vec()
        );
    }

    #[test]
    fn round_trips() {
        for name in [
            "house",
            "mystructure:house",
            "understudy:players",
            "a.b-c_1",
        ] {
            assert_eq!(decode(&encode(name)).unwrap(), qualify(name));
        }
    }

    #[test]
    fn decodes_a_real_key() {
        assert_eq!(
            decode(b"structuretemplate_mystructure:copy").unwrap(),
            "mystructure:copy"
        );
    }

    #[test]
    fn rejects_keys_without_the_prefix() {
        assert_eq!(decode(b"AutonomousEntities"), None);
        assert_eq!(decode(b""), None);
        assert_eq!(decode(&[0x00, 0x01, 0x02]), None);
    }

    #[test]
    fn rejects_a_prefixed_key_whose_body_is_not_utf8() {
        let mut k = PREFIX.to_vec();
        k.extend_from_slice(&[0xff, 0xfe]);
        assert_eq!(decode(&k), None);
    }

    #[test]
    fn display_name_strips_only_the_default_namespace() {
        assert_eq!(display_name("mystructure:house"), "house");
        assert_eq!(display_name("understudy:players"), "understudy:players");
    }

    #[test]
    fn an_unqualified_key_is_reachable_by_its_own_name() {
        assert_eq!(encode_exact("foo"), b"structuretemplate_foo".to_vec());
        assert_eq!(
            candidates("foo"),
            vec![
                b"structuretemplate_mystructure:foo".to_vec(),
                b"structuretemplate_foo".to_vec(),
            ]
        );
    }

    #[test]
    fn a_qualified_name_has_exactly_one_candidate() {
        assert_eq!(
            candidates("understudy:players"),
            vec![b"structuretemplate_understudy:players".to_vec()]
        );
    }
}
