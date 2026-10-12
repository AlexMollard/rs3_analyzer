// Favourites keyed by item name (compatible with the old favorites.json format)
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

pub const FAVORITES_FILE: &str = "favorites.json";

pub struct Favorites {
    path: PathBuf,
    map: HashMap<String, bool>,
    writable: bool,  // false if an existing file could not be read or backed up
    pub startup_warning: Option<String>,
}

impl Favorites {
    /// A corrupt file is moved to favorites.json.bak instead of being silently overwritten.
    pub fn load(path: PathBuf) -> Self {
        let shown = path.display().to_string();
        let (map, writable, warning) = match fs::read_to_string(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (HashMap::new(), true, None),
            Err(e) => (HashMap::new(), false, Some(format!(
                "Could not read {}: {} — favourites will not be saved this session", shown, e))),
            Ok(data) => match serde_json::from_str(&data) {
                Ok(map) => (map, true, None),
                Err(e) => {
                    let bak = path.with_extension("json.bak");
                    match fs::rename(&path, &bak) {
                        Ok(()) => (HashMap::new(), true, Some(format!(
                            "{} was corrupt ({}); moved it to {} and started with no favourites",
                            shown, e, bak.display()))),
                        Err(re) => (HashMap::new(), false, Some(format!(
                            "{} is corrupt ({}) and could not be backed up ({}) — favourites will not be saved this session",
                            shown, e, re))),
                    }
                }
            },
        };
        Self { path, map, writable, startup_warning: warning }
    }

    /// Used when there is no usable config directory: favourites work for this session only.
    pub fn unsaved(reason: String) -> Self {
        Self { path: PathBuf::new(), map: HashMap::new(), writable: false, startup_warning: Some(reason) }
    }

    pub fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.map.iter().filter(|(_, on)| **on).map(|(n, _)| n.clone()).collect();
        names.sort();
        names
    }

    /// The change is kept in memory even if saving fails, so the session still behaves as expected.
    pub fn set(&mut self, name: String, on: bool) -> Result<(), String> {
        self.map.insert(name, on);
        if !self.writable {
            return Err("Favourites not saved: the existing favourites file could not be read or backed up".to_string());
        }
        // Write to a temp file then rename, so a crash mid-write never leaves a truncated file
        let tmp = self.path.with_extension("json.tmp");
        serde_json::to_string(&self.map)
            .map_err(|e| e.to_string())
            .and_then(|json| fs::write(&tmp, json).map_err(|e| e.to_string()))
            .and_then(|()| fs::rename(&tmp, &self.path).map_err(|e| e.to_string()))
            .map_err(|e| format!("Failed to save {}: {}", self.path.display(), e))
    }
}

/// First run: copy a favorites.json from an older build into the config dir. The original is
/// left in place. Returns a message describing the migration, or an error message.
pub fn migrate_legacy(target: &Path, legacy_dirs: &[PathBuf]) -> Option<String> {
    if target.exists() {
        return None;
    }
    let source = legacy_dirs.iter().map(|d| d.join(FAVORITES_FILE)).find(|p| p.is_file())?;
    Some(match fs::copy(&source, target) {
        Ok(_) => format!("Imported favourites from {}", source.display()),
        Err(e) => format!("Could not import favourites from {}: {}", source.display(), e),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corrupt_file_is_backed_up_and_saving_still_works() {
        let dir = std::env::temp_dir().join("rs3_analyzer_fav_test");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let legacy = dir.join("legacy");
        fs::create_dir_all(&legacy).unwrap();
        fs::write(legacy.join(FAVORITES_FILE), "{not json").unwrap();

        let path = dir.join(FAVORITES_FILE);
        assert!(migrate_legacy(&path, std::slice::from_ref(&legacy)).unwrap().starts_with("Imported"));
        let mut favs = Favorites::load(path.clone());
        assert!(favs.startup_warning.as_deref().unwrap().contains("corrupt"));
        assert!(dir.join("favorites.json.bak").exists());

        favs.set("Coal".into(), true).unwrap();
        favs.set("Bond".into(), false).unwrap();
        let reloaded = Favorites::load(path);
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(reloaded.names(), vec!["Coal".to_string()]);
        assert!(reloaded.startup_warning.is_none());
    }
}
