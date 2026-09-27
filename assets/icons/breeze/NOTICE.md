# Breeze-Chameleon icons in Dunit

The `*.rgba` files in this directory are 32×32, straight-alpha, `R,G,B,A`
byte-order rasterisations of icons from the **Breeze-Chameleon-Icons** theme
(the "Breeze Chameleon Dark" variant), used for the DWM dock/launcher and,
later, the file manager.

- Upstream: https://github.com/L4ki/Breeze-Chameleon-Icons
- Original theme: Breeze Icon Theme by the KDE Visual Design Group,
  modified by l4k1 (see AUTHORS upstream).
- License: GNU GPL v3 — full text in `LICENSE`.

Source SVG → RGBA mapping (all from `Breeze Chameleon Dark/`):

| RGBA asset          | app        | source SVG                              |
|---------------------|------------|-----------------------------------------|
| `gui_client.rgba`   | gui_client | `categories/32/applications-other.svg`  |
| `gui_calc.rgba`     | gui_calc   | `apps/48/accessories-calculator.svg`    |
| `gui_stat.rgba`     | gui_stat   | `apps/48/utilities-system-monitor.svg`  |
| `gui_files.rgba`    | gui_files  | `apps/64/system-file-manager.svg`       |
| `gui_terminal.rgba` | gui_terminal | `apps/64/utilities-terminal.svg`      |

Rasterised with `rsvg-convert -w 32 -h 32` then `magick … RGBA:`. The raw form
(no header, 4096 bytes = 32·32·4) matches what the compositor loads at startup
via the VFS and blits with per-pixel alpha over the dock buttons and menu rows.
