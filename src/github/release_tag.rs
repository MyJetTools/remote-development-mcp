use super::Version;

/// How a release tag is spelled for a given service.
///
/// Straight from the house release guide:
/// - single repo (one service = one GitHub repo): the tag *is* the version,
///   `0.1.0`
/// - monorepo: the tag is `{service}-{version}`, `price-feed-binance-0.1.0`,
///   and `.github/workflows/release-{service}.yaml` triggers on `{service}-*`
#[derive(Debug, Clone)]
pub struct ReleaseTag {
    /// `None` for a single-repo service.
    pub service: Option<String>,
}

impl ReleaseTag {
    pub fn new(service: Option<String>) -> Self {
        Self {
            service: service
                .map(|service| service.trim().to_string())
                .filter(|service| !service.is_empty()),
        }
    }

    /// `price-feed-binance-` — what every tag of this service starts with, and
    /// what the GitHub ref search is narrowed by. Empty for a single repo.
    pub fn prefix(&self) -> String {
        match self.service.as_ref() {
            Some(service) => format!("{}-", service),
            None => String::new(),
        }
    }

    pub fn render(&self, version: &Version) -> String {
        format!("{}{}", self.prefix(), version)
    }

    /// Reads the version out of a tag, or `None` when the tag does not belong to
    /// this service.
    ///
    /// The strictness is the point. In a monorepo holding both `price-feed` and
    /// `price-feed-binance`, the tag `price-feed-binance-0.1.0` starts with
    /// `price-feed-`; taking it for a `price-feed` release would read its version
    /// as `binance-0.1.0`. Requiring what remains to parse as a pure version
    /// rejects it, so one service can never inherit another's numbering.
    pub fn version_of(&self, tag: &str) -> Option<Version> {
        let tag = tag.trim();

        let rest = match self.service.as_ref() {
            Some(_) => tag.strip_prefix(&self.prefix())?,
            None => tag,
        };

        Version::parse(rest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monorepo(service: &str) -> ReleaseTag {
        ReleaseTag::new(Some(service.to_string()))
    }

    fn single_repo() -> ReleaseTag {
        ReleaseTag::new(None)
    }

    #[test]
    fn a_monorepo_tag_carries_the_service_name() {
        let tag = monorepo("price-feed-binance");

        assert_eq!(tag.prefix(), "price-feed-binance-");
        assert_eq!(
            tag.render(&Version::parse("0.1.0").unwrap()),
            "price-feed-binance-0.1.0"
        );
    }

    #[test]
    fn a_single_repo_tag_is_just_the_version() {
        let tag = single_repo();

        assert_eq!(tag.prefix(), "");
        assert_eq!(tag.render(&Version::parse("0.1.0").unwrap()), "0.1.0");
    }

    #[test]
    fn a_blank_service_is_treated_as_a_single_repo() {
        assert!(ReleaseTag::new(Some("   ".to_string())).service.is_none());
        assert_eq!(ReleaseTag::new(Some("".to_string())).prefix(), "");
    }

    #[test]
    fn reads_its_own_version_back() {
        let tag = monorepo("margin-engine");

        assert_eq!(
            tag.version_of("margin-engine-0.2.1").unwrap().to_string(),
            "0.2.1"
        );
    }

    /// The collision that would otherwise make one service adopt another's
    /// version numbers.
    #[test]
    fn a_longer_service_name_sharing_the_prefix_is_not_mistaken_for_this_one() {
        let tag = monorepo("price-feed");

        assert!(tag.version_of("price-feed-binance-0.1.0").is_none());
        assert!(tag.version_of("price-feed-kraken-2.0.0").is_none());

        // Its own tags still read fine.
        assert_eq!(
            tag.version_of("price-feed-0.1.0").unwrap().to_string(),
            "0.1.0"
        );
    }

    #[test]
    fn another_service_entirely_is_ignored() {
        let tag = monorepo("margin-engine");

        assert!(tag.version_of("price-feed-0.1.0").is_none());
        assert!(tag.version_of("0.1.0").is_none());
    }

    #[test]
    fn a_tag_that_is_not_a_version_is_ignored() {
        let tag = monorepo("margin-engine");

        assert!(tag.version_of("margin-engine-latest").is_none());
        assert!(tag.version_of("margin-engine-1.0.0-rc1").is_none());
        assert!(tag.version_of("margin-engine").is_none());
    }

    #[test]
    fn a_single_repo_ignores_service_style_and_v_prefixed_tags() {
        let tag = single_repo();

        assert_eq!(tag.version_of("1.2.3").unwrap().to_string(), "1.2.3");
        assert!(tag.version_of("v1.2.3").is_none());
        assert!(tag.version_of("price-feed-1.2.3").is_none());
    }
}
