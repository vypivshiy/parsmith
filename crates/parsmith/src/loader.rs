//! Source loading abstraction (`SourceLoader`) for filesystem and in-memory virtual files.

use std::collections::{BTreeMap, HashMap};
use std::path::{Component, Path, PathBuf};

use indexmap::IndexSet;
use serde::{Deserialize, Serialize};

use crate::diagnostic::{Diagnostic, DiagnosticCode};
use crate::kdl::Span;

/// A resolved KDL source unit returned by a [`SourceLoader`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoadedSource {
    /// Canonical logical path of the resolved module.
    pub path: String,
    /// UTF-8 source text of the module (with any leading UTF-8 BOM stripped).
    pub source: String,
}

impl LoadedSource {
    /// Creates a new `LoadedSource` with the given canonical path and source content.
    pub fn new(path: impl Into<String>, source: impl Into<String>) -> Self {
        let raw = source.into();
        let cleaned = raw
            .strip_prefix('\u{feff}')
            .map(str::to_owned)
            .unwrap_or(raw);
        Self {
            path: path.into(),
            source: cleaned,
        }
    }
}

/// Errors that can occur when resolving or loading a `.kdl` module through a [`SourceLoader`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize, Deserialize)]
pub enum LoaderError {
    /// The requested module file does not exist.
    #[error("file not found: {path}")]
    NotFound {
        /// Normalized path that could not be found.
        path: String,
    },
    /// An I/O error occurred while reading the module.
    #[error("cannot read file '{path}': {message}")]
    Io {
        /// Path of the file that failed to read.
        path: String,
        /// Underlying I/O error description.
        message: String,
    },
    /// A relative `import` was encountered when compiling a raw source string without a base path.
    #[error("cannot resolve import '{import_path}' without a file path")]
    NoBasePath {
        /// Raw import path from the `import` declaration.
        import_path: String,
    },
    /// The import path is malformed or empty.
    #[error("invalid import path '{path}': {message}")]
    InvalidPath {
        /// Raw path string.
        path: String,
        /// Reason the path was rejected.
        message: String,
    },
}

/// Compiler abstraction for resolving and loading `.kdl` schema files from a filesystem
/// or an in-memory virtual file map (`VFS`).
pub trait SourceLoader: Send + Sync {
    /// Resolves `import_path` relative to `from_path` (or loads `import_path` directly as an entry
    /// module when `from_path` is `None`) and returns its canonical path and UTF-8 content.
    fn resolve(
        &self,
        from_path: Option<&str>,
        import_path: &str,
    ) -> Result<LoadedSource, LoaderError>;

    /// Convenience helper to load an entry module by path (`self.resolve(None, path)`).
    fn load(&self, path: &str) -> Result<LoadedSource, LoaderError> {
        self.resolve(None, path)
    }
}

/// Filesystem-backed [`SourceLoader`] that resolves imports relative to the importing file's
/// parent directory and normalizes canonical paths across Windows and POSIX.
#[derive(Debug, Clone, Default)]
pub struct FsSourceLoader {
    base_dir: Option<PathBuf>,
}

impl FsSourceLoader {
    /// Creates a new `FsSourceLoader`.
    pub fn new() -> Self {
        Self { base_dir: None }
    }

    /// Creates a new `FsSourceLoader` anchored at `base_dir` for relative entry paths.
    pub fn with_base_dir(base_dir: impl Into<PathBuf>) -> Self {
        Self {
            base_dir: Some(base_dir.into()),
        }
    }
}

impl SourceLoader for FsSourceLoader {
    fn resolve(
        &self,
        from_path: Option<&str>,
        import_path: &str,
    ) -> Result<LoadedSource, LoaderError> {
        let trimmed = import_path.trim();
        if trimmed.is_empty() {
            return Err(LoaderError::InvalidPath {
                path: import_path.to_string(),
                message: "import path cannot be empty".to_string(),
            });
        }

        let candidate = match from_path {
            Some(from) => {
                if from.trim().is_empty() {
                    return Err(LoaderError::NoBasePath {
                        import_path: import_path.to_string(),
                    });
                }
                let parent = Path::new(from)
                    .parent()
                    .filter(|p| !p.as_os_str().is_empty())
                    .unwrap_or_else(|| Path::new("."));
                parent.join(import_path)
            }
            None => {
                let p = Path::new(import_path);
                if p.is_relative() {
                    if let Some(base) = &self.base_dir {
                        base.join(p)
                    } else {
                        p.to_path_buf()
                    }
                } else {
                    p.to_path_buf()
                }
            }
        };

        let normalized = normalize_fs_path(&candidate);
        let normalized_str = normalized.to_string_lossy().into_owned();

        if !normalized.exists() || !normalized.is_file() {
            return Err(LoaderError::NotFound {
                path: normalized_str,
            });
        }

        let content = std::fs::read_to_string(&normalized).map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                LoaderError::NotFound {
                    path: normalized_str.clone(),
                }
            } else {
                LoaderError::Io {
                    path: normalized_str.clone(),
                    message: err.to_string(),
                }
            }
        })?;

        Ok(LoadedSource::new(normalized_str, content))
    }
}

/// Expands an iterator of file and/or directory paths into an ordered, deduplicated list of
/// `.kdl` file paths, along with any `E003` directory discovery diagnostics.
///
/// - Directories are scanned flat (`fs::read_dir`) for direct `.kdl` child files (case-insensitive)
///   and sorted lexicographically by file name.
/// - When `skip_underscore_in_dirs` is `true` (used by `generate`), files inside directories whose
///   file name starts with `'_'` (`_*.kdl`) are skipped as root entrypoints.
/// - If a directory contains no matching `.kdl` files (or cannot be read), a `DiagnosticCode::E003`
///   error is recorded.
/// - Canonical paths (`normalize_fs_path`) are deduplicated while preserving first-seen order.
pub fn expand_kdl_input_paths<I, P>(
    paths: I,
    skip_underscore_in_dirs: bool,
) -> (Vec<PathBuf>, Vec<Diagnostic>)
where
    I: IntoIterator<Item = P>,
    P: AsRef<Path>,
{
    let raw_paths: Vec<PathBuf> = paths
        .into_iter()
        .map(|p| p.as_ref().to_path_buf())
        .collect();
    if raw_paths.is_empty() {
        return (
            Vec::new(),
            vec![Diagnostic::error(
                DiagnosticCode::E003,
                "no input paths provided",
                Span::EMPTY,
            )],
        );
    }

    let mut expanded = Vec::new();
    let mut diagnostics = Vec::new();
    let mut seen_canonical = IndexSet::new();

    for raw_path in raw_paths {
        if raw_path.is_dir() {
            let entries = match std::fs::read_dir(&raw_path) {
                Ok(rd) => rd,
                Err(err) => {
                    diagnostics.push(
                        Diagnostic::error(
                            DiagnosticCode::E003,
                            format!("cannot read directory '{}': {err}", raw_path.display()),
                            Span::EMPTY,
                        )
                        .with_path(raw_path.to_string_lossy().into_owned()),
                    );
                    continue;
                }
            };

            let mut dir_files = Vec::new();
            for entry in entries.flatten() {
                let entry_path = entry.path();
                if !entry_path.is_file() {
                    continue;
                }
                let is_kdl = entry_path
                    .extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| e.eq_ignore_ascii_case("kdl"));
                if !is_kdl {
                    continue;
                }
                if skip_underscore_in_dirs
                    && entry_path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.starts_with('_'))
                {
                    continue;
                }
                dir_files.push(entry_path);
            }

            dir_files.sort_by(|a, b| a.file_name().cmp(&b.file_name()));

            if dir_files.is_empty() {
                diagnostics.push(
                    Diagnostic::error(
                        DiagnosticCode::E003,
                        format!("no .kdl files found in directory '{}'", raw_path.display()),
                        Span::EMPTY,
                    )
                    .with_path(raw_path.to_string_lossy().into_owned()),
                );
                continue;
            }

            for file_path in dir_files {
                let canon = normalize_fs_path(&file_path);
                if seen_canonical.insert(canon) {
                    expanded.push(file_path);
                }
            }
        } else {
            let canon = normalize_fs_path(&raw_path);
            if seen_canonical.insert(canon) {
                expanded.push(raw_path);
            }
        }
    }

    (expanded, diagnostics)
}

/// Normalizes a filesystem path into a canonical path with Windows verbatim (`\\?\`) prefixes stripped.
pub fn normalize_fs_path(path: &Path) -> PathBuf {
    if let Ok(canon) = std::fs::canonicalize(path) {
        return strip_windows_verbatim(canon);
    }

    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else if let Ok(cwd) = std::env::current_dir() {
        cwd.join(path)
    } else {
        path.to_path_buf()
    };

    // Walk up to find the deepest existing ancestor so symlinks/casing on existing dirs are resolved.
    let mut existing = abs.as_path();
    let mut tail_components = Vec::new();
    while !existing.exists() {
        if let Some(name) = existing.file_name() {
            tail_components.push(name.to_os_string());
        }
        if let Some(parent) = existing.parent() {
            if parent == existing {
                break;
            }
            existing = parent;
        } else {
            break;
        }
    }

    let mut base = if existing.exists() {
        std::fs::canonicalize(existing)
            .map(strip_windows_verbatim)
            .unwrap_or_else(|_| existing.to_path_buf())
    } else {
        abs.clone()
    };

    for comp in tail_components.into_iter().rev() {
        base.push(comp);
    }

    lexical_clean_path(&base)
}

fn strip_windows_verbatim(path: PathBuf) -> PathBuf {
    let Some(s) = path.to_str() else {
        return path;
    };
    if let Some(unc) = s.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{unc}"));
    }
    if let Some(stripped) = s.strip_prefix(r"\\?\") {
        return PathBuf::from(stripped);
    }
    path
}

fn lexical_clean_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::Prefix(p) => out.push(p.as_os_str()),
            Component::RootDir => out.push(comp.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                let can_pop = matches!(out.components().next_back(), Some(Component::Normal(_)));
                if can_pop {
                    out.pop();
                } else if !out.has_root() {
                    out.push("..");
                }
            }
            Component::Normal(c) => out.push(c),
        }
    }
    if out.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        out
    }
}

/// In-memory virtual filesystem [`SourceLoader`] backed by a normalized `BTreeMap<String, String>`.
///
/// Resolves `./` and `../` relative paths lexically without touching disk.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemorySourceLoader {
    files: BTreeMap<String, String>,
}

/// Canonical domain alias for [`MemorySourceLoader`] (`MemorySourceMap`).
pub type MemorySourceMap = MemorySourceLoader;

impl MemorySourceLoader {
    /// Creates an empty `MemorySourceLoader`.
    pub fn new() -> Self {
        Self {
            files: BTreeMap::new(),
        }
    }

    /// Creates a `MemorySourceLoader` from an iterator of `(path, source)` pairs.
    pub fn from_map<I, K, V>(iter: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        let mut loader = Self::new();
        for (k, v) in iter {
            loader.insert(k, v);
        }
        loader
    }

    /// Inserts a virtual file into the map, normalizing its logical path.
    pub fn insert(&mut self, path: impl Into<String>, source: impl Into<String>) {
        let norm = normalize_virtual_path(&path.into());
        let raw = source.into();
        let cleaned = raw
            .strip_prefix('\u{feff}')
            .map(str::to_owned)
            .unwrap_or(raw);
        self.files.insert(norm, cleaned);
    }

    /// Builder helper: inserts a virtual file and returns `self`.
    pub fn with_file(mut self, path: impl Into<String>, source: impl Into<String>) -> Self {
        self.insert(path, source);
        self
    }

    /// Returns the source content for `path` (after normalizing `path`), if present.
    pub fn get(&self, path: &str) -> Option<&str> {
        let norm = normalize_virtual_path(path);
        self.files.get(&norm).map(String::as_str)
    }

    /// Returns a reference to the underlying normalized virtual file map.
    pub fn files(&self) -> &BTreeMap<String, String> {
        &self.files
    }

    /// Returns `true` if the virtual file map contains no files.
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// Returns the number of virtual files in the loader.
    pub fn len(&self) -> usize {
        self.files.len()
    }
}

impl From<BTreeMap<String, String>> for MemorySourceLoader {
    fn from(map: BTreeMap<String, String>) -> Self {
        Self::from_map(map)
    }
}

impl From<HashMap<String, String>> for MemorySourceLoader {
    fn from(map: HashMap<String, String>) -> Self {
        Self::from_map(map)
    }
}

impl<K, V> FromIterator<(K, V)> for MemorySourceLoader
where
    K: Into<String>,
    V: Into<String>,
{
    fn from_iter<T: IntoIterator<Item = (K, V)>>(iter: T) -> Self {
        Self::from_map(iter)
    }
}

impl SourceLoader for MemorySourceLoader {
    fn resolve(
        &self,
        from_path: Option<&str>,
        import_path: &str,
    ) -> Result<LoadedSource, LoaderError> {
        let trimmed = import_path.trim();
        if trimmed.is_empty() {
            return Err(LoaderError::InvalidPath {
                path: import_path.to_string(),
                message: "import path cannot be empty".to_string(),
            });
        }

        let resolved_path = match from_path {
            Some(from) => {
                if from.trim().is_empty() {
                    return Err(LoaderError::NoBasePath {
                        import_path: import_path.to_string(),
                    });
                }
                resolve_virtual_relative(from, trimmed)
            }
            None => normalize_virtual_path(trimmed),
        };

        match self.files.get(&resolved_path) {
            Some(source) => Ok(LoadedSource::new(resolved_path, source.clone())),
            None => Err(LoaderError::NotFound {
                path: resolved_path,
            }),
        }
    }
}

/// Normalizes a virtual path by replacing `\` with `/` and collapsing `.` and `..` segments.
pub fn normalize_virtual_path(path: &str) -> String {
    let unified = path.replace('\\', "/");
    let is_abs = unified.starts_with('/');
    let mut stack: Vec<&str> = Vec::new();

    for segment in unified.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                if let Some(&last) = stack.last() {
                    if last != ".." {
                        stack.pop();
                        continue;
                    }
                }
                if !is_abs {
                    stack.push("..");
                }
            }
            seg => stack.push(seg),
        }
    }

    if is_abs {
        format!("/{}", stack.join("/"))
    } else {
        stack.join("/")
    }
}

/// Resolves `import_path` relative to the parent directory of `from_path` in virtual path space.
pub fn resolve_virtual_relative(from_path: &str, import_path: &str) -> String {
    let import_unified = import_path.replace('\\', "/");
    if import_unified.starts_with('/') {
        return normalize_virtual_path(&import_unified);
    }

    let norm_from = normalize_virtual_path(from_path);
    let base_dir = match norm_from.rfind('/') {
        Some(0) => "/",
        Some(idx) => &norm_from[..idx],
        None => "",
    };

    if base_dir.is_empty() {
        normalize_virtual_path(&import_unified)
    } else if base_dir == "/" {
        normalize_virtual_path(&format!("/{import_unified}"))
    } else {
        normalize_virtual_path(&format!("{base_dir}/{import_unified}"))
    }
}
