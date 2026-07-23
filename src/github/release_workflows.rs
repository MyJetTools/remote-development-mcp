use std::path::Path;

/// What the repository's `.github/workflows` folder says about how it releases.
///
/// This is worth reading before creating anything. The release guide is explicit
/// that a tag created without its workflow present simply does not build — so a
/// tool that skipped this check would report a successful release and quietly
/// produce no image at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReleaseLayout {
    /// One service per repository: the tag is the bare version, and a single
    /// `release.yaml` triggers on any tag.
    SingleRepo,
    /// Several services: each has `release-{service}.yaml` triggering on
    /// `{service}-*`, and the tag carries the service name.
    Monorepo { services: Vec<String> },
    /// No release workflow at all — releasing would create a tag nothing acts
    /// on.
    None,
}

/// Reads the layout from `.github/workflows/`.
///
/// The service names come from the tag patterns inside the workflows rather than
/// from folder names or from the file name alone: the tag pattern is the thing
/// GitHub actually matches, so it is the only definition that cannot disagree
/// with what triggers a build.
pub async fn read_release_layout(repo_root: &Path) -> ReleaseLayout {
    let folder = repo_root.join(".github").join("workflows");

    let mut reader = match tokio::fs::read_dir(&folder).await {
        Ok(reader) => reader,
        Err(_) => return ReleaseLayout::None,
    };

    let mut services = Vec::new();
    let mut has_bare_release = false;

    loop {
        let entry = match reader.next_entry().await {
            Ok(Some(entry)) => entry,
            Ok(None) => break,
            Err(_) => break,
        };

        let file_name = entry.file_name().to_string_lossy().to_string();

        let stem = match workflow_stem(&file_name) {
            Some(stem) => stem,
            None => continue,
        };

        if stem == "release" {
            has_bare_release = true;
            continue;
        }

        let from_name = match stem.strip_prefix("release-") {
            Some(from_name) => from_name.to_string(),
            None => continue,
        };

        // The tag pattern is authoritative; the file name is the fallback for a
        // workflow written in a shape this parser does not recognise.
        let content = tokio::fs::read_to_string(entry.path())
            .await
            .unwrap_or_default();

        match service_from_tag_patterns(&content) {
            Some(service) => services.push(service),
            None => services.push(from_name),
        }
    }

    if !services.is_empty() {
        services.sort();
        services.dedup();

        return ReleaseLayout::Monorepo { services };
    }

    if has_bare_release {
        return ReleaseLayout::SingleRepo;
    }

    ReleaseLayout::None
}

/// `release-margin-engine.yaml` -> `release-margin-engine`
fn workflow_stem(file_name: &str) -> Option<&str> {
    for extension in [".yaml", ".yml"] {
        if let Some(stem) = file_name.strip_suffix(extension) {
            return Some(stem);
        }
    }

    None
}

/// Pulls the service name out of the workflow's tag patterns.
///
/// Handles the two shapes these workflows are written in — `'margin-engine-*'`
/// and `'release-mcp-[0-9]*'`, the second being how a service guards against a
/// longer sibling name matching its own pattern.
fn service_from_tag_patterns(content: &str) -> Option<String> {
    for line in content.lines() {
        let line = line.trim();

        // Tag patterns appear as list entries under `tags:`.
        let candidate = match line.strip_prefix('-') {
            Some(candidate) => candidate.trim(),
            None => continue,
        };

        let candidate = candidate.trim_matches(['\'', '"'].as_ref());

        // A negation (`!other-*`) excludes, it does not name this service.
        if candidate.starts_with('!') {
            continue;
        }

        if let Some(service) = service_from_tag_pattern(candidate) {
            return Some(service);
        }
    }

    None
}

fn service_from_tag_pattern(pattern: &str) -> Option<String> {
    // `release-mcp-[0-9]*` — the digit guard.
    if let Some(prefix) = pattern.strip_suffix("-[0-9]*") {
        return non_empty(prefix);
    }

    // `margin-engine-*`
    if let Some(prefix) = pattern.strip_suffix("-*") {
        return non_empty(prefix);
    }

    None
}

fn non_empty(src: &str) -> Option<String> {
    let src = src.trim();

    if src.is_empty() || src == "*" {
        return None;
    }

    Some(src.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_plain_service_pattern() {
        let workflow = r#"
on:
  push:
    tags:
      - 'margin-engine-*'
"#;

        assert_eq!(
            service_from_tag_patterns(workflow).unwrap(),
            "margin-engine"
        );
    }

    /// The shape a service uses to stop a longer sibling name matching it.
    #[test]
    fn reads_the_digit_guarded_pattern() {
        let workflow = r#"
on:
  push:
    tags:
      - 'release-mcp-[0-9]*'
"#;

        assert_eq!(service_from_tag_patterns(workflow).unwrap(), "release-mcp");
    }

    #[test]
    fn a_negated_pattern_does_not_name_the_service() {
        let workflow = r#"
on:
  push:
    tags:
      - '!mt-admin-api-*'
      - 'mt-admin-*'
"#;

        assert_eq!(service_from_tag_patterns(workflow).unwrap(), "mt-admin");
    }

    #[test]
    fn a_bare_wildcard_names_nothing() {
        let workflow = r#"
on:
  push:
    tags: "*"
"#;

        assert!(service_from_tag_patterns(workflow).is_none());
    }

    #[test]
    fn double_quotes_and_no_quotes_both_work() {
        assert_eq!(
            service_from_tag_patterns("      - \"web-*\"").unwrap(),
            "web"
        );
        assert_eq!(service_from_tag_patterns("      - web-*").unwrap(), "web");
    }

    #[test]
    fn workflow_stems_are_read_from_both_extensions() {
        assert_eq!(workflow_stem("release-web.yaml").unwrap(), "release-web");
        assert_eq!(workflow_stem("release.yml").unwrap(), "release");
        assert!(workflow_stem("notes.md").is_none());
    }

    async fn layout_of(name: &str, files: &[(&str, &str)]) -> ReleaseLayout {
        let root = std::env::temp_dir()
            .join("remote-development-mcp-tests-workflows")
            .join(name);

        let _ = std::fs::remove_dir_all(&root);

        let folder = root.join(".github").join("workflows");
        std::fs::create_dir_all(&folder).unwrap();

        for (file_name, content) in files {
            std::fs::write(folder.join(file_name), content).unwrap();
        }

        read_release_layout(&root).await
    }

    #[tokio::test]
    async fn a_monorepo_lists_its_services() {
        let layout = layout_of(
            "monorepo",
            &[
                (
                    "release-margin-engine.yaml",
                    "on:\n  push:\n    tags:\n      - 'margin-engine-*'\n",
                ),
                (
                    "release-price-feed.yaml",
                    "on:\n  push:\n    tags:\n      - 'price-feed-*'\n",
                ),
                ("ci.yaml", "on: push\n"),
            ],
        )
        .await;

        assert_eq!(
            layout,
            ReleaseLayout::Monorepo {
                services: vec!["margin-engine".to_string(), "price-feed".to_string()],
            }
        );
    }

    #[tokio::test]
    async fn a_single_repo_has_only_a_bare_release_workflow() {
        let layout = layout_of(
            "single",
            &[("release.yaml", "on:\n  push:\n    tags: \"*\"\n")],
        )
        .await;

        assert_eq!(layout, ReleaseLayout::SingleRepo);
    }

    #[tokio::test]
    async fn a_repository_with_no_release_workflow_says_so() {
        let layout = layout_of("none", &[("ci.yaml", "on: push\n")]).await;

        assert_eq!(layout, ReleaseLayout::None);
    }

    #[tokio::test]
    async fn a_missing_workflows_folder_is_not_an_error() {
        let root = std::env::temp_dir().join("remote-development-mcp-tests-workflows-absent");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        assert_eq!(read_release_layout(&root).await, ReleaseLayout::None);
    }

    /// The tag pattern wins over the file name, because the pattern is what
    /// GitHub matches.
    #[tokio::test]
    async fn the_tag_pattern_beats_the_file_name() {
        let layout = layout_of(
            "pattern_wins",
            &[(
                "release-web-ui.yaml",
                "on:\n  push:\n    tags:\n      - 'web-*'\n",
            )],
        )
        .await;

        assert_eq!(
            layout,
            ReleaseLayout::Monorepo {
                services: vec!["web".to_string()],
            }
        );
    }
}
