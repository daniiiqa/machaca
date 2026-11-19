//! La superficie de machaca (layer-shell, encima de todo y con el teclado):
//! oscurece un poco la pantalla y pone el panel al centro. Lo que se cambia
//! va a `QuickSettings.sh hacer …`; lo que se abre cierra el panel primero.

use crate::estado::{self, Abre, Estado, Tipo, columnas, herramientas, looks};
use crate::ui::{self, Hit, Pagina, Ui, Zona, renglones};
use disco::look::Look;
use disco::paint::Painter;
use smithay_client_toolkit::reexports::calloop::EventLoop;
use smithay_client_toolkit::reexports::calloop::channel::{Event, channel};
use smithay_client_toolkit::reexports::calloop_wayland_source::WaylandSource;
use smithay_client_toolkit::reexports::client::globals::registry_queue_init;
use smithay_client_toolkit::reexports::client::protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface};
use smithay_client_toolkit::reexports::client::{Connection, QueueHandle};
use smithay_client_toolkit::reexports::protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::{Shape, WpCursorShapeDeviceV1};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, FrameCallbackData},
    delegate_registry,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    seat::{
        Capability, SeatHandler, SeatState,
        keyboard::{KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers},
        pointer::{PointerEvent, PointerEventKind, PointerHandler, cursor_shape::CursorShapeManager},
    },
    shell::{
        WaylandSurface,
        wlr_layer::{Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface, LayerSurfaceConfigure},
    },
    shm::{Shm, ShmHandler, slot::SlotPool},
};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime};
use tiny_skia::{FilterQuality, Pixmap, PixmapPaint, Transform};

const ABRIR: f32 = 0.16;
const CERRAR: f32 = 0.12;
/// Después de cambiar algo, cuánto no se le cree a la lectura (el script
/// tarda en aplicar y la lectura vieja lo regresaría).
const ESPERA: Duration = Duration::from_millis(1500);
/// La barra (tamaño de letra) se aplica cuando dejas de moverla.
const BARRA: Duration = Duration::from_millis(450);
const BTN_LEFT: u32 = 0x110;

#[derive(Clone, Copy, PartialEq)]
enum Fase {
    Abriendo(Instant),
    Abierto,
    Cerrando(Instant),
}

/// Lo que llega del hilo que lee: el estado y la miniatura del wallpaper.
enum Llega {
    Estado(Estado),
    Miniatura(Pixmap),
}

struct App {
    registry: RegistryState,
    seat: SeatState,
    output: OutputState,
    compositor: CompositorState,
    layer_shell: LayerShell,
    shm: Shm,
    qh: QueueHandle<App>,
    shapes: Option<CursorShapeManager>,
    pointer: Option<wl_pointer::WlPointer>,
    shape_dev: Option<WpCursorShapeDeviceV1>,
    keyboard: Option<wl_keyboard::WlKeyboard>,

    look: Look,
    painter: Painter,
    ui: Ui,
    miniatura: Option<Pixmap>,
    areas: Vec<ui::Area>,
    /// dónde cae el panel (pixeles lógicos) y el tamaño de una celda
    caja: (f32, f32, f32, f32),
    celda: (f32, f32),

    layer: Option<LayerSurface>,
    pool: Option<SlotPool>,
    size: (u32, u32),
    scale: i32,
    frame_pending: bool,
    dirty: bool,
    fase: Fase,
    cambiado: Option<Instant>,
    barra: Option<(&'static str, i32, Instant)>,
    /// lo que se lanza al cerrar (ya sin el panel en pantalla)
    luego: Option<Vec<String>>,
    exit: bool,
}

/// `QuickSettings.sh <args>` suelto, sin esperarlo.
fn lanza(args: &[String]) {
    let _ = Command::new("setsid")
        .arg("-f")
        .args(args)
        .env("PATH", estado::path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

fn qs(args: &[&str]) -> Vec<String> {
    std::iter::once(estado::qs().to_string_lossy().into_owned()).chain(args.iter().map(|s| s.to_string())).collect()
}

impl App {
    fn redraw(&mut self) {
        if self.frame_pending {
            self.dirty = true;
        } else {
            self.draw();
        }
    }

    fn cerrar(&mut self) {
        if !matches!(self.fase, Fase::Cerrando(_)) {
            self.fase = Fase::Cerrando(Instant::now());
            self.redraw();
        }
    }

    fn cerrar_y(&mut self, args: Vec<String>) {
        self.luego = Some(args);
        self.cerrar();
    }

    // ─── acciones ───────────────────────────────────────────────────────────

    fn abrir(&mut self, a: &Abre) {
        match a {
            Abre::Looks => {
                self.ui.pagina = Pagina::Looks;
                let actual = self.ui.estado.get("look").unwrap_or("").to_string();
                self.ui.look_idx = self.ui.looks.iter().position(|l| l.nombre == actual).unwrap_or(0);
                self.redraw();
            }
            Abre::Hacer(x) => self.cerrar_y(qs(&["hacer", x])),
            Abre::Menu(x) => self.cerrar_y(qs(&["menu", x])),
        }
    }

    /// Mueve una opción o la barra `d` pasos (o prende/apaga un interruptor).
    fn cambiar(&mut self, c: usize, i: usize, d: i32) {
        let cols = columnas(&self.ui.estado);
        let Some(r) = renglones(&cols, c).get(i).map(|r| (*r).clone()) else { return };
        match &r.tipo {
            Tipo::Opcion { opts, cur } => {
                let n = opts.len() as i32;
                if n == 0 {
                    return;
                }
                let nuevo = (*cur as i32 + d).rem_euclid(n) as usize;
                let v = opts[nuevo].0.clone();
                self.poner(r.clave, &v);
                lanza(&qs(&["hacer", r.clave, &v]));
            }
            Tipo::Barra { min, max, cur } => {
                let nuevo = (*cur + d).clamp(*min, *max);
                if nuevo != *cur {
                    self.poner(r.clave, &nuevo.to_string());
                    self.barra = Some((r.clave, nuevo, Instant::now()));
                }
            }
            Tipo::Interruptor(on) => {
                self.poner(r.clave, if *on { "no" } else { "yes" });
                lanza(&qs(&["hacer", r.clave]));
            }
            Tipo::Abre(_) => {}
        }
        self.redraw();
    }

    fn activar(&mut self, c: usize, i: usize) {
        let cols = columnas(&self.ui.estado);
        let Some(r) = renglones(&cols, c).get(i).map(|r| (*r).clone()) else { return };
        match &r.tipo {
            Tipo::Abre(a) => self.abrir(a),
            _ => self.cambiar(c, i, 1),
        }
    }

    /// El valor nuevo, ya en pantalla (sin esperar a que el script lo aplique).
    fn poner(&mut self, clave: &str, v: &str) {
        let lab = self.ui.estado.opciones.get(clave).and_then(|o| o.iter().find(|(x, _)| x == v)).map(|(_, l)| l.clone()).unwrap_or_else(|| v.to_string());
        self.ui.estado.v.insert(clave.to_string(), (v.to_string(), lab));
        self.cambiado = Some(Instant::now());
    }

    fn herramienta(&mut self, i: usize) {
        if let Some((_, a)) = herramientas(&self.ui.estado).get(i) {
            let a = a.clone();
            self.abrir(&a);
        }
    }

    fn aplicar_look(&mut self, i: usize) {
        let Some(l) = self.ui.looks.get(i) else { return };
        if Some(l.nombre.as_str()) == self.ui.estado.get("look") {
            self.ui.pagina = Pagina::Panel;
            self.redraw();
            return;
        }
        let s = estado::scripts().join("LookSwitch.sh").to_string_lossy().into_owned();
        self.cerrar_y(vec![s, l.nombre.clone()]);
    }

    fn llega(&mut self, m: Llega) {
        match m {
            Llega::Estado(e) => {
                if self.cambiado.is_some_and(|t| t.elapsed() < ESPERA) || self.barra.is_some() {
                    return;
                }
                if e != self.ui.estado {
                    let lang = e.es() != self.ui.estado.es();
                    let look = e.get("look") != self.ui.estado.get("look");
                    self.ui.estado = e;
                    if lang || look {
                        self.ui.looks = looks(self.ui.estado.es());
                    }
                    self.ajustar_sel();
                    self.redraw();
                }
            }
            Llega::Miniatura(p) => {
                self.miniatura = Some(p);
                self.redraw();
            }
        }
    }

    /// Que la selección no se quede fuera si cambiaron los renglones.
    fn ajustar_sel(&mut self) {
        let cols = columnas(&self.ui.estado);
        let n = match self.ui.zona {
            Zona::Col(c) => renglones(&cols, c).len(),
            Zona::Herr => herramientas(&self.ui.estado).len(),
        };
        self.ui.idx = self.ui.idx.min(n.saturating_sub(1));
    }

    // ─── teclado ────────────────────────────────────────────────────────────

    fn key(&mut self, k: Keysym) {
        if self.ui.pagina == Pagina::Looks {
            return self.key_looks(k);
        }
        let cols = columnas(&self.ui.estado);
        let n_col = |c: usize| renglones(&cols, c).len();
        let n_herr = herramientas(&self.ui.estado).len();
        let (zona, idx) = (self.ui.zona, self.ui.idx);
        match k {
            Keysym::Escape => return self.cerrar(),
            Keysym::Return | Keysym::KP_Enter | Keysym::space => {
                return match zona {
                    Zona::Col(c) => self.activar(c, idx),
                    Zona::Herr => self.herramienta(idx),
                };
            }
            Keysym::Down | Keysym::j => match zona {
                Zona::Col(c) if idx + 1 < n_col(c) => self.ui.idx += 1,
                Zona::Col(c) => {
                    self.ui.zona = Zona::Herr;
                    self.ui.idx = if c == 0 { 0 } else { n_herr.saturating_sub(1) };
                }
                Zona::Herr => {
                    self.ui.zona = Zona::Col(0);
                    self.ui.idx = 0;
                }
            },
            Keysym::Up | Keysym::k => match zona {
                Zona::Col(_) if idx > 0 => self.ui.idx -= 1,
                Zona::Col(_) => {
                    self.ui.zona = Zona::Herr;
                    self.ui.idx = 0;
                }
                Zona::Herr => {
                    let c = if idx < n_herr / 2 { 0 } else { 1 };
                    self.ui.zona = Zona::Col(c);
                    self.ui.idx = n_col(c).saturating_sub(1);
                }
            },
            Keysym::Left | Keysym::h | Keysym::Right | Keysym::l => {
                let d = if matches!(k, Keysym::Left | Keysym::h) { -1 } else { 1 };
                match zona {
                    Zona::Herr => self.ui.idx = (idx as i32 + d).rem_euclid(n_herr as i32) as usize,
                    Zona::Col(c) => {
                        let ajustable = renglones(&cols, c).get(idx).is_some_and(|r| matches!(r.tipo, Tipo::Opcion { .. } | Tipo::Barra { .. } | Tipo::Interruptor(_)));
                        if ajustable {
                            return self.cambiar(c, idx, d);
                        }
                        let otra = 1 - c;
                        self.ui.zona = Zona::Col(otra);
                        self.ui.idx = idx.min(n_col(otra).saturating_sub(1));
                    }
                }
            }
            Keysym::Tab => {
                if let Zona::Col(c) = zona {
                    let otra = 1 - c;
                    self.ui.zona = Zona::Col(otra);
                    self.ui.idx = idx.min(n_col(otra).saturating_sub(1));
                }
            }
            _ => return,
        }
        self.redraw();
    }

    fn key_looks(&mut self, k: Keysym) {
        let n = self.ui.looks.len();
        if n == 0 {
            self.ui.pagina = Pagina::Panel;
            return self.redraw();
        }
        let i = self.ui.look_idx;
        self.ui.look_idx = match k {
            Keysym::Escape | Keysym::BackSpace => {
                self.ui.pagina = Pagina::Panel;
                return self.redraw();
            }
            Keysym::Return | Keysym::KP_Enter | Keysym::space => return self.aplicar_look(i),
            Keysym::Down | Keysym::j => (i + 1) % n,
            Keysym::Up | Keysym::k => (i + n - 1) % n,
            Keysym::Right | Keysym::l => (i + 9).min(n - 1),
            Keysym::Left | Keysym::h => i.saturating_sub(9),
            _ => return,
        };
        self.redraw();
    }

    // ─── mouse ──────────────────────────────────────────────────────────────

    fn hit(&self, x: f32, y: f32) -> Option<Hit> {
        let (bx, by, bw, bh) = self.caja;
        if x < bx || y < by || x >= bx + bw || y >= by + bh {
            return None;
        }
        let (cx, cy) = (((x - bx) / self.celda.0) as u16, ((y - by) / self.celda.1) as u16);
        // las flechas antes que el renglón entero
        let mut found = None;
        for a in &self.areas {
            if cx >= a.r.x && cx < a.r.right() && cy >= a.r.y && cy < a.r.bottom() {
                match a.hit {
                    Hit::Menos(..) | Hit::Mas(..) => return Some(a.hit),
                    h => found = found.or(Some(h)),
                }
            }
        }
        found
    }

    fn dentro(&self, x: f32, y: f32) -> bool {
        let (bx, by, bw, bh) = self.caja;
        x >= bx && y >= by && x < bx + bw && y < by + bh
    }

    fn motion(&mut self, x: f32, y: f32) {
        let antes = (self.ui.zona, self.ui.idx, self.ui.look_idx);
        match self.hit(x, y) {
            Some(Hit::Renglon(c, i) | Hit::Menos(c, i) | Hit::Mas(c, i)) if self.ui.pagina == Pagina::Panel => {
                self.ui.zona = Zona::Col(c);
                self.ui.idx = i;
            }
            Some(Hit::Herr(i)) if self.ui.pagina == Pagina::Panel => {
                self.ui.zona = Zona::Herr;
                self.ui.idx = i;
            }
            Some(Hit::Look(i)) => self.ui.look_idx = i,
            _ => {}
        }
        if antes != (self.ui.zona, self.ui.idx, self.ui.look_idx) {
            self.redraw();
        }
    }

    fn click(&mut self, x: f32, y: f32) {
        if !self.dentro(x, y) {
            return self.cerrar();
        }
        match self.hit(x, y) {
            Some(Hit::Renglon(0, 0)) if self.ui.pagina == Pagina::Panel => self.abrir(&Abre::Looks),
            Some(Hit::Renglon(c, i)) if self.ui.pagina == Pagina::Panel => self.activar(c, i),
            Some(Hit::Menos(c, i)) => self.cambiar(c, i, -1),
            Some(Hit::Mas(c, i)) => self.cambiar(c, i, 1),
            Some(Hit::Herr(i)) => self.herramienta(i),
            Some(Hit::Look(i)) => self.aplicar_look(i),
            Some(Hit::Volver) => {
                self.ui.pagina = Pagina::Panel;
                self.redraw();
            }
            _ => {}
        }
    }

    fn rueda(&mut self, x: f32, y: f32, d: i32) {
        if let Some(Hit::Renglon(c, i) | Hit::Menos(c, i) | Hit::Mas(c, i)) = self.hit(x, y)
            && self.ui.pagina == Pagina::Panel
        {
            self.cambiar(c, i, d);
        }
    }

    // ─── dibujo ─────────────────────────────────────────────────────────────

    fn draw(&mut self) {
        let Some(layer) = self.layer.clone() else { return };
        let s = self.scale.max(1);
        let (w, h) = (self.size.0 as i32 * s, self.size.1 as i32 * s);
        if w <= 0 || h <= 0 {
            return;
        }
        let Some(mut pix) = Pixmap::new(w as u32, h as u32) else { return };
        let (e, animando, fin) = match self.fase {
            Fase::Abriendo(t0) => {
                let e = ease(t0.elapsed().as_secs_f32() / ABRIR);
                if e >= 1.0 {
                    self.fase = Fase::Abierto;
                }
                (e, e < 1.0, false)
            }
            Fase::Abierto => (1.0, false, false),
            Fase::Cerrando(t0) => {
                let e = 1.0 - ease(t0.elapsed().as_secs_f32() / CERRAR);
                (e, e > 0.0, e <= 0.0)
            }
        };
        let t = self.look.theme;
        pix.fill(sk(t.base, 0.26 * e));

        let (buf, areas, thumb) = self.ui.render(&self.look);
        let sf = s as f32;
        let (pw, ph) = self.painter.size(buf.area.width, buf.area.height);
        let (cw, ch) = {
            let (a, b) = self.painter.size(100, 100);
            (a as f32 / 100.0, b as f32 / 100.0)
        };
        self.celda = (cw, ch);
        let (lw, lh) = (self.size.0 as f32, self.size.1 as f32);
        // al abrir sube un poquito
        let sube = (1.0 - e) * 14.0;
        let (bx, by) = (((lw - pw as f32) / 2.0).round(), ((lh - ph as f32) / 2.0 + sube).round());
        self.caja = (bx, by, pw as f32, ph as f32);
        self.areas = areas;

        if let Some(mut card) = Pixmap::new((pw as f32 * sf).ceil() as u32, (ph as f32 * sf).ceil() as u32) {
            pintar(&mut self.painter, &mut card, &buf, &self.look, (cw * sf, ch * sf), sf);
            if let Some(m) = &self.miniatura {
                let r = (thumb.x as f32 * cw * sf, thumb.y as f32 * ch * sf, thumb.width as f32 * cw * sf, thumb.height as f32 * ch * sf);
                blit_cover(&mut card, m, r, if self.look.border_set().top_left == "╭" { 8.0 * sf } else { 0.0 });
            }
            let paint = PixmapPaint { opacity: e, quality: FilterQuality::Nearest, ..Default::default() };
            pix.draw_pixmap((bx * sf) as i32, (by * sf) as i32, card.as_ref(), &paint, Transform::identity(), None);
        }

        let pool = match &mut self.pool {
            Some(p) => p,
            None => self.pool.insert(SlotPool::new((w * h * 4) as usize, &self.shm).expect("memoria compartida")),
        };
        let Ok((buffer, canvas)) = pool.create_buffer(w, h, w * 4, wl_shm::Format::Argb8888) else { return };
        for (dst, src) in canvas.as_chunks_mut::<4>().0.iter_mut().zip(pix.data().as_chunks::<4>().0) {
            dst[0] = src[2];
            dst[1] = src[1];
            dst[2] = src[0];
            dst[3] = src[3];
        }
        let surface = layer.wl_surface();
        surface.set_buffer_scale(s);
        surface.damage_buffer(0, 0, w, h);
        surface.frame(&self.qh, FrameCallbackData(surface.clone()));
        self.frame_pending = true;
        self.dirty = animando;
        if buffer.attach_to(surface).is_ok() {
            layer.commit();
        }
        if fin {
            self.exit = true;
        }
    }
}

/// El panel en tres pasadas, para que los fondos de color (lo elegido, los
/// botones, las muestras) sean un rectángulo liso y no celdas con costuras:
/// el fondo y el borde, luego los fondos de color, y encima el texto.
fn pintar(painter: &mut Painter, pix: &mut Pixmap, buf: &ratatui::buffer::Buffer, look: &Look, (cw, ch): (f32, f32), s: f32) {
    let base = look.theme.base;
    let a = buf.area;
    let orilla = |x: u16, y: u16| x == 0 || y == 0 || x == a.width - 1 || y == a.height - 1 || buf[(x, y)].symbol().chars().all(|c| "─━═┄├┤┣┫╠╣".contains(c)) && x != 0 && buf[(1, y)].symbol() != " " && buf[(a.width / 2, y)].symbol() == buf[(1, y)].symbol();
    let mut fondo = buf.clone();
    let mut texto = buf.clone();
    let mut tiras: Vec<(u16, u16, u16, ratatui::style::Color)> = Vec::new();
    for y in 0..a.height {
        let mut x = 0;
        while x < a.width {
            let bg = buf[(x, y)].bg;
            if orilla(x, y) {
                texto[(x, y)].set_symbol(" ");
            } else {
                fondo[(x, y)].set_symbol(" ");
            }
            if bg != base && !orilla(x, y) {
                let x0 = x;
                while x < a.width && buf[(x, y)].bg == bg && !orilla(x, y) {
                    texto[(x, y)].set_bg(base);
                    fondo[(x, y)].set_bg(base);
                    if x != x0 {
                        fondo[(x, y)].set_symbol(" ");
                    }
                    x += 1;
                }
                tiras.push((x0, y, x - x0, bg));
                continue;
            }
            x += 1;
        }
    }
    let redondo = look.border_set().top_left == "╭";
    // sin blur (obsidiana lo trae apagado) un panel translúcido deja ver el
    // texto de atrás: entonces va opaco. Con blur, como diga el look.
    let alfa = look.alpha();
    if alfa < 1.0 && !blur_prendido() {
        let extra = 1.0;
        let (x, y) = ((cw / 2.0).floor(), (ch / 2.0).floor());
        let (w, h) = ((a.width - 1) as f32 * cw, (a.height - 1) as f32 * ch);
        disco::paint::rounded(pix, x, y, w, h, if redondo { cw / 2.0 } else { 0.0 }, base, extra);
    }
    painter.card(pix, &fondo, look, 0.0, 0.0, s);
    for (x, y, w, c) in tiras {
        let (x0, x1) = ((x as f32 * cw).round(), ((x + w) as f32 * cw).round());
        let (y0, y1) = ((y as f32 * ch).round(), ((y + 1) as f32 * ch).round());
        disco::paint::rounded(pix, x0, y0, x1 - x0, y1 - y0, if redondo { 3.0 * s } else { 0.0 }, c, 1.0);
    }
    painter.cells(pix, &texto, look, 0.0, 0.0, s);
}

fn ease(t: f32) -> f32 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}

fn sk(c: ratatui::style::Color, a: f32) -> tiny_skia::Color {
    let (r, g, b) = match c {
        ratatui::style::Color::Rgb(r, g, b) => (r, g, b),
        _ => (0, 0, 0),
    };
    tiny_skia::Color::from_rgba8(r, g, b, (a.clamp(0.0, 1.0) * 255.0) as u8)
}

/// La miniatura llenando `r` (ya viene con su forma), con esquinas si el look las tiene.
fn blit_cover(dst: &mut Pixmap, src: &Pixmap, r: (f32, f32, f32, f32), radio: f32) {
    let (x, y, w, h) = r;
    let (sx, sy) = (w / src.width() as f32, h / src.height() as f32);
    let mask = (radio > 0.0)
        .then(|| {
            let mut m = tiny_skia::Mask::new(dst.width(), dst.height())?;
            let path = {
                let mut pb = tiny_skia::PathBuilder::new();
                let rr = radio.min(w / 2.0).min(h / 2.0);
                pb.move_to(x + rr, y);
                pb.line_to(x + w - rr, y);
                pb.quad_to(x + w, y, x + w, y + rr);
                pb.line_to(x + w, y + h - rr);
                pb.quad_to(x + w, y + h, x + w - rr, y + h);
                pb.line_to(x + rr, y + h);
                pb.quad_to(x, y + h, x, y + h - rr);
                pb.line_to(x, y + rr);
                pb.quad_to(x, y, x + rr, y);
                pb.close();
                pb.finish()?
            };
            m.fill_path(&path, tiny_skia::FillRule::Winding, true, Transform::identity());
            Some(m)
        })
        .flatten();
    let paint = PixmapPaint { quality: FilterQuality::Bilinear, ..Default::default() };
    dst.draw_pixmap(0, 0, src.as_ref(), &paint, Transform::from_row(sx, 0.0, 0.0, sy, x, y), mask.as_ref());
}

// ─── la miniatura del wallpaper ─────────────────────────────────────────────

fn wallpaper_actual() -> PathBuf {
    estado::home().join(".config/hypr/wallpaper_effects/.wallpaper_current")
}

fn mtime(p: &std::path::Path) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

/// El wallpaper recortado a `w`×`h` (como awww), guardado en ~/.cache/machaca.
fn miniatura(w: u32, h: u32) -> Option<Pixmap> {
    let src = wallpaper_actual();
    let stamp = mtime(&src)?.duration_since(SystemTime::UNIX_EPOCH).ok()?.as_secs();
    let dir = estado::home().join(".cache/machaca");
    let out = dir.join(format!("wall-{stamp}-{w}x{h}.ppm"));
    if !out.exists() {
        std::fs::create_dir_all(&dir).ok()?;
        // las viejas fuera
        for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            if e.file_name().to_string_lossy().starts_with("wall-") {
                let _ = std::fs::remove_file(e.path());
            }
        }
        let geo = format!("{w}x{h}");
        Command::new("magick")
            .arg(format!("{}[0]", src.display()))
            .args(["-resize", &format!("{geo}^"), "-gravity", "center", "-extent", &geo])
            .arg(format!("ppm:{}", out.display()))
            .status()
            .ok()?;
    }
    ppm(&std::fs::read(out).ok()?)
}

fn ppm(data: &[u8]) -> Option<Pixmap> {
    let mut fields = Vec::new();
    let mut i = 0;
    while fields.len() < 4 {
        while i < data.len() && data[i].is_ascii_whitespace() {
            i += 1;
        }
        if data.get(i) == Some(&b'#') {
            while i < data.len() && data[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        let start = i;
        while i < data.len() && !data[i].is_ascii_whitespace() {
            i += 1;
        }
        fields.push(std::str::from_utf8(&data[start..i]).ok()?);
    }
    if fields[0] != "P6" || fields[3] != "255" {
        return None;
    }
    let (w, h): (u32, u32) = (fields[1].parse().ok()?, fields[2].parse().ok()?);
    let px = data.get(i + 1..i + 1 + (w * h * 3) as usize)?;
    let mut pix = Pixmap::new(w, h)?;
    for (d, s) in pix.data_mut().as_chunks_mut::<4>().0.iter_mut().zip(px.as_chunks::<3>().0) {
        d.copy_from_slice(&[s[0], s[1], s[2], 255]);
    }
    Some(pix)
}

// ─── arrancar ───────────────────────────────────────────────────────────────

pub fn run(sigs: libc::sigset_t) -> Result<(), String> {
    let estado0 = Estado::leer();
    if estado0.v.is_empty() {
        return Err("QuickSettings.sh estado no dijo nada (¿discada actualizado?)".into());
    }
    let looks0 = looks(estado0.es());
    let painter = Painter::new();
    let (cw, ch) = {
        let (a, b) = painter.size(100, 100);
        (a as f32 / 100.0, b as f32 / 100.0)
    };
    // la miniatura a 2x: se ve fina con o sin escala
    let (tw, th) = ((ui::THUMB_W as f32 * cw * 2.0) as u32, (6.0 * ch * 2.0) as u32);

    let conn = Connection::connect_to_env().map_err(|e| format!("Wayland: {e}"))?;
    let (globals, queue) = registry_queue_init(&conn).map_err(|e| format!("Wayland: {e}"))?;
    let qh = queue.handle();
    let mut event_loop: EventLoop<App> = EventLoop::try_new().map_err(|e| e.to_string())?;
    let handle = event_loop.handle();
    WaylandSource::new(conn.clone(), queue).insert(handle.clone()).map_err(|e| e.to_string())?;

    // `machaca` otra vez: cerrar
    let (tx_sig, rx_sig) = channel::<()>();
    std::thread::spawn(move || {
        loop {
            let mut sig = 0;
            if unsafe { libc::sigwait(&sigs, &mut sig) } == 0 && tx_sig.send(()).is_err() {
                return;
            }
        }
    });
    handle
        .insert_source(rx_sig, |ev, _, app: &mut App| {
            if let Event::Msg(()) = ev {
                app.cerrar();
            }
        })
        .map_err(|e| e.to_string())?;

    // el estado cada segundo y la miniatura cuando cambie el wallpaper
    let (tx, rx) = channel::<Llega>();
    std::thread::spawn(move || {
        let mut visto = None;
        loop {
            let m = mtime(&wallpaper_actual());
            if m != visto {
                visto = m;
                if let Some(p) = miniatura(tw, th)
                    && tx.send(Llega::Miniatura(p)).is_err()
                {
                    return;
                }
            }
            std::thread::sleep(Duration::from_millis(900));
            if tx.send(Llega::Estado(Estado::leer())).is_err() {
                return;
            }
        }
    });
    handle
        .insert_source(rx, |ev, _, app: &mut App| {
            if let Event::Msg(m) = ev {
                app.llega(m);
            }
        })
        .map_err(|e| e.to_string())?;

    let shapes = CursorShapeManager::bind(&globals, &qh).ok();
    let mut app = App {
        registry: RegistryState::new(&globals),
        seat: SeatState::new(&globals, &qh),
        output: OutputState::new(&globals, &qh),
        compositor: CompositorState::bind(&globals, &qh).map_err(|_| "el compositor no tiene wl_compositor")?,
        layer_shell: LayerShell::bind(&globals, &qh).map_err(|_| "el compositor no tiene layer-shell")?,
        shm: Shm::bind(&globals, &qh).map_err(|_| "el compositor no tiene wl_shm")?,
        qh: qh.clone(),
        shapes,
        pointer: None,
        shape_dev: None,
        keyboard: None,
        look: Look::load("machaca"),
        painter,
        ui: Ui { estado: estado0, looks: looks0, pagina: Pagina::Panel, zona: Zona::Col(0), idx: 0, look_idx: 0 },
        miniatura: None,
        areas: Vec::new(),
        caja: (0.0, 0.0, 0.0, 0.0),
        celda: (cw, ch),
        layer: None,
        pool: None,
        size: (0, 0),
        scale: 1,
        frame_pending: false,
        dirty: false,
        fase: Fase::Abriendo(Instant::now()),
        cambiado: None,
        barra: None,
        luego: None,
        exit: false,
    };

    let surface = app.compositor.create_surface(&qh);
    let layer = app.layer_shell.create_layer_surface(&qh, surface, Layer::Overlay, Some("machaca"), None);
    layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
    layer.set_exclusive_zone(-1);
    layer.set_keyboard_interactivity(KeyboardInteractivity::Exclusive);
    layer.set_size(0, 0);
    layer.commit();
    app.layer = Some(layer);

    // para probar sin teclado: MACHACA_TECLAS="Tab Down Return" (una cada 0.6 s)
    let mut teclas: Vec<Keysym> = std::env::var("MACHACA_TECLAS")
        .unwrap_or_default()
        .split_whitespace()
        .rev()
        .filter_map(|n| match n {
            "Up" => Some(Keysym::Up),
            "Down" => Some(Keysym::Down),
            "Left" => Some(Keysym::Left),
            "Right" => Some(Keysym::Right),
            "Tab" => Some(Keysym::Tab),
            "Return" => Some(Keysym::Return),
            "Escape" => Some(Keysym::Escape),
            _ => None,
        })
        .collect();
    let mut proxima = Instant::now() + Duration::from_millis(900);
    while !app.exit {
        event_loop.dispatch(Duration::from_millis(50), &mut app).map_err(|e| e.to_string())?;
        if !teclas.is_empty() && Instant::now() >= proxima {
            proxima = Instant::now() + Duration::from_millis(600);
            if let Some(k) = teclas.pop() {
                app.key(k);
            }
        }
        // la barra se aplica cuando la dejas quieta
        if let Some((clave, v, t)) = app.barra
            && t.elapsed() >= BARRA
        {
            app.barra = None;
            app.cambiado = Some(Instant::now());
            lanza(&qs(&["hacer", clave, &v.to_string()]));
        }
        if app.look.refresh() {
            app.painter = Painter::new();
            app.redraw();
        }
    }
    app.layer = None;
    let _ = conn.flush();
    if let Some((clave, v, _)) = app.barra.take() {
        lanza(&qs(&["hacer", clave, &v.to_string()]));
    }
    if let Some(args) = app.luego.take() {
        // que Hyprland alcance a quitar el panel antes (los menús y el
        // telón de farula toman foto de la pantalla)
        std::thread::sleep(Duration::from_millis(60));
        lanza(&args);
    }
    Ok(())
}

// ─── Wayland ────────────────────────────────────────────────────────────────

impl CompositorHandler for App {
    fn scale_factor_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, f: i32) {
        if f != self.scale {
            self.scale = f;
            self.pool = None;
            self.redraw();
        }
    }
    fn transform_changed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: wl_output::Transform) {}
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {
        self.frame_pending = false;
        if self.dirty {
            self.draw();
        }
    }
    fn surface_enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
    fn surface_leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: &wl_output::WlOutput) {}
}

impl OutputHandler for App {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl LayerShellHandler for App {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        self.exit = true;
    }
    fn configure(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface, cfg: LayerSurfaceConfigure, _: u32) {
        if cfg.new_size.0 > 0 && cfg.new_size.1 > 0 && cfg.new_size != self.size {
            self.size = cfg.new_size;
            self.pool = None;
        }
        if let Fase::Abriendo(_) = self.fase {
            self.fase = Fase::Abriendo(Instant::now());
        }
        self.redraw();
    }
}

impl SeatHandler for App {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat
    }
    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
    fn new_capability(&mut self, _: &Connection, qh: &QueueHandle<Self>, seat: wl_seat::WlSeat, cap: Capability) {
        if cap == Capability::Pointer && self.pointer.is_none() {
            self.pointer = self.seat.get_pointer(qh, &seat).ok();
            if let (Some(p), Some(shapes)) = (&self.pointer, &self.shapes) {
                self.shape_dev = Some(shapes.get_shape_device(p, qh));
            }
        }
        if cap == Capability::Keyboard && self.keyboard.is_none() {
            self.keyboard = self.seat.get_keyboard(qh, &seat, None).ok();
        }
    }
    fn remove_capability(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat, _: Capability) {}
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl PointerHandler for App {
    fn pointer_frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_pointer::WlPointer, events: &[PointerEvent]) {
        if matches!(self.fase, Fase::Cerrando(_)) {
            return;
        }
        for e in events {
            let (x, y) = (e.position.0 as f32, e.position.1 as f32);
            match e.kind {
                PointerEventKind::Enter { serial } => {
                    if let Some(dev) = &self.shape_dev {
                        dev.set_shape(serial, Shape::Default);
                    }
                    self.motion(x, y);
                }
                PointerEventKind::Motion { .. } => self.motion(x, y),
                PointerEventKind::Press { button: BTN_LEFT, .. } => self.click(x, y),
                PointerEventKind::Axis { vertical, .. } => {
                    let d = if vertical.discrete != 0 { vertical.discrete } else { vertical.absolute.signum() as i32 };
                    if d != 0 {
                        // rueda hacia arriba = más
                        self.rueda(x, y, -d.signum());
                    }
                }
                _ => {}
            }
        }
    }
}

impl KeyboardHandler for App {
    fn enter(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: &wl_surface::WlSurface, _: u32, _: &[u32], _: &[Keysym]) {}
    fn leave(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: &wl_surface::WlSurface, _: u32) {}
    fn press_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, event: KeyEvent) {
        if !matches!(self.fase, Fase::Cerrando(_)) {
            self.key(event.keysym);
        }
    }
    fn repeat_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, event: KeyEvent) {
        if !matches!(self.fase, Fase::Cerrando(_)) && matches!(event.keysym, Keysym::Up | Keysym::Down | Keysym::Left | Keysym::Right | Keysym::h | Keysym::j | Keysym::k | Keysym::l) {
            self.key(event.keysym);
        }
    }
    fn release_key(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, _: KeyEvent) {}
    fn update_modifiers(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_keyboard::WlKeyboard, _: u32, _: Modifiers, _: RawModifiers, _: u32) {}
}

impl ShmHandler for App {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

delegate_registry!(App);

impl ProvidesRegistryState for App {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry
    }
    registry_handlers![OutputState, SeatState];
}

smithay_client_toolkit::delegate_dispatch2!(App);

/// ¿Hyprland difumina lo de atrás? (los looks lo prenden o apagan)
fn blur_prendido() -> bool {
    use std::sync::OnceLock;
    static B: OnceLock<bool> = OnceLock::new();
    *B.get_or_init(|| {
        Command::new("hyprctl")
            .args(["getoption", "decoration:blur:enabled", "-j"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .is_some_and(|s| s.contains("\"bool\": true") || s.contains("\"int\": 1"))
    })
}
