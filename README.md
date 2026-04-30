# gmenu

A centered, dmenu-like launcher for X11 written in Rust. Reads newline-separated
items on stdin, lets you filter them with a substring search, and prints the
selection to stdout.

- Centered floating popup (override-redirect) on the monitor under the pointer.
- Substring matching, case-insensitive by default; matched spans are highlighted.
- Designed to stay snappy on extremely large lists (parallel filtering with
  incremental refinement; only visible rows are drawn).
- Pango/Cairo rendering — any font that fontconfig can find works.
- Styling via a config file at `~/.config/gmenu/config.toml`.

## Build

Requires a Rust toolchain and the system libraries `libx11`/`libxcb`, `cairo`,
`pango`, and `pangocairo`. On Debian/Ubuntu:

```sh
sudo apt install libxcb1-dev libcairo2-dev libpango1.0-dev
cargo build --release
```

## Install

```sh
make install                # copies to ~/.local/bin/gmenu
make install PREFIX=/usr/local   # or anywhere else
```

## Use

```sh
ls /usr/bin | gmenu                         # pick a binary
gmenu --prompt "run> " < ~/recipes.txt      # prompt + custom input
seq 1 5000000 | gmenu                       # 5M items, still responsive
```

Selected line is written to stdout. Pressing Enter with no match prints the
query verbatim. Escape exits with status 1.

### CLI flags

Per-invocation only — styling lives in the config file.

| Flag | Default | Description |
| --- | --- | --- |
| `-p`, `--prompt TEXT` | `""` | Text shown before the input field |
| `-s`, `--case-sensitive` | off | Case-sensitive substring matching |
| `--max-matches N` | `100000` | Cap on matches retained for filtering |

### Keys

| Key | Action |
| --- | --- |
| `Enter` | Print selection and exit 0 (or query if no match) |
| `Escape` | Exit 1 |
| `↑` / `↓`, `Ctrl+K` / `Ctrl+J` | Move selection |
| `Tab` | Move selection down |
| `PgUp` / `PgDn` | Page |
| `Home` / `End` | Jump to first / last match |
| `Backspace` | Delete char (`Ctrl+Backspace` or `Ctrl+W` = word) |
| `Ctrl+L` | Clear query |

## Configuration

Copy [`examples/config.toml`](examples/config.toml) to
`~/.config/gmenu/config.toml` and edit. All keys are optional.

```toml
font = "PragmataPro 12"

bg      = "#222222"
fg      = "#dddddd"
selbg   = "#005577"
selfg   = "#ffffff"
matchbg = "#3b3b00"
matchfg = "#ffff66"

border       = "#888888"
border_width = 2

width  = 700
height = 420

padding_x     = 10  # left/right inset for content
padding_y     = 6   # top/bottom inset for content
row_padding_y = 6   # extra vertical space per item row
```

The font value is a Pango font description (`<Family> [Style] <Size>`). Find
installed family names with `fc-list | grep -i <name>`.

## License

MIT — see [LICENSE](LICENSE).
