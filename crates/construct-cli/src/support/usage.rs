use crate::cli::SourceArg;
use crate::failure::{Failure, Result};

pub fn check_source_against_world(
    usage: &str,
    verb: &str,
    world: Option<&String>,
    source: Option<SourceArg>,
    world_excludes_shared: bool,
) -> Result {
    let Some(source) = source else { return Ok(()) };
    let value = source.as_str();
    if world.is_none() && source.needs_a_world() {
        return Err(Failure::usage(
            format!("--source {value} needs a world to {verb}"),
            format!("construct {usage} --world <world> --source {value}"),
        ));
    }
    if world.is_some() && !source.needs_a_world() && world_excludes_shared {
        return Err(Failure::usage(
            format!("--source {value} cannot be combined with --world"),
            format!(
                "The shared copy of Construct serves every world using it, so a command \
                 aimed at one world never reaches it. Drop --world to {verb} the shared \
                 copy:\n\n\
                 construct {usage} --source {value}"
            ),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usage_of(result: Result) -> (String, String) {
        match result {
            Err(Failure::Usage { message, hint }) => (message, hint),
            Err(_) => panic!("expected a usage refusal, got another failure"),
            Ok(()) => panic!("expected a usage refusal, got Ok"),
        }
    }

    #[test]
    fn no_source_is_always_fine() {
        assert!(check_source_against_world("x", "read from", None, None, true).is_ok());
    }

    #[test]
    fn a_world_scoped_source_without_a_world_names_the_flag_it_needs() {
        let (message, hint) = usage_of(check_source_against_world(
            "export <structure>",
            "read from",
            None,
            Some(SourceArg::WorldDb),
            true,
        ));
        assert_eq!(message, "--source world-db needs a world to read from");
        assert!(hint.contains("--world <world> --source world-db"));
    }

    #[test]
    fn world_pack_needs_a_world_too() {
        assert!(
            check_source_against_world("x", "read from", None, Some(SourceArg::WorldPack), false)
                .is_err()
        );
    }

    #[test]
    fn shared_pack_with_no_world_is_the_ordinary_case() {
        assert!(
            check_source_against_world("x", "read from", None, Some(SourceArg::SharedPack), true)
                .is_ok()
        );
    }

    #[test]
    fn shared_pack_under_world_is_refused_where_world_means_that_world_only() {
        let world = "Amelix".to_string();
        let (message, _) = usage_of(check_source_against_world(
            "delete <structure>",
            "delete from",
            Some(&world),
            Some(SourceArg::SharedPack),
            true,
        ));
        assert_eq!(
            message,
            "--source shared-pack cannot be combined with --world"
        );
    }

    #[test]
    fn shared_pack_under_world_is_allowed_for_a_listing() {
        let world = "Amelix".to_string();
        assert!(
            check_source_against_world(
                "structures",
                "read from",
                Some(&world),
                Some(SourceArg::SharedPack),
                false,
            )
            .is_ok()
        );
    }

    #[test]
    fn a_world_scoped_source_with_a_world_is_what_it_is_for() {
        let world = "Amelix".to_string();
        assert!(
            check_source_against_world(
                "x",
                "read from",
                Some(&world),
                Some(SourceArg::WorldDb),
                true
            )
            .is_ok()
        );
    }
}
