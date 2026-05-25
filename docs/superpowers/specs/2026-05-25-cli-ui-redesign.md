# NexaNet CLI UI/UX Redesign — Design Spec

## Goal

Modernize the nexa CLI with a btop-style live TUI dashboard (`nexa top`) and polished box-panel output for all existing commands. Nerd/hacker aesthetic — GitHub Dark colors, Unicode box-drawing, Nerd Font icons with fallback.

## Architecture

Two rendering paths in one CLI, sharing a color/icon palette:

1. **One-shot panel renderer** (`src/output/`) — commands like `nexa status`, `nexa pods`, `nexa deploy` print box-bordered panels to stdout using `console` crate. No ratatui. Output remains pipeable, `--json` mode unchanged.

2. **TUI app** (`src/tui/`) — `nexa top` launches a ratatui alternate-screen app with live refresh, keyboard navigation, and basic pod actions.

```
src/
├── output/              # One-shot rendering (existing, enhanced)
│   ├── mod.rs           # JSON mode toggle (unchanged)
│   ├── style.rs         # Shared palette, icons, Nerd Font fallback
│   ├── panel.rs         # NEW — PanelBuilder (bordered box with title/content/footer)
│   ├── table.rs         # REFACTORED — renders inside panels, dim headers, row separators, ● status icons
│   ├── spinner.rs       # Unchanged (indicatif)
│   ├── deploy.rs        # NEW — Deploy progress panel (steps + timing footer)
│   └── age.rs           # Unchanged
├── tui/                 # NEW — nexa top
│   ├── mod.rs           # Entry point: setup terminal, run event loop, restore
│   ├── app.rs           # App state (active panel, pod/node/event data, cursor position)
│   ├── event.rs         # Event loop: crossterm key events + 2s API refresh ticker + SSE events
│   ├── ui.rs            # ratatui rendering: layout constraints, draw each zone
│   ├── actions.rs       # Keyboard action dispatch → HTTP client calls
│   └── widgets/         # Custom ratatui widgets
│       ├── pod_table.rs
│       ├── node_gauge.rs
│       └── event_list.rs
├── commands/            # Existing command handlers (updated to use PanelBuilder)
└── client.rs            # HTTP client (existing, reused by TUI)
```

## Section 1: Shared Style System (`output/style.rs`)

### Color palette (GitHub Dark)

| Role | Hex | Usage |
|------|-----|-------|
| bg | `#0d1117` | Terminal background (TUI only) |
| surface | `#161b22` | Panel headers, footers |
| border | `#30363d` | Box borders, separators |
| border-subtle | `#21262d` | Row separators within tables |
| text | `#e6edf3` | Primary text |
| text-secondary | `#8b949e` | Labels, column headers, timestamps |
| text-muted | `#484f58` | Disabled, placeholder |
| accent | `#58a6ff` | Panel titles, active elements |
| green | `#3fb950` | Running, success, healthy (0-60% gauge) |
| yellow | `#d29922` | Warning, pending, restarting (60-80% gauge) |
| red | `#f85149` | Error, failed, OOM (80%+ gauge) |
| cyan | `#56d4dd` | Creating, pulling |

### Icons

Two modes controlled by `NEXA_ICONS` env var (default: `unicode`):

| Semantic | Nerd Font | Unicode fallback |
|----------|-----------|------------------|
| cluster | `` | `⊞` |
| node | `󰐻` | `◆` |
| pod | `` | `●` |
| deploy | `◎` | `◎` |
| event | `` | `•` |
| success | `✓` | `✓` |
| error | `✗` | `✗` |
| warning | `⚠` | `⚠` |
| running | `⏻` | `⏻` |

### Status dot colors

- `● running` → green
- `● pending` / `● creating` → cyan
- `● restarting` / `● degraded` → yellow
- `● failed` / `● crashloopbackoff` → red
- `● stopped` / `● stopping` → text-muted

## Section 2: PanelBuilder (`output/panel.rs`)

API:

```rust
Panel::new("  Pods")          // Title with icon
    .count("3 total")          // Right-aligned in header
    .table(&headers, &rows)    // Table content
    .render();                 // Print to stdout

Panel::new(" Cluster Status")
    .kv(&[
        ("Mode", "single-node"),
        ("Status", "● running"),         // pre-colored
        ("Projects", "3"),
        ("Deployments", "4 running · 1 stopped"),
        ("Pods", "11 running · 1 restarting"),
    ])
    .render();

Panel::new("  Deploying")
    .subtitle("web-api")
    .steps(&[
        ("✓", "Image", "nginx:latest pulled"),
        ("✓", "Pod", "web-api-a1b2c running on node-1"),
        ("✓", "Route", "web-api.localhost → :8080"),
    ])
    .footer_left("● Deployed")
    .footer_right("2/2 pods ready · 1.4s")
    .render();
```

Box drawing uses single-line characters: `┌─┐│└─┘`. Panel header has `surface` background (achieved via `console` crate `Style::new().on_color256()`). Row separators use `border-subtle` color.

In JSON mode (`--json`), `Panel::render()` is a no-op — commands output JSON directly as they do today.

Terminal width detection via `console::Term::stdout().size()` for responsive column widths.

## Section 3: Command Output Changes

### Commands that get panels (list/detail views):

| Command | Panel title | Content type | Footer |
|---------|------------|--------------|--------|
| `nexa status` | ` Cluster Status` | key-value rows | — |
| `nexa pods` | `  Pods` | table (name, project, deployment, status, image, age) | total count |
| `nexa deployments` | ` Deployments` | table (name, project, status, replicas, image, age) | total count |
| `nexa nodes` | `󰐻 Nodes` | table + inline CPU/MEM gauge bars | total count |
| `nexa deploy FILE` | `  Deploying` | progressive steps | status + timing |
| `nexa project list` | ` Projects` | table (name, age) | total count |
| `nexa secret list` | ` Secrets` | table (name, project) | total count |
| `nexa routes` | ` Routes` | table (domain, project, deployment, tls, created) | total count |

### Commands that stay simple (action confirmations):

These print a single-line message with icon — no panel:

- `nexa stop NAME` → `✓ Deployment 'NAME' stopped`
- `nexa rm NAME` → `✓ Deployment 'NAME' removed`
- `nexa scale NAME N` → `✓ web-api: 2 → 5 replicas`
- `nexa secret set/rm` → `✓ Secret 'NAME' set in project 'PROJECT'`
- `nexa route add/rm` → `✓ Route 'DOMAIN' → project/deployment`
- `nexa project create/suspend/resume/delete` → `✓ Project 'NAME' created`

### `nexa logs` enhancement:

Replace raw `data: ...` lines with formatted output:

```
12:03:41 web-api-a1b2c │ GET /health 200 1ms
12:03:42 web-api-a1b2c │ GET /api/v1/pods 200 12ms
```

Timestamp in `text-secondary`, pod name in `accent`, separator `│` in `border`, log content in `text`.

### Gauge bars in `nexa nodes`:

Inline in the table for CPU and MEM columns:

```
████████░░░░░░░░ 45%
```

Bar characters: `█` (filled) and `░` (empty). Color follows the threshold rules (green/yellow/red).

## Section 4: `nexa top` — TUI Dashboard

### Entry and exit

`nexa top` enters crossterm raw mode + alternate screen. On `q` or `Esc`, restores the terminal. Panic hook restores terminal on crash.

### Layout

```
┌─────────────────────────────────────────────────────────────────────┐
│  NexaNet          ⏻ running  │ 󰐻 3 nodes │  12 pods │ ◎ 5 deploys│  Row 0: status bar (3 lines)
├──────────────────────────────────┬──────────────────────────────────┤
│   Pods                          │ 󰐻  Nodes                        │
│  NAME         STATUS   CPU  MEM │  node-1  cpu ████░░░░ 38%       │  Row 1: split (flex)
│  web-api      ● run    12% 128M │          mem █████░░░ 72%       │
│  worker       ● run    45% 512M │  node-2  cpu ██░░░░░░ 21%       │
│> cron-job     ● fail    —    —  │          mem ███░░░░░ 45%       │  (> = cursor)
├─────────────────────────────────────────────────────────────────────┤
│   Events                                                           │
│  12:03  Pod  web-api-7f8d9 started                                 │  Row 2: events (25%)
│  12:01  Pod  db-migrate-x9z2 pending                               │
│  11:58  Pod  cron-job-p4q7 OOMKilled                               │
├─────────────────────────────────────────────────────────────────────┤
│ q quit  ? help  Tab panel  ↑↓ nav  l logs  d delete  s scale  / ▌ │  Row 3: keybinds (1 line)
└─────────────────────────────────────────────────────────────────────┘
```

ratatui `Layout::default().constraints([Length(3), Min(10), Percentage(25), Length(1)])` for the 4 rows. Row 1 splits horizontally: `[Percentage(60), Percentage(40)]`.

### Data refresh

- **Ticker**: tokio interval every 2 seconds polls `GET /api/v1/pods`, `GET /api/v1/nodes`, `GET /api/v1/deployments`
- **Events**: SSE stream on `GET /api/v1/events` (persistent connection, parsed as they arrive)
- **Render**: re-draw on every tick or incoming event

If nexad is unreachable, the status bar shows `● disconnected` in red and retries every 5 seconds.

### Keyboard navigation

| Key | Action |
|-----|--------|
| `Tab` / `Shift+Tab` | Cycle active panel: pods → nodes → events |
| `↑` `↓` / `j` `k` | Move cursor within active panel |
| `Enter` / `l` | (on pod) Stream logs full-screen. `Esc`/`q` returns to dashboard |
| `d` | (on pod) Delete pod — shows confirmation `Delete pod-name? [y/N]` in footer |
| `s` | (on pod's deployment) Scale — prompt `Replicas:` in footer, type number + Enter |
| `/` | Filter mode — type text to filter active panel, `Esc` clears filter |
| `q` / `Esc` | Quit (when not in sub-view or filter) |
| `?` | Toggle help overlay |

Active panel border changes from `border` to `accent` color.

### Log sub-view

Pressing `l` on a pod enters full-screen log streaming:

```
┌─  Logs — web-api-7f8d9 ────────────────────────────────────────────┐
│ 12:03:41 │ GET /health 200 1ms                                     │
│ 12:03:42 │ GET /api/v1/pods 200 12ms                               │
│ 12:03:43 │ POST /api/v1/deploy 201 143ms                           │
│ ...                                                                 │
├─────────────────────────────────────────────────────────────────────┤
│ q back  / filter  G bottom  g top                                   │
└─────────────────────────────────────────────────────────────────────┘
```

Auto-scrolls to bottom. `G` jumps to bottom, `g` to top. `/` filters log lines.

## Section 5: nexad API additions

Two new endpoints needed for `nexa top`:

### `GET /api/v1/nodes`

Returns node list with resource usage. Uses the `sysinfo` crate (already a dependency) for CPU/memory stats on the local node. Multi-node clusters aggregate via gRPC from workers.

```json
[
  {
    "name": "node-1",
    "role": "master",
    "status": "ready",
    "address": "192.168.1.10",
    "cpu_cores": 8,
    "cpu_usage_percent": 38.2,
    "memory_total_bytes": 17179869184,
    "memory_used_bytes": 12348030976,
    "pod_count": 8,
    "age": "2026-05-20T10:00:00Z"
  }
]
```

### `GET /api/v1/events` (SSE)

Server-sent event stream of cluster events. Backed by the container event watcher (observability work). Each event:

```json
{
  "timestamp": "2026-05-25T12:03:41Z",
  "kind": "pod",
  "name": "web-api-7f8d9",
  "action": "started",
  "message": "Pod web-api-7f8d9 started on node-1"
}
```

Kinds: `pod`, `deployment`, `scale`, `node`. The existing event watcher records `die`, `oom`, `start` from Docker — this endpoint exposes those plus orchestrator-level events (deploy, scale).

## Section 6: Dependencies

### New in nexa-cli:

```toml
ratatui = "0.29"
crossterm = "0.28"
```

### No changes to:

- `console` (0.15) — still used for one-shot panel rendering
- `indicatif` (0.17) — still used for spinners
- `dialoguer` (0.11) — still used for `nexa init`
- `clap` (4) — adds `top` subcommand

### nexad:

No new dependencies. `sysinfo` (0.32) is already present. New handler code in `src/api/handlers.rs` for `/api/v1/nodes` and `/api/v1/events`.

## Out of Scope

- Theme/config file — colors are hardcoded (GitHub Dark)
- Mouse support in `nexa top`
- Sparklines / historical charts
- Plugin system
- Customizable keybindings
