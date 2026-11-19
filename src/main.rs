//! machaca: el panel de discada (SUPER+Shift+E). Todo lo que se ajusta del
//! rice en un solo lugar, ordenado y con el look de ahora:
//!
//!   arriba        el wallpaper y el look (clic: cambiar de look)
//!   Apariencia    look, wallpaper, tamaño de letra, blur, shader, cacha,
//!                 visualizador, entrada
//!   Sistema       idioma, avisos, no molestar, game mode, teclado, batería
//!   Tus compus    machín, picha, trompo
//!   abajo         grabador, tu semana, atajos, editar config, audio y red,
//!                 actualizar
//!
//!   ↑↓ / jk elegir · ←→ / hl cambiar el valor · Enter abrir · Esc cerrar
//!   clic en ‹ › o la rueda del mouse también cambian el valor
//!   `machaca` otra vez (o clic fuera) cierra
//!
//! Lo que se lee y se cambia lo hace `QuickSettings.sh estado|hacer|menu`:
//! machaca solo es la cara. Colores, fuente y bordes salen del look (disco).

mod app;
mod estado;
mod ui;

use std::path::PathBuf;

fn pidfile() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR").map_or_else(std::env::temp_dir, PathBuf::from).join("machaca.pid")
}

fn main() {
    if matches!(std::env::args().nth(1).as_deref(), Some("-V" | "--version")) {
        println!("machaca {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    // otra vez `machaca` con uno abierto: que se cierre (el atajo de ida y vuelta)
    if let Some(pid) = std::fs::read_to_string(pidfile()).ok().and_then(|p| p.trim().parse::<i32>().ok())
        && unsafe { libc::kill(pid, 0) } == 0
        && std::fs::read_to_string(format!("/proc/{pid}/comm")).is_ok_and(|c| c.trim() == "machaca")
    {
        unsafe { libc::kill(pid, libc::SIGUSR1) };
        return;
    }
    let sigs = unsafe {
        let mut s: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut s);
        libc::sigaddset(&mut s, libc::SIGUSR1);
        libc::pthread_sigmask(libc::SIG_BLOCK, &s, std::ptr::null_mut());
        s
    };
    let _ = std::fs::write(pidfile(), std::process::id().to_string());
    let r = app::run(sigs);
    let _ = std::fs::remove_file(pidfile());
    if let Err(e) = r {
        eprintln!("machaca: {e}");
        std::process::exit(1);
    }
}
