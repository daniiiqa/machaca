# machaca

El panel de [discada](https://github.com/daniiiqa/dotfiles) (SUPER+Shift+E): todo lo
que se ajusta del rice en un solo lugar, ordenado y con el look de ahora.

```
arriba        el wallpaper y el look (clic: cambiar de look)
Apariencia    look, wallpaper, tamaño de letra, blur, shader, cacha,
              visualizador, entrada
Sistema       idioma, avisos, no molestar, game mode, teclado, batería
Tus compus    machín, picha, trompo
abajo         grabador, tu semana, atajos, editar config, audio y red, actualizar
```

- ↑↓ / jk elegir · ←→ / hl cambiar el valor · Enter abrir · Esc cerrar
- clic en ‹ ›, o la rueda del mouse sobre un renglón, también cambian el valor
- `machaca` otra vez, o un clic fuera del panel, lo cierra
- la página de looks enseña la paleta de los looks de colores fijos; Enter lo pone

Es solo la cara: lo que se lee y se cambia lo hace `QuickSettings.sh` de discada
(`estado`, `hacer CLAVE [VALOR]`, `menu NOMBRE`), así la lógica no se duplica y el
menú viejo de jale sigue funcionando (`QuickSettings.sh lista`).

Se dibuja en celdas, como rola o vato, y [disco](https://github.com/daniiiqa/disco)
las pinta con los colores, la fuente, los bordes y el estilo del look. Si el look es
translúcido pero no tiene blur, el panel va opaco para que se lea.

## Instalar

```sh
cargo install --path . --root ~/.local
```

Necesita Hyprland (layer-shell), `magick` (la miniatura del wallpaper) y discada.

Para probar sin teclado: `MACHACA_TECLAS="Tab Down Return" machaca` (una tecla cada
0.6 s: Up, Down, Left, Right, Tab, Return, Escape).
