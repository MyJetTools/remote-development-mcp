/// A release version — the `major.minor.patch` that ends a tag.
///
/// Exactly three components, each a decimal integer with no leading zeros. That
/// is not arbitrary strictness; it is what the tags in these repositories
/// actually look like, and both looser shapes cause real damage:
///
/// - A **hyphen** (`1.0.0-rc1`) survives into the tag, and the release workflow
///   extracts the version with `VERSION="${TAG##*-}"` — everything after the
///   *last* hyphen — so CI would write `rc1` into `Cargo.toml` and tag the image
///   `:rc1`.
/// - A **leading zero** (`0.01`) would parse to a number and render back as
///   `0.1`, so the tag created would not be the tag that was asked for.
#[derive(Debug, Clone, Copy)]
pub struct Version {
    major: u32,
    minor: u32,
    patch: u32,
}

impl Version {
    /// Parses `0.1.9`. Returns `None` for anything else at all — a `v` prefix, a
    /// hyphen, a `+`, two components, four components, a leading zero, or
    /// whitespace inside.
    pub fn parse(src: &str) -> Option<Self> {
        let src = src.trim();

        let mut parts = src.split('.');

        let major = parse_component(parts.next()?)?;
        let minor = parse_component(parts.next()?)?;
        let patch = parse_component(parts.next()?)?;

        // A fourth component means this is not the shape we release.
        if parts.next().is_some() {
            return None;
        }

        Some(Self {
            major,
            minor,
            patch,
        })
    }

    /// The next version: the patch component plus one.
    ///
    /// Only the last number moves. Raising the minor or the major is a decision
    /// a person makes, and they make it by passing the version explicitly.
    pub fn bump_patch(&self) -> Option<Self> {
        Some(Self {
            major: self.major,
            minor: self.minor,
            patch: self.patch.checked_add(1)?,
        })
    }
}

/// One component: digits only, and no leading zero unless the component *is*
/// zero.
fn parse_component(src: &str) -> Option<u32> {
    if src.is_empty() {
        return None;
    }

    if !src.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }

    if src.len() > 1 && src.starts_with('0') {
        return None;
    }

    src.parse::<u32>().ok()
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

impl PartialEq for Version {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == std::cmp::Ordering::Equal
    }
}

impl Eq for Version {}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Version {
    /// Component-wise and numeric.
    ///
    /// Comparing the rendered strings would put `0.1.10` *before* `0.1.9`, so
    /// "the latest release" would come back wrong from the tenth patch onwards.
    /// That is not hypothetical here: services in these repositories are at
    /// `0.3.100` and `0.6.182`.
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.major
            .cmp(&other.major)
            .then_with(|| self.minor.cmp(&other.minor))
            .then_with(|| self.patch.cmp(&other.patch))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(src: &str) -> Version {
        Version::parse(src).unwrap()
    }

    #[test]
    fn parses_and_renders_unchanged() {
        for src in ["0.1.0", "1.2.3", "0.1.24", "10.0.0", "0.3.100", "0.6.182"] {
            assert_eq!(version(src).to_string(), src);
        }
    }

    #[test]
    fn refuses_everything_that_is_not_major_minor_patch() {
        for src in [
            "",
            "   ",
            "1",
            "1.2",
            "1.2.3.4",
            "v1.0.0",
            // The one that reaches CI as "rc1".
            "1.0.0-rc1",
            "1.0.0+build",
            "1..2",
            ".1.0",
            "1.0.",
            "1.0.x",
            "latest",
            "1. 2.3",
        ] {
            assert!(Version::parse(src).is_none(), "{} should be refused", src);
        }
    }

    /// A leading zero would render back differently from what was typed, so the
    /// tag created would not be the tag that was asked for.
    #[test]
    fn refuses_leading_zeros_but_allows_a_bare_zero() {
        assert!(Version::parse("0.01.0").is_none());
        assert!(Version::parse("01.0.0").is_none());
        assert!(Version::parse("0.0.01").is_none());

        assert_eq!(version("0.0.0").to_string(), "0.0.0");
    }

    #[test]
    fn compares_numerically_not_as_text() {
        // As strings, "0.1.10" < "0.1.9" — the trap this exists to avoid.
        assert!(version("0.1.10") > version("0.1.9"));
        assert!(version("0.3.100") > version("0.3.99"));
        assert!(version("1.0.0") > version("0.99.99"));
        assert!(version("2.0.0") > version("1.9.9"));
    }

    #[test]
    fn bumping_raises_only_the_patch() {
        assert_eq!(version("0.1.0").bump_patch().unwrap().to_string(), "0.1.1");
        assert_eq!(version("0.1.9").bump_patch().unwrap().to_string(), "0.1.10");
        assert_eq!(
            version("0.3.99").bump_patch().unwrap().to_string(),
            "0.3.100"
        );
        assert_eq!(version("1.2.3").bump_patch().unwrap().to_string(), "1.2.4");
    }

    #[test]
    fn the_bumped_version_is_always_greater() {
        for src in ["0.0.1", "0.1.9", "1.2.3", "10.0.0"] {
            assert!(version(src).bump_patch().unwrap() > version(src));
        }
    }

    #[test]
    fn a_patch_at_the_ceiling_does_not_wrap_around() {
        let highest = Version::parse(&format!("1.0.{}", u32::MAX)).unwrap();

        assert!(highest.bump_patch().is_none());
    }

    #[test]
    fn a_component_too_large_for_u32_is_refused_rather_than_wrapped() {
        assert!(Version::parse("1.0.4294967296").is_none());
    }

    #[test]
    fn sorting_a_list_puts_the_real_latest_last() {
        let mut all: Vec<Version> = ["0.1.9", "0.1.24", "0.2.0", "0.1.2"]
            .iter()
            .map(|src| version(src))
            .collect();

        all.sort();

        assert_eq!(all.last().unwrap().to_string(), "0.2.0");
        // And the lexicographic trap specifically.
        assert_eq!(all[2].to_string(), "0.1.24");
    }
}
