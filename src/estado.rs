//! Lo que muestra el panel. Los valores los da `QuickSettings.sh estado` (la
//! lógica de discada sigue en bash: aquí solo se lee) y los looks salen de
//! `lena list` y de cada look.toml.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;

pub fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_default())
}

pub fn scripts() -> PathBuf {
    home().join(".config/hypr/scripts")
}

pub fn qs() -> PathBuf {
    scripts().join("QuickSettings.sh")
}

/// PATH con ~/.local/bin (el de Hyprland no lo trae).
pub fn path() -> String {
    format!("{}/.local/bin:{}", home().display(), std::env::var("PATH").unwrap_or_default())
}

#[derive(Clone, Default, PartialEq)]
pub struct Estado {
    /// clave → (valor, etiqueta)
    pub v: HashMap<String, (String, String)>,
    /// clave → [(valor, etiqueta)]
    pub opciones: HashMap<String, Vec<(String, String)>>,
}

impl Estado {
    pub fn leer() -> Estado {
        let out = Command::new(qs()).arg("estado").env("PATH", path()).output();
        let text = out.map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
        let mut e = Estado::default();
        for l in text.lines() {
            let p: Vec<&str> = l.split('\t').collect();
            match p.as_slice() {
                ["opcion", k, v, lab] => e.opciones.entry(k.to_string()).or_default().push((v.to_string(), lab.to_string())),
                [k, v, lab] => {
                    e.v.insert(k.to_string(), (v.to_string(), lab.to_string()));
                }
                [k, v] => {
                    e.v.insert(k.to_string(), (v.to_string(), v.to_string()));
                }
                _ => {}
            }
        }
        e
    }

    pub fn get(&self, k: &str) -> Option<&str> {
        self.v.get(k).map(|(v, _)| v.as_str())
    }

    pub fn label(&self, k: &str) -> Option<&str> {
        self.v.get(k).map(|(_, l)| l.as_str())
    }

    pub fn es(&self) -> bool {
        self.get("lang") != Some("en")
    }
}

#[derive(Clone, PartialEq)]
pub struct LookInfo {
    pub nombre: String,
    pub about: String,
    /// colores fijos (los de paleta propia); vacío si sale del wallpaper
    pub muestra: Vec<(u8, u8, u8)>,
}

fn hex(s: &str) -> Option<(u8, u8, u8)> {
    let s = s.strip_prefix('#')?;
    let n = u32::from_str_radix(s.get(..6)?, 16).ok()?;
    Some(((n >> 16) as u8, (n >> 8) as u8, n as u8))
}

pub fn looks(es: bool) -> Vec<LookInfo> {
    let dir = home().join(".config/hypr/looks");
    let lena = Command::new(home().join(".local/bin/lena")).arg("list").output();
    let mut names: Vec<String> = lena.map(|o| String::from_utf8_lossy(&o.stdout).lines().map(str::to_string).filter(|l| !l.is_empty()).collect()).unwrap_or_default();
    if names.is_empty() {
        names = std::fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path().join("look.toml").exists())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
    }
    let lang = if es { "es" } else { "en" };
    names
        .into_iter()
        .map(|nombre| {
            let t: toml::Table = std::fs::read_to_string(dir.join(&nombre).join("look.toml")).ok().and_then(|s| s.parse().ok()).unwrap_or_default();
            let about = t.get("about").and_then(|a| a.get(lang)).and_then(|v| v.as_str()).unwrap_or("").to_string();
            let mut muestra = Vec::new();
            if let Some(c) = t.get("colors").and_then(|v| v.as_table()) {
                for k in ["background", "color4", "color12", "color11", "color9", "color2", "foreground"] {
                    if let Some(rgb) = c.get(k).and_then(|v| v.as_str()).and_then(hex) {
                        muestra.push(rgb);
                    }
                }
            }
            LookInfo { nombre, about, muestra }
        })
        .collect()
}

// ─── los renglones del panel ────────────────────────────────────────────────

#[derive(Clone, PartialEq)]
pub enum Tipo {
    /// ‹ valor › : `hacer clave valor`
    Opcion { opts: Vec<(String, String)>, cur: usize },
    /// una barra: `hacer clave n`
    Barra { min: i32, max: i32, cur: i32 },
    /// sí / no: `hacer clave`
    Interruptor(bool),
    /// algo que se abre (y el panel se cierra)
    Abre(Abre),
}

#[derive(Clone, PartialEq)]
pub enum Abre {
    /// la página de looks, dentro del panel
    Looks,
    /// `QuickSettings.sh hacer X`
    Hacer(&'static str),
    /// `QuickSettings.sh menu X` (un submenú de jale)
    Menu(&'static str),
}

#[derive(Clone, PartialEq)]
pub struct Renglon {
    pub clave: &'static str,
    pub nombre: String,
    pub tipo: Tipo,
    /// lo que se ve a la derecha si es un `Abre`
    pub valor: String,
}

pub struct Seccion {
    pub titulo: String,
    pub renglones: Vec<Renglon>,
}

fn opcion(e: &Estado, clave: &'static str, nombre: String) -> Option<Renglon> {
    let opts = e.opciones.get(clave)?.clone();
    let v = e.get(clave)?;
    let cur = opts.iter().position(|(o, _)| o == v).unwrap_or(0);
    Some(Renglon { clave, nombre, tipo: Tipo::Opcion { opts, cur }, valor: String::new() })
}

fn abre(clave: &'static str, nombre: String, a: Abre, valor: impl Into<String>) -> Renglon {
    Renglon { clave, nombre, tipo: Tipo::Abre(a), valor: valor.into() }
}

fn si(e: &Estado, k: &str) -> bool {
    e.get(k) == Some("yes")
}

/// Las secciones de las dos columnas: [Apariencia], [Sistema, Tus compus].
pub fn columnas(e: &Estado) -> [Vec<Seccion>; 2] {
    let es = e.es();
    let l = |a: &str, b: &str| if es { a.to_string() } else { b.to_string() };

    let mut ap = vec![abre("look", "look".into(), Abre::Looks, e.get("look").unwrap_or("?"))];
    ap.push(abre("wallpaper", "wallpaper".into(), Abre::Hacer("wallpaper"), l("elegir", "choose")));
    let font = e.get("font").and_then(|f| f.parse().ok()).unwrap_or(0);
    ap.push(Renglon { clave: "font", nombre: l("tamaño de letra", "font size"), tipo: Tipo::Barra { min: -2, max: 4, cur: font }, valor: String::new() });
    ap.extend(opcion(e, "blur", "blur".into()));
    if e.get("shader").is_some() {
        ap.push(Renglon { clave: "shader", nombre: "shader".into(), tipo: Tipo::Interruptor(si(e, "shader")), valor: String::new() });
    }
    ap.extend(opcion(e, "fetch", "cacha".into()));
    ap.push(abre("vis", l("visualizador", "visualizer"), Abre::Menu("vis"), e.label("vis").unwrap_or("")));
    ap.extend(opcion(e, "entrada", l("entrada", "entrance")));

    let mut sis = Vec::new();
    sis.extend(opcion(e, "lang", l("idioma", "language")));
    sis.extend(opcion(e, "notif", l("avisos", "notifications")));
    if e.get("dnd").is_some() {
        sis.push(Renglon { clave: "dnd", nombre: l("no molestar", "do not disturb"), tipo: Tipo::Interruptor(si(e, "dnd")), valor: String::new() });
    }
    sis.push(Renglon { clave: "game", nombre: "game mode".into(), tipo: Tipo::Interruptor(si(e, "game")), valor: String::new() });
    if let Some(t) = e.label("teclado") {
        sis.push(abre("teclado", l("teclado", "keyboard"), Abre::Menu("teclado"), t));
    }
    if let Some(b) = e.label("bateria") {
        sis.push(abre("bateria", l("batería", "battery"), Abre::Menu("bateria"), b));
    }

    let mut compus = Vec::new();
    if let Some(m) = e.label("machin") {
        compus.push(abre("machin", "machín".to_string(), Abre::Hacer("machin"), m));
    }
    if e.get("picha").is_some() {
        compus.push(abre("picha", "picha".into(), Abre::Hacer("picha"), l("mandar y recibir", "send and receive")));
    }
    if let Some(n) = e.get("trompo") {
        let v = match n.parse::<u32>().unwrap_or(0) {
            0 => l("emparejar el cel", "pair your phone"),
            1 => l("1 cel · emparejar otro", "1 phone · pair another"),
            n => if es { format!("{n} cels · emparejar otro") } else { format!("{n} phones · pair another") },
        };
        compus.push(abre("trompo", "trompo".into(), Abre::Hacer("trompo"), v));
    }

    let mut der = vec![Seccion { titulo: l("sistema", "system"), renglones: sis }];
    if !compus.is_empty() {
        der.push(Seccion { titulo: l("tus compus y el cel", "your computers"), renglones: compus });
    }
    [vec![Seccion { titulo: l("apariencia", "appearance"), renglones: ap }], der]
}

/// La fila de abajo: herramientas (se abren y el panel se cierra).
pub fn herramientas(e: &Estado) -> Vec<(String, Abre)> {
    let es = e.es();
    let l = |a: &str, b: &str| if es { a.to_string() } else { b.to_string() };
    vec![
        (l("grabador", "recorder"), Abre::Menu("rec")),
        (l("tu semana", "your week"), Abre::Hacer("semana")),
        (l("atajos", "shortcuts"), Abre::Hacer("atajos")),
        (l("editar config", "edit config"), Abre::Menu("edit")),
        (l("audio y red", "audio & network"), Abre::Menu("tools")),
        (l("actualizar", "update"), Abre::Hacer("update")),
    ]
}
