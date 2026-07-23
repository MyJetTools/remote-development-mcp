/// The `owner/repo` a repository publishes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoSlug {
    pub owner: String,
    pub repo: String,
}

impl std::fmt::Display for RepoSlug {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.owner, self.repo)
    }
}

impl RepoSlug {
    /// Reads `owner/repo` out of a git remote URL.
    ///
    /// Taken from the checkout's own remote rather than configured separately:
    /// the endpoint already points at exactly one working tree, so the remote is
    /// the answer — and one that cannot drift out of step with the repository
    /// the tools are operating on.
    ///
    /// Handles the forms git actually writes:
    /// `git@github.com:org/repo.git`, `https://github.com/org/repo.git`,
    /// `ssh://git@github.com/org/repo.git`, with or without the `.git` suffix.
    pub fn parse_remote(url: &str) -> Option<Self> {
        let url = url.trim();

        if url.is_empty() {
            return None;
        }

        let path = match url.strip_prefix("git@") {
            // scp-like syntax: everything after the first ':' is the path.
            Some(rest) => rest.split_once(':').map(|(_host, path)| path)?,
            None => after_host(strip_scheme(url)?)?,
        };

        let path = path.trim_matches('/');
        let path = path.strip_suffix(".git").unwrap_or(path);

        let (owner, repo) = path.split_once('/')?;

        // A deeper path is not an owner/repo pair; refusing beats guessing which
        // two segments were meant.
        if owner.is_empty() || repo.is_empty() || repo.contains('/') {
            return None;
        }

        Some(Self {
            owner: owner.to_string(),
            repo: repo.to_string(),
        })
    }
}

/// Drops `user@host[:port]` and returns the path that follows.
fn after_host(rest: &str) -> Option<&str> {
    let (_authority, path) = rest.split_once('/')?;

    Some(path)
}

/// Removes the URL scheme, or `None` when there is none we recognise.
fn strip_scheme(url: &str) -> Option<&str> {
    ["ssh://", "https://", "http://"]
        .iter()
        .find_map(|scheme| url.strip_prefix(scheme))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slug(url: &str) -> RepoSlug {
        RepoSlug::parse_remote(url).unwrap_or_else(|| panic!("could not parse {}", url))
    }

    #[test]
    fn reads_the_forms_git_actually_writes() {
        let expected = RepoSlug {
            owner: "MyJetTools".to_string(),
            repo: "my-ssh".to_string(),
        };

        for url in [
            "git@github.com:MyJetTools/my-ssh.git",
            "git@github.com:MyJetTools/my-ssh",
            "https://github.com/MyJetTools/my-ssh.git",
            "https://github.com/MyJetTools/my-ssh",
            "ssh://git@github.com/MyJetTools/my-ssh.git",
            "ssh://git@github.com:22/MyJetTools/my-ssh.git",
            "  https://github.com/MyJetTools/my-ssh.git  \n",
        ] {
            assert_eq!(slug(url), expected, "{}", url);
        }
    }

    #[test]
    fn renders_as_owner_slash_repo() {
        assert_eq!(
            slug("git@github.com:org/thing.git").to_string(),
            "org/thing"
        );
    }

    #[test]
    fn a_repository_name_containing_a_dot_keeps_it() {
        assert_eq!(slug("git@github.com:org/my.service.git").repo, "my.service");
        assert_eq!(slug("git@github.com:org/my.service").repo, "my.service");
    }

    #[test]
    fn refuses_what_it_can_not_read() {
        for url in [
            "",
            "   ",
            "not-a-url",
            "git@github.com",
            "https://github.com/only-owner",
            // Deeper than owner/repo — ambiguous, so refused.
            "https://github.com/org/group/repo.git",
        ] {
            assert!(
                RepoSlug::parse_remote(url).is_none(),
                "{} should be refused",
                url
            );
        }
    }
}
