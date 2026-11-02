//! Which files a request may name, and writing them.
//!
//! The browser sends paths, so every one of them is checked here before it
//! touches the disk: plain folder names under one of a few folders of the
//! root, with the extension the request is about, and nothing else.

use std::path::{Component, Path, PathBuf};

/// Where scenarios live.
pub(crate) const SCENARIOS: &[&str] = &["scenarios"];
/// Where creatures live: the committed ones, and whatever sits beside the
/// scenarios that use it.
pub(crate) const CREATURES: &[&str] = &["content", "scenarios"];

/// What a file or folder may be called: nothing a shell or another system
/// would read as anything but a name.
fn plain(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('.')
        && !name.ends_with([' ', '.'])
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ' '))
}

/// The file `path` names under the root, if it is a `what` this server
/// handles: under one of `tops`, ending in `.extension`.
pub(crate) fn place(
    root: &Path,
    path: &str,
    tops: &[&str],
    extension: &str,
    what: &str,
) -> Result<PathBuf, String> {
    let refused = || format!("`{path}` is not a {what} in the library");
    let relative = Path::new(path);
    let mut names = Vec::new();
    for component in relative.components() {
        match component {
            Component::Normal(name) => names.push(name.to_str().ok_or_else(refused)?),
            _ => return Err(refused()),
        }
    }
    let under = names.first().is_some_and(|top| tops.contains(top));
    let named = names.len() > 1 && path.ends_with(&format!(".{extension}"));
    if !under || !named || !names.iter().all(|name| plain(name)) {
        return Err(refused());
    }
    Ok(root.join(relative))
}

/// [`place`], for a file that has to be there already.
pub(crate) fn existing(
    root: &Path,
    path: &str,
    tops: &[&str],
    extension: &str,
    what: &str,
) -> Result<PathBuf, String> {
    let file = place(root, path, tops, extension, what)?;
    if !file.is_file() {
        return Err(format!("there is no {what} at `{path}`"));
    }
    Ok(file)
}

/// Write a file, making the folders above it. `fresh` is a file that must
/// not replace one already there.
pub(crate) fn write(file: &Path, path: &str, text: &str, fresh: bool) -> Result<(), String> {
    if fresh && file.exists() {
        return Err(format!(
            "there is already a file at `{path}` - pick another name"
        ));
    }
    if let Some(folder) = file.parent() {
        std::fs::create_dir_all(folder).map_err(|e| format!("{path}: {e}"))?;
    }
    std::fs::write(file, text).map_err(|e| format!("{path}: {e}"))
}

/// A path under the root the way the browser names it: forward slashes.
pub(crate) fn named(root: &Path, file: &Path) -> Vec<String> {
    file.strip_prefix(root)
        .unwrap_or(file)
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect()
}

/// Every file with this extension under a folder, however deep.
pub(crate) fn walk(dir: &Path, extension: &str, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, extension, out);
        } else if path.extension().is_some_and(|e| e == extension) {
            out.push(path);
        }
    }
}
