use std::time::Duration;

use flurl::{body::HttpRequestBody, FlUrl};
use serde::{Deserialize, Serialize};

use super::{ReleaseTag, RepoSlug, Version, WorkflowJobs, WorkflowRun, WorkflowRunsPage};

const GITHUB_API: &str = "https://api.github.com";

/// GitHub rejects requests without one.
const USER_AGENT: &str = "remote-development-mcp";

/// Pinned rather than left to float, so a future default can not change the
/// shape of the responses parsed below.
const API_VERSION: &str = "2022-11-28";

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// The maximum GitHub allows, so a service with many releases still comes back
/// in one call in the common case.
const PER_PAGE: usize = 100;

/// A service with more tags than this has bigger problems; the cap stops a
/// pathological repository turning one tool call into hundreds of requests.
const MAX_PAGES: usize = 10;

#[derive(Deserialize)]
struct GitRef {
    #[serde(rename = "ref")]
    reference: String,
}

#[derive(Serialize)]
struct CreateReleaseBody<'s> {
    tag_name: &'s str,
    name: &'s str,
    body: &'s str,
}

#[derive(Deserialize)]
struct CreatedRelease {
    html_url: Option<String>,
}

/// Talks to the GitHub REST API directly.
///
/// Directly rather than through the `gh` CLI on purpose: the CLI would have to
/// be installed and separately authenticated on whichever machine runs this
/// server — the same missing-binary problem that made `search` fail — and its
/// human-facing output would have to be parsed. A token from the settings and
/// two REST calls have neither issue.
pub struct GitHubClient {
    token: String,
}

impl GitHubClient {
    pub fn new(token: String) -> Self {
        Self { token }
    }

    /// Every tag of one service, newest version last.
    ///
    /// Asks for refs matching the service prefix rather than listing releases:
    /// a tag is what a release creates and what a new one would collide with, and
    /// the prefix filter keeps the answer small even in a monorepo with hundreds
    /// of releases across many services.
    pub async fn released_versions(
        &self,
        slug: &RepoSlug,
        tag: &ReleaseTag,
    ) -> Result<Vec<Version>, String> {
        let mut versions = Vec::new();

        for page in 1..=MAX_PAGES {
            let refs = self.matching_tag_refs(slug, &tag.prefix(), page).await?;

            let received = refs.len();

            for git_ref in refs {
                // `refs/tags/price-feed-0.1.0` → `price-feed-0.1.0`
                let name = match git_ref.reference.strip_prefix("refs/tags/") {
                    Some(name) => name,
                    None => continue,
                };

                // Anything that is not this service's, or not a plain version,
                // is skipped — see `ReleaseTag::version_of`.
                if let Some(version) = tag.version_of(name) {
                    versions.push(version);
                }
            }

            if received < PER_PAGE {
                break;
            }
        }

        versions.sort();
        versions.dedup();

        Ok(versions)
    }

    async fn matching_tag_refs(
        &self,
        slug: &RepoSlug,
        prefix: &str,
        page: usize,
    ) -> Result<Vec<GitRef>, String> {
        // `git/matching-refs/tags/<prefix>` returns every tag starting with the
        // prefix. With no prefix (a single-repo service) it returns all tags,
        // which is what we want there.
        let mut request = FlUrl::new(GITHUB_API)
            .append_path_segment("repos")
            .append_path_segment(slug.owner.as_str())
            .append_path_segment(slug.repo.as_str())
            .append_path_segment("git")
            .append_path_segment("matching-refs")
            .append_path_segment("tags");

        if !prefix.is_empty() {
            request = request.append_path_segment(prefix);
        }

        let mut response = self
            .with_headers(request)
            .append_query_param("per_page", Some(PER_PAGE.to_string().as_str()))
            .append_query_param("page", Some(page.to_string().as_str()))
            .get()
            .await
            .map_err(|err| format!("Can not reach the GitHub API. Err: {:?}", err))?;

        let status = response.get_status_code();

        // A repository with no tags at all answers 404 here rather than an empty
        // list — that is "nothing released yet", not a failure.
        if status == 404 {
            return Ok(Vec::new());
        }

        if status != 200 {
            return Err(describe_failure(status, &mut response).await);
        }

        response
            .get_json()
            .await
            .map_err(|err| format!("Can not read the GitHub tag list. Err: {:?}", err))
    }

    /// Creates the release, which is what also creates the tag and triggers the
    /// build workflow.
    pub async fn create_release(
        &self,
        slug: &RepoSlug,
        tag_name: &str,
    ) -> Result<Option<String>, String> {
        let body = CreateReleaseBody {
            tag_name,
            // Title equals the tag, per the release guide.
            name: tag_name,
            body: "",
        };

        let request = FlUrl::new(GITHUB_API)
            .append_path_segment("repos")
            .append_path_segment(slug.owner.as_str())
            .append_path_segment(slug.repo.as_str())
            .append_path_segment("releases");

        let mut response = self
            .with_headers(request)
            .post(HttpRequestBody::as_json(&body))
            .await
            .map_err(|err| format!("Can not reach the GitHub API. Err: {:?}", err))?;

        let status = response.get_status_code();

        if status != 201 {
            return Err(describe_failure(status, &mut response).await);
        }

        let created: CreatedRelease = response
            .get_json()
            .await
            .map_err(|err| format!("Can not read the created release. Err: {:?}", err))?;

        Ok(created.html_url)
    }

    /// The most recent Actions runs, newest first — what GitHub returns by
    /// default.
    pub async fn list_workflow_runs(
        &self,
        slug: &RepoSlug,
        amount: usize,
    ) -> Result<Vec<WorkflowRun>, String> {
        let request = FlUrl::new(GITHUB_API)
            .append_path_segment("repos")
            .append_path_segment(slug.owner.as_str())
            .append_path_segment(slug.repo.as_str())
            .append_path_segment("actions")
            .append_path_segment("runs");

        let mut response = self
            .with_headers(request)
            .append_query_param("per_page", Some(amount.to_string().as_str()))
            .get()
            .await
            .map_err(|err| format!("Can not reach the GitHub API. Err: {:?}", err))?;

        let status = response.get_status_code();

        if status != 200 {
            return Err(describe_failure(status, &mut response).await);
        }

        let page: WorkflowRunsPage = response
            .get_json()
            .await
            .map_err(|err| format!("Can not read the workflow runs. Err: {:?}", err))?;

        Ok(page.workflow_runs)
    }

    pub async fn get_workflow_run(
        &self,
        slug: &RepoSlug,
        run_id: u64,
    ) -> Result<WorkflowRun, String> {
        let request = FlUrl::new(GITHUB_API)
            .append_path_segment("repos")
            .append_path_segment(slug.owner.as_str())
            .append_path_segment(slug.repo.as_str())
            .append_path_segment("actions")
            .append_path_segment("runs")
            .append_path_segment(run_id.to_string());

        let mut response = self
            .with_headers(request)
            .get()
            .await
            .map_err(|err| format!("Can not reach the GitHub API. Err: {:?}", err))?;

        let status = response.get_status_code();

        if status != 200 {
            return Err(describe_failure(status, &mut response).await);
        }

        response
            .get_json()
            .await
            .map_err(|err| format!("Can not read the workflow run. Err: {:?}", err))
    }

    /// Which job and step failed. Only worth asking once a run has failed, so
    /// the caller decides when to spend the request.
    pub async fn failed_step(&self, slug: &RepoSlug, run_id: u64) -> Option<String> {
        let request = FlUrl::new(GITHUB_API)
            .append_path_segment("repos")
            .append_path_segment(slug.owner.as_str())
            .append_path_segment(slug.repo.as_str())
            .append_path_segment("actions")
            .append_path_segment("runs")
            .append_path_segment(run_id.to_string())
            .append_path_segment("jobs");

        let mut response = self.with_headers(request).get().await.ok()?;

        if response.get_status_code() != 200 {
            return None;
        }

        let jobs: WorkflowJobs = response.get_json().await.ok()?;

        jobs.first_failure()
    }

    fn with_headers(&self, request: FlUrl) -> FlUrl {
        request
            .with_header("Authorization", format!("Bearer {}", self.token))
            .with_header("Accept", "application/vnd.github+json")
            .with_header("X-GitHub-Api-Version", API_VERSION)
            .with_header("User-Agent", USER_AGENT)
            .set_timeout(REQUEST_TIMEOUT)
    }
}

/// Turns a failed call into something the caller can act on, rather than a bare
/// status number.
async fn describe_failure(status: u16, response: &mut flurl::FlUrlResponse) -> String {
    let body = response
        .get_body_as_str()
        .await
        .map(|body| body.to_string())
        .unwrap_or_default();

    let hint = match status {
        401 => " — the github_token is missing, expired or wrong",
        403 => " — the token lacks permission, or a rate limit was hit",
        404 => " — the repository does not exist, or the token can not see it",
        422 => " — GitHub refused the values; a release for this tag most likely already exists",
        _ => "",
    };

    format!("GitHub answered {}{}. {}", status, hint, first_line(&body))
}

fn first_line(body: &str) -> String {
    let trimmed = body.trim();

    if trimmed.is_empty() {
        return String::new();
    }

    let clamped: String = trimmed.chars().take(300).collect();

    clamped.replace('\n', " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failure_body_is_folded_onto_one_line_and_clamped() {
        let long = format!("line one\nline two\n{}", "x".repeat(1000));

        let rendered = first_line(&long);

        assert!(!rendered.contains('\n'));
        assert!(rendered.chars().count() <= 300);
    }

    #[test]
    fn an_empty_failure_body_renders_as_nothing() {
        assert_eq!(first_line("   \n  "), "");
    }
}
