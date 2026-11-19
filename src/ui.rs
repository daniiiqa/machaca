//! El panel en celdas (como rola o vato), que disco pinta en pixeles: arriba
//! el wallpaper y el look de ahora, abajo dos columnas (Apariencia; Sistema y
//! Tus compus) y la fila de herramientas. También la página de looks.
//! Junto con las celdas sale dónde cae cada cosa, para el mouse.

use crate::estado::{Estado, LookInfo, Renglon, Seccion, Tipo, columnas, herramientas};
use disco::look::Look;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::widgets::Widget;

/// Ancho del panel, en celdas.
pub const W: u16 = 92;
/// Alto de la cabecera (wallpaper y look), sin el borde.
const HEAD: u16 = 6;
/// Ancho de la miniatura del wallpaper.
pub const THUMB_W: u16 = 26;
const COL_W: u16 = (W - 6) / 2;
const COL_X: [u16; 2] = [2, 2 + COL_W + 2];
/// Renglones de la página de looks por columna.
const LOOKS_ROWS: usize = 9;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Zona {
    Col(usize),
    Herr,
}

#[derive(Clone, Copy, PartialEq)]
pub enum Pagina {
    Panel,
    Looks,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Hit {
    Renglon(usize, usize),
    Menos(usize, usize),
    Mas(usize, usize),
    Herr(usize),
    Look(usize),
    Volver,
}

pub struct Area {
    pub r: Rect,
    pub hit: Hit,
}

pub struct Ui {
    pub estado: Estado,
    pub looks: Vec<LookInfo>,
    pub pagina: Pagina,
    pub zona: Zona,
    pub idx: usize,
    pub look_idx: usize,
}

/// Los renglones elegibles de una columna, en orden.
pub fn renglones(cols: &[Vec<Seccion>; 2], c: usize) -> Vec<&Renglon> {
    cols[c].iter().flat_map(|s| s.renglones.iter()).collect()
}

fn alto_columna(secs: &[Seccion]) -> u16 {
    secs.iter().map(|s| s.renglones.len() as u16 + 2).sum::<u16>().saturating_sub(1)
}

/// El alto del cuerpo (lo mismo en las dos páginas, para que el panel no brinque).
fn alto_cuerpo(cols: &[Vec<Seccion>; 2]) -> u16 {
    alto_columna(&cols[0]).max(alto_columna(&cols[1])).max(LOOKS_ROWS as u16 + 2)
}

pub fn alto(e: &Estado) -> u16 {
    // borde, cabecera, línea, cuerpo, aire, herramientas, borde
    1 + HEAD + 1 + 1 + alto_cuerpo(&columnas(e)) + 1 + 1 + 1
}

fn trunc(s: &str, w: usize) -> String {
    let n = s.chars().count();
    if n <= w {
        s.to_string()
    } else if w == 0 {
        String::new()
    } else {
        format!("{}…", s.chars().take(w - 1).collect::<String>())
    }
}

fn put(buf: &mut Buffer, x: u16, y: u16, s: &str, st: Style) -> u16 {
    let (nx, _) = buf.set_stringn(x, y, s, (buf.area.right().saturating_sub(x)) as usize, st);
    nx
}

impl Ui {
    fn l<'a>(&self, a: &'a str, b: &'a str) -> &'a str {
        if self.estado.es() { a } else { b }
    }

    /// Las celdas, las zonas para el mouse y dónde va la miniatura.
    pub fn render(&self, look: &Look) -> (Buffer, Vec<Area>, Rect) {
        let t = look.theme;
        let cols = columnas(&self.estado);
        let h = alto(&self.estado);
        let area = Rect::new(0, 0, W, h);
        let mut buf = Buffer::empty(area);
        let base = Style::new().bg(t.base).fg(t.fg);
        buf.set_style(area, base);
        let maquina = self.estado.get("maquina").unwrap_or("discada").to_string();
        look.block(false)
            .title(format!(" discada · {maquina} ").fg(t.muted))
            .title_bottom(format!(" {} ", self.l("↑↓ elegir · ←→ cambiar · Enter abrir · Esc cerrar", "↑↓ move · ←→ change · Enter open · Esc close")).fg(t.muted))
            .render(area, &mut buf);
        let mut areas = Vec::new();
        let muted = Style::new().fg(t.muted);

        // ── cabecera: el wallpaper y el look ──
        let thumb = Rect::new(2, 1, THUMB_W, HEAD);
        let x0 = 2 + THUMB_W + 3;
        let ancho = (W - x0 - 2) as usize;
        let actual = self.estado.get("look").unwrap_or("?");
        let info = self.looks.iter().find(|l| l.nombre == actual);
        put(&mut buf, x0, 2, &look.tx(self.l("look", "look")), muted);
        put(&mut buf, x0, 3, actual, Style::new().fg(t.primary).add_modifier(Modifier::BOLD));
        if let Some(i) = info {
            put(&mut buf, x0, 4, &trunc(&i.about, ancho), muted);
            let mut x = x0;
            for c in i.muestra.iter().take(7) {
                buf.set_string(x, 6, "  ", Style::new().bg(Color::Rgb(c.0, c.1, c.2)));
                x += 2;
            }
            if i.muestra.is_empty() {
                put(&mut buf, x0, 6, self.l("colores del wallpaper", "colors from the wallpaper"), muted);
            }
        }
        let cambiar = format!("{} ›", self.l("cambiar", "change"));
        let cx = W - 3 - cambiar.chars().count() as u16;
        put(&mut buf, cx, 3, &cambiar, Style::new().fg(t.accent));
        areas.push(Area { r: Rect::new(cx, 3, cambiar.chars().count() as u16, 1), hit: Hit::Renglon(0, 0) });

        // la línea bajo la cabecera, unida al borde si el look tiene
        let set = look.border_set();
        let y_line = 1 + HEAD;
        let bst = Style::new().fg(t.border);
        if set.vertical_left.trim().is_empty() {
            buf.set_string(1, y_line, set.horizontal_top.repeat((W - 2) as usize), bst);
        } else {
            let (izq, der) = match set.vertical_left {
                "┃" => ("┣", "┫"),
                "║" => ("╠", "╣"),
                _ => ("├", "┤"),
            };
            buf.set_string(0, y_line, izq, bst);
            buf.set_string(1, y_line, set.horizontal_top.repeat((W - 2) as usize), bst);
            buf.set_string(W - 1, y_line, der, bst);
        }

        let y_body = y_line + 2;
        match self.pagina {
            Pagina::Panel => self.cuerpo(&mut buf, &mut areas, look, &cols, y_body),
            Pagina::Looks => self.pagina_looks(&mut buf, &mut areas, look, y_body),
        }

        // ── herramientas ──
        let y_herr = y_body + alto_cuerpo(&cols) + 1;
        let mut x = 2;
        for (i, (nombre, _)) in herramientas(&self.estado).iter().enumerate() {
            let txt = format!(" {} ", look.tx(nombre));
            let w = txt.chars().count() as u16;
            let elegido = self.pagina == Pagina::Panel && self.zona == Zona::Herr && self.idx == i;
            let st = if elegido { look.sel(true).0 } else { Style::new().fg(t.fg).bg(t.hl) };
            put(&mut buf, x, y_herr, &txt, st);
            areas.push(Area { r: Rect::new(x, y_herr, w, 1), hit: Hit::Herr(i) });
            x += w + 1;
        }
        (buf, areas, thumb)
    }

    fn cuerpo(&self, buf: &mut Buffer, areas: &mut Vec<Area>, look: &Look, cols: &[Vec<Seccion>; 2], y0: u16) {
        let t = look.theme;
        for (c, secs) in cols.iter().enumerate() {
            let x0 = COL_X[c];
            let mut y = y0;
            let mut i = 0;
            for (k, s) in secs.iter().enumerate() {
                if k > 0 {
                    y += 1;
                }
                put(buf, x0, y, &s.titulo.to_uppercase(), Style::new().fg(t.accent).add_modifier(Modifier::BOLD));
                y += 1;
                for r in &s.renglones {
                    let elegido = self.zona == Zona::Col(c) && self.idx == i;
                    self.renglon(buf, areas, look, r, c, i, x0, y, elegido);
                    y += 1;
                    i += 1;
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn renglon(&self, buf: &mut Buffer, areas: &mut Vec<Area>, look: &Look, r: &Renglon, c: usize, i: usize, x0: u16, y: u16, elegido: bool) {
        let t = look.theme;
        let (sel_st, marca) = look.sel(true);
        let fila = Rect::new(x0, y, COL_W, 1);
        let mut nombre_st = Style::new().fg(t.fg);
        if elegido {
            buf.set_style(fila, sel_st);
            nombre_st = sel_st;
            put(buf, x0, y, marca, sel_st);
        }
        put(buf, x0 + 2, y, &look.tx(&r.nombre), nombre_st);
        areas.push(Area { r: fila, hit: Hit::Renglon(c, i) });
        let fin = x0 + COL_W - 1;
        let val_st = if elegido { sel_st } else { Style::new().fg(t.fg) };
        let flecha = Style::new().fg(if elegido { t.accent } else { t.muted });
        match &r.tipo {
            Tipo::Opcion { opts, cur } => {
                let txt = trunc(&opts.get(*cur).map(|o| o.1.clone()).unwrap_or_default(), (COL_W / 2) as usize);
                let w = txt.chars().count() as u16;
                let xs = fin - w - 3;
                put(buf, xs - 1, y, "‹", flecha);
                put(buf, xs + 1, y, &txt, val_st);
                put(buf, fin - 1, y, "›", flecha);
                areas.push(Area { r: Rect::new(xs - 2, y, 3, 1), hit: Hit::Menos(c, i) });
                areas.push(Area { r: Rect::new(fin - 2, y, 3, 1), hit: Hit::Mas(c, i) });
            }
            Tipo::Barra { min, max, cur } => {
                let frac = (*cur - *min) as f64 / (*max - *min).max(1) as f64;
                let (lleno, resto) = look.bar(frac, 10);
                let num = if *cur > 0 { format!("+{cur}") } else { cur.to_string() };
                let xs = fin - 10 - 2 - 4;
                put(buf, xs - 1, y, "‹", flecha);
                let nx = put(buf, xs + 1, y, &lleno, Style::new().fg(t.primary));
                put(buf, nx, y, &resto, Style::new().fg(t.border));
                put(buf, xs + 12, y, &format!("{num:>2}"), val_st);
                put(buf, fin - 1, y, "›", flecha);
                areas.push(Area { r: Rect::new(xs - 2, y, 3, 1), hit: Hit::Menos(c, i) });
                areas.push(Area { r: Rect::new(fin - 2, y, 3, 1), hit: Hit::Mas(c, i) });
            }
            Tipo::Interruptor(on) => {
                let (txt, st) = if *on {
                    (format!("● {}", self.l("sí", "on")), Style::new().fg(t.green).add_modifier(Modifier::BOLD))
                } else {
                    (format!("○ {}", self.l("no", "off")), Style::new().fg(t.muted))
                };
                let w = txt.chars().count() as u16;
                put(buf, fin - w, y, &txt, if elegido { sel_st } else { st });
            }
            Tipo::Abre(_) => {
                let txt = trunc(&r.valor, (COL_W / 2) as usize);
                let w = txt.chars().count() as u16;
                put(buf, fin - w - 2, y, &txt, if elegido { sel_st } else { Style::new().fg(t.muted) });
                put(buf, fin - 1, y, "›", flecha);
            }
        }
    }

    fn pagina_looks(&self, buf: &mut Buffer, areas: &mut Vec<Area>, look: &Look, y0: u16) {
        let t = look.theme;
        put(buf, COL_X[0], y0, "LOOKS", Style::new().fg(t.accent).add_modifier(Modifier::BOLD));
        let volver = format!("‹ {}", self.l("volver", "back"));
        let vx = W - 3 - volver.chars().count() as u16;
        put(buf, vx, y0, &volver, Style::new().fg(t.muted));
        areas.push(Area { r: Rect::new(vx, y0, volver.chars().count() as u16, 1), hit: Hit::Volver });
        let actual = self.estado.get("look").unwrap_or("");
        let (sel_st, marca) = look.sel(true);
        for (i, l) in self.looks.iter().enumerate() {
            let (c, row) = (i / LOOKS_ROWS, i % LOOKS_ROWS);
            if c > 1 {
                break;
            }
            let (x0, y) = (COL_X[c], y0 + 1 + row as u16);
            let fila = Rect::new(x0, y, COL_W, 1);
            let elegido = i == self.look_idx;
            let mut st = Style::new().fg(t.fg);
            if elegido {
                buf.set_style(fila, sel_st);
                st = sel_st;
                put(buf, x0, y, marca, sel_st);
            }
            if l.nombre == actual {
                put(buf, x0 + 1, y, "●", Style::new().fg(t.primary));
            }
            put(buf, x0 + 3, y, &trunc(&l.nombre, 13), st.add_modifier(Modifier::BOLD));
            let sx = x0 + 17;
            if l.muestra.is_empty() {
                put(buf, sx, y, "░░░░░░░", Style::new().fg(t.border));
            } else {
                for (k, c) in l.muestra.iter().take(7).enumerate() {
                    buf.set_string(sx + k as u16, y, " ", Style::new().bg(Color::Rgb(c.0, c.1, c.2)));
                }
            }
            put(buf, sx + 9, y, &trunc(&l.about, (COL_W - 27) as usize), if elegido { sel_st } else { Style::new().fg(t.muted) });
            areas.push(Area { r: fila, hit: Hit::Look(i) });
        }
    }
}
