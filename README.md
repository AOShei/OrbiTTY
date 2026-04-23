# OrbiTTY

Workspace-based, tiling GTK4 terminal emulator for Linux.

OrbiTTY gives you a **Mission Control** view of your terminal sessions: up to 4 active terminals tiled in a central arena, with the rest visible as live previews in a monitoring sidebar. Switch contexts instantly — sessions are never restarted, just reparented between views.

## Features

- **Workspace tabs** — Each tab is an independent workspace with its own arena + sidebar layout
- **Tiling arena** — Auto-tiles 1–4 promoted sessions (full, 50/50, 1+2, 2×2)
- **Monitoring sidebar** — Demoted sessions stay visible as preview cards with status pips
- **Promote/demote** — One-click or keyboard swap between arena and sidebar
- **Workspace templates** — Presets (Empty, Fullstack, Microservices, Dev) to bootstrap common layouts
- **Session cloning** — Spawn a new shell at the same working directory
- **Zoom** — Global font scaling across all sessions
- **Theme switcher** — System / Light / Dark via libadwaita

## Requirements

- GTK 4.14+
- libadwaita 1.5+
- VTE for GTK4 (`vte-2.91-gtk4`)
- Rust 2021 edition

### Installing dependencies

**Fedora:**
```sh
sudo dnf install gtk4-devel libadwaita-devel vte291-gtk4-devel
```

**Ubuntu/Debian (24.04+):**
```sh
sudo apt install libgtk-4-dev libadwaita-1-dev libvte-2.91-gtk4-dev
```

**Arch:**
```sh
sudo pacman -S gtk4 libadwaita vte4
```

## Building

```sh
cargo build            # debug
cargo build --release  # optimized (thin LTO, stripped)
```

## Running

```sh
cargo run
# or after building:
target/debug/orbit
```

## Keyboard Shortcuts

| Shortcut | Action |
|---|---|
| `Ctrl+T` | New workspace |
| `Ctrl+W` | Close workspace |
| `F2` | Rename workspace |
| `Ctrl+Shift+Return` | New shell in current workspace |
| `Ctrl+Shift+E` | Toggle split orientation |
| `Ctrl+Shift+O` | Show all tabs (overview) |
| `Ctrl+Shift+N` | New window |
| `Ctrl+Shift+F11` | Fullscreen |
| `Alt+1…9` | Focus session by index |
| `Ctrl+Shift+D` | Send focused arena session to the monitoring dock |
| `Alt+Space` | Peek the most relevant docked session |
| `Ctrl+Tab` / `Ctrl+Shift+Tab` | Cycle arena focus forward / backward |
| `Ctrl++` / `Ctrl+-` / `Ctrl+0` | Zoom in / out / reset |
| `Ctrl+Q` | Quit |

## License

MIT
