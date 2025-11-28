use std::env;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SemanticStatus {
    Missing,
    Indexing,
    Ready,
}

pub(crate) fn find_sgrep_binary() -> Option<PathBuf> {
    bundled_sgrep_path()
        .filter(|path| path.is_file())
        .or_else(find_in_path)
}

fn bundled_sgrep_path() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".codex-kaioken/bin/sgrep"))
}

fn find_in_path() -> Option<PathBuf> {
    let path_var = env::var_os("PATH")?;
    for dir in env::split_paths(&path_var) {
        let candidate = dir.join("sgrep");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}
