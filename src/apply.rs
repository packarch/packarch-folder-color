//! Génération du thème dérivé et changement du thème d'icônes actif.

use crate::recolor::{self, Rgb};
use crate::theme::{self, KEY_BASE, KEY_COLOR};
use anyhow::{Context, Result, bail};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub struct Report {
    pub theme_id: String,
    pub written: usize,
    pub skipped_png: usize,
}

/// Nom du thème dérivé : la couleur en fait partie, pour qu'un changement de couleur
/// change aussi le nom et que les applications rechargent les icônes.
pub fn custom_theme_id(base: &str, color: Rgb) -> String {
    format!("{base}-{}", &color.hex()[1..])
}

/// Thèmes générés à partir de `base` présents dans le dossier de l'utilisateur.
pub fn generated_for(base: &str) -> Vec<PathBuf> {
    let Ok(rd) = fs::read_dir(theme::user_icons_dir()) else { return Vec::new() };
    rd.flatten()
        .map(|e| e.path())
        .filter(|p| {
            let index = fs::read_to_string(p.join("index.theme")).unwrap_or_default();
            index.lines().any(|l| l.trim() == format!("{KEY_BASE}={base}"))
        })
        .collect()
}

/// Couleur actuelle des dossiers du thème de base.
pub fn detect_base_color(base: &str) -> Option<Rgb> {
    let p = theme::find_reference_icon(base, "folder")?;
    recolor::detect_color(&fs::read_to_string(p).ok()?)
}

/// Supprime un thème généré, en vérifiant d'abord qu'il vient bien de ce logiciel.
fn remove_generated(dir: &Path) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    let index = fs::read_to_string(dir.join("index.theme")).unwrap_or_default();
    if !index.contains(KEY_BASE) {
        bail!(
            "{} existe déjà et n'a pas été créé par ce logiciel, je n'y touche pas",
            dir.display()
        );
    }
    fs::remove_dir_all(dir).with_context(|| format!("suppression de {}", dir.display()))
}

/// Crée (ou recrée) `~/.local/share/icons/<base>-<rrggbb>` avec les dossiers recolorés.
pub fn generate(base: &str, color: Rgb) -> Result<Report> {
    let base_theme = theme::load_theme(base).context("thème introuvable")?;
    let from = detect_base_color(base)
        .context("impossible de trouver la couleur des dossiers de ce thème")?;
    let set = theme::collect_folder_icons(base);
    if set.icons.is_empty() {
        bail!("ce thème n'a pas d'icônes de dossier au format SVG");
    }

    let id = custom_theme_id(base, color);
    let final_dir = theme::user_icons_dir().join(&id);
    let tmp_dir = theme::user_icons_dir().join(format!(".{id}.tmp"));
    if tmp_dir.exists() {
        fs::remove_dir_all(&tmp_dir)?;
    }

    // Couleur d'origine par sous-dossier : certains thèmes n'ont pas la même couleur
    // selon la taille ou l'échelle (liens vers une autre variante, par exemple).
    let mut dir_colors: std::collections::HashMap<&str, Rgb> = std::collections::HashMap::new();
    for icon in &set.icons {
        if icon.file == "folder.svg"
            && let Some(c) = fs::read_to_string(&icon.src)
                .ok()
                .and_then(|s| recolor::detect_color(&s))
            {
                dir_colors.insert(icon.dir.as_str(), c);
            }
    }

    // Beaucoup de noms d'icônes sont des liens vers le même fichier : chaque fichier
    // source n'est recoloré et écrit qu'une fois, les autres noms deviennent des liens.
    let mut done: std::collections::HashMap<(PathBuf, String), PathBuf> =
        std::collections::HashMap::new();
    let mut written = 0;
    let mut used_dirs: Vec<String> = Vec::new();
    for icon in &set.icons {
        let Ok(real) = fs::canonicalize(&icon.src) else { continue };
        let from = dir_colors.get(icon.dir.as_str()).copied().unwrap_or(from);
        let dir = tmp_dir.join(&icon.dir);
        fs::create_dir_all(&dir)?;
        let dest = dir.join(&icon.file);
        // Chemin relatif de l'icône vers la racine du thème, pour des liens internes.
        let up = "../".repeat(Path::new(&icon.dir).components().count());
        let key = (real.clone(), from.hex());
        if let Some(first) = done.get(&key) {
            std::os::unix::fs::symlink(format!("{up}{}", first.display()), &dest)?;
        } else {
            let Ok(svg) = fs::read_to_string(&real) else { continue };
            match recolor::recolor(&svg, from, color) {
                Some(out) => {
                    fs::write(&dest, out)?;
                    done.insert(key, Path::new(&icon.dir).join(&icon.file));
                }
                // Rien à recolorer : un lien vers l'original suffit (il faut quand même
                // l'icône ici, sinon le thème parent prendrait le relais avec une autre
                // taille).
                None => std::os::unix::fs::symlink(&real, &dest)?,
            }
        }
        written += 1;
        if !used_dirs.contains(&icon.dir) {
            used_dirs.push(icon.dir.clone());
        }
    }
    used_dirs.sort();

    let mut index = String::new();
    index.push_str("[Icon Theme]\n");
    index.push_str(&format!("Name={} (dossiers {})\n", base_theme.name, color.hex()));
    index.push_str(&format!(
        "Comment={} avec des dossiers recolorés par packarch-folder-color\n",
        base_theme.name
    ));
    index.push_str(&format!("Inherits={base}\n"));
    index.push_str("Example=folder\n");
    index.push_str(&format!("{KEY_BASE}={base}\n"));
    index.push_str(&format!("{KEY_COLOR}={}\n", color.hex()));
    index.push_str(&format!("Directories={}\n", used_dirs.join(",")));
    for d in &used_dirs {
        index.push_str(&format!("\n[{d}]\n"));
        for line in set.dir_sections.get(d).map(Vec::as_slice).unwrap_or(&[]) {
            index.push_str(line);
            index.push('\n');
        }
    }
    fs::write(tmp_dir.join("index.theme"), index)?;

    remove_generated(&final_dir)?;
    fs::rename(&tmp_dir, &final_dir)
        .with_context(|| format!("installation dans {}", final_dir.display()))?;

    // Le cache accélère le chargement ; son absence n'est pas bloquante.
    let _ = Command::new("gtk-update-icon-cache")
        .args(["-f", "-t", "-q"])
        .arg(&final_dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();

    Ok(Report {
        theme_id: id,
        written,
        skipped_png: set.skipped_png,
    })
}

/// Supprime tous les thèmes générés par ce logiciel sauf `keep` (et leurs dossiers
/// temporaires restés après une interruption). Renvoie le nombre de thèmes supprimés.
pub fn remove_other_generated(keep: &str) -> usize {
    let dir = theme::user_icons_dir();
    let Ok(rd) = fs::read_dir(&dir) else { return 0 };
    let mut n = 0;
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name == keep {
            continue;
        }
        let path = e.path();
        if name.starts_with('.') && name.ends_with(".tmp") {
            let _ = fs::remove_dir_all(&path);
            continue;
        }
        let index = fs::read_to_string(path.join("index.theme")).unwrap_or_default();
        if index.contains(KEY_BASE) && fs::remove_dir_all(&path).is_ok() {
            n += 1;
        }
    }
    n
}

/// Prépare le thème d'icônes à utiliser pour `base` avec des dossiers `color`, et
/// renvoie son nom : `base` lui-même si c'est déjà sa couleur, sinon le thème généré.
/// Dans les deux cas, il ne reste ensuite que ce thème généré (ou aucun).
pub fn prepare(base: &str, color: Rgb) -> Result<String> {
    if detect_base_color(base) == Some(color) {
        remove_other_generated("");
        return Ok(base.to_string());
    }
    // Déjà généré dans cette couleur : on le garde tel quel.
    let id = custom_theme_id(base, color);
    let dir = theme::user_icons_dir().join(&id);
    let index = fs::read_to_string(dir.join("index.theme")).unwrap_or_default();
    let has = |k: &str, v: &str| index.lines().any(|l| l.trim() == format!("{k}={v}"));
    if !(has(KEY_BASE, base) && has(KEY_COLOR, &color.hex())) {
        generate(base, color)?;
    }
    remove_other_generated(&id);
    Ok(id)
}

/// Supprime les thèmes dérivés d'un thème de base.
pub fn remove(base: &str) -> Result<()> {
    for dir in generated_for(base) {
        remove_generated(&dir)?;
    }
    Ok(())
}

fn run_quiet(cmd: &str, args: &[&str]) -> bool {
    Command::new(cmd)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Remplace (ou ajoute dans `section`) une ligne `key=value` d'un fichier de configuration
/// existant. Ne crée pas le fichier.
fn set_ini_key(path: &Path, section: Option<&str>, key: &str, value: &str) {
    let Ok(text) = fs::read_to_string(path) else { return };
    let mut out = Vec::new();
    let mut done = false;
    let mut in_section = section.is_none();
    let mut section_seen = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            if in_section && !done && section.is_some() {
                out.push(format!("{key}={value}"));
                done = true;
            }
            in_section = section.is_some_and(|s| t == format!("[{s}]"));
            section_seen |= in_section;
        }
        if in_section
            && !done
            && t.split_once('=').is_some_and(|(k, _)| k.trim() == key)
        {
            out.push(format!("{key}={value}"));
            done = true;
            continue;
        }
        out.push(line.to_string());
    }
    if !done {
        // Section absente : on ne touche pas à un fichier qui ne la gère pas.
        if section.is_some() && !section_seen {
            return;
        }
        out.push(format!("{key}={value}"));
    }
    let mut new = out.join("\n");
    new.push('\n');
    if new != text {
        let _ = fs::write(path, new);
    }
}

/// Change le thème d'icônes partout où il est configuré.
pub fn set_icon_theme(id: &str) {
    // Application immédiate (xfsettingsd, puis GNOME).
    run_quiet("xfconf-query", &["-c", "xsettings", "-p", "/Net/IconThemeName", "-s", id]);
    run_quiet("gsettings", &["set", "org.gnome.desktop.interface", "icon-theme", id]);

    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else { return };
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"));

    for gtk in ["gtk-3.0", "gtk-4.0"] {
        set_ini_key(
            &config.join(gtk).join("settings.ini"),
            Some("Settings"),
            "gtk-icon-theme-name",
            id,
        );
    }
    set_ini_key(
        &home.join(".gtkrc-2.0"),
        None,
        "gtk-icon-theme-name",
        &format!("\"{id}\""),
    );
    for qt in ["qt5ct/qt5ct.conf", "qt6ct/qt6ct.conf"] {
        set_ini_key(&config.join(qt), Some("Appearance"), "icon_theme", id);
    }
    set_ini_key(&config.join("kdeglobals"), Some("Icons"), "Theme", id);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_only_one_generated_theme() {
        let root = std::env::temp_dir().join(format!("pfc-clean-{}", std::process::id()));
        let icons = root.join("icons");
        let mk = |name: &str, index: &str| {
            fs::create_dir_all(icons.join(name)).unwrap();
            fs::write(icons.join(name).join("index.theme"), index).unwrap();
        };
        mk("A-Custom", &format!("[Icon Theme]\n{KEY_BASE}=A\n"));
        mk("B-Custom", &format!("[Icon Theme]\n{KEY_BASE}=B\n"));
        mk("C-Custom", "[Icon Theme]\nName=Pas à nous\n");
        mk("Autre", "[Icon Theme]\n");
        fs::create_dir_all(icons.join(".B-Custom.tmp")).unwrap();
        // SAFETY: test isolé, aucune autre lecture de cette variable en parallèle.
        unsafe { std::env::set_var("XDG_DATA_HOME", &root) };
        assert_eq!(remove_other_generated("A-Custom"), 1);
        let mut left: Vec<_> = fs::read_dir(&icons)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        left.sort();
        assert_eq!(left, ["A-Custom", "Autre", "C-Custom"]);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn ini_replace() {
        let dir = std::env::temp_dir().join(format!("pfc-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join("settings.ini");
        fs::write(&p, "[Settings]\ngtk-icon-theme-name=Old\ngtk-font-name=Sans 10\n").unwrap();
        set_ini_key(&p, Some("Settings"), "gtk-icon-theme-name", "New");
        assert_eq!(
            fs::read_to_string(&p).unwrap(),
            "[Settings]\ngtk-icon-theme-name=New\ngtk-font-name=Sans 10\n"
        );
        fs::write(&p, "[Other]\na=1\n[Settings]\nb=2\n[Z]\n").unwrap();
        set_ini_key(&p, Some("Settings"), "gtk-icon-theme-name", "New");
        assert_eq!(
            fs::read_to_string(&p).unwrap(),
            "[Other]\na=1\n[Settings]\nb=2\ngtk-icon-theme-name=New\n[Z]\n"
        );
        fs::remove_dir_all(dir).unwrap();
    }
}
