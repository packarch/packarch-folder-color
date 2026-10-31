//! Découverte des thèmes d'icônes installés et des icônes de dossier qu'ils contiennent.

use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};

/// Clés écrites dans l'index.theme des thèmes générés par ce logiciel.
pub const KEY_BASE: &str = "X-Packarch-FolderColor-Base";
pub const KEY_COLOR: &str = "X-Packarch-FolderColor-Color";

/// Noms de couleur utilisés par les variantes de dossiers (Papirus, etc.) :
/// `folder-red.svg`, `folder-blue-documents.svg`… On ne les recolore pas,
/// ce sont des alternatives et non les icônes réellement utilisées.
const COLOR_VARIANTS: &[&str] = &[
    "adwaita", "black", "blue", "bluegrey", "breeze", "brown", "carmine", "cyan", "darkcyan",
    "deeporange", "green", "grey", "indigo", "magenta", "nordic", "orange", "palebrown",
    "paleorange", "pink", "purple", "red", "teal", "violet", "white", "yaru", "yellow",
];

/// Noms d'icônes de lieux qui sont dessinés comme des dossiers.
const EXTRA_FOLDER_NAMES: &[&str] = &[
    "user-home",
    "user-desktop",
    "user-bookmarks",
    "inode-directory",
    "folder",
];

/// Un fichier index.theme découpé en sections, en gardant le texte brut
/// de chaque section pour pouvoir le recopier tel quel.
#[derive(Debug, Clone, Default)]
pub struct IndexFile {
    pub sections: Vec<(String, Vec<String>)>,
}

impl IndexFile {
    pub fn parse(text: &str) -> Self {
        let mut sections: Vec<(String, Vec<String>)> = Vec::new();
        for line in text.lines() {
            let t = line.trim();
            if t.starts_with('[') && t.ends_with(']') {
                sections.push((t[1..t.len() - 1].to_string(), Vec::new()));
            } else if let Some(last) = sections.last_mut()
                && !t.is_empty() && !t.starts_with('#') {
                    last.1.push(t.to_string());
                }
        }
        Self { sections }
    }

    pub fn section(&self, name: &str) -> Option<&[String]> {
        self.sections
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, l)| l.as_slice())
    }

    pub fn get(&self, section: &str, key: &str) -> Option<String> {
        self.section(section)?.iter().find_map(|l| {
            let (k, v) = l.split_once('=')?;
            (k.trim() == key).then(|| v.trim().to_string())
        })
    }

    fn list(&self, section: &str, key: &str) -> Vec<String> {
        self.get(section, key)
            .map(|v| {
                v.split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone)]
pub struct IconTheme {
    /// Nom du dossier du thème (ce qu'on passe à gtk-icon-theme-name).
    pub id: String,
    /// Nom lisible (clé Name=).
    pub name: String,
    pub path: PathBuf,
    pub index: IndexFile,
}

impl IconTheme {
    pub fn inherits(&self) -> Vec<String> {
        self.index.list("Icon Theme", "Inherits")
    }

    pub fn directories(&self) -> Vec<String> {
        let mut d = self.index.list("Icon Theme", "Directories");
        for s in self.index.list("Icon Theme", "ScaledDirectories") {
            if !d.contains(&s) {
                d.push(s);
            }
        }
        d
    }

    /// Thème de base si ce thème a été généré par ce logiciel.
    pub fn generated_base(&self) -> Option<String> {
        self.index.get("Icon Theme", KEY_BASE)
    }

    pub fn generated_color(&self) -> Option<String> {
        self.index.get("Icon Theme", KEY_COLOR)
    }
}

/// Dossiers où chercher les thèmes, par ordre de priorité.
pub fn search_paths() -> Vec<PathBuf> {
    let mut v = Vec::new();
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if let Some(h) = &home {
        v.push(h.join(".icons"));
    }
    v.push(user_icons_dir());
    let data_dirs = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    for d in data_dirs.split(':').filter(|s| !s.is_empty()) {
        let p = Path::new(d).join("icons");
        if !v.contains(&p) {
            v.push(p);
        }
    }
    v
}

/// `~/.local/share/icons` (ou `$XDG_DATA_HOME/icons`) : là où on écrit les thèmes générés.
pub fn user_icons_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("XDG_DATA_HOME").filter(|s| !s.is_empty()) {
        return PathBuf::from(d).join("icons");
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    home.join(".local/share/icons")
}

pub fn load_theme(id: &str) -> Option<IconTheme> {
    search_paths().into_iter().find_map(|base| {
        let path = base.join(id);
        let text = fs::read_to_string(path.join("index.theme")).ok()?;
        let index = IndexFile::parse(&text);
        index.section("Icon Theme")?;
        let name = index
            .get("Icon Theme", "Name")
            .unwrap_or_else(|| id.to_string());
        Some(IconTheme {
            id: id.to_string(),
            name,
            path,
            index,
        })
    })
}

/// Tous les thèmes d'icônes installés qui ont des icônes de dossier en SVG.
/// Les thèmes générés par ce logiciel sont exclus (on part toujours de l'original).
pub fn list_themes() -> Vec<IconTheme> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for base in search_paths() {
        let Ok(rd) = fs::read_dir(&base) else { continue };
        for e in rd.flatten() {
            let id = e.file_name().to_string_lossy().into_owned();
            if !seen.insert(id.clone()) {
                continue;
            }
            let Some(t) = load_theme(&id) else { continue };
            if t.generated_base().is_some()
                || t.index.get("Icon Theme", "Hidden").as_deref() == Some("true")
                || t.directories().is_empty()
            {
                continue;
            }
            // Il faut des dossiers SVG avec une couleur identifiable.
            let colored = find_reference_icon(&t.id, "folder")
                .and_then(|p| fs::read_to_string(p).ok())
                .and_then(|svg| crate::recolor::detect_color(&svg))
                .is_some();
            if colored {
                out.push(t);
            }
        }
    }
    out.sort_by_key(|t| t.name.to_lowercase());
    out
}

/// Chaîne d'héritage d'un thème (lui-même en premier), sans doublons.
pub fn inheritance_chain(id: &str) -> Vec<IconTheme> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut queue = VecDeque::from([id.to_string()]);
    while let Some(cur) = queue.pop_front() {
        if !seen.insert(cur.clone()) {
            continue;
        }
        if let Some(t) = load_theme(&cur) {
            queue.extend(t.inherits());
            out.push(t);
        }
    }
    out
}

/// Vrai si un fichier d'icône (sans extension) doit être recoloré.
pub fn is_folder_icon(stem: &str) -> bool {
    if stem.ends_with("-symbolic") {
        return false;
    }
    if EXTRA_FOLDER_NAMES.contains(&stem) {
        return true;
    }
    let Some(rest) = stem.strip_prefix("folder-") else {
        return false;
    };
    // folder-red, folder-blue-documents… : variantes de couleur, ignorées.
    let first = rest.split('-').next().unwrap_or("");
    !COLOR_VARIANTS.contains(&first)
}

fn context_ok(theme: &IconTheme, dir: &str) -> bool {
    let ctx = theme.index.get(dir, "Context").unwrap_or_default().to_lowercase();
    let d = dir.to_lowercase();
    ctx == "places" || ctx == "mimetypes" || d.contains("places") || d.contains("mimetypes")
}

#[derive(Debug, Clone)]
pub struct FolderIcon {
    /// Sous-dossier dans le thème, ex. `scalable/places`.
    pub dir: String,
    /// Nom de fichier, ex. `folder-documents.svg`.
    pub file: String,
    /// Fichier source (liens symboliques résolus à la lecture).
    pub src: PathBuf,
}

pub struct FolderSet {
    pub icons: Vec<FolderIcon>,
    /// Section brute de index.theme pour chaque sous-dossier utilisé.
    pub dir_sections: HashMap<String, Vec<String>>,
    /// Nombre d'icônes de dossier en PNG ignorées.
    pub skipped_png: usize,
}

/// Collecte les icônes de dossier d'un thème en suivant l'héritage, comme GTK :
/// pour chaque nom d'icône, c'est le premier thème de la chaîne qui la fournit qui gagne.
pub fn collect_folder_icons(id: &str) -> FolderSet {
    let mut taken: HashSet<String> = HashSet::new();
    let mut icons = Vec::new();
    let mut dir_sections: HashMap<String, Vec<String>> = HashMap::new();
    let mut skipped_png = 0;

    for theme in inheritance_chain(id) {
        let mut found: HashMap<String, Vec<FolderIcon>> = HashMap::new();
        for dir in theme.directories() {
            if !context_ok(&theme, &dir) {
                continue;
            }
            let Ok(rd) = fs::read_dir(theme.path.join(&dir)) else { continue };
            for e in rd.flatten() {
                let file = e.file_name().to_string_lossy().into_owned();
                let (stem, ext) = match file.rsplit_once('.') {
                    Some((s, x)) => (s.to_string(), x.to_lowercase()),
                    None => continue,
                };
                if !is_folder_icon(&stem) {
                    continue;
                }
                match ext.as_str() {
                    "svg" => {}
                    "png" | "xpm" => {
                        if !taken.contains(&stem) {
                            skipped_png += 1;
                        }
                        continue;
                    }
                    _ => continue,
                }
                // Lien cassé : on ignore.
                if fs::metadata(e.path()).is_err() {
                    continue;
                }
                found.entry(stem).or_default().push(FolderIcon {
                    dir: dir.clone(),
                    file,
                    src: e.path(),
                });
            }
        }
        let mut names: Vec<_> = found.keys().cloned().collect();
        names.sort();
        for name in names {
            if !taken.insert(name.clone()) {
                continue;
            }
            for icon in found.remove(&name).unwrap() {
                if !dir_sections.contains_key(&icon.dir) {
                    let sec = theme.index.section(&icon.dir).unwrap_or(&[]).to_vec();
                    dir_sections.insert(icon.dir.clone(), sec);
                }
                icons.push(icon);
            }
        }
    }
    FolderSet {
        icons,
        dir_sections,
        skipped_png,
    }
}

/// Meilleur fichier SVG pour un nom d'icône (le plus grand, scalable de préférence),
/// en suivant la chaîne d'héritage. Sert à l'aperçu et à la détection de la couleur.
pub fn find_reference_icon(id: &str, stem: &str) -> Option<PathBuf> {
    for theme in inheritance_chain(id) {
        let mut best: Option<(i64, PathBuf)> = None;
        for dir in theme.directories() {
            let p = theme.path.join(&dir).join(format!("{stem}.svg"));
            if fs::metadata(&p).is_err() {
                continue;
            }
            let get = |k: &str| theme.index.get(&dir, k).and_then(|v| v.parse::<i64>().ok());
            let size = get("Size").unwrap_or(0);
            let scale = get("Scale").unwrap_or(1);
            let scalable = theme
                .index
                .get(&dir, "Type")
                .is_some_and(|t| t.eq_ignore_ascii_case("scalable"));
            // Échelle 1 d'abord, puis les SVG « scalable », puis les grandes tailles.
            let score = if scale == 1 { 100_000 } else { 0 }
                + if scalable { 10_000 } else { 0 }
                + size;
            if best.as_ref().is_none_or(|(s, _)| score > *s) {
                best = Some((score, p));
            }
        }
        if let Some((_, p)) = best {
            return Some(p);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_names() {
        assert!(is_folder_icon("folder"));
        assert!(is_folder_icon("folder-documents"));
        assert!(is_folder_icon("user-home"));
        assert!(!is_folder_icon("folder-red"));
        assert!(!is_folder_icon("folder-blue-documents"));
        assert!(!is_folder_icon("folder-symbolic"));
        assert!(!is_folder_icon("user-trash"));
    }
}
