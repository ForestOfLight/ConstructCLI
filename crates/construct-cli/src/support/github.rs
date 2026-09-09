use construct_core::install::releases;

pub fn client() -> releases::GitHub {
    let token = std::env::var("CONSTRUCT_GITHUB_TOKEN")
        .or_else(|_| std::env::var("GITHUB_TOKEN"))
        .ok();
    releases::GitHub::new(token)
}
