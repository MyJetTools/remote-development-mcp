use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

/// Resolves `requested` against `root` and guarantees the result stays inside it.
///
/// `requested` is normally relative to the repository root; an absolute path is
/// accepted too, but only if it happens to point inside the root.
///
/// The interesting case is a path that does not exist yet — `write_file` into a
/// new file, `mkdir` into a new directory. Plain `canonicalize` fails there, so
/// instead the deepest *existing* ancestor is canonicalized (which is what
/// actually resolves symlinks and `..`), checked against the root, and the
/// remaining components are appended literally.
///
/// A dangling symlink is refused rather than treated as a free-form name:
/// writing through `link -> /nonexistent/elsewhere` would otherwise create a
/// file outside the root.
///
/// `root` must already be canonical — [`super::RepoContext`] canonicalizes it
/// once at startup.
/// Caps on the caller-supplied path before the ancestor walk. The walk does one
/// blocking `symlink_metadata` per component on a runtime thread; bounding the
/// length and the component count keeps its cost constant rather than letting a
/// pathological path occupy a worker thread.
const MAX_PATH_BYTES: usize = 4096;
const MAX_PATH_COMPONENTS: usize = 255;

pub fn resolve_inside_root(root: &Path, requested: &str) -> Result<PathBuf, String> {
    let requested = requested.trim();

    if requested.is_empty() || requested == "." {
        return Ok(root.to_path_buf());
    }

    if requested.len() > MAX_PATH_BYTES {
        return Err(format!(
            "Path is too long ({} bytes, limit {})",
            requested.len(),
            MAX_PATH_BYTES
        ));
    }

    let requested_path = Path::new(requested);

    if requested_path.components().count() > MAX_PATH_COMPONENTS {
        return Err(format!(
            "Path has too many components (limit {})",
            MAX_PATH_COMPONENTS
        ));
    }

    let joined = if requested_path.is_absolute() {
        requested_path.to_path_buf()
    } else {
        root.join(requested_path)
    };

    let mut tail: Vec<OsString> = Vec::new();
    let mut cursor = joined.clone();

    let existing = loop {
        // symlink_metadata does not follow the link, so a dangling symlink is
        // seen as existing here and then rejected by canonicalize below.
        if cursor.symlink_metadata().is_ok() {
            break cursor.canonicalize().map_err(|err| {
                format!(
                    "Can not resolve path '{}'. It is most likely a broken symlink. Err: {}",
                    cursor.display(),
                    err
                )
            })?;
        }

        let file_name = match cursor.file_name() {
            Some(file_name) => file_name.to_os_string(),
            None => {
                return Err(format!(
                    "Path '{}' can not be resolved inside the repository",
                    requested
                ))
            }
        };

        tail.push(file_name);

        if !cursor.pop() {
            return Err(format!(
                "Path '{}' can not be resolved inside the repository",
                requested
            ));
        }
    };

    if !existing.starts_with(root) {
        return Err(rejected(requested, root));
    }

    let mut result = existing;

    // Collected from the end while walking up, so put them back in order.
    while let Some(file_name) = tail.pop() {
        result.push(file_name);
    }

    // `file_name()` never yields `.` or `..`, so the loop above can not climb
    // out of the root. Verified anyway — this is the invariant the whole server
    // rests on.
    if !result.starts_with(root) {
        return Err(rejected(requested, root));
    }

    Ok(result)
}

fn rejected(requested: &str, root: &Path) -> String {
    format!(
        "Path '{}' resolves outside of the repository root '{}' and was refused",
        requested,
        root.display()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestRepo {
        root: PathBuf,
    }

    impl TestRepo {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir()
                .join("remote-development-mcp-tests")
                .join(name);

            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(root.join("src")).unwrap();
            std::fs::write(root.join("src").join("main.rs"), "fn main() {}").unwrap();

            Self {
                root: root.canonicalize().unwrap(),
            }
        }

        fn resolve(&self, requested: &str) -> Result<PathBuf, String> {
            resolve_inside_root(&self.root, requested)
        }
    }

    impl Drop for TestRepo {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn resolves_existing_relative_path() {
        let repo = TestRepo::new("existing_relative");

        let resolved = repo.resolve("src/main.rs").unwrap();

        assert_eq!(resolved, repo.root.join("src").join("main.rs"));
    }

    #[test]
    fn empty_path_means_repository_root() {
        let repo = TestRepo::new("empty_path");

        assert_eq!(repo.resolve("").unwrap(), repo.root);
        assert_eq!(repo.resolve(".").unwrap(), repo.root);
    }

    #[test]
    fn resolves_path_which_does_not_exist_yet() {
        let repo = TestRepo::new("not_existing_yet");

        let resolved = repo.resolve("src/generated/models.rs").unwrap();

        assert_eq!(
            resolved,
            repo.root.join("src").join("generated").join("models.rs")
        );
    }

    #[test]
    fn dot_dot_staying_inside_is_allowed() {
        let repo = TestRepo::new("dot_dot_inside");

        let resolved = repo.resolve("src/../src/main.rs").unwrap();

        assert_eq!(resolved, repo.root.join("src").join("main.rs"));
    }

    #[test]
    fn dot_dot_escape_is_refused() {
        let repo = TestRepo::new("dot_dot_escape");

        let err = repo.resolve("../../../etc/passwd").unwrap_err();

        assert!(err.contains("outside of the repository root"), "{}", err);
    }

    #[test]
    fn dot_dot_escape_hidden_behind_existing_dir_is_refused() {
        let repo = TestRepo::new("dot_dot_hidden");

        let err = repo.resolve("src/../../..").unwrap_err();

        assert!(err.contains("outside of the repository root"), "{}", err);
    }

    #[test]
    fn absolute_path_outside_root_is_refused() {
        let repo = TestRepo::new("absolute_outside");

        let err = repo.resolve("/etc/passwd").unwrap_err();

        assert!(err.contains("outside of the repository root"), "{}", err);
    }

    #[test]
    fn absolute_path_inside_root_is_allowed() {
        let repo = TestRepo::new("absolute_inside");

        let requested = repo.root.join("src").join("main.rs");
        let resolved = repo.resolve(requested.to_str().unwrap()).unwrap();

        assert_eq!(resolved, requested);
    }

    #[test]
    fn sibling_directory_sharing_a_name_prefix_is_refused() {
        let repo = TestRepo::new("prefix_sibling");

        // `<root>-evil` shares a string prefix with the root but is a different
        // directory — a string-based check would let this through.
        let sibling = PathBuf::from(format!("{}-evil", repo.root.display()));
        std::fs::create_dir_all(&sibling).unwrap();
        std::fs::write(sibling.join("secret.txt"), "secret").unwrap();

        let err = repo
            .resolve(sibling.join("secret.txt").to_str().unwrap())
            .unwrap_err();

        let _ = std::fs::remove_dir_all(&sibling);

        assert!(err.contains("outside of the repository root"), "{}", err);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_pointing_outside_is_refused() {
        let repo = TestRepo::new("symlink_escape");

        let outside = std::env::temp_dir()
            .join("remote-development-mcp-tests")
            .join("symlink_escape_target");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("secret.txt"), "secret").unwrap();

        std::os::unix::fs::symlink(&outside, repo.root.join("escape")).unwrap();

        let err = repo.resolve("escape/secret.txt").unwrap_err();

        let _ = std::fs::remove_dir_all(&outside);

        assert!(err.contains("outside of the repository root"), "{}", err);
    }

    #[cfg(unix)]
    #[test]
    fn symlink_staying_inside_is_allowed() {
        let repo = TestRepo::new("symlink_inside");

        std::os::unix::fs::symlink(repo.root.join("src"), repo.root.join("src-link")).unwrap();

        let resolved = repo.resolve("src-link/main.rs").unwrap();

        assert_eq!(resolved, repo.root.join("src").join("main.rs"));
    }

    #[cfg(unix)]
    #[test]
    fn dangling_symlink_is_refused() {
        let repo = TestRepo::new("dangling_symlink");

        let outside = std::env::temp_dir()
            .join("remote-development-mcp-tests")
            .join("dangling_target_which_does_not_exist");
        let _ = std::fs::remove_file(&outside);

        std::os::unix::fs::symlink(&outside, repo.root.join("dangling")).unwrap();

        let err = repo.resolve("dangling").unwrap_err();

        assert!(err.contains("broken symlink"), "{}", err);
    }
}
