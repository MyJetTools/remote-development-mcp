use std::sync::Arc;

use ahash::AHashMap;

use super::RepoContext;

/// One MCP URL, and the projects reachable through it.
///
/// An endpoint is a *view*: it owns no repository state of its own, it only
/// decides which projects a caller can name. That is what keeps one tool set
/// serving many repositories without duplicating it per repository, and it is
/// also the whole confinement story now — a project absent from this endpoint
/// can not be reached through it even by passing its id, because [`Self::resolve`]
/// only ever looks inside `by_id`.
///
/// Two endpoints exposing the same project share one [`RepoContext`], on
/// purpose: the job registry and the concurrency limit live there, so a caller
/// can not dodge the limit by connecting to the other URL.
pub struct Endpoint {
    /// `McpMiddleware::new` wants a `&'static str` and this comes from a config
    /// file, so it is leaked once at startup. Bounded: once per configured
    /// endpoint, never per request.
    pub url: &'static str,

    /// Free-form preamble from the settings, ahead of the generated project
    /// list in the instructions.
    pub description: Option<String>,

    /// In configured order, which is the order the instructions list them in.
    projects: Vec<Arc<RepoContext>>,

    by_id: AHashMap<String, Arc<RepoContext>>,
}

impl Endpoint {
    pub fn new(
        url: &str,
        description: Option<String>,
        projects: Vec<Arc<RepoContext>>,
    ) -> Result<Self, String> {
        let url = url.trim();

        if !url.starts_with('/') {
            return Err(format!("Endpoint url '{}' must start with '/'", url));
        }

        if projects.is_empty() {
            return Err(format!(
                "Endpoint '{}' exposes no projects, so every tool call through it would fail",
                url
            ));
        }

        let mut by_id = AHashMap::with_capacity(projects.len());

        for project in projects.iter() {
            if by_id
                .insert(project.name.clone(), project.clone())
                .is_some()
            {
                return Err(format!(
                    "Endpoint '{}' lists project '{}' more than once",
                    url, project.name
                ));
            }
        }

        Ok(Self {
            url: Box::leak(url.to_string().into_boxed_str()),
            description,
            projects,
            by_id,
        })
    }

    pub fn projects(&self) -> &[Arc<RepoContext>] {
        &self.projects
    }

    /// True when this endpoint serves exactly one project, so the `project`
    /// argument can be left out.
    pub fn is_single_project(&self) -> bool {
        self.projects.len() == 1
    }

    /// Turns the `project` argument of a tool call into the project it names.
    ///
    /// Omitting it is only allowed when there is nothing to choose between. On
    /// an endpoint serving several projects a missing or unknown id is an error
    /// listing what is available, never a guess — picking a default here would
    /// mean `write_file` silently landing in the wrong repository.
    pub fn resolve(&self, requested: Option<&str>) -> Result<&Arc<RepoContext>, String> {
        let requested = requested
            .map(|itm| itm.trim())
            .filter(|itm| !itm.is_empty());

        let requested = match requested {
            Some(requested) => requested,
            None => {
                return match self.projects.first() {
                    Some(only) if self.is_single_project() => Ok(only),
                    _ => Err(format!(
                        "This endpoint serves several projects, so 'project' has to say which one \
                         to use. Available: {}",
                        self.available()
                    )),
                }
            }
        };

        match self.by_id.get(requested) {
            Some(project) => Ok(project),
            None => Err(format!(
                "There is no project '{}' on this endpoint. Available: {}",
                requested,
                self.available()
            )),
        }
    }

    /// Resolves a job id of the form `<project>:<job>`.
    ///
    /// Job ids carry their project so the job tools need no `project` argument
    /// of their own: a build started in a dependency can be polled after the
    /// conversation has moved back to the main project, with nothing to keep in
    /// sync. Returns the canonical id to look up, which matters for the bare
    /// form accepted below.
    pub fn resolve_job(&self, job_id: &str) -> Result<(&Arc<RepoContext>, String), String> {
        let job_id = job_id.trim();

        match job_id.split_once(':') {
            Some((project_id, _)) => {
                let project = self.resolve(Some(project_id)).map_err(|_| {
                    format!(
                        "Job id '{}' names project '{}', which this endpoint does not serve. \
                         Available: {}",
                        job_id,
                        project_id,
                        self.available()
                    )
                })?;

                Ok((project, job_id.to_string()))
            }

            // Every id this server hands out is prefixed, so a bare one is
            // something a human retyped. Accepting it where it can only mean one
            // thing is friendlier than a lecture about the format.
            None => {
                let project = self.resolve(None).map_err(|_| {
                    format!(
                        "Job id '{}' is missing its project prefix. Ids look like \
                         '<project>:job-000001' — use the one run_command returned",
                        job_id
                    )
                })?;

                Ok((project, format!("{}:{}", project.name, job_id)))
            }
        }
    }

    fn available(&self) -> String {
        self.projects
            .iter()
            .map(|itm| itm.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::repo::test_support::{build_test_repo, TestRepoOptions};

    async fn endpoint(names: &[&str]) -> Endpoint {
        let mut projects = Vec::new();

        for name in names {
            projects.push(build_test_repo(name, TestRepoOptions::default()).await);
        }

        Endpoint::new("/mcp", None, projects).unwrap()
    }

    #[tokio::test]
    async fn a_named_project_is_resolved() {
        let endpoint = endpoint(&["endpoint-a", "endpoint-b"]).await;

        assert_eq!(
            endpoint.resolve(Some("endpoint-b")).unwrap().name,
            "endpoint-b"
        );
    }

    #[tokio::test]
    async fn a_project_this_endpoint_does_not_serve_is_refused() {
        let endpoint = endpoint(&["endpoint-only"]).await;

        // The whole confinement story: the id is not looked up anywhere but in
        // this endpoint's own set, so naming a project served elsewhere on the
        // machine reaches nothing.
        let err = endpoint.resolve(Some("something-else")).err().unwrap();

        assert!(err.contains("no project 'something-else'"), "{}", err);
        assert!(err.contains("endpoint-only"), "{}", err);
    }

    #[tokio::test]
    async fn one_project_means_the_argument_can_be_left_out() {
        let endpoint = endpoint(&["endpoint-single"]).await;

        assert_eq!(endpoint.resolve(None).unwrap().name, "endpoint-single");
        assert_eq!(
            endpoint.resolve(Some("  ")).unwrap().name,
            "endpoint-single"
        );
    }

    #[tokio::test]
    async fn several_projects_means_it_can_not() {
        let endpoint = endpoint(&["endpoint-x", "endpoint-y"]).await;

        // Deliberately an error rather than a default: guessing here is
        // write_file landing in the wrong repository.
        let err = endpoint.resolve(None).err().unwrap();

        assert!(err.contains("has to say which one"), "{}", err);
        assert!(err.contains("endpoint-x, endpoint-y"), "{}", err);
    }

    #[tokio::test]
    async fn a_job_id_routes_itself_by_its_prefix() {
        let endpoint = endpoint(&["endpoint-j1", "endpoint-j2"]).await;

        let (project, job_id) = endpoint.resolve_job("endpoint-j2:job-000007").unwrap();

        assert_eq!(project.name, "endpoint-j2");
        assert_eq!(job_id, "endpoint-j2:job-000007");
    }

    #[tokio::test]
    async fn a_job_id_of_a_project_served_elsewhere_is_refused() {
        let endpoint = endpoint(&["endpoint-j3"]).await;

        let err = endpoint
            .resolve_job("other-project:job-000001")
            .err()
            .unwrap();

        assert!(err.contains("does not serve"), "{}", err);
    }

    #[tokio::test]
    async fn a_bare_job_id_is_accepted_where_it_can_only_mean_one_thing() {
        let single = endpoint(&["endpoint-j4"]).await;

        let (project, job_id) = single.resolve_job("job-000003").unwrap();

        assert_eq!(project.name, "endpoint-j4");
        // Canonicalized, because the registry is keyed by the full id.
        assert_eq!(job_id, "endpoint-j4:job-000003");

        let several = endpoint(&["endpoint-j5", "endpoint-j6"]).await;

        assert!(several.resolve_job("job-000003").is_err());
    }

    #[tokio::test]
    async fn an_endpoint_serving_nothing_is_a_configuration_error() {
        let err = Endpoint::new("/mcp", None, Vec::new()).err().unwrap();

        assert!(err.contains("exposes no projects"), "{}", err);
    }

    #[tokio::test]
    async fn a_url_has_to_be_a_path() {
        let project = build_test_repo("endpoint-url", TestRepoOptions::default()).await;

        let err = Endpoint::new("mcp", None, vec![project]).err().unwrap();

        assert!(err.contains("must start with '/'"), "{}", err);
    }
}
