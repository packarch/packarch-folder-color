//! Sélecteur de couleur intégré à la fenêtre : teinte, saturation, luminosité et code
//! hexadécimal. Remplace le dialogue de couleur de GTK, qui s'ouvre dans une fenêtre à
//! part trop petite (contenu coupé) sous certains gestionnaires de fenêtres (Openbox).

use crate::recolor::{Hsl, Rgb};
use adw::prelude::*;
use gtk::gdk;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

type Callback = Box<dyn Fn(Rgb)>;

pub struct ColorPicker {
    pub row: adw::ExpanderRow,
    /// Lignes affichées quand `row` est dépliée.
    rows: Vec<gtk::Widget>,
    swatch: gtk::DrawingArea,
    hue: gtk::Scale,
    sat: gtk::Scale,
    lig: gtk::Scale,
    entry: adw::EntryRow,
    css: gtk::CssProvider,
    color: Cell<Rgb>,
    /// Vrai pendant une mise à jour par le programme (pas de rappel).
    updating: Cell<bool>,
    on_change: RefCell<Option<Callback>>,
}

const STATIC_CSS: &str = "
scale.pfc trough { min-height: 12px; border-radius: 6px; }
scale.pfc trough highlight { background: none; box-shadow: none; }
scale.pfc-hue trough {
  background-image: linear-gradient(to right, #ff0000, #ffff00, #00ff00, #00ffff,
    #0000ff, #ff00ff, #ff0000);
}
";

fn scale(max: f64) -> gtk::Scale {
    let s = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, max, 1.0);
    s.set_draw_value(false);
    // Largeur fixe : les trois curseurs sont alignés quelle que soit la longueur du titre.
    s.set_valign(gtk::Align::Center);
    s.set_width_request(320);
    s.add_css_class("pfc");
    s
}

fn slider_row(title: &str, s: &gtk::Scale) -> adw::ActionRow {
    let row = adw::ActionRow::builder().title(title).build();
    row.add_suffix(s);
    row
}

impl ColorPicker {
    pub fn new() -> Rc<Self> {
        let static_css = gtk::CssProvider::new();
        static_css.load_from_string(STATIC_CSS);
        let css = gtk::CssProvider::new();
        if let Some(display) = gdk::Display::default() {
            for p in [&static_css, &css] {
                gtk::style_context_add_provider_for_display(
                    &display,
                    p,
                    gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
                );
            }
        }

        let swatch = gtk::DrawingArea::builder()
            .content_width(36)
            .content_height(24)
            .valign(gtk::Align::Center)
            .build();
        let hue = scale(359.0);
        hue.add_css_class("pfc-hue");
        let sat = scale(100.0);
        sat.add_css_class("pfc-sat");
        let lig = scale(100.0);
        lig.add_css_class("pfc-lig");
        let entry = adw::EntryRow::builder().title("Code hexadécimal").build();

        let row = adw::ExpanderRow::builder()
            .title("Couleur personnalisée")
            .build();
        row.add_suffix(&swatch);
        let rows: Vec<gtk::Widget> = vec![
            slider_row("Teinte", &hue).upcast(),
            slider_row("Saturation", &sat).upcast(),
            slider_row("Luminosité", &lig).upcast(),
            entry.clone().upcast(),
        ];
        for r in &rows {
            row.add_row(r);
        }

        let picker = Rc::new(Self {
            row,
            rows,
            swatch,
            hue,
            sat,
            lig,
            entry,
            css,
            color: Cell::new(Rgb(0, 150, 255)),
            updating: Cell::new(false),
            on_change: RefCell::new(None),
        });

        {
            let weak = Rc::downgrade(&picker);
            picker.swatch.set_draw_func(move |_, cr, w, h| {
                let Some(p) = weak.upgrade() else { return };
                let Rgb(r, g, b) = p.color.get();
                let (w, h, rad) = (w as f64, h as f64, 6.0);
                cr.new_sub_path();
                cr.arc(w - rad, rad, rad, -90f64.to_radians(), 0.0);
                cr.arc(w - rad, h - rad, rad, 0.0, 90f64.to_radians());
                cr.arc(rad, h - rad, rad, 90f64.to_radians(), 180f64.to_radians());
                cr.arc(rad, rad, rad, 180f64.to_radians(), 270f64.to_radians());
                cr.close_path();
                cr.set_source_rgb(r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0);
                let _ = cr.fill_preserve();
                cr.set_source_rgba(1.0, 1.0, 1.0, 0.25);
                cr.set_line_width(1.0);
                let _ = cr.stroke();
            });
        }
        for s in [&picker.hue, &picker.sat, &picker.lig] {
            let weak = Rc::downgrade(&picker);
            s.connect_value_changed(move |_| {
                if let Some(p) = weak.upgrade() {
                    p.sliders_changed();
                }
            });
        }
        {
            let weak = Rc::downgrade(&picker);
            picker.entry.connect_changed(move |e| {
                let Some(p) = weak.upgrade() else { return };
                if p.updating.get() {
                    return;
                }
                let text = e.text();
                let t = text.trim();
                // Seulement un code complet : pas de saut de couleur pendant la saisie.
                if t.trim_start_matches('#').len() == 6
                    && let Some(c) = Rgb::parse(t)
                {
                    p.set_color(c);
                    p.emit(c);
                }
            });
        }
        picker.set_color(picker.color.get());
        picker
    }

    /// Hauteur qu'ajoutent les lignes dépliées, pour une largeur de `width` pixels.
    pub fn expanded_height(&self, width: i32) -> i32 {
        self.rows
            .iter()
            // + 1 : séparateur entre les lignes de la liste
            .map(|r| r.measure(gtk::Orientation::Vertical, width).1 + 1)
            .sum()
    }

    pub fn color(&self) -> Rgb {
        self.color.get()
    }

    pub fn connect_changed(&self, f: impl Fn(Rgb) + 'static) {
        *self.on_change.borrow_mut() = Some(Box::new(f));
    }

    fn emit(&self, c: Rgb) {
        if let Some(f) = self.on_change.borrow().as_ref() {
            f(c);
        }
    }

    /// Change la couleur affichée, sans prévenir (`connect_changed`).
    pub fn set_color(&self, c: Rgb) {
        self.color.set(c);
        let hsl = c.to_hsl();
        self.updating.set(true);
        // Gris : la teinte n'a pas de sens, on garde celle du curseur.
        if hsl.s > 0.0 {
            self.hue.set_value(hsl.h.round());
        }
        self.sat.set_value((hsl.s * 100.0).round());
        self.lig.set_value((hsl.l * 100.0).round());
        if Rgb::parse(&self.entry.text()) != Some(c) {
            self.entry.set_text(&c.hex());
        }
        self.updating.set(false);
        self.refresh();
    }

    fn sliders_changed(&self) {
        if self.updating.get() {
            return;
        }
        let c = Hsl {
            h: self.hue.value(),
            s: self.sat.value() / 100.0,
            l: self.lig.value() / 100.0,
        }
        .to_rgb();
        self.color.set(c);
        self.updating.set(true);
        self.entry.set_text(&c.hex());
        self.updating.set(false);
        self.refresh();
        self.emit(c);
    }

    /// Aperçu et dégradés des curseurs de saturation et de luminosité.
    fn refresh(&self) {
        self.swatch.queue_draw();
        let (h, s, l) = (
            self.hue.value(),
            self.sat.value() / 100.0,
            self.lig.value() / 100.0,
        );
        let hex = |s: f64, l: f64| Hsl { h, s, l }.to_rgb().hex();
        let css = format!(
            "scale.pfc-sat trough {{ background-image: linear-gradient(to right, {}, {}); }}\n\
             scale.pfc-lig trough {{ background-image: linear-gradient(to right, {}, {}, {}); }}\n",
            hex(0.0, l),
            hex(1.0, l),
            hex(s, 0.0),
            hex(s, 0.5),
            hex(s, 1.0),
        );
        self.css.load_from_string(&css);
        self.row.set_subtitle(&self.color.get().hex());
    }
}
