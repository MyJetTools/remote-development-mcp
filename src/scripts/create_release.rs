use std::sync::Arc;

use crate::{
    audit::AuditMutation,
    github::{read_release_layout, GitHubClient, ReleaseLayout, ReleaseTag, RepoSlug, Version},
    repo::RepoContext,
};

use super::{git_capture, resolve_working_dir};

/// What the very first release of a service is numbered, matching the example in
/// the release guide.
const FIRST_VERSION: &str = "0.1.0";

pub struct CreateReleaseRequest {
    /// Service name in a monorepo. Absent means the repository holds a single
    /// service and the tag is the bare version.
    pub service: Option<String>,
    /// Absent means "the next one" — the highest released version with its last
    /// number raised.
    pub version: Option<String>,
    /// Work out the tag and report it without creating anything.
    pub dry_run: bool,
    /// Subfolder holding the repository to release, when the root holds several.
    pub path: Option<String>,
}

pub struct CreateReleaseResult {
    pub tag: String,
    pub version: String,
    pub previous_version: Option<String>,
    pub repository: String,
    pub created: bool,
    pub dry_run: bool,
    pub release_url: Option<String>,
}

/// Creates the next GitHub release for a service, exactly as the release guide
/// describes it.
///
/// Creating the release is what creates the tag and triggers the build workflow,
/// so the tag has to be right the first time. Everything that could make it
/// wrong is checked before anything is published: the version is validated
/// against what CI will extract from the tag, the existing tags are read to find
/// the real latest version, and a collision with an existing tag is refused
/// rather than resolved.
pub async fn create_release(
    repo: &Arc<RepoContext>,
    request: CreateReleaseRequest,
) -> Result<CreateReleaseResult, String> {
    let token =
        match repo.github_token.as_ref() {
            Some(token) => token.clone(),
            None => return Err(
                "No github_token is configured, so releases can not be created. Add one to the \
                 settings and restart"
                    .to_string(),
            ),
        };

    // Which checkout is being released. With a root holding several
    // repositories, this is what picks one — its remote and its workflows are
    // what get read.
    let working_dir = resolve_working_dir(repo, request.path.as_deref())?;

    let slug = read_repo_slug(repo, &working_dir).await?;

    // Checked before anything is published. The release guide is explicit that a
    // tag created without its workflow present does not build — so skipping this
    // would report a successful release and quietly produce no image.
    let service = check_against_workflows(&working_dir, request.service.as_deref()).await?;

    let tag = ReleaseTag::new(service);

    let client = GitHubClient::new(token);

    let released = client.released_versions(&slug, &tag).await?;
    let previous = released.last().cloned();

    let version = match request.version.as_ref() {
        Some(asked) => parse_requested_version(asked)?,
        None => match previous.as_ref() {
            Some(previous) => previous
                .bump_patch()
                .ok_or_else(|| format!("Version {} can not be raised any further", previous))?,
            // Nothing released yet. This is only reached when the tag lookup
            // *succeeded* and came back empty — a failed lookup returns its own
            // error above, so "no tags" is never inferred from a failure.
            None => Version::parse(FIRST_VERSION).expect("the first version is a literal"),
        },
    };

    // Refused rather than silently bumped past: a caller who named a version
    // meant that one, and re-releasing is a delete-then-create decision the
    // release guide spells out separately.
    if released.iter().any(|existing| existing == &version) {
        return Err(format!(
            "Version {} is already released for this service (tag '{}'). Pick another version, or \
             delete that release first if you meant to rebuild it",
            version,
            tag.render(&version)
        ));
    }

    let tag_name = tag.render(&version);

    if request.dry_run {
        return Ok(CreateReleaseResult {
            tag: tag_name,
            version: version.to_string(),
            previous_version: previous.map(|previous| previous.to_string()),
            repository: slug.to_string(),
            created: false,
            dry_run: true,
            release_url: None,
        });
    }

    let release_url = client.create_release(&slug, &tag_name).await?;

    repo.audit
        .mutation(AuditMutation {
            repo: &repo.name,
            action: "create_release",
            target: &tag_name,
            detail: Some(format!("{} on {}", version, slug)),
        })
        .await;

    Ok(CreateReleaseResult {
        tag: tag_name,
        version: version.to_string(),
        previous_version: previous.map(|previous| previous.to_string()),
        repository: slug.to_string(),
        created: true,
        dry_run: false,
        release_url,
    })
}

/// Confirms the repository can actually build what is about to be tagged, and
/// returns the service name to use.
///
/// The workflows are the authority. A tag only builds when a workflow triggers
/// on it, so releasing a service with no `release-{service}.yaml`, or releasing
/// a bare version in a repository whose workflows all expect a service prefix,
/// produces a tag nothing acts on — a release that looks successful and ships
/// nothing.
async fn check_against_workflows(
    working_dir: &std::path::Path,
    requested: Option<&str>,
) -> Result<Option<String>, String> {
    let layout = read_release_layout(working_dir).await;

    let requested = requested
        .map(|service| service.trim())
        .filter(|service| !service.is_empty());

    match (&layout, requested) {
        (ReleaseLayout::None, _) => Err(
            "This repository has no release workflow in .github/workflows, so a tag would not \
             build anything. Add one before releasing"
                .to_string(),
        ),

        (ReleaseLayout::Monorepo { services }, None) => Err(format!(
            "This repository releases per service, so a service is required — a bare version tag \
             matches no workflow here. Available: {}",
            services.join(", ")
        )),

        (ReleaseLayout::Monorepo { services }, Some(service)) => {
            if services.iter().any(|known| known == service) {
                return Ok(Some(service.to_string()));
            }

            Err(format!(
                "There is no release workflow for '{}', so tagging it would build nothing. \
                 Available: {}",
                service,
                services.join(", ")
            ))
        }

        (ReleaseLayout::SingleRepo, Some(service)) => Err(format!(
            "This repository holds a single service, where the tag is the bare version. Leave the \
             service out — passing '{}' would create the tag '{}-…', which is the monorepo shape",
            service, service
        )),

        (ReleaseLayout::SingleRepo, None) => Ok(None),
    }
}

fn parse_requested_version(asked: &str) -> Result<Version, String> {
    Version::parse(asked).ok_or_else(|| {
        format!(
            "'{}' is not a usable version. It must be numbers separated by dots, such as 0.1.4. \
             In particular it can not contain a hyphen: the build workflow reads the version out \
             of the tag as everything after the last hyphen, so '1.0.0-rc1' would reach CI as \
             'rc1' and tag the image with that",
            asked
        )
    })
}

async fn read_repo_slug(
    _repo: &Arc<RepoContext>,
    working_dir: &std::path::Path,
) -> Result<RepoSlug, String> {
    let output = git_capture(&["remote", "get-url", "origin"], working_dir, None).await?;

    if !output.success {
        return Err(format!(
            "Can not read the git remote 'origin' of this repository, so there is no way to tell \
             which GitHub repository to release. {}",
            output.stderr.trim()
        ));
    }

    RepoSlug::parse_remote(&output.stdout).ok_or_else(|| {
        format!(
            "The git remote 'origin' ('{}') is not a GitHub owner/repo URL",
            output.stdout.trim()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hyphenated_version_is_refused_with_the_reason_why() {
        let err = parse_requested_version("1.0.0-rc1").unwrap_err();

        // The message has to explain the CI extraction, or the refusal looks
        // arbitrary.
        assert!(err.contains("after the last hyphen"), "{}", err);
    }

    #[test]
    fn a_plain_version_is_accepted() {
        assert_eq!(
            parse_requested_version("0.1.4").unwrap().to_string(),
            "0.1.4"
        );
    }

    #[test]
    fn other_nonsense_is_refused_too() {
        for asked in ["", "v1.0.0", "latest", "1.0.x"] {
            assert!(parse_requested_version(asked).is_err(), "{}", asked);
        }
    }

    #[test]
    fn the_first_version_is_a_valid_literal() {
        assert_eq!(Version::parse(FIRST_VERSION).unwrap().to_string(), "0.1.0");
    }
}
