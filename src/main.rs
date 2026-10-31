mod apply;
mod picker;
mod recolor;
mod theme;

use adw::prelude::*;
use gtk::{gdk, gio, glib};
use recolor::Rgb;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

const APP_ID: &str = "org.packarch.FolderColor";
const PREVIEW_PX: i32 = 64;

const PREVIEW_ICONS: &[(&str, &str)] = &[
    ("folder", "Dossier"),
    ("user-home", "Accueil"),
    ("folder-documents", "Documents"),
    ("folder-download", "Téléchargements"),
    ("folder-pictures", "Images"),
    ("folder-music", "Musique"),
];

const PRESETS: &[(&str, &str)] = &[
    ("Bleu", "#3584e4"),
    ("Azur", "#0096ff"),
    ("Turquoise", "#1fb5a5"),
    ("Vert", "#33d17a"),
    ("Lime", "#8fc93a"),
    ("Jaune", "#f6d32d"),
    ("Ambre", "#ffc86a"),
    ("Orange", "#ff7800"),
    ("Rouge", "#ff4747"),
    ("Rose", "#e66497"),
    ("Violet", "#9141ac"),
    ("Indigo", "#5e5cc6"),
    ("Marron", "#986a44"),
    ("Gris", "#77767b"),
    ("Ardoise", "#4b5d6e"),
];

#[derive(Default)]
struct State {
    themes: Vec<theme::IconTheme>,
    base: String,
    base_color: Option<Rgb>,
    color: Option<Rgb>,
    /// (nom de l'icône, SVG d'origine) pour l'aperçu.
    sources: Vec<(String, String)>,
}

struct Ui {
    state: RefCell<State>,
    toasts: adw::ToastOverlay,
    theme_row: adw::ComboRow,
    origin_label: gtk::Label,
    picker: Rc<picker::ColorPicker>,
    pictures: Vec<(gtk::Picture, gtk::Box)>,
    apply_button: gtk::Button,
    restore_button: gtk::Button,
}

fn render_svg(svg: &str, px: u32) -> Option<gdk::Texture> {
    use resvg::{tiny_skia, usvg};
    let tree = usvg::Tree::from_str(svg, &usvg::Options::default()).ok()?;
    let size = tree.size();
    let scale = px as f32 / size.width().max(size.height());
    let tx = (px as f32 - size.width() * scale) / 2.0;
    let ty = (px as f32 - size.height() * scale) / 2.0;
    let mut pixmap = tiny_skia::Pixmap::new(px, px)?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_row(scale, 0.0, 0.0, scale, tx, ty),
        &mut pixmap.as_mut(),
    );
    let bytes = glib::Bytes::from_owned(pixmap.take());
    Some(
        gdk::MemoryTexture::new(
            px as i32,
            px as i32,
            gdk::MemoryFormat::R8g8b8a8Premultiplied,
            &bytes,
            px as usize * 4,
        )
        .upcast(),
    )
}

fn active_icon_theme() -> String {
    gtk::Settings::default()
        .and_then(|s| s.gtk_icon_theme_name())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "hicolor".into())
}

impl Ui {
    fn toast(&self, msg: &str) {
        self.toasts.add_toast(adw::Toast::new(msg));
    }

    fn refresh_preview(&self) {
        let st = self.state.borrow();
        let scale = self.toasts.scale_factor().max(1) as u32;
        for (i, (pic, cell)) in self.pictures.iter().enumerate() {
            let Some((_, svg)) = st.sources.get(i) else {
                cell.set_visible(false);
                continue;
            };
            cell.set_visible(true);
            let svg = match (st.base_color, st.color) {
                (Some(from), Some(to)) => recolor::recolor(svg, from, to),
                _ => None,
            };
            let svg = svg.as_deref().unwrap_or(&st.sources[i].1);
            pic.set_paintable(render_svg(svg, PREVIEW_PX as u32 * scale).as_ref());
        }
    }

    fn load_base(&self, base: &str) {
        self.theme_row.set_subtitle(base);
        let base_color = apply::detect_base_color(base);
        let mut sources = Vec::new();
        for (name, _) in PREVIEW_ICONS {
            if let Some(p) = theme::find_reference_icon(base, name)
                && let Ok(svg) = std::fs::read_to_string(p) {
                    sources.push((name.to_string(), svg));
                }
        }
        {
            let mut st = self.state.borrow_mut();
            st.base = base.to_string();
            st.base_color = base_color;
            st.sources = sources;
            if st.color.is_none() {
                st.color = base_color;
            }
        }
        // Réordonne les cellules de l'aperçu selon les icônes trouvées.
        {
            let st = self.state.borrow();
            for (i, (_, cell)) in self.pictures.iter().enumerate() {
                if let Some((name, _)) = st.sources.get(i) {
                    let label = PREVIEW_ICONS
                        .iter()
                        .find(|(n, _)| n == name)
                        .map(|(_, l)| *l)
                        .unwrap_or("");
                    if let Some(l) = cell.last_child().and_downcast::<gtk::Label>() {
                        l.set_label(label);
                    }
                }
            }
        }
        match base_color {
            Some(c) => self.origin_label.set_label(&c.hex()),
            None => self.origin_label.set_label("non détectée"),
        }
        self.apply_button.set_sensitive(base_color.is_some());
        self.restore_button
            .set_sensitive(!apply::generated_for(base).is_empty());
        self.refresh_preview();
    }

    fn set_color(&self, c: Rgb) {
        if self.state.borrow().color == Some(c) {
            return;
        }
        self.state.borrow_mut().color = Some(c);
        if self.picker.color() != c {
            self.picker.set_color(c);
        }
        self.refresh_preview();
    }
}

fn apply_clicked(ui: Rc<Ui>) {
    let (base, color) = {
        let st = ui.state.borrow();
        (st.base.clone(), st.color)
    };
    let Some(color) = color else { return };
    if ui.state.borrow().base_color == Some(color) {
        // Couleur d'origine : pas besoin de thème généré.
        restore_clicked(ui);
        return;
    }
    ui.apply_button.set_sensitive(false);
    glib::spawn_future_local(async move {
        let b = base.clone();
        let res = gio::spawn_blocking(move || apply::generate(&b, color)).await;
        ui.apply_button.set_sensitive(true);
        match res {
            Ok(Ok(report)) => {
                apply::set_icon_theme(&report.theme_id);
                ui.restore_button.set_sensitive(true);
                // Un seul thème généré à la fois : les anciens sont supprimés une fois que
                // les applications sont passées au nouveau.
                let keep = report.theme_id.clone();
                glib::timeout_add_local_once(Duration::from_millis(2000), move || {
                    apply::remove_other_generated(&keep);
                });
                let mut msg = format!(
                    "Thème « {} » appliqué ({} icônes)",
                    report.theme_id, report.written
                );
                if report.skipped_png > 0 {
                    msg.push_str(&format!(", {} icônes PNG non modifiables", report.skipped_png));
                }
                ui.toast(&msg);
            }
            Ok(Err(e)) => ui.toast(&format!("Erreur : {e:#}")),
            Err(_) => ui.toast("Erreur interne pendant la génération"),
        }
    });
}

fn restore_clicked(ui: Rc<Ui>) {
    let base = ui.state.borrow().base.clone();
    apply::set_icon_theme(&base);
    ui.restore_button.set_sensitive(false);
    // On laisse aux applications le temps de quitter le thème avant de le supprimer.
    glib::timeout_add_local_once(Duration::from_millis(1500), move || {
        match apply::remove(&base) {
            Ok(()) => ui.toast(&format!("Thème « {base} » d'origine restauré")),
            Err(e) => ui.toast(&format!("Erreur : {e:#}")),
        }
    });
}

fn build_ui(app: &adw::Application) {
    // Pastilles de couleur.
    let mut css = String::from(
        ".swatch { min-width: 30px; min-height: 30px; padding: 0; border-radius: 999px; \
         border: 2px solid alpha(currentColor, 0.15); }\n",
    );
    for (i, (_, hex)) in PRESETS.iter().enumerate() {
        css.push_str(&format!(".swatch-{i} {{ background: {hex}; }}\n"));
    }
    css.push_str(".preview-card { padding: 18px 12px; }\n");
    let provider = gtk::CssProvider::new();
    provider.load_from_string(&css);
    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }

    let themes = theme::list_themes();

    // Thème de départ : le thème actif (ou sa base s'il a été généré par nous).
    let active = active_icon_theme();
    let (start_base, start_color) = match theme::load_theme(&active) {
        Some(t) if t.generated_base().is_some() => (
            t.generated_base().unwrap(),
            t.generated_color().and_then(|c| Rgb::parse(&c)),
        ),
        _ => (active, None),
    };

    // --- Aperçu
    let preview_box = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    preview_box.set_homogeneous(true);
    preview_box.add_css_class("card");
    preview_box.add_css_class("preview-card");
    let mut pictures = Vec::new();
    for _ in PREVIEW_ICONS {
        let cell = gtk::Box::new(gtk::Orientation::Vertical, 6);
        let pic = gtk::Picture::new();
        pic.set_size_request(PREVIEW_PX, PREVIEW_PX);
        pic.set_content_fit(gtk::ContentFit::Contain);
        pic.set_halign(gtk::Align::Center);
        let label = gtk::Label::new(None);
        label.add_css_class("caption");
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        cell.append(&pic);
        cell.append(&label);
        preview_box.append(&cell);
        pictures.push((pic, cell));
    }

    // --- Thème
    let names: Vec<String> = themes.iter().map(|t| t.name.clone()).collect();
    let model = gtk::StringList::new(&names.iter().map(String::as_str).collect::<Vec<_>>());
    let theme_row = adw::ComboRow::builder()
        .title("Thème d'icônes")
        .model(&model)
        .enable_search(true)
        .build();
    let origin_label = gtk::Label::new(None);
    origin_label.add_css_class("dim-label");
    origin_label.set_selectable(true);
    let origin_row = adw::ActionRow::builder()
        .title("Couleur d'origine des dossiers")
        .build();
    origin_row.add_suffix(&origin_label);
    let theme_group = adw::PreferencesGroup::builder().title("Thème").build();
    theme_group.add(&theme_row);
    theme_group.add(&origin_row);

    // --- Couleur
    let picker = picker::ColorPicker::new();

    let flow = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .max_children_per_line(PRESETS.len() as u32)
        .min_children_per_line(5)
        .column_spacing(8)
        .row_spacing(8)
        .margin_top(12)
        .margin_bottom(12)
        .margin_start(12)
        .margin_end(12)
        .homogeneous(true)
        .build();
    let mut preset_buttons = Vec::new();
    for (i, (name, hex)) in PRESETS.iter().enumerate() {
        let b = gtk::Button::builder()
            .tooltip_text(format!("{name} {hex}"))
            .halign(gtk::Align::Center)
            .valign(gtk::Align::Center)
            .build();
        b.add_css_class("swatch");
        b.add_css_class(&format!("swatch-{i}"));
        flow.insert(&b, -1);
        preset_buttons.push((b, Rgb::parse(hex).unwrap()));
    }
    let presets_row = adw::PreferencesRow::builder()
        .activatable(false)
        .child(&flow)
        .build();

    let color_group = adw::PreferencesGroup::builder().title("Couleur").build();
    color_group.add(&presets_row);
    color_group.add(&picker.row);

    // --- Boutons
    let restore_button = gtk::Button::builder()
        .label("Restaurer l'original")
        .tooltip_text("Revenir au thème d'origine et supprimer le thème personnalisé")
        .build();
    restore_button.add_css_class("pill");
    let apply_button = gtk::Button::builder().label("Appliquer").build();
    apply_button.add_css_class("pill");
    apply_button.add_css_class("suggested-action");
    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    buttons.set_halign(gtk::Align::Center);
    buttons.append(&restore_button);
    buttons.append(&apply_button);

    let content = gtk::Box::new(gtk::Orientation::Vertical, 24);
    content.set_margin_top(18);
    content.set_margin_bottom(24);
    content.set_margin_start(18);
    content.set_margin_end(18);
    content.append(&preview_box);
    content.append(&theme_group);
    content.append(&color_group);
    content.append(&buttons);

    let clamp = adw::Clamp::builder().maximum_size(640).child(&content).build();
    let scrolled = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&clamp)
        .build();
    let toasts = adw::ToastOverlay::new();
    toasts.set_child(Some(&scrolled));

    let header = adw::HeaderBar::new();
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&header);
    toolbar.set_content(Some(&toasts));

    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Couleur des dossiers")
        .default_width(620)
        .default_height(680)
        .content(&toolbar)
        .build();

    let ui = Rc::new(Ui {
        state: RefCell::new(State {
            themes,
            color: start_color,
            ..Default::default()
        }),
        toasts,
        theme_row: theme_row.clone(),
        origin_label,
        picker: picker.clone(),
        pictures,
        apply_button: apply_button.clone(),
        restore_button: restore_button.clone(),
    });

    // Sélection initiale.
    let start_index = ui
        .state
        .borrow()
        .themes
        .iter()
        .position(|t| t.id == start_base);
    if let Some(i) = start_index {
        ui.theme_row.set_selected(i as u32);
    }
    let selected_id = |ui: &Ui| {
        ui.state
            .borrow()
            .themes
            .get(ui.theme_row.selected() as usize)
            .map(|t| t.id.clone())
    };
    if let Some(id) = selected_id(&ui) {
        ui.load_base(&id);
    } else {
        ui.apply_button.set_sensitive(false);
        ui.restore_button.set_sensitive(false);
        ui.toast("Aucun thème d'icônes avec des dossiers SVG n'a été trouvé");
    }
    if let Some(c) = ui.state.borrow().color {
        ui.picker.set_color(c);
    }

    {
        let ui = ui.clone();
        theme_row.connect_selected_notify(move |_| {
            if let Some(id) = selected_id(&ui) {
                ui.load_base(&id);
            }
        });
    }
    {
        let ui = ui.clone();
        picker.connect_changed(move |c| ui.set_color(c));
    }
    for (b, c) in preset_buttons {
        let ui = ui.clone();
        b.connect_clicked(move |_| ui.set_color(c));
    }
    {
        let ui = ui.clone();
        apply_button.connect_clicked(move |_| apply_clicked(ui.clone()));
    }
    {
        let ui = ui.clone();
        restore_button.connect_clicked(move |_| restore_clicked(ui.clone()));
    }
    {
        // L'aperçu est rendu à la résolution de l'écran.
        let ui = ui.clone();
        window.connect_scale_factor_notify(move |_| ui.refresh_preview());
    }

    window.present();
}

/// Mode ligne de commande :
///   packarch-folder-color --generate <thème> <#couleur>   génère le thème sans l'activer
///   packarch-folder-color --apply <thème> <#couleur>      génère, active et supprime
///                                                         les autres thèmes générés
///   packarch-folder-color --list                          thèmes compatibles
fn cli(args: &[String]) -> Option<glib::ExitCode> {
    let fail = |msg: String| {
        eprintln!("{msg}");
        Some(glib::ExitCode::FAILURE)
    };
    match args.first().map(String::as_str) {
        Some("--list") => {
            for t in theme::list_themes() {
                let c = apply::detect_base_color(&t.id).map(|c| c.hex()).unwrap_or_default();
                println!("{}\t{}\t{}", t.id, c, t.name);
            }
            Some(glib::ExitCode::SUCCESS)
        }
        Some("--generate") => {
            let (Some(base), Some(color)) = (args.get(1), args.get(2).and_then(|c| Rgb::parse(c)))
            else {
                return fail("usage : --generate <thème> <#rrggbb>".into());
            };
            match apply::generate(base, color) {
                Ok(r) => {
                    println!(
                        "{} : {} icônes écrites, {} PNG ignorées",
                        r.theme_id, r.written, r.skipped_png
                    );
                    Some(glib::ExitCode::SUCCESS)
                }
                Err(e) => fail(format!("erreur : {e:#}")),
            }
        }
        Some(cmd @ ("--prepare" | "--apply")) => {
            let (Some(base), Some(color)) = (args.get(1), args.get(2).and_then(|c| Rgb::parse(c)))
            else {
                return fail(format!("usage : {cmd} <thème> <#rrggbb>"));
            };
            match apply::prepare(base, color) {
                Ok(id) => {
                    if cmd == "--apply" {
                        apply::set_icon_theme(&id);
                    }
                    // Sur la sortie standard, seulement le nom du thème : lu par les scripts.
                    println!("{id}");
                    Some(glib::ExitCode::SUCCESS)
                }
                Err(e) => fail(format!("erreur : {e:#}")),
            }
        }
        Some("--clean") => {
            let n = apply::remove_other_generated("");
            eprintln!("{n} thème(s) généré(s) supprimé(s)");
            Some(glib::ExitCode::SUCCESS)
        }
        Some("--help" | "-h") => {
            println!(
                "Usage : packarch-folder-color [COMMANDE]\n\n\
                 Sans commande : interface graphique.\n\n\
                 --list                       thèmes compatibles et couleur de leurs dossiers\n\
                 --prepare <thème> <#couleur> prépare le thème et affiche le nom à utiliser\n\
                 \x20                            (le thème lui-même si c'est sa couleur)\n\
                 --apply <thème> <#couleur>   comme --prepare, puis active le thème\n\
                 --generate <thème> <#couleur> génère <thème>-<rrggbb> sans rien activer\n\
                 --clean                      supprime tous les thèmes générés"
            );
            Some(glib::ExitCode::SUCCESS)
        }
        _ => None,
    }
}

fn main() -> glib::ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(code) = cli(&args) {
        return code;
    }
    let app = adw::Application::builder().application_id(APP_ID).build();
    app.connect_activate(build_ui);
    app.run()
}
