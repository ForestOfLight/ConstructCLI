use construct_core::install::releases;

pub fn client() -> releases::GitHub {
    let token = std::env::var("CONSTRUCT_GITHUB_TOKEN")
        .or_else(|_| std::env::var("GITHUB_TOKEN"))
        .ok();
    #[cfg(debug_assertions)]
    if let Ok(base) = std::env::var("CONSTRUCT_GITHUB_API") {
        return releases::GitHub::with_base(base, token);
    }
    releases::GitHub::new(token)
}
