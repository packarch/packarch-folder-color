# packarch-folder-color

Petit logiciel GTK4/libadwaita en Rust pour changer la couleur des dossiers de n'importe
quel thème d'icônes SVG installé (Packarch, Papirus, Breeze, Qogir, Luv-Folders…).
Les thèmes Openbox de Packarch (`packarch-skin`) et le sélecteur de thèmes XFCE
s'en servent pour donner au thème d'icônes Packarch la couleur de chaque thème.

## Fonctionnement

- Le thème d'origine (`/usr/share/icons/...`) n'est jamais modifié.
- Un thème dérivé `~/.local/share/icons/<Thème>-<rrggbb>` est créé : il hérite du thème
  d'origine et ne contient que les icônes de dossier recolorées (les noms qui partagent
  un même fichier deviennent des liens). La couleur fait partie du nom : changer de
  couleur change de thème, et les applications rechargent les icônes.
- Il n'existe jamais qu'un thème généré à la fois : les anciens sont supprimés.
- Si la couleur demandée est celle du thème d'origine, aucun thème n'est généré.
- La couleur est transposée en gardant les écarts de teinte et de luminosité du thème :
  les thèmes à plusieurs nuances gardent leur relief.
- Le thème actif est changé via xfconf (xfsettingsd), gsettings, `gtk-3.0/settings.ini`,
  `gtk-4.0/settings.ini`, `~/.gtkrc-2.0`, qt5ct/qt6ct et kdeglobals (seulement les fichiers
  qui existent déjà).

## Ligne de commande

```
packarch-folder-color                                # interface graphique
packarch-folder-color --list                         # thèmes compatibles et couleur actuelle
packarch-folder-color --prepare <thème> '#rrggbb'    # prépare et affiche le nom du thème à utiliser
packarch-folder-color --apply <thème> '#rrggbb'      # comme --prepare, puis active le thème
packarch-folder-color --generate <thème> '#rrggbb'   # génère sans activer ni nettoyer
packarch-folder-color --clean                        # supprime tous les thèmes générés
```

`--prepare` n'écrit sur la sortie standard que le nom du thème (le thème lui-même pour
sa couleur d'origine, sinon `<thème>-<rrggbb>`) : c'est la commande des scripts.

## Compilation

```
cargo build --release
install -Dm755 target/release/packarch-folder-color /usr/bin/packarch-folder-color
install -Dm644 data/org.packarch.FolderColor.desktop /usr/share/applications/org.packarch.FolderColor.desktop
```

Dépendances : gtk4 ≥ 4.12, libadwaita ≥ 1.5.

## Licence

GPL-3.0-or-later.
