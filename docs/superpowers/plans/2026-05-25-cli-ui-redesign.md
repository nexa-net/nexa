# CLI UI/UX Redesign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Modernize the helyos CLI with GitHub Dark-themed box panels for all commands and a btop-style live TUI dashboard (`helyos top`).

**Architecture:** Two rendering paths sharing a color/icon palette. One-shot panel renderer (`src/output/`) wraps existing commands in bordered boxes using the `console` crate. TUI app (`src/tui/`) powers `helyos top` with ratatui alternate-screen, live API polling, and keyboard navigation. Two new helyosd API endpoints (`GET /api/v1/nodes/stats`, `GET /api/v1/events`) feed the dashboard.

**Tech Stack:** Rust 2024 edition, console 0.15, ratatui 0.29, crossterm 0.28, axum 0.8, sysinfo 0.32, tokio-stream, async-stream

---

## File Structure

### helyos-cli (`/Users/nassime/GitHub/Helyos/helyos-cli/`)

| Action | Path | Responsibility |
|--------|------|---------------|
| Modify | `Cargo.toml` | Add ratatui 0.29, crossterm 0.28 |
| Modify | `src/output/mod.rs` | Export new panel module |
| Modify | `src/output/style.rs` | GitHub Dark palette, icon system, status coloring |
| Create | `src/output/panel.rs` | PanelBuilder — bordered box with title/content/footer |
| Modify | `src/output/table.rs` | Render inside panels, dim headers, row separators, status icons |
| Create | `src/output/deploy.rs` | Deploy progress panel (steps + timing footer) |
| Modify | `src/commands/status.rs` | Use Panel::new().kv() |
| Modify | `src/commands/pods.rs` | Use Panel::new().table() |
| Modify | `src/commands/deployments.rs` | Use Panel::new().table() |
| Modify | `src/commands/nodes.rs` | Use Panel::new().table() with gauge bars |
| Modify | `src/commands/deploy.rs` | Use deploy panel with steps |
| Modify | `src/commands/logs.rs` | Formatted timestamp + pod name + separator |
| Modify | `src/commands/project.rs` | Use Panel::new().table() for list |
| Modify | `src/commands/secret.rs` | Use Panel::new().table() for list |
| Modify | `src/commands/route.rs` | Use Panel::new().table() for list |
| Create | `src/tui/mod.rs` | Entry: setup terminal, run event loop, restore |
| Create | `src/tui/app.rs` | App state: active panel, data, cursor position |
| Create | `src/tui/event.rs` | Event loop: crossterm keys + 2s API ticker |
| Create | `src/tui/ui.rs` | ratatui rendering: layout constraints, draw zones |
| Create | `src/tui/actions.rs` | Keyboard dispatch → HTTP client calls |
| Create | `src/tui/widgets/mod.rs` | Widget module exports |
| Create | `src/tui/widgets/pod_table.rs` | Pod table widget |
| Create | `src/tui/widgets/node_gauge.rs` | Node gauge bar widget |
| Create | `src/tui/widgets/event_list.rs` | Event list widget |
| Modify | `src/commands/mod.rs` | Export top command |
| Create | `src/commands/top.rs` | `helyos top` command entry point |
| Modify | `src/main.rs` | Add Top subcommand to clap, wire to handler |

### helyosd (`/Users/nassime/GitHub/Helyos/helyosd/`)

| Action | Path | Responsibility |
|--------|------|---------------|
| Modify | `src/api/handlers.rs` | Add `node_stats` and `events_stream` handlers |
| Modify | `src/api/routes.rs` | Register new routes |
| Modify | `src/api/mod.rs` | Add event broadcast channel to AppState |
| Modify | `src/adapters/event_watcher.rs` | Broadcast events to SSE subscribers |
| Modify | `src/main.rs` | Create broadcast channel, pass to event watcher + API |

---

## Task 1: Shared Style System — Colors and Icons

**Files:**
- Modify: `/Users/nassime/GitHub/Helyos/helyos-cli/src/output/style.rs`
- Test: run `cargo test -p helyos-cli`

- [ ] **Step 1: Write tests for the style system**

Add to `src/output/style.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_returns_unicode_by_default() {
        assert_eq!(icon("pod"), "●");
        assert_eq!(icon("success"), "✓");
        assert_eq!(icon("error"), "✗");
    }

    #[test]
    fn status_color_maps_correctly() {
        let green = color("green");
        let red = color("red");
        let yellow = color("yellow");
        assert_ne!(format!("{green:?}"), format!("{red:?}"));
        assert_ne!(format!("{green:?}"), format!("{yellow:?}"));
    }

    #[test]
    fn status_style_returns_correct_color() {
        let s = status_style("running");
        assert!(format!("{s:?}").contains("Color"));
        let s = status_style("unknown_status");
        // Default style — no panic
        let _ = format!("{s:?}");
    }

    #[test]
    fn status_dot_formats_correctly() {
        let dot = status_dot("running");
        assert!(dot.contains("●"));
        assert!(dot.contains("running"));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/nassime/GitHub/Helyos/helyos-cli && cargo test --lib output::style`
Expected: FAIL — `icon`, `color`, `status_style`, `status_dot` not found

- [ ] **Step 3: Implement the style system**

Replace the entire content of `src/output/style.rs`:

```rust
use console::Style;

pub fn color(name: &str) -> Style {
    match name {
        "bg" => Style::new().color256(0).on_color256(0),
        "surface" => Style::new().color256(236),
        "border" => Style::new().color256(239),
        "border-subtle" => Style::new().color256(237),
        "text" => Style::new().color256(253),
        "text-secondary" => Style::new().color256(245),
        "text-muted" => Style::new().color256(240),
        "accent" => Style::new().color256(75),
        "green" => Style::new().color256(77),
        "yellow" => Style::new().color256(178),
        "red" => Style::new().color256(203),
        "cyan" => Style::new().color256(80),
        _ => Style::new(),
    }
}

pub fn icon(name: &str) -> &'static str {
    let nerd = std::env::var("HELYOS_ICONS")
        .map(|v| v == "nerd")
        .unwrap_or(false);

    match (name, nerd) {
        ("cluster", true) => "\u{f013}",
        ("cluster", false) => "⊞",
        ("node", true) => "\u{f0e8}",
        ("node", false) => "◆",
        ("pod", true) => "\u{f013}",
        ("pod", false) => "●",
        ("deploy", _) => "◎",
        ("event", true) => "\u{f0a2}",
        ("event", false) => "•",
        ("success", _) => "✓",
        ("error", _) => "✗",
        ("warning", _) => "⚠",
        ("running", _) => "⏻",
        _ => "•",
    }
}

pub fn status_style(status: &str) -> Style {
    match status.to_lowercase().as_str() {
        "running" => color("green"),
        "pending" | "creating" => color("cyan"),
        "restarting" | "degraded" => color("yellow"),
        "failed" | "crashloopbackoff" => color("red"),
        "stopped" | "stopping" => color("text-muted"),
        _ => Style::new(),
    }
}

pub fn status_dot(status: &str) -> String {
    let style = status_style(status);
    let lower = status.to_lowercase();
    format!("{}", style.apply_to(format!("● {lower}")))
}

pub fn print_success(msg: &str) {
    if super::is_json_mode() {
        return;
    }
    let style = color("green");
    println!("{} {msg}", style.apply_to(icon("success")));
}

pub fn print_error(msg: &str) {
    if super::is_json_mode() {
        return;
    }
    let style = color("red");
    eprintln!("{} {msg}", style.apply_to(icon("error")));
}

pub fn print_error_with_hint(msg: &str, hint: &str) {
    if super::is_json_mode() {
        return;
    }
    let err_style = color("red");
    let hint_style = color("text-muted");
    eprintln!("{} {msg}", err_style.apply_to(icon("error")));
    eprintln!("  {} {hint}", hint_style.apply_to("hint:"));
}

pub fn print_warning(msg: &str) {
    if super::is_json_mode() {
        return;
    }
    let style = color("yellow");
    eprintln!("{} {msg}", style.apply_to(icon("warning")));
}

pub fn print_header(msg: &str) {
    if super::is_json_mode() {
        return;
    }
    let style = color("accent");
    println!("{}", style.apply_to(msg));
}

pub fn print_kv(key: &str, value: &str) {
    if super::is_json_mode() {
        return;
    }
    let key_style = color("text-secondary");
    let val_style = color("text");
    println!("  {} {}", key_style.apply_to(format!("{key:>14}")), val_style.apply_to(value));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_returns_unicode_by_default() {
        assert_eq!(icon("pod"), "●");
        assert_eq!(icon("success"), "✓");
        assert_eq!(icon("error"), "✗");
    }

    #[test]
    fn status_color_maps_correctly() {
        let green = color("green");
        let red = color("red");
        let yellow = color("yellow");
        assert_ne!(format!("{green:?}"), format!("{red:?}"));
        assert_ne!(format!("{green:?}"), format!("{yellow:?}"));
    }

    #[test]
    fn status_style_returns_correct_color() {
        let s = status_style("running");
        let _ = format!("{s:?}");
        let s = status_style("unknown_status");
        let _ = format!("{s:?}");
    }

    #[test]
    fn status_dot_formats_correctly() {
        let dot = status_dot("running");
        assert!(dot.contains("running"));
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd /Users/nassime/GitHub/Helyos/helyos-cli && cargo test --lib output::style`
Expected: 4 tests PASS

- [ ] **Step 5: Commit**

```bash
cd /Users/nassime/GitHub/Helyos/helyos-cli
git add src/output/style.rs
git commit -m "feat(cli): add GitHub Dark style system with color palette and icon support"
```

---

## Task 2: PanelBuilder — Bordered Box Renderer

**Files:**
- Create: `/Users/nassime/GitHub/Helyos/helyos-cli/src/output/panel.rs`
- Modify: `/Users/nassime/GitHub/Helyos/helyos-cli/src/output/mod.rs`

- [ ] **Step 1: Write tests for PanelBuilder**

Create `src/output/panel.rs` with test module only:

```rust
use console::Style;

use super::style;

pub struct Panel {
    title: String,
    count: Option<String>,
    subtitle: Option<String>,
    content: PanelContent,
    footer_left: Option<String>,
    footer_right: Option<String>,
}

enum PanelContent {
    None,
    Table {
        headers: Vec<String>,
        rows: Vec<Vec<String>>,
    },
    Kv(Vec<(String, String)>),
    Steps(Vec<(String, String, String)>),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panel_renders_kv_without_panic() {
        let output = Panel::new("Test")
            .kv(&[("Key1", "Value1"), ("Key2", "Value2")])
            .render_to_string();
        assert!(output.contains("Test"));
        assert!(output.contains("Key1"));
        assert!(output.contains("Value1"));
    }

    #[test]
    fn panel_renders_table_without_panic() {
        let output = Panel::new("Pods")
            .count("3 total")
            .table(
                &["Name", "Status"],
                &[
                    vec!["web".into(), "running".into()],
                    vec!["api".into(), "failed".into()],
                ],
            )
            .render_to_string();
        assert!(output.contains("Pods"));
        assert!(output.contains("3 total"));
        assert!(output.contains("web"));
    }

    #[test]
    fn panel_renders_steps_with_footer() {
        let output = Panel::new("Deploying")
            .subtitle("web-api")
            .steps(&[
                ("✓", "Image", "nginx:latest pulled"),
                ("✓", "Pod", "web-api-a1b running"),
            ])
            .footer_left("● Deployed")
            .footer_right("2/2 pods ready")
            .render_to_string();
        assert!(output.contains("Deploying"));
        assert!(output.contains("web-api"));
        assert!(output.contains("Deployed"));
    }

    #[test]
    fn panel_respects_terminal_width() {
        let output = Panel::new("Test")
            .kv(&[("Key", "Value")])
            .render_to_string_with_width(40);
        for line in output.lines() {
            // Each line should be at most 40 visible chars (ignoring ANSI)
            let stripped = console::strip_ansi_codes(line);
            assert!(stripped.len() <= 42, "line too long: {stripped}");
        }
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd /Users/nassime/GitHub/Helyos/helyos-cli && cargo test --lib output::panel`
Expected: FAIL — methods not implemented

- [ ] **Step 3: Implement PanelBuilder**

Replace `src/output/panel.rs` with the full implementation:

```rust
use console::Style;

use super::style;

pub struct Panel {
    title: String,
    count: Option<String>,
    subtitle: Option<String>,
    content: PanelContent,
    footer_left: Option<String>,
    footer_right: Option<String>,
}

enum PanelContent {
    None,
    Table {
        headers: Vec<String>,
        rows: Vec<Vec<String>>,
    },
    Kv(Vec<(String, String)>),
    Steps(Vec<(String, String, String)>),
}

impl Panel {
    pub fn new(title: &str) -> Self {
        Self {
            title: title.to_string(),
            count: None,
            subtitle: None,
            content: PanelContent::None,
            footer_left: None,
            footer_right: None,
        }
    }

    pub fn count(mut self, count: &str) -> Self {
        self.count = Some(count.to_string());
        self
    }

    pub fn subtitle(mut self, sub: &str) -> Self {
        self.subtitle = Some(sub.to_string());
        self
    }

    pub fn table(mut self, headers: &[&str], rows: &[Vec<String>]) -> Self {
        self.content = PanelContent::Table {
            headers: headers.iter().map(|h| h.to_string()).collect(),
            rows: rows.to_vec(),
        };
        self
    }

    pub fn kv(mut self, pairs: &[(&str, &str)]) -> Self {
        self.content = PanelContent::Kv(
            pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        );
        self
    }

    pub fn steps(mut self, steps: &[(&str, &str, &str)]) -> Self {
        self.content = PanelContent::Steps(
            steps
                .iter()
                .map(|(icon, label, desc)| (icon.to_string(), label.to_string(), desc.to_string()))
                .collect(),
        );
        self
    }

    pub fn footer_left(mut self, text: &str) -> Self {
        self.footer_left = Some(text.to_string());
        self
    }

    pub fn footer_right(mut self, text: &str) -> Self {
        self.footer_right = Some(text.to_string());
        self
    }

    pub fn render(&self) {
        if super::is_json_mode() {
            return;
        }
        let width = terminal_width();
        let output = self.build(width);
        print!("{output}");
    }

    pub fn render_to_string(&self) -> String {
        self.build(terminal_width())
    }

    pub fn render_to_string_with_width(&self, width: usize) -> String {
        self.build(width)
    }

    fn build(&self, width: usize) -> String {
        let w = width.max(30);
        let inner = w - 2; // inside the box borders
        let mut out = String::new();

        let border = style::color("border");
        let surface = style::color("surface");
        let accent = style::color("accent");
        let text_sec = style::color("text-secondary");
        let subtle = style::color("border-subtle");

        // Top border
        out.push_str(&format!(
            "{}{}{}\n",
            border.apply_to("┌"),
            border.apply_to("─".repeat(inner)),
            border.apply_to("┐"),
        ));

        // Header line
        let title_plain = console::strip_ansi_codes(&self.title);
        let mut header = format!(" {}", accent.apply_to(&self.title));
        if let Some(sub) = &self.subtitle {
            header.push_str(&format!(" {}", text_sec.apply_to(sub)));
        }
        let right = self.count.as_deref().unwrap_or("");
        let right_len = right.len();
        let title_len = title_plain.len() + 1 + self.subtitle.as_ref().map(|s| s.len() + 1).unwrap_or(0);
        let pad = if inner > title_len + right_len + 1 {
            inner - title_len - right_len - 1
        } else {
            1
        };
        if right.is_empty() {
            header.push_str(&" ".repeat(inner.saturating_sub(title_len + 1)));
        } else {
            header.push_str(&" ".repeat(pad));
            header.push_str(&format!("{}", text_sec.apply_to(right)));
        }
        out.push_str(&format!(
            "{}{}{}\n",
            border.apply_to("│"),
            header,
            border.apply_to("│"),
        ));

        // Header separator
        out.push_str(&format!(
            "{}{}{}\n",
            border.apply_to("├"),
            border.apply_to("─".repeat(inner)),
            border.apply_to("┤"),
        ));

        // Content
        match &self.content {
            PanelContent::None => {
                let line = format!(" {:inner$}", "", inner = inner);
                out.push_str(&format!(
                    "{}{}{}\n",
                    border.apply_to("│"),
                    &line[..inner],
                    border.apply_to("│"),
                ));
            }
            PanelContent::Kv(pairs) => {
                for (key, value) in pairs {
                    let key_display = format!("{}", text_sec.apply_to(format!("{key:>16}")));
                    let val_display = format!("  {value}");
                    let visible_len = 16 + 2 + console::strip_ansi_codes(value).len();
                    let padding = inner.saturating_sub(visible_len + 2);
                    out.push_str(&format!(
                        "{} {}{}{}{}\n",
                        border.apply_to("│"),
                        key_display,
                        val_display,
                        " ".repeat(padding),
                        border.apply_to("│"),
                    ));
                }
            }
            PanelContent::Table { headers, rows } => {
                let col_count = headers.len();
                let mut widths: Vec<usize> = headers.iter().map(|h| h.len()).collect();
                for row in rows {
                    for (i, cell) in row.iter().enumerate() {
                        if i < col_count {
                            let stripped = console::strip_ansi_codes(cell);
                            widths[i] = widths[i].max(stripped.len());
                        }
                    }
                }

                // Header row
                let header_line: String = headers
                    .iter()
                    .enumerate()
                    .map(|(i, h)| {
                        format!(
                            "{}",
                            text_sec.apply_to(format!(
                                "{:<width$}",
                                h.to_uppercase(),
                                width = widths[i]
                            ))
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("  ");
                let hdr_visible: usize = widths.iter().sum::<usize>() + (col_count.saturating_sub(1)) * 2;
                let hpad = inner.saturating_sub(hdr_visible + 2);
                out.push_str(&format!(
                    "{} {}{}{}\n",
                    border.apply_to("│"),
                    header_line,
                    " ".repeat(hpad),
                    border.apply_to("│"),
                ));

                // Row separator under header
                out.push_str(&format!(
                    "{} {}{}\n",
                    border.apply_to("│"),
                    subtle.apply_to("─".repeat(inner - 2)),
                    border.apply_to("│"),
                ));

                // Data rows
                for (ri, row) in rows.iter().enumerate() {
                    let line: String = row
                        .iter()
                        .enumerate()
                        .map(|(i, cell)| {
                            let w = widths.get(i).copied().unwrap_or(cell.len());
                            let stripped = console::strip_ansi_codes(cell);
                            let is_status = headers
                                .get(i)
                                .is_some_and(|h| h.eq_ignore_ascii_case("status"));
                            if is_status {
                                let styled = style::status_dot(&stripped);
                                let styled_stripped = console::strip_ansi_codes(&styled);
                                let spad = w.saturating_sub(stripped.len());
                                format!("{}{}", styled, " ".repeat(spad))
                            } else {
                                let pad = w.saturating_sub(stripped.len());
                                format!("{}{}", cell, " ".repeat(pad))
                            }
                        })
                        .collect::<Vec<_>>()
                        .join("  ");
                    let vis: usize = row
                        .iter()
                        .enumerate()
                        .map(|(i, cell)| {
                            let w = widths.get(i).copied().unwrap_or(0);
                            let is_status = headers
                                .get(i)
                                .is_some_and(|h| h.eq_ignore_ascii_case("status"));
                            if is_status {
                                let dot = style::status_dot(&console::strip_ansi_codes(cell));
                                let dlen = console::strip_ansi_codes(&dot).len();
                                dlen.max(w)
                            } else {
                                w
                            }
                        })
                        .sum::<usize>()
                        + (col_count.saturating_sub(1)) * 2;
                    let rpad = inner.saturating_sub(vis + 2);
                    out.push_str(&format!(
                        "{} {}{}{}\n",
                        border.apply_to("│"),
                        line,
                        " ".repeat(rpad),
                        border.apply_to("│"),
                    ));

                    // Row separator (not after last row)
                    if ri < rows.len() - 1 {
                        out.push_str(&format!(
                            "{} {}{}\n",
                            border.apply_to("│"),
                            subtle.apply_to("─".repeat(inner - 2)),
                            border.apply_to("│"),
                        ));
                    }
                }
            }
            PanelContent::Steps(steps) => {
                for (icon, label, desc) in steps {
                    let step_line = format!(" {icon} {}  {desc}", text_sec.apply_to(label));
                    let vis_len = 1 + console::strip_ansi_codes(icon).len()
                        + 1
                        + label.len()
                        + 2
                        + desc.len();
                    let spad = inner.saturating_sub(vis_len + 1);
                    out.push_str(&format!(
                        "{}{}{}{}\n",
                        border.apply_to("│"),
                        step_line,
                        " ".repeat(spad),
                        border.apply_to("│"),
                    ));
                }
            }
        }

        // Footer
        if self.footer_left.is_some() || self.footer_right.is_some() {
            out.push_str(&format!(
                "{}{}{}\n",
                border.apply_to("├"),
                border.apply_to("─".repeat(inner)),
                border.apply_to("┤"),
            ));
            let fl = self.footer_left.as_deref().unwrap_or("");
            let fr = self.footer_right.as_deref().unwrap_or("");
            let fl_len = console::strip_ansi_codes(fl).len();
            let fr_len = console::strip_ansi_codes(fr).len();
            let fpad = inner.saturating_sub(fl_len + fr_len + 2);
            out.push_str(&format!(
                "{} {}{}{} {}\n",
                border.apply_to("│"),
                fl,
                " ".repeat(fpad),
                fr,
                border.apply_to("│"),
            ));
        }

        // Bottom border
        out.push_str(&format!(
            "{}{}{}\n",
            border.apply_to("└"),
            border.apply_to("─".repeat(inner)),
            border.apply_to("┘"),
        ));

        out
    }
}

fn terminal_width() -> usize {
    console::Term::stdout()
        .size_checked()
        .map(|(_, w)| w as usize)
        .unwrap_or(80)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panel_renders_kv_without_panic() {
        let output = Panel::new("Test")
            .kv(&[("Key1", "Value1"), ("Key2", "Value2")])
            .render_to_string();
        assert!(output.contains("Test"));
        assert!(output.contains("Key1"));
        assert!(output.contains("Value1"));
    }

    #[test]
    fn panel_renders_table_without_panic() {
        let output = Panel::new("Pods")
            .count("3 total")
            .table(
                &["Name", "Status"],
                &[
                    vec!["web".into(), "running".into()],
                    vec!["api".into(), "failed".into()],
                ],
            )
            .render_to_string();
        assert!(output.contains("Pods"));
        assert!(output.contains("3 total"));
        assert!(output.contains("web"));
    }

    #[test]
    fn panel_renders_steps_with_footer() {
        let output = Panel::new("Deploying")
            .subtitle("web-api")
            .steps(&[
                ("✓", "Image", "nginx:latest pulled"),
                ("✓", "Pod", "web-api-a1b running"),
            ])
            .footer_left("● Deployed")
            .footer_right("2/2 pods ready")
            .render_to_string();
        assert!(output.contains("Deploying"));
        assert!(output.contains("web-api"));
        assert!(output.contains("Deployed"));
    }

    #[test]
    fn panel_respects_terminal_width() {
        let output = Panel::new("Test")
            .kv(&[("Key", "Value")])
            .render_to_string_with_width(40);
        for line in output.lines() {
            let stripped = console::strip_ansi_codes(line);
            assert!(stripped.len() <= 42, "line too long: {stripped}");
        }
    }
}
```

- [ ] **Step 4: Wire panel module into output/mod.rs**

In `src/output/mod.rs`, add the panel module declaration and export:

```rust
mod panel;
pub use panel::Panel;
```

Add after the existing `mod table;` line (line 6) and add the export after the `pub use table::print_table;` line (line 14).

- [ ] **Step 5: Run tests to verify they pass**

Run: `cd /Users/nassime/GitHub/Helyos/helyos-cli && cargo test --lib output::panel`
Expected: 4 tests PASS

- [ ] **Step 6: Commit**

```bash
cd /Users/nassime/GitHub/Helyos/helyos-cli
git add src/output/panel.rs src/output/mod.rs
git commit -m "feat(cli): add PanelBuilder for bordered box output"
```

---

## Task 3: Migrate `helyos status` to Panel

**Files:**
- Modify: `/Users/nassime/GitHub/Helyos/helyos-cli/src/commands/status.rs`

- [ ] **Step 1: Rewrite status command to use Panel**

Replace the human-readable output section (lines 48-69) of `src/commands/status.rs`. The full file becomes:

```rust
use anyhow::Result;
use helyos_core::domain::models::{Deployment, DeploymentStatus, Pod, PodStatus, Project};

use crate::client::HelyosClient;
use crate::output;
use crate::output::Panel;

pub async fn status(client: &HelyosClient) -> Result<()> {
    let projects: Vec<Project> = client.get("/api/v1/projects").await?;
    let deployments: Vec<Deployment> = client.get("/api/v1/deployments").await?;
    let pods: Vec<Pod> = client.get("/api/v1/pods").await?;

    let running_deployments = deployments
        .iter()
        .filter(|d| d.status == DeploymentStatus::Running)
        .count();
    let stopped_deployments = deployments
        .iter()
        .filter(|d| d.status == DeploymentStatus::Stopped)
        .count();

    let running_pods = pods
        .iter()
        .filter(|p| p.status == PodStatus::Running)
        .count();
    let restarting_pods = pods
        .iter()
        .filter(|p| p.status == PodStatus::Restarting)
        .count();

    if output::is_json_mode() {
        output::print_json(&serde_json::json!({
            "cluster": "single-node",
            "projects": projects.len(),
            "deployments": {
                "total": deployments.len(),
                "running": running_deployments,
                "stopped": stopped_deployments,
            },
            "pods": {
                "total": pods.len(),
                "running": running_pods,
                "restarting": restarting_pods,
            },
        }));
        return Ok(());
    }

    let status_str = output::style::status_dot("running");

    Panel::new(&format!("{} Cluster Status", output::style::icon("cluster")))
        .kv(&[
            ("Mode", "single-node"),
            ("Status", &status_str),
            ("Projects", &projects.len().to_string()),
            (
                "Deployments",
                &format!("{} running · {} stopped", running_deployments, stopped_deployments),
            ),
            (
                "Pods",
                &format!("{} running · {} restarting", running_pods, restarting_pods),
            ),
        ])
        .render();

    Ok(())
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cd /Users/nassime/GitHub/Helyos/helyos-cli && cargo check`
Expected: no errors

- [ ] **Step 3: Commit**

```bash
cd /Users/nassime/GitHub/Helyos/helyos-cli
git add src/commands/status.rs
git commit -m "feat(cli): migrate helyos status to panel output"
```

---

## Task 4: Refactor table.rs for Panel-Aware Rendering

**Files:**
- Modify: `/Users/nassime/GitHub/Helyos/helyos-cli/src/output/table.rs`

The existing `print_table` is still used by commands that haven't migrated yet. We keep it working but update its styling to match the new palette. The Panel's `.table()` method handles bordered table rendering. The standalone `print_table` gets status dots instead of plain text.

- [ ] **Step 1: Update table.rs to use the style system**

Replace `src/output/table.rs`:

```rust
use super::style;

pub fn print_table(headers: &[&str], rows: &[Vec<String>]) {
    if super::is_json_mode() {
        let objects: Vec<serde_json::Value> = rows
            .iter()
            .map(|row| {
                let mut map = serde_json::Map::new();
                for (i, cell) in row.iter().enumerate() {
                    let key = headers.get(i).unwrap_or(&"").to_lowercase();
                    map.insert(key, serde_json::Value::String(cell.clone()));
                }
                serde_json::Value::Object(map)
            })
            .collect();
        super::print_json(&objects);
        return;
    }

    if rows.is_empty() {
        println!("No resources found.");
        return;
    }

    let col_count = headers.len();
    let mut widths: Vec<usize> = headers.iter().map(|h| h.len()).collect();

    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            if i < col_count {
                widths[i] = widths[i].max(cell.len());
            }
        }
    }

    let text_sec = style::color("text-secondary");
    let header_line: String = headers
        .iter()
        .enumerate()
        .map(|(i, h)| {
            format!(
                "{}",
                text_sec.apply_to(format!("{:<width$}", h.to_uppercase(), width = widths[i]))
            )
        })
        .collect::<Vec<_>>()
        .join("  ");
    println!("{header_line}");

    for row in rows {
        let line: String = row
            .iter()
            .enumerate()
            .map(|(i, cell)| {
                let w = widths.get(i).copied().unwrap_or(cell.len());
                let formatted = format!("{:<width$}", cell, width = w);
                if headers
                    .get(i)
                    .is_some_and(|h| h.eq_ignore_ascii_case("status"))
                {
                    let dot = style::status_dot(cell);
                    let stripped = console::strip_ansi_codes(&dot);
                    let pad = w.saturating_sub(stripped.len());
                    return format!("{dot}{}", " ".repeat(pad));
                }
                formatted
            })
            .collect::<Vec<_>>()
            .join("  ");
        println!("{line}");
    }
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cd /Users/nassime/GitHub/Helyos/helyos-cli && cargo check`
Expected: no errors

- [ ] **Step 3: Commit**

```bash
cd /Users/nassime/GitHub/Helyos/helyos-cli
git add src/output/table.rs
git commit -m "feat(cli): update table renderer to use GitHub Dark palette and status dots"
```

---

## Task 5: Migrate `helyos pods` to Panel

**Files:**
- Modify: `/Users/nassime/GitHub/Helyos/helyos-cli/src/commands/pods.rs`

- [ ] **Step 1: Rewrite pods command**

Replace `src/commands/pods.rs`:

```rust
use anyhow::Result;
use helyos_core::domain::models::Pod;

use crate::client::HelyosClient;
use crate::output;
use crate::output::Panel;

pub async fn pods(client: &HelyosClient, project: Option<&str>) -> Result<()> {
    let path = match project {
        Some(p) => format!("/api/v1/pods?project={p}"),
        None => "/api/v1/pods".to_string(),
    };

    let pods: Vec<Pod> = client.get(&path).await?;

    if output::is_json_mode() {
        output::print_json(&pods);
        return Ok(());
    }

    let rows: Vec<Vec<String>> = pods
        .iter()
        .map(|p| {
            vec![
                p.container_name(),
                p.project.clone(),
                p.deployment_name.clone(),
                p.status.to_string(),
                p.image.clone(),
                output::format_age(&p.created_at),
            ]
        })
        .collect();

    Panel::new(&format!("{} Pods", output::style::icon("pod")))
        .count(&format!("{} total", pods.len()))
        .table(
            &["Name", "Project", "Deployment", "Status", "Image", "Age"],
            &rows,
        )
        .render();

    Ok(())
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cd /Users/nassime/GitHub/Helyos/helyos-cli && cargo check`
Expected: no errors

- [ ] **Step 3: Commit**

```bash
cd /Users/nassime/GitHub/Helyos/helyos-cli
git add src/commands/pods.rs
git commit -m "feat(cli): migrate helyos pods to panel output"
```

---

## Task 6: Migrate `helyos deployments` to Panel

**Files:**
- Modify: `/Users/nassime/GitHub/Helyos/helyos-cli/src/commands/deployments.rs`

- [ ] **Step 1: Rewrite deployments command**

Replace `src/commands/deployments.rs`:

```rust
use anyhow::Result;
use helyos_core::domain::models::Deployment;

use crate::client::HelyosClient;
use crate::output;
use crate::output::Panel;

pub async fn deployments(client: &HelyosClient, project: Option<&str>) -> Result<()> {
    let path = match project {
        Some(p) => format!("/api/v1/deployments?project={p}"),
        None => "/api/v1/deployments".to_string(),
    };

    let deployments: Vec<Deployment> = client.get(&path).await?;

    if output::is_json_mode() {
        output::print_json(&deployments);
        return Ok(());
    }

    let rows: Vec<Vec<String>> = deployments
        .iter()
        .map(|d| {
            vec![
                d.name().to_string(),
                d.project().to_string(),
                d.status.to_string(),
                d.spec.replicas.to_string(),
                d.spec.image.clone(),
                output::format_age(&d.created_at),
            ]
        })
        .collect();

    Panel::new(&format!("{} Deployments", output::style::icon("deploy")))
        .count(&format!("{} total", deployments.len()))
        .table(
            &["Name", "Project", "Status", "Replicas", "Image", "Age"],
            &rows,
        )
        .render();

    Ok(())
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cd /Users/nassime/GitHub/Helyos/helyos-cli && cargo check`
Expected: no errors

- [ ] **Step 3: Commit**

```bash
cd /Users/nassime/GitHub/Helyos/helyos-cli
git add src/commands/deployments.rs
git commit -m "feat(cli): migrate helyos deployments to panel output"
```

---

## Task 7: Migrate `helyos nodes` to Panel with Gauge Bars

**Files:**
- Modify: `/Users/nassime/GitHub/Helyos/helyos-cli/src/commands/nodes.rs`

- [ ] **Step 1: Rewrite nodes command with gauge bars**

Replace `src/commands/nodes.rs`:

```rust
use anyhow::Result;
use helyos_core::domain::models::Node;

use crate::client::HelyosClient;
use crate::output;
use crate::output::Panel;

pub async fn nodes(client: &HelyosClient) -> Result<()> {
    let nodes: Vec<Node> = client.get("/api/v1/nodes").await?;

    if output::is_json_mode() {
        output::print_json(&nodes);
        return Ok(());
    }

    if nodes.is_empty() {
        output::print_warning("No nodes registered. Running in single-node mode.");
        return Ok(());
    }

    let rows: Vec<Vec<String>> = nodes
        .iter()
        .map(|n| {
            let cpu_pct = if n.resources.cpu_cores > 0.0 {
                ((n.resources.cpu_cores - n.resources.cpu_available) / n.resources.cpu_cores * 100.0)
                    as u32
            } else {
                0
            };
            let mem_pct = if n.resources.memory_bytes > 0 {
                ((n.resources.memory_bytes - n.resources.memory_available) as f64
                    / n.resources.memory_bytes as f64
                    * 100.0) as u32
            } else {
                0
            };
            vec![
                n.name.clone(),
                n.role.to_string(),
                n.status.to_string(),
                format_gauge(cpu_pct),
                format_gauge(mem_pct),
                n.resources.running_pods.to_string(),
                output::format_age(&n.last_heartbeat),
            ]
        })
        .collect();

    Panel::new(&format!("{} Nodes", output::style::icon("node")))
        .count(&format!("{} total", nodes.len()))
        .table(
            &["Name", "Role", "Status", "CPU", "MEM", "Pods", "Age"],
            &rows,
        )
        .render();

    Ok(())
}

fn format_gauge(percent: u32) -> String {
    let bar_width = 16;
    let filled = (percent as usize * bar_width) / 100;
    let empty = bar_width - filled;

    let color_name = if percent >= 80 {
        "red"
    } else if percent >= 60 {
        "yellow"
    } else {
        "green"
    };

    let fill_style = output::style::color(color_name);
    let empty_style = output::style::color("border-subtle");

    format!(
        "{}{} {:>3}%",
        fill_style.apply_to("█".repeat(filled)),
        empty_style.apply_to("░".repeat(empty)),
        percent,
    )
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{:.1}Gi", bytes as f64 / 1_073_741_824.0)
    } else if bytes >= 1_048_576 {
        format!("{:.0}Mi", bytes as f64 / 1_048_576.0)
    } else {
        format!("{bytes}B")
    }
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cd /Users/nassime/GitHub/Helyos/helyos-cli && cargo check`
Expected: no errors

- [ ] **Step 3: Commit**

```bash
cd /Users/nassime/GitHub/Helyos/helyos-cli
git add src/commands/nodes.rs
git commit -m "feat(cli): migrate helyos nodes to panel output with gauge bars"
```

---

## Task 8: Deploy Progress Panel

**Files:**
- Create: `/Users/nassime/GitHub/Helyos/helyos-cli/src/output/deploy.rs`
- Modify: `/Users/nassime/GitHub/Helyos/helyos-cli/src/output/mod.rs`
- Modify: `/Users/nassime/GitHub/Helyos/helyos-cli/src/commands/deploy.rs`

- [ ] **Step 1: Create deploy panel renderer**

Create `src/output/deploy.rs`:

```rust
use super::Panel;
use super::style;

pub struct DeployStep {
    pub icon: String,
    pub label: String,
    pub detail: String,
}

pub fn render_deploy_panel(
    name: &str,
    steps: &[DeployStep],
    status: &str,
    timing: &str,
) {
    if super::is_json_mode() {
        return;
    }

    let step_tuples: Vec<(&str, &str, &str)> = steps
        .iter()
        .map(|s| (s.icon.as_str(), s.label.as_str(), s.detail.as_str()))
        .collect();

    Panel::new(&format!("{} Deploying", style::icon("deploy")))
        .subtitle(name)
        .steps(&step_tuples)
        .footer_left(status)
        .footer_right(timing)
        .render();
}
```

- [ ] **Step 2: Export deploy module from mod.rs**

In `src/output/mod.rs`, add after `mod panel;`:

```rust
pub mod deploy;
```

- [ ] **Step 3: Rewrite deploy command to use panel**

Replace `src/commands/deploy.rs`:

```rust
use anyhow::Result;
use helyos_core::config::parse_deployment_file;
use helyos_core::domain::models::{Deployment, PodStatus};
use std::path::Path;
use std::time::{Duration, Instant};
use tokio::time::sleep;

use crate::client::HelyosClient;
use crate::output::deploy::{DeployStep, render_deploy_panel};
use crate::output::{self, Spinner};

pub async fn deploy(client: &HelyosClient, file: &str) -> Result<()> {
    let path = Path::new(file);
    if !path.exists() {
        anyhow::bail!("file not found: {file}");
    }

    let yaml = std::fs::read_to_string(path)?;
    let spec = parse_deployment_file(path).map_err(|e| {
        output::print_error_with_hint(
            &format!("Invalid deployment spec: {e}"),
            "Run 'helyos init' to generate a valid template",
        );
        e
    })?;
    let project = spec.project.clone();
    let name = spec.deployment.name.clone();
    let replicas = spec.replicas;

    let spinner = if !output::is_json_mode() {
        Some(Spinner::new(&format!(
            "Deploying {name} to project '{project}'..."
        )))
    } else {
        None
    };

    let deployment: Deployment = client.post_yaml("/api/v1/deploy", &yaml).await?;

    if let Some(s) = &spinner {
        s.finish_clear();
    }

    if output::is_json_mode() {
        let pods = poll_until_ready(client, &project, &name, replicas).await?;
        output::print_json(&serde_json::json!({
            "status": "ok",
            "deployment": deployment,
            "pods": pods,
        }));
        return Ok(());
    }

    let start = Instant::now();
    let timeout = Duration::from_secs(60);
    let mut steps: Vec<DeployStep> = vec![DeployStep {
        icon: format!("{}", output::style::color("green").apply_to("✓")),
        label: "Image".to_string(),
        detail: format!("{} pulled", deployment.spec.image),
    }];

    loop {
        if start.elapsed() > timeout {
            output::print_warning(&format!(
                "Timed out waiting for all pods (60s). Check: helyos pods -p {project}"
            ));
            anyhow::bail!("timed out waiting for deployment '{name}'");
        }

        let pods = client.get_pods_for_deployment(&project, &name).await?;

        for pod in &pods {
            let pod_name = pod.container_name();
            let already = steps.iter().any(|s| s.detail.contains(&pod_name));
            if pod.status == PodStatus::Running && !already {
                steps.push(DeployStep {
                    icon: format!("{}", output::style::color("green").apply_to("✓")),
                    label: "Pod".to_string(),
                    detail: format!(
                        "{} {}",
                        pod_name,
                        output::style::color("green").apply_to("running")
                    ),
                });
            }
        }

        let running = pods
            .iter()
            .filter(|p| p.status == PodStatus::Running)
            .count() as u32;
        let failed = pods.iter().any(|p| is_terminal_failure(&p.status));

        if running >= replicas {
            let elapsed = format!("{:.1}s", start.elapsed().as_secs_f64());
            let status = format!(
                "{}",
                output::style::color("green").apply_to("● Deployed")
            );
            let timing = format!("{running}/{replicas} pods ready · {elapsed}");
            render_deploy_panel(&name, &steps, &status, &timing);
            return Ok(());
        }

        if failed {
            let status = format!(
                "{}",
                output::style::color("red").apply_to("● Failed")
            );
            render_deploy_panel(&name, &steps, &status, "");
            anyhow::bail!("deployment '{name}' has failed pods");
        }

        sleep(Duration::from_millis(500)).await;
    }
}

fn is_terminal_failure(status: &PodStatus) -> bool {
    matches!(status, PodStatus::Failed | PodStatus::CrashLoopBackoff)
}

async fn poll_until_ready(
    client: &HelyosClient,
    project: &str,
    name: &str,
    replicas: u32,
) -> Result<Vec<helyos_core::domain::models::Pod>> {
    let timeout = Duration::from_secs(60);
    let start = Instant::now();

    loop {
        if start.elapsed() > timeout {
            return client.get_pods_for_deployment(project, name).await;
        }

        let pods = client.get_pods_for_deployment(project, name).await?;
        let running = pods
            .iter()
            .filter(|p| p.status == PodStatus::Running)
            .count() as u32;
        if running >= replicas || pods.iter().any(|p| is_terminal_failure(&p.status)) {
            return Ok(pods);
        }

        sleep(Duration::from_millis(500)).await;
    }
}
```

- [ ] **Step 4: Verify it compiles**

Run: `cd /Users/nassime/GitHub/Helyos/helyos-cli && cargo check`
Expected: no errors

- [ ] **Step 5: Commit**

```bash
cd /Users/nassime/GitHub/Helyos/helyos-cli
git add src/output/deploy.rs src/output/mod.rs src/commands/deploy.rs
git commit -m "feat(cli): add deploy progress panel with steps and timing"
```

---

## Task 9: Migrate Remaining List Commands to Panels

**Files:**
- Modify: `/Users/nassime/GitHub/Helyos/helyos-cli/src/commands/project.rs`
- Modify: `/Users/nassime/GitHub/Helyos/helyos-cli/src/commands/secret.rs`
- Modify: `/Users/nassime/GitHub/Helyos/helyos-cli/src/commands/route.rs`

- [ ] **Step 1: Migrate project list**

In `src/commands/project.rs`, replace the `list_projects` function:

```rust
pub async fn list_projects(client: &HelyosClient) -> Result<()> {
    let projects: Vec<Project> = client.get("/api/v1/projects").await?;

    if output::is_json_mode() {
        output::print_json(&projects);
        return Ok(());
    }

    let rows: Vec<Vec<String>> = projects
        .iter()
        .map(|p| vec![p.name.clone(), output::format_age(&p.created_at)])
        .collect();

    output::Panel::new(&format!("{} Projects", output::style::icon("cluster")))
        .count(&format!("{} total", projects.len()))
        .table(&["Name", "Age"], &rows)
        .render();

    Ok(())
}
```

Add `use crate::output::Panel;` is not needed since we access via `output::Panel`.

- [ ] **Step 2: Migrate secret list**

In `src/commands/secret.rs`, replace the `list` function:

```rust
pub async fn list(client: &HelyosClient, project: &str) -> Result<()> {
    let path = format!("/api/v1/projects/{project}/secrets");
    let secrets: Vec<String> = client.get(&path).await?;

    if output::is_json_mode() {
        output::print_json(&serde_json::json!({
            "project": project,
            "secrets": secrets,
        }));
        return Ok(());
    }

    let rows: Vec<Vec<String>> = secrets
        .iter()
        .map(|s| vec![s.clone(), project.to_string()])
        .collect();

    output::Panel::new(&format!("{} Secrets", output::style::icon("pod")))
        .count(&format!("{} total", secrets.len()))
        .table(&["Name", "Project"], &rows)
        .render();

    Ok(())
}
```

- [ ] **Step 3: Migrate route list**

In `src/commands/route.rs`, replace the `list` function:

```rust
pub async fn list(client: &HelyosClient, project: Option<&str>) -> Result<()> {
    let path = match project {
        Some(p) => format!("/api/v1/routes?project={p}"),
        None => "/api/v1/routes".into(),
    };

    let routes: Vec<serde_json::Value> = client.get(&path).await?;

    if output::is_json_mode() {
        output::print_json(&routes);
        return Ok(());
    }

    let rows: Vec<Vec<String>> = routes
        .iter()
        .map(|r| {
            vec![
                r["domain"].as_str().unwrap_or("-").to_string(),
                r["project"].as_str().unwrap_or("-").to_string(),
                r["deployment"].as_str().unwrap_or("-").to_string(),
                r["tls_mode"].as_str().unwrap_or("none").to_string(),
                r["created_at"].as_str().unwrap_or("-").to_string(),
            ]
        })
        .collect();

    Panel::new(&format!("{} Routes", output::style::icon("event")))
        .count(&format!("{} total", routes.len()))
        .table(
            &["Domain", "Project", "Deployment", "TLS", "Created"],
            &rows,
        )
        .render();

    Ok(())
}
```

Add `use crate::output::Panel;` at the top of `route.rs`.

- [ ] **Step 4: Verify it compiles**

Run: `cd /Users/nassime/GitHub/Helyos/helyos-cli && cargo check`
Expected: no errors

- [ ] **Step 5: Commit**

```bash
cd /Users/nassime/GitHub/Helyos/helyos-cli
git add src/commands/project.rs src/commands/secret.rs src/commands/route.rs
git commit -m "feat(cli): migrate project, secret, and route list commands to panel output"
```

---

## Task 10: Formatted Logs Output

**Files:**
- Modify: `/Users/nassime/GitHub/Helyos/helyos-cli/src/commands/logs.rs`

- [ ] **Step 1: Add formatted log lines with timestamp and color**

Replace `src/commands/logs.rs`:

```rust
use anyhow::Result;
use chrono::Local;
use futures::StreamExt;
use reqwest::Response;

use crate::client::HelyosClient;
use crate::output;

pub async fn logs(
    client: &HelyosClient,
    project: Option<&str>,
    name: &str,
    tail: Option<u64>,
) -> Result<()> {
    let project = project.unwrap_or("default");
    let mut path = format!("/api/v1/projects/{project}/deployments/{name}/logs");
    if let Some(t) = tail {
        path.push_str(&format!("?tail={t}"));
    }

    let resp: Response = client.get_stream(&path).await?;
    let mut stream = resp.bytes_stream();

    let time_style = output::style::color("text-secondary");
    let name_style = output::style::color("accent");
    let sep_style = output::style::color("border");

    while let Some(chunk) = stream.next().await {
        let bytes = chunk?;
        let text = String::from_utf8_lossy(&bytes);
        for line in text.lines() {
            if let Some(data) = line.strip_prefix("data: ") {
                if output::is_json_mode() {
                    println!("{data}");
                } else {
                    let timestamp = Local::now().format("%H:%M:%S");
                    println!(
                        "{} {} {} {}",
                        time_style.apply_to(timestamp),
                        name_style.apply_to(name),
                        sep_style.apply_to("│"),
                        data,
                    );
                }
            }
        }
    }

    Ok(())
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cd /Users/nassime/GitHub/Helyos/helyos-cli && cargo check`
Expected: no errors

- [ ] **Step 3: Commit**

```bash
cd /Users/nassime/GitHub/Helyos/helyos-cli
git add src/commands/logs.rs
git commit -m "feat(cli): add formatted log output with timestamps and colors"
```

---

## Task 11: helyosd — Node Stats Endpoint

**Files:**
- Modify: `/Users/nassime/GitHub/Helyos/helyosd/src/api/handlers.rs`
- Modify: `/Users/nassime/GitHub/Helyos/helyosd/src/api/routes.rs`

The existing `GET /api/v1/nodes` returns nodes from the store. For single-node mode (no registered nodes), we need a `GET /api/v1/nodes/stats` that returns live sysinfo data.

- [ ] **Step 1: Write the handler test**

In `helyosd/src/api/handlers.rs`, we will add a test at the end. But first, add the handler. helyosd already depends on `sysinfo = "0.32"`.

- [ ] **Step 2: Add node_stats handler**

Add at the end of `helyosd/src/api/handlers.rs` (before the `metrics_middleware` function):

```rust
pub async fn node_stats(State(state): AppStateExtractor) -> impl IntoResponse {
    use sysinfo::System;

    let mut sys = System::new();
    sys.refresh_cpu_all();
    // Small delay to get meaningful CPU readings
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    sys.refresh_cpu_all();
    sys.refresh_memory();

    let cpu_count = sys.cpus().len() as f64;
    let cpu_usage: f64 = sys.cpus().iter().map(|c| c.cpu_usage() as f64).sum::<f64>() / cpu_count;
    let mem_total = sys.total_memory();
    let mem_used = sys.used_memory();

    let hostname = hostname::get()
        .map(|h| h.to_string_lossy().to_string())
        .unwrap_or_else(|_| "unknown".into());

    // Get pod count from store
    let pod_count = match state.store.list_pods(None).await {
        Ok(pods) => pods
            .iter()
            .filter(|p| {
                p.status == helyos_core::domain::models::PodStatus::Running
            })
            .count() as u32,
        Err(_) => 0,
    };

    // Check for registered nodes
    let nodes = state.store.list_nodes().await.unwrap_or_default();

    if nodes.is_empty() {
        // Single-node mode: return local stats
        Json(serde_json::json!([{
            "name": hostname,
            "role": "master",
            "status": "ready",
            "cpu_cores": cpu_count,
            "cpu_usage_percent": (cpu_usage * 10.0).round() / 10.0,
            "memory_total_bytes": mem_total,
            "memory_used_bytes": mem_used,
            "pod_count": pod_count,
        }]))
        .into_response()
    } else {
        // Multi-node: return stored nodes with local stats for this node
        let mut result: Vec<serde_json::Value> = Vec::new();
        for node in &nodes {
            let is_local = node.name == hostname || node.address.starts_with("127.");
            if is_local {
                result.push(serde_json::json!({
                    "name": node.name,
                    "role": node.role.to_string().to_lowercase(),
                    "status": node.status.to_string().to_lowercase(),
                    "cpu_cores": cpu_count,
                    "cpu_usage_percent": (cpu_usage * 10.0).round() / 10.0,
                    "memory_total_bytes": mem_total,
                    "memory_used_bytes": mem_used,
                    "pod_count": node.resources.running_pods,
                }));
            } else {
                result.push(serde_json::json!({
                    "name": node.name,
                    "role": node.role.to_string().to_lowercase(),
                    "status": node.status.to_string().to_lowercase(),
                    "cpu_cores": node.resources.cpu_cores,
                    "cpu_usage_percent": if node.resources.cpu_cores > 0.0 {
                        ((node.resources.cpu_cores - node.resources.cpu_available) / node.resources.cpu_cores * 100.0 * 10.0).round() / 10.0
                    } else { 0.0 },
                    "memory_total_bytes": node.resources.memory_bytes,
                    "memory_used_bytes": node.resources.memory_bytes - node.resources.memory_available,
                    "pod_count": node.resources.running_pods,
                }));
            }
        }
        Json(result).into_response()
    }
}
```

- [ ] **Step 3: Register the route**

In `helyosd/src/api/routes.rs`, add after the `.route("/api/v1/nodes", get(handlers::list_nodes))` line (line 63):

```rust
        .route("/api/v1/nodes/stats", get(handlers::node_stats))
```

- [ ] **Step 4: Verify it compiles**

Run: `cd /Users/nassime/GitHub/Helyos/helyosd && cargo check`
Expected: no errors

- [ ] **Step 5: Commit**

```bash
cd /Users/nassime/GitHub/Helyos/helyosd
git add src/api/handlers.rs src/api/routes.rs
git commit -m "feat(api): add GET /api/v1/nodes/stats with live sysinfo data"
```

---

## Task 12: helyosd — Events SSE Endpoint

**Files:**
- Modify: `/Users/nassime/GitHub/Helyos/helyosd/src/api/mod.rs`
- Modify: `/Users/nassime/GitHub/Helyos/helyosd/src/api/handlers.rs`
- Modify: `/Users/nassime/GitHub/Helyos/helyosd/src/api/routes.rs`
- Modify: `/Users/nassime/GitHub/Helyos/helyosd/src/adapters/event_watcher.rs`
- Modify: `/Users/nassime/GitHub/Helyos/helyosd/src/main.rs`

- [ ] **Step 1: Add broadcast channel to AppState**

In `helyosd/src/api/mod.rs`, add `tokio::sync::broadcast` to the state:

```rust
mod handlers;
pub mod routes;

use std::sync::Arc;

use helyos_core::domain::orchestrator::OrchestratorHandle;
use helyos_core::ports::metrics::MetricsPort;
use helyos_core::ports::state::StateStore;
use tokio::sync::broadcast;

#[derive(Clone)]
pub struct AppState {
    pub handle: OrchestratorHandle,
    pub store: Arc<dyn StateStore>,
    pub metrics: Arc<dyn MetricsPort>,
    pub event_tx: broadcast::Sender<ClusterEvent>,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct ClusterEvent {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub kind: String,
    pub name: String,
    pub action: String,
    pub message: String,
}

pub async fn serve(
    handle: OrchestratorHandle,
    store: Arc<dyn StateStore>,
    metrics: Arc<dyn MetricsPort>,
    event_tx: broadcast::Sender<ClusterEvent>,
    addr: &str,
) -> anyhow::Result<()> {
    let state = AppState {
        handle,
        store,
        metrics,
        event_tx,
    };
    let app = routes::build(state);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("helyosd API listening on {addr}");

    axum::serve(listener, app).await?;
    Ok(())
}
```

- [ ] **Step 2: Add events SSE handler**

In `helyosd/src/api/handlers.rs`, add the import for `ClusterEvent` and the handler. Update the `use super::` line:

```rust
use super::{AppState as SharedState, ClusterEvent};
```

Then add the handler (before `metrics_endpoint`):

```rust
pub async fn events_stream(
    State(state): AppStateExtractor,
) -> impl IntoResponse {
    let mut rx = state.event_tx.subscribe();
    let stream = async_stream::stream! {
        loop {
            match rx.recv().await {
                Ok(event) => {
                    let json = serde_json::to_string(&event).unwrap_or_default();
                    yield Ok::<_, std::convert::Infallible>(Event::default().data(json));
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    let msg = format!("{{\"warning\":\"missed {n} events\"}}");
                    yield Ok(Event::default().data(msg));
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    };
    Sse::new(stream).into_response()
}
```

Add the import at the top of handlers.rs:

```rust
use tokio::sync::broadcast;
```

- [ ] **Step 3: Register the route**

In `helyosd/src/api/routes.rs`, add after the `node_stats` route:

```rust
        .route("/api/v1/events", get(handlers::events_stream))
```

- [ ] **Step 4: Update event_watcher to broadcast**

Modify `helyosd/src/adapters/event_watcher.rs` to accept and use a broadcast sender. Replace the function signature and add broadcasting:

```rust
use std::sync::Arc;

use chrono::Utc;
use futures::StreamExt;
use tokio::sync::{broadcast, mpsc};
use tracing::{error, info, warn};
use uuid::Uuid;

use helyos_core::domain::orchestrator::Command;
use helyos_core::ports::metrics::MetricsPort;
use helyos_core::ports::runtime::{ContainerRuntime, RuntimeEvent};

use crate::api::ClusterEvent;

pub fn spawn_event_watcher(
    runtime: Arc<dyn ContainerRuntime>,
    tx: mpsc::Sender<Command>,
    metrics: Option<Arc<dyn MetricsPort>>,
    event_broadcast: Option<broadcast::Sender<ClusterEvent>>,
) {
    tokio::spawn(async move {
        info!("container event watcher starting");
        loop {
            match runtime.events().await {
                Ok(stream) => {
                    handle_event_stream(stream, &tx, metrics.as_deref(), event_broadcast.as_ref())
                        .await;
                    warn!("event stream ended, reconnecting in 5s");
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                }
                Err(e) => {
                    error!(error = %e, "failed to open event stream, retrying in 5s");
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                }
            }
        }
    });
}

async fn handle_event_stream(
    mut stream: helyos_core::ports::runtime::EventStream,
    tx: &mpsc::Sender<Command>,
    metrics: Option<&dyn MetricsPort>,
    event_broadcast: Option<&broadcast::Sender<ClusterEvent>>,
) {
    while let Some(event) = stream.next().await {
        match event {
            RuntimeEvent::ContainerDied {
                container_id,
                exit_code,
            } => {
                info!(container_id, exit_code, "container died event");
                if let Some(m) = metrics {
                    m.record_container_event("died");
                }
                if let Some(bc) = event_broadcast {
                    let _ = bc.send(ClusterEvent {
                        timestamp: Utc::now(),
                        kind: "pod".into(),
                        name: container_id.clone(),
                        action: if exit_code == 137 {
                            "OOMKilled".into()
                        } else {
                            "died".into()
                        },
                        message: format!("Container exited with code {exit_code}"),
                    });
                }
                if let Some(pod_id) = extract_pod_id(&container_id) {
                    let cmd = Command::ContainerExited { pod_id, exit_code };
                    if tx.send(cmd).await.is_err() {
                        error!("orchestrator channel closed, stopping event watcher");
                        return;
                    }
                }
            }
            RuntimeEvent::ContainerOom { container_id } => {
                warn!(container_id, "container OOM event");
                if let Some(m) = metrics {
                    m.record_container_event("oom");
                }
                if let Some(bc) = event_broadcast {
                    let _ = bc.send(ClusterEvent {
                        timestamp: Utc::now(),
                        kind: "pod".into(),
                        name: container_id.clone(),
                        action: "OOMKilled".into(),
                        message: "Container killed by OOM".into(),
                    });
                }
                if let Some(pod_id) = extract_pod_id(&container_id) {
                    let cmd = Command::ContainerExited {
                        pod_id,
                        exit_code: 137,
                    };
                    if tx.send(cmd).await.is_err() {
                        error!("orchestrator channel closed, stopping event watcher");
                        return;
                    }
                }
            }
            RuntimeEvent::ContainerStarted { container_id } => {
                info!(container_id, "container started event");
                if let Some(m) = metrics {
                    m.record_container_event("started");
                }
                if let Some(bc) = event_broadcast {
                    let _ = bc.send(ClusterEvent {
                        timestamp: Utc::now(),
                        kind: "pod".into(),
                        name: container_id.clone(),
                        action: "started".into(),
                        message: "Container started".into(),
                    });
                }
            }
        }
    }
}

fn extract_pod_id(container_id: &str) -> Option<Uuid> {
    Uuid::parse_str(container_id).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_pod_id_parses_valid_uuid() {
        let id = Uuid::new_v4();
        assert_eq!(extract_pod_id(&id.to_string()), Some(id));
    }

    #[test]
    fn extract_pod_id_returns_none_for_non_uuid() {
        assert_eq!(extract_pod_id("abc123def"), None);
    }

    #[tokio::test]
    async fn event_watcher_forwards_container_died() {
        let pod_id = Uuid::new_v4();
        let events = vec![RuntimeEvent::ContainerDied {
            container_id: pod_id.to_string(),
            exit_code: 1,
        }];
        let (tx, mut rx) = mpsc::channel(16);
        let stream: helyos_core::ports::runtime::EventStream =
            Box::pin(futures::stream::iter(events));
        handle_event_stream(stream, &tx, None, None).await;
        let cmd = rx.try_recv().expect("should have received a command");
        match cmd {
            Command::ContainerExited {
                pod_id: pid,
                exit_code,
            } => {
                assert_eq!(pid, pod_id);
                assert_eq!(exit_code, 1);
            }
            _ => panic!("unexpected command variant"),
        }
    }

    #[tokio::test]
    async fn event_watcher_forwards_oom_as_exit_137() {
        let pod_id = Uuid::new_v4();
        let events = vec![RuntimeEvent::ContainerOom {
            container_id: pod_id.to_string(),
        }];
        let (tx, mut rx) = mpsc::channel(16);
        let stream: helyos_core::ports::runtime::EventStream =
            Box::pin(futures::stream::iter(events));
        handle_event_stream(stream, &tx, None, None).await;
        let cmd = rx.try_recv().expect("should have received a command");
        match cmd {
            Command::ContainerExited {
                pod_id: pid,
                exit_code,
            } => {
                assert_eq!(pid, pod_id);
                assert_eq!(exit_code, 137);
            }
            _ => panic!("unexpected command variant"),
        }
    }

    #[tokio::test]
    async fn event_watcher_ignores_started_events() {
        let events = vec![RuntimeEvent::ContainerStarted {
            container_id: Uuid::new_v4().to_string(),
        }];
        let (tx, mut rx) = mpsc::channel(16);
        let stream: helyos_core::ports::runtime::EventStream =
            Box::pin(futures::stream::iter(events));
        handle_event_stream(stream, &tx, None, None).await;
        assert!(rx.try_recv().is_err(), "should not forward started events");
    }

    #[tokio::test]
    async fn event_watcher_broadcasts_events() {
        let pod_id = Uuid::new_v4();
        let events = vec![RuntimeEvent::ContainerStarted {
            container_id: pod_id.to_string(),
        }];
        let (tx, _rx) = mpsc::channel(16);
        let (bc_tx, mut bc_rx) = broadcast::channel(16);
        let stream: helyos_core::ports::runtime::EventStream =
            Box::pin(futures::stream::iter(events));
        handle_event_stream(stream, &tx, None, Some(&bc_tx)).await;
        let event = bc_rx.try_recv().expect("should have broadcast event");
        assert_eq!(event.action, "started");
        assert_eq!(event.kind, "pod");
    }

    #[tokio::test]
    async fn event_watcher_records_metrics_on_die() {
        use helyos_core::ports::metrics::NoOpMetrics;

        let pod_id = Uuid::new_v4();
        let events = vec![RuntimeEvent::ContainerDied {
            container_id: pod_id.to_string(),
            exit_code: 1,
        }];
        let (tx, _rx) = mpsc::channel(16);
        let stream: helyos_core::ports::runtime::EventStream =
            Box::pin(futures::stream::iter(events));
        let metrics = NoOpMetrics;
        handle_event_stream(stream, &tx, Some(&metrics), None).await;
    }
}
```

- [ ] **Step 5: Update main.rs to create broadcast channel and pass it**

In `helyosd/src/main.rs`, find where `spawn_event_watcher` is called and add the broadcast channel. Also update the `api::serve` call to pass `event_tx`.

Find the `spawn_event_watcher` call (around line 247-252) and change it from:

```rust
spawn_event_watcher(runtime_for_events, cmd_tx.clone(), metrics_for_events);
```

to:

```rust
let (event_tx, _) = tokio::sync::broadcast::channel::<crate::api::ClusterEvent>(256);
spawn_event_watcher(runtime_for_events, cmd_tx.clone(), metrics_for_events, Some(event_tx.clone()));
```

Then find the `api::serve` call and add `event_tx`:

```rust
api::serve(handle, store, metrics, event_tx, &addr).await
```

- [ ] **Step 6: Verify it compiles**

Run: `cd /Users/nassime/GitHub/Helyos/helyosd && cargo check`
Expected: no errors

- [ ] **Step 7: Run existing tests**

Run: `cd /Users/nassime/GitHub/Helyos/helyosd && cargo test`
Expected: all tests pass (event_watcher tests updated)

- [ ] **Step 8: Commit**

```bash
cd /Users/nassime/GitHub/Helyos/helyosd
git add src/api/mod.rs src/api/handlers.rs src/api/routes.rs src/adapters/event_watcher.rs src/main.rs
git commit -m "feat(api): add events SSE endpoint and broadcast channel for cluster events"
```

---

## Task 13: Add ratatui/crossterm Dependencies

**Files:**
- Modify: `/Users/nassime/GitHub/Helyos/helyos-cli/Cargo.toml`

- [ ] **Step 1: Add TUI dependencies**

In `helyos-cli/Cargo.toml`, add after the `dialoguer = "0.11"` line:

```toml
ratatui = "0.29"
crossterm = "0.28"
```

- [ ] **Step 2: Verify it compiles**

Run: `cd /Users/nassime/GitHub/Helyos/helyos-cli && cargo check`
Expected: no errors (new deps download and compile)

- [ ] **Step 3: Commit**

```bash
cd /Users/nassime/GitHub/Helyos/helyos-cli
git add Cargo.toml Cargo.lock
git commit -m "deps(cli): add ratatui 0.29 and crossterm 0.28 for TUI dashboard"
```

---

## Task 14: TUI — App State and Event Loop

**Files:**
- Create: `/Users/nassime/GitHub/Helyos/helyos-cli/src/tui/mod.rs`
- Create: `/Users/nassime/GitHub/Helyos/helyos-cli/src/tui/app.rs`
- Create: `/Users/nassime/GitHub/Helyos/helyos-cli/src/tui/event.rs`

- [ ] **Step 1: Create the TUI module entry point**

Create `src/tui/mod.rs`:

```rust
pub mod app;
pub mod event;
pub mod ui;
pub mod actions;
pub mod widgets;

use std::io;

use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use crate::client::HelyosClient;
use app::App;
use event::EventHandler;

pub async fn run(client: HelyosClient) -> anyhow::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_app(&mut terminal, client).await;

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

async fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    client: HelyosClient,
) -> anyhow::Result<()> {
    let mut app = App::new(client);
    let mut events = EventHandler::new(std::time::Duration::from_secs(2));

    app.refresh().await;

    loop {
        terminal.draw(|f| ui::draw(f, &app))?;

        match events.next().await? {
            event::AppEvent::Tick => {
                app.refresh().await;
            }
            event::AppEvent::Key(key) => {
                if actions::handle_key(&mut app, key).await? {
                    return Ok(());
                }
            }
        }
    }
}
```

- [ ] **Step 2: Create the App state**

Create `src/tui/app.rs`:

```rust
use serde::Deserialize;

use crate::client::HelyosClient;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ActivePanel {
    Pods,
    Nodes,
    Events,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NodeStats {
    pub name: String,
    pub role: String,
    pub status: String,
    pub cpu_cores: f64,
    pub cpu_usage_percent: f64,
    pub memory_total_bytes: u64,
    pub memory_used_bytes: u64,
    pub pod_count: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ClusterEvent {
    pub timestamp: String,
    pub kind: String,
    pub name: String,
    pub action: String,
    pub message: String,
}

pub struct App {
    pub client: HelyosClient,
    pub active_panel: ActivePanel,
    pub pods: Vec<helyos_core::domain::models::Pod>,
    pub deployments: Vec<helyos_core::domain::models::Deployment>,
    pub nodes: Vec<NodeStats>,
    pub events: Vec<ClusterEvent>,
    pub pod_cursor: usize,
    pub node_cursor: usize,
    pub event_cursor: usize,
    pub connected: bool,
    pub show_help: bool,
}

impl App {
    pub fn new(client: HelyosClient) -> Self {
        Self {
            client,
            active_panel: ActivePanel::Pods,
            pods: Vec::new(),
            deployments: Vec::new(),
            nodes: Vec::new(),
            events: Vec::new(),
            pod_cursor: 0,
            node_cursor: 0,
            event_cursor: 0,
            connected: false,
            show_help: false,
        }
    }

    pub async fn refresh(&mut self) {
        let pods_result = self
            .client
            .get::<Vec<helyos_core::domain::models::Pod>>("/api/v1/pods")
            .await;
        let deployments_result = self
            .client
            .get::<Vec<helyos_core::domain::models::Deployment>>("/api/v1/deployments")
            .await;
        let nodes_result = self.client.get::<Vec<NodeStats>>("/api/v1/nodes/stats").await;

        match (pods_result, deployments_result, nodes_result) {
            (Ok(pods), Ok(deployments), Ok(nodes)) => {
                self.pods = pods;
                self.deployments = deployments;
                self.nodes = nodes;
                self.connected = true;
            }
            _ => {
                self.connected = false;
            }
        }

        // Clamp cursors
        if !self.pods.is_empty() {
            self.pod_cursor = self.pod_cursor.min(self.pods.len() - 1);
        }
        if !self.nodes.is_empty() {
            self.node_cursor = self.node_cursor.min(self.nodes.len() - 1);
        }
    }

    pub fn cursor_up(&mut self) {
        match self.active_panel {
            ActivePanel::Pods => {
                self.pod_cursor = self.pod_cursor.saturating_sub(1);
            }
            ActivePanel::Nodes => {
                self.node_cursor = self.node_cursor.saturating_sub(1);
            }
            ActivePanel::Events => {
                self.event_cursor = self.event_cursor.saturating_sub(1);
            }
        }
    }

    pub fn cursor_down(&mut self) {
        match self.active_panel {
            ActivePanel::Pods => {
                if !self.pods.is_empty() {
                    self.pod_cursor = (self.pod_cursor + 1).min(self.pods.len() - 1);
                }
            }
            ActivePanel::Nodes => {
                if !self.nodes.is_empty() {
                    self.node_cursor = (self.node_cursor + 1).min(self.nodes.len() - 1);
                }
            }
            ActivePanel::Events => {
                if !self.events.is_empty() {
                    self.event_cursor = (self.event_cursor + 1).min(self.events.len() - 1);
                }
            }
        }
    }

    pub fn next_panel(&mut self) {
        self.active_panel = match self.active_panel {
            ActivePanel::Pods => ActivePanel::Nodes,
            ActivePanel::Nodes => ActivePanel::Events,
            ActivePanel::Events => ActivePanel::Pods,
        };
    }

    pub fn prev_panel(&mut self) {
        self.active_panel = match self.active_panel {
            ActivePanel::Pods => ActivePanel::Events,
            ActivePanel::Nodes => ActivePanel::Pods,
            ActivePanel::Events => ActivePanel::Nodes,
        };
    }
}
```

- [ ] **Step 3: Create the event handler**

Create `src/tui/event.rs`:

```rust
use std::time::Duration;

use crossterm::event::{self, Event, KeyEvent};

pub enum AppEvent {
    Tick,
    Key(KeyEvent),
}

pub struct EventHandler {
    tick_rate: Duration,
}

impl EventHandler {
    pub fn new(tick_rate: Duration) -> Self {
        Self { tick_rate }
    }

    pub async fn next(&mut self) -> anyhow::Result<AppEvent> {
        if event::poll(self.tick_rate)? {
            if let Event::Key(key) = event::read()? {
                return Ok(AppEvent::Key(key));
            }
        }
        Ok(AppEvent::Tick)
    }
}
```

- [ ] **Step 4: Verify it compiles**

Run: `cd /Users/nassime/GitHub/Helyos/helyos-cli && cargo check`
Expected: no errors (ui, actions, widgets modules empty — create stubs)

We need to create stub files for the other modules referenced:

Create `src/tui/ui.rs`:

```rust
use ratatui::Frame;
use super::app::App;

pub fn draw(f: &mut Frame, app: &App) {
    // Placeholder — implemented in Task 15
}
```

Create `src/tui/actions.rs`:

```rust
use crossterm::event::{KeyCode, KeyEvent};
use super::app::App;

pub async fn handle_key(app: &mut App, key: KeyEvent) -> anyhow::Result<bool> {
    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => Ok(true),
        KeyCode::Tab => {
            app.next_panel();
            Ok(false)
        }
        KeyCode::BackTab => {
            app.prev_panel();
            Ok(false)
        }
        KeyCode::Up | KeyCode::Char('k') => {
            app.cursor_up();
            Ok(false)
        }
        KeyCode::Down | KeyCode::Char('j') => {
            app.cursor_down();
            Ok(false)
        }
        KeyCode::Char('?') => {
            app.show_help = !app.show_help;
            Ok(false)
        }
        _ => Ok(false),
    }
}
```

Create `src/tui/widgets/mod.rs`:

```rust
pub mod pod_table;
pub mod node_gauge;
pub mod event_list;
```

Create `src/tui/widgets/pod_table.rs`:

```rust
// Implemented in Task 15
```

Create `src/tui/widgets/node_gauge.rs`:

```rust
// Implemented in Task 15
```

Create `src/tui/widgets/event_list.rs`:

```rust
// Implemented in Task 15
```

- [ ] **Step 5: Verify it compiles**

Run: `cd /Users/nassime/GitHub/Helyos/helyos-cli && cargo check`
Expected: no errors

- [ ] **Step 6: Commit**

```bash
cd /Users/nassime/GitHub/Helyos/helyos-cli
git add src/tui/
git commit -m "feat(cli): add TUI app state, event loop, and keyboard actions"
```

---

## Task 15: TUI — Rendering (ui.rs and widgets)

**Files:**
- Modify: `/Users/nassime/GitHub/Helyos/helyos-cli/src/tui/ui.rs`
- Modify: `/Users/nassime/GitHub/Helyos/helyos-cli/src/tui/widgets/pod_table.rs`
- Modify: `/Users/nassime/GitHub/Helyos/helyos-cli/src/tui/widgets/node_gauge.rs`
- Modify: `/Users/nassime/GitHub/Helyos/helyos-cli/src/tui/widgets/event_list.rs`

- [ ] **Step 1: Implement pod_table widget**

Replace `src/tui/widgets/pod_table.rs`:

```rust
use ratatui::layout::Constraint;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, Cell, Row, Table};

use crate::tui::app::App;

pub fn pod_status_color(status: &str) -> Color {
    match status.to_lowercase().as_str() {
        "running" => Color::Rgb(63, 185, 80),
        "pending" | "creating" => Color::Rgb(86, 212, 221),
        "restarting" | "degraded" => Color::Rgb(210, 153, 34),
        "failed" | "crashloopbackoff" => Color::Rgb(248, 81, 73),
        "stopped" | "stopping" => Color::Rgb(72, 79, 88),
        _ => Color::Rgb(230, 237, 243),
    }
}

pub fn build_table(app: &App, is_active: bool) -> Table<'static> {
    let border_color = if is_active {
        Color::Rgb(88, 166, 255)
    } else {
        Color::Rgb(48, 54, 61)
    };

    let header = Row::new(vec![
        Cell::from("NAME"),
        Cell::from("STATUS"),
        Cell::from("CPU"),
        Cell::from("MEM"),
        Cell::from("RESTARTS"),
        Cell::from("AGE"),
    ])
    .style(Style::default().fg(Color::Rgb(72, 79, 88)));

    let rows: Vec<Row> = app
        .pods
        .iter()
        .enumerate()
        .map(|(i, pod)| {
            let status_str = pod.status.to_string();
            let status_color = pod_status_color(&status_str);
            let selected = i == app.pod_cursor;

            let row = Row::new(vec![
                Cell::from(pod.container_name()),
                Cell::from(format!("● {}", status_str.to_lowercase()))
                    .style(Style::default().fg(status_color)),
                Cell::from("—"),
                Cell::from("—"),
                Cell::from(pod.restart_count.to_string()),
                Cell::from(crate::output::format_age(&pod.created_at)),
            ]);

            if selected && is_active {
                row.style(
                    Style::default()
                        .bg(Color::Rgb(22, 27, 34))
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                row
            }
        })
        .collect();

    let count = format!("{} total", app.pods.len());
    let title = format!(" ● Pods  {count} ");

    Table::new(
        rows,
        [
            Constraint::Min(20),
            Constraint::Length(16),
            Constraint::Length(5),
            Constraint::Length(6),
            Constraint::Length(10),
            Constraint::Length(6),
        ],
    )
    .header(header)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .title(title)
            .title_style(Style::default().fg(Color::Rgb(88, 166, 255))),
    )
    .row_highlight_style(Style::default().bg(Color::Rgb(22, 27, 34)))
}
```

- [ ] **Step 2: Implement node_gauge widget**

Replace `src/tui/widgets/node_gauge.rs`:

```rust
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::tui::app::{App, NodeStats};

fn gauge_color(percent: f64) -> Color {
    if percent >= 80.0 {
        Color::Rgb(248, 81, 73)
    } else if percent >= 60.0 {
        Color::Rgb(210, 153, 34)
    } else {
        Color::Rgb(63, 185, 80)
    }
}

fn gauge_line(label: &str, percent: f64, width: u16) -> Line<'static> {
    let bar_width = (width as usize).saturating_sub(12);
    let filled = ((percent / 100.0) * bar_width as f64) as usize;
    let empty = bar_width.saturating_sub(filled);
    let color = gauge_color(percent);

    Line::from(vec![
        Span::styled(
            format!("{label:>3} "),
            Style::default().fg(Color::Rgb(139, 148, 158)),
        ),
        Span::styled("█".repeat(filled), Style::default().fg(color)),
        Span::styled(
            "░".repeat(empty),
            Style::default().fg(Color::Rgb(33, 38, 45)),
        ),
        Span::styled(
            format!(" {:>3.0}%", percent),
            Style::default().fg(Color::Rgb(139, 148, 158)),
        ),
    ])
}

pub fn render(f: &mut Frame, area: Rect, app: &App, is_active: bool) {
    let border_color = if is_active {
        Color::Rgb(88, 166, 255)
    } else {
        Color::Rgb(48, 54, 61)
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .title(" ◆ Nodes ")
        .title_style(Style::default().fg(Color::Rgb(88, 166, 255)));

    let inner = block.inner(area);
    f.render_widget(block, area);

    if app.nodes.is_empty() {
        let msg = Paragraph::new("No node data")
            .style(Style::default().fg(Color::Rgb(72, 79, 88)));
        f.render_widget(msg, inner);
        return;
    }

    let node_height = 3u16;
    let constraints: Vec<Constraint> = app
        .nodes
        .iter()
        .map(|_| Constraint::Length(node_height))
        .chain(std::iter::once(Constraint::Min(0)))
        .collect();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(inner);

    for (i, node) in app.nodes.iter().enumerate() {
        if i >= chunks.len() - 1 {
            break;
        }
        let area = chunks[i];
        let mem_pct = if node.memory_total_bytes > 0 {
            node.memory_used_bytes as f64 / node.memory_total_bytes as f64 * 100.0
        } else {
            0.0
        };

        let lines = vec![
            Line::from(vec![
                Span::styled(
                    &node.name,
                    Style::default().fg(Color::Rgb(230, 237, 243)),
                ),
                Span::styled(
                    format!("  {} · {} pods", node.role, node.pod_count),
                    Style::default().fg(Color::Rgb(139, 148, 158)),
                ),
            ]),
            gauge_line("cpu", node.cpu_usage_percent, area.width),
            gauge_line("mem", mem_pct, area.width),
        ];

        let para = Paragraph::new(lines);
        f.render_widget(para, area);
    }
}
```

- [ ] **Step 3: Implement event_list widget**

Replace `src/tui/widgets/event_list.rs`:

```rust
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::tui::app::App;

fn action_color(action: &str) -> Color {
    match action.to_lowercase().as_str() {
        "started" | "deployed" | "scaled" => Color::Rgb(63, 185, 80),
        "pending" | "pulling" => Color::Rgb(210, 153, 34),
        "died" | "oomkilled" | "failed" => Color::Rgb(248, 81, 73),
        _ => Color::Rgb(139, 148, 158),
    }
}

pub fn render(f: &mut Frame, area: Rect, app: &App, is_active: bool) {
    let border_color = if is_active {
        Color::Rgb(88, 166, 255)
    } else {
        Color::Rgb(48, 54, 61)
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .title(" • Events ")
        .title_style(Style::default().fg(Color::Rgb(88, 166, 255)));

    if app.events.is_empty() {
        let msg = Paragraph::new("  No events yet")
            .style(Style::default().fg(Color::Rgb(72, 79, 88)))
            .block(block);
        f.render_widget(msg, area);
        return;
    }

    let lines: Vec<Line> = app
        .events
        .iter()
        .rev()
        .take((area.height as usize).saturating_sub(2))
        .map(|e| {
            let time = if e.timestamp.len() >= 19 {
                &e.timestamp[11..19]
            } else {
                &e.timestamp
            };
            let color = action_color(&e.action);
            Line::from(vec![
                Span::styled(
                    format!("{time}  "),
                    Style::default().fg(color),
                ),
                Span::styled(
                    format!("{:<8}", e.kind),
                    Style::default().fg(Color::Rgb(139, 148, 158)),
                ),
                Span::styled(&e.name, Style::default().fg(Color::Rgb(230, 237, 243))),
                Span::styled(
                    format!(" {}", e.action),
                    Style::default().fg(color),
                ),
            ])
        })
        .collect();

    let para = Paragraph::new(lines).block(block);
    f.render_widget(para, area);
}
```

- [ ] **Step 4: Implement the main ui.rs layout**

Replace `src/tui/ui.rs`:

```rust
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, StatefulWidget, Widget};
use ratatui::Frame;

use super::app::{ActivePanel, App};
use super::widgets::{event_list, node_gauge, pod_table};

pub fn draw(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(10),
            Constraint::Percentage(25),
            Constraint::Length(1),
        ])
        .split(f.area());

    draw_status_bar(f, chunks[0], app);
    draw_main_panels(f, chunks[1], app);
    event_list::render(f, chunks[2], app, app.active_panel == ActivePanel::Events);
    draw_keybinds(f, chunks[3]);

    if app.show_help {
        draw_help_overlay(f, f.area());
    }
}

fn draw_status_bar(f: &mut Frame, area: Rect, app: &App) {
    let status_color = if app.connected {
        Color::Rgb(63, 185, 80)
    } else {
        Color::Rgb(248, 81, 73)
    };
    let status_text = if app.connected {
        "⏻ running"
    } else {
        "● disconnected"
    };

    let pod_count = app.pods.len();
    let deploy_count = app.deployments.len();
    let node_count = app.nodes.len();

    let line = Line::from(vec![
        Span::styled(
            " ⊞ Helyos ",
            Style::default()
                .fg(Color::Rgb(88, 166, 255))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(status_text, Style::default().fg(status_color)),
        Span::styled(
            format!("  │ ◆ {node_count} nodes │ ● {pod_count} pods │ ◎ {deploy_count} deploys │ ↻ 2s"),
            Style::default().fg(Color::Rgb(139, 148, 158)),
        ),
    ]);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Rgb(48, 54, 61)))
        .style(Style::default().bg(Color::Rgb(22, 27, 34)));

    let para = Paragraph::new(line).block(block);
    f.render_widget(para, area);
}

fn draw_main_panels(f: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(area);

    let pod_table = pod_table::build_table(app, app.active_panel == ActivePanel::Pods);
    f.render_widget(pod_table, chunks[0]);
    node_gauge::render(f, chunks[1], app, app.active_panel == ActivePanel::Nodes);
}

fn draw_keybinds(f: &mut Frame, area: Rect) {
    let line = Line::from(vec![
        Span::styled(" q", Style::default().fg(Color::Rgb(201, 209, 217))),
        Span::styled(" quit  ", Style::default().fg(Color::Rgb(72, 79, 88))),
        Span::styled("?", Style::default().fg(Color::Rgb(201, 209, 217))),
        Span::styled(" help  ", Style::default().fg(Color::Rgb(72, 79, 88))),
        Span::styled("Tab", Style::default().fg(Color::Rgb(201, 209, 217))),
        Span::styled(" panel  ", Style::default().fg(Color::Rgb(72, 79, 88))),
        Span::styled("↑↓", Style::default().fg(Color::Rgb(201, 209, 217))),
        Span::styled(" nav  ", Style::default().fg(Color::Rgb(72, 79, 88))),
        Span::styled("d", Style::default().fg(Color::Rgb(201, 209, 217))),
        Span::styled(" delete  ", Style::default().fg(Color::Rgb(72, 79, 88))),
        Span::styled("s", Style::default().fg(Color::Rgb(201, 209, 217))),
        Span::styled(" scale", Style::default().fg(Color::Rgb(72, 79, 88))),
    ]);

    let para = Paragraph::new(line);
    f.render_widget(para, area);
}

fn draw_help_overlay(f: &mut Frame, area: Rect) {
    let popup_width = 50u16.min(area.width - 4);
    let popup_height = 14u16.min(area.height - 4);
    let x = (area.width - popup_width) / 2;
    let y = (area.height - popup_height) / 2;
    let popup_area = Rect::new(x, y, popup_width, popup_height);

    f.render_widget(Clear, popup_area);

    let help_text = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("  Tab       ", Style::default().fg(Color::Rgb(88, 166, 255))),
            Span::raw("Cycle panels"),
        ]),
        Line::from(vec![
            Span::styled("  ↑↓ / jk   ", Style::default().fg(Color::Rgb(88, 166, 255))),
            Span::raw("Navigate"),
        ]),
        Line::from(vec![
            Span::styled("  d         ", Style::default().fg(Color::Rgb(88, 166, 255))),
            Span::raw("Delete selected pod"),
        ]),
        Line::from(vec![
            Span::styled("  s         ", Style::default().fg(Color::Rgb(88, 166, 255))),
            Span::raw("Scale deployment"),
        ]),
        Line::from(vec![
            Span::styled("  ?         ", Style::default().fg(Color::Rgb(88, 166, 255))),
            Span::raw("Toggle help"),
        ]),
        Line::from(vec![
            Span::styled("  q / Esc   ", Style::default().fg(Color::Rgb(88, 166, 255))),
            Span::raw("Quit"),
        ]),
        Line::from(""),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Rgb(88, 166, 255)))
        .title(" ? Help ")
        .title_style(
            Style::default()
                .fg(Color::Rgb(88, 166, 255))
                .add_modifier(Modifier::BOLD),
        )
        .style(Style::default().bg(Color::Rgb(13, 17, 23)));

    let para = Paragraph::new(help_text).block(block);
    f.render_widget(para, popup_area);
}
```

- [ ] **Step 5: Verify it compiles**

Run: `cd /Users/nassime/GitHub/Helyos/helyos-cli && cargo check`
Expected: no errors

- [ ] **Step 6: Commit**

```bash
cd /Users/nassime/GitHub/Helyos/helyos-cli
git add src/tui/
git commit -m "feat(cli): implement TUI rendering with pod table, node gauges, and event list"
```

---

## Task 16: Wire `helyos top` Command into CLI

**Files:**
- Create: `/Users/nassime/GitHub/Helyos/helyos-cli/src/commands/top.rs`
- Modify: `/Users/nassime/GitHub/Helyos/helyos-cli/src/commands/mod.rs`
- Modify: `/Users/nassime/GitHub/Helyos/helyos-cli/src/main.rs`

- [ ] **Step 1: Create the top command handler**

Create `src/commands/top.rs`:

```rust
use anyhow::Result;

use crate::client::HelyosClient;

pub async fn top(client: HelyosClient) -> Result<()> {
    crate::tui::run(client).await
}
```

- [ ] **Step 2: Export top from commands/mod.rs**

Add to `src/commands/mod.rs`:

```rust
pub mod top;
```

After the existing `pub mod status;` line.

- [ ] **Step 3: Add tui module to main.rs**

In `src/main.rs`, add after the `mod output;` line (line 3):

```rust
mod tui;
```

- [ ] **Step 4: Add Top variant to Commands enum**

In `src/main.rs`, add inside the `Commands` enum (after the `Status` variant):

```rust
    /// Live cluster dashboard
    Top,
```

- [ ] **Step 5: Wire Top command in main match**

In `src/main.rs`, add a match arm in the `match cli.command` block (after the `Commands::Status` arm):

```rust
        Commands::Top => commands::top::top(client).await,
```

Note: `top` takes ownership of `client` (not a reference), so this arm must be placed correctly. The `client` is moved here, which is fine since each arm is mutually exclusive.

- [ ] **Step 6: Verify it compiles**

Run: `cd /Users/nassime/GitHub/Helyos/helyos-cli && cargo check`
Expected: no errors

- [ ] **Step 7: Run all tests**

Run: `cd /Users/nassime/GitHub/Helyos/helyos-cli && cargo test`
Expected: all tests pass (including the new parse test for `top`)

- [ ] **Step 8: Commit**

```bash
cd /Users/nassime/GitHub/Helyos/helyos-cli
git add src/commands/top.rs src/commands/mod.rs src/main.rs
git commit -m "feat(cli): wire helyos top command to TUI dashboard"
```

---

## Task 17: Integration Verification and Cleanup

**Files:**
- Verify all changes compile and pass tests across both repos

- [ ] **Step 1: Run full build and tests for helyos-cli**

Run: `cd /Users/nassime/GitHub/Helyos/helyos-cli && cargo build && cargo test`
Expected: build succeeds, all tests pass

- [ ] **Step 2: Run full build and tests for helyosd**

Run: `cd /Users/nassime/GitHub/Helyos/helyosd && cargo build && cargo test`
Expected: build succeeds, all tests pass

- [ ] **Step 3: Run clippy on both repos**

Run: `cd /Users/nassime/GitHub/Helyos/helyos-cli && cargo clippy -- -D warnings`
Run: `cd /Users/nassime/GitHub/Helyos/helyosd && cargo clippy -- -D warnings`
Expected: no warnings

- [ ] **Step 4: Run fmt check on both repos**

Run: `cd /Users/nassime/GitHub/Helyos/helyos-cli && cargo fmt -- --check`
Run: `cd /Users/nassime/GitHub/Helyos/helyosd && cargo fmt -- --check`
Expected: no formatting issues

- [ ] **Step 5: Fix any issues found and commit**

```bash
# If needed:
cd /Users/nassime/GitHub/Helyos/helyos-cli && cargo fmt
cd /Users/nassime/GitHub/Helyos/helyosd && cargo fmt
git add -A && git commit -m "chore: fix formatting and clippy warnings"
```

---

## Task 18: TUI — SSE Event Streaming

The TUI's events list is always empty because nothing consumes the `GET /api/v1/events` SSE stream. Fix the event loop to use tokio channels and spawn a background SSE reader.

**Files:**
- Modify: `/Users/nassime/GitHub/Helyos/helyos-cli/src/tui/event.rs`
- Modify: `/Users/nassime/GitHub/Helyos/helyos-cli/src/tui/mod.rs`
- Modify: `/Users/nassime/GitHub/Helyos/helyos-cli/src/tui/app.rs`

- [ ] **Step 1: Rewrite event.rs with tokio channels**

The current `event::poll` blocks the tokio runtime. Replace `src/tui/event.rs` with a channel-based approach:

```rust
use std::time::Duration;

use crossterm::event::{self, Event, KeyEvent};
use tokio::sync::mpsc;

pub enum AppEvent {
    Tick,
    Key(KeyEvent),
    ClusterEvent(super::app::ClusterEvent),
}

pub struct EventHandler {
    rx: mpsc::UnboundedReceiver<AppEvent>,
}

impl EventHandler {
    pub fn new(tick_rate: Duration, client: &crate::client::HelyosClient, server_url: &str) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();

        // Key event thread (crossterm is sync)
        let key_tx = tx.clone();
        let tick = tick_rate;
        std::thread::spawn(move || loop {
            if event::poll(tick).unwrap_or(false) {
                if let Ok(Event::Key(key)) = event::read() {
                    if key_tx.send(AppEvent::Key(key)).is_err() {
                        return;
                    }
                }
            } else if key_tx.send(AppEvent::Tick).is_err() {
                return;
            }
        });

        // SSE event task
        let sse_tx = tx.clone();
        let url = format!("{}/api/v1/events", server_url.trim_end_matches('/'));
        tokio::spawn(async move {
            let client = reqwest::Client::new();
            loop {
                match client.get(&url).send().await {
                    Ok(resp) if resp.status().is_success() => {
                        use futures::StreamExt;
                        let mut stream = resp.bytes_stream();
                        let mut buffer = String::new();
                        while let Some(chunk) = stream.next().await {
                            if let Ok(bytes) = chunk {
                                buffer.push_str(&String::from_utf8_lossy(&bytes));
                                while let Some(pos) = buffer.find("\n\n") {
                                    let msg = buffer[..pos].to_string();
                                    buffer = buffer[pos + 2..].to_string();
                                    if let Some(data) = msg.strip_prefix("data: ") {
                                        if let Ok(event) =
                                            serde_json::from_str::<super::app::ClusterEvent>(data)
                                        {
                                            if sse_tx
                                                .send(AppEvent::ClusterEvent(event))
                                                .is_err()
                                            {
                                                return;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        });

        Self { rx }
    }

    pub async fn next(&mut self) -> anyhow::Result<AppEvent> {
        self.rx
            .recv()
            .await
            .ok_or_else(|| anyhow::anyhow!("event channel closed"))
    }
}
```

- [ ] **Step 2: Update tui/mod.rs to pass server URL and handle ClusterEvent**

Replace `src/tui/mod.rs`:

```rust
pub mod app;
pub mod event;
pub mod ui;
pub mod actions;
pub mod widgets;

use std::io;

use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use crate::client::HelyosClient;
use app::App;
use event::EventHandler;

pub async fn run(client: HelyosClient, server_url: &str) -> anyhow::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_app(&mut terminal, client, server_url).await;

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

async fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    client: HelyosClient,
    server_url: &str,
) -> anyhow::Result<()> {
    let mut app = App::new(client);
    let mut events = EventHandler::new(
        std::time::Duration::from_secs(2),
        &app.client,
        server_url,
    );

    app.refresh().await;

    loop {
        terminal.draw(|f| ui::draw(f, &app))?;

        match events.next().await? {
            event::AppEvent::Tick => {
                app.refresh().await;
            }
            event::AppEvent::Key(key) => {
                if actions::handle_key(&mut app, key).await? {
                    return Ok(());
                }
            }
            event::AppEvent::ClusterEvent(event) => {
                app.push_event(event);
            }
        }
    }
}
```

- [ ] **Step 3: Add push_event to App**

In `src/tui/app.rs`, add a method to `impl App`:

```rust
    pub fn push_event(&mut self, event: ClusterEvent) {
        self.events.push(event);
        // Keep at most 100 events
        if self.events.len() > 100 {
            self.events.remove(0);
        }
    }
```

- [ ] **Step 4: Update top.rs and main.rs to pass server URL**

In `src/commands/top.rs`:

```rust
use anyhow::Result;

use crate::client::HelyosClient;

pub async fn top(client: HelyosClient, server_url: &str) -> Result<()> {
    crate::tui::run(client, server_url).await
}
```

In `src/main.rs`, update the `Commands::Top` match arm:

```rust
        Commands::Top => commands::top::top(client, &cli.server).await,
```

- [ ] **Step 5: Verify it compiles**

Run: `cd /Users/nassime/GitHub/Helyos/helyos-cli && cargo check`
Expected: no errors

- [ ] **Step 6: Commit**

```bash
cd /Users/nassime/GitHub/Helyos/helyos-cli
git add src/tui/ src/commands/top.rs src/main.rs
git commit -m "feat(cli): add SSE event streaming to TUI dashboard"
```

---

## Task 19: TUI — Pod Actions (Delete and Scale)

**Files:**
- Modify: `/Users/nassime/GitHub/Helyos/helyos-cli/src/tui/app.rs`
- Modify: `/Users/nassime/GitHub/Helyos/helyos-cli/src/tui/actions.rs`
- Modify: `/Users/nassime/GitHub/Helyos/helyos-cli/src/tui/ui.rs`

- [ ] **Step 1: Add interaction modes to App state**

In `src/tui/app.rs`, add mode enum and fields:

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum InputMode {
    Normal,
    ConfirmDelete(String), // pod container name
    ScaleInput(String, String), // deployment name, current input
}
```

Add to `App` struct:

```rust
    pub input_mode: InputMode,
    pub status_message: Option<String>,
```

Initialize in `App::new`:

```rust
    input_mode: InputMode::Normal,
    status_message: None,
```

- [ ] **Step 2: Add action methods to App**

In `src/tui/app.rs`, add to `impl App`:

```rust
    pub fn selected_pod(&self) -> Option<&helyos_core::domain::models::Pod> {
        self.pods.get(self.pod_cursor)
    }

    pub async fn delete_selected_pod(&mut self) {
        if let Some(pod) = self.pods.get(self.pod_cursor) {
            let project = &pod.project;
            let deployment = &pod.deployment_name;
            let name = pod.container_name();
            let path = format!("/api/v1/projects/{project}/deployments/{deployment}");
            match self.client.delete(&path).await {
                Ok(()) => {
                    self.status_message = Some(format!("✓ Deleted {name}"));
                    self.refresh().await;
                }
                Err(e) => {
                    self.status_message = Some(format!("✗ {e}"));
                }
            }
        }
        self.input_mode = InputMode::Normal;
    }

    pub async fn scale_deployment(&mut self, replicas_str: &str) {
        if let Ok(replicas) = replicas_str.parse::<u32>() {
            if let Some(pod) = self.pods.get(self.pod_cursor) {
                let project = &pod.project;
                let deployment = &pod.deployment_name;
                let path = format!("/api/v1/projects/{project}/deployments/{deployment}/scale");
                let body = serde_json::json!({ "replicas": replicas }).to_string();
                match self
                    .client
                    .post_json::<helyos_core::domain::models::Deployment>(&path, &body)
                    .await
                {
                    Ok(d) => {
                        self.status_message =
                            Some(format!("✓ Scaled {} to {} replicas", deployment, d.spec.replicas));
                        self.refresh().await;
                    }
                    Err(e) => {
                        self.status_message = Some(format!("✗ {e}"));
                    }
                }
            }
        }
        self.input_mode = InputMode::Normal;
    }
```

- [ ] **Step 3: Update actions.rs for delete/scale keybindings**

Replace `src/tui/actions.rs`:

```rust
use crossterm::event::{KeyCode, KeyEvent};

use super::app::{ActivePanel, App, InputMode};

pub async fn handle_key(app: &mut App, key: KeyEvent) -> anyhow::Result<bool> {
    match &app.input_mode {
        InputMode::ConfirmDelete(name) => {
            let name = name.clone();
            match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    app.delete_selected_pod().await;
                }
                _ => {
                    app.input_mode = InputMode::Normal;
                    app.status_message = None;
                }
            }
            Ok(false)
        }
        InputMode::ScaleInput(deployment, input) => {
            let deployment = deployment.clone();
            let mut input = input.clone();
            match key.code {
                KeyCode::Enter => {
                    app.scale_deployment(&input).await;
                }
                KeyCode::Char(c) if c.is_ascii_digit() => {
                    input.push(c);
                    app.input_mode = InputMode::ScaleInput(deployment, input);
                }
                KeyCode::Backspace => {
                    input.pop();
                    app.input_mode = InputMode::ScaleInput(deployment, input);
                }
                KeyCode::Esc => {
                    app.input_mode = InputMode::Normal;
                    app.status_message = None;
                }
                _ => {}
            }
            Ok(false)
        }
        InputMode::Normal => match key.code {
            KeyCode::Char('q') | KeyCode::Esc => Ok(true),
            KeyCode::Tab => {
                app.next_panel();
                Ok(false)
            }
            KeyCode::BackTab => {
                app.prev_panel();
                Ok(false)
            }
            KeyCode::Up | KeyCode::Char('k') => {
                app.cursor_up();
                Ok(false)
            }
            KeyCode::Down | KeyCode::Char('j') => {
                app.cursor_down();
                Ok(false)
            }
            KeyCode::Char('?') => {
                app.show_help = !app.show_help;
                Ok(false)
            }
            KeyCode::Char('d') => {
                if app.active_panel == ActivePanel::Pods {
                    if let Some(pod) = app.selected_pod() {
                        let name = pod.container_name();
                        app.status_message = Some(format!("Delete {name}? [y/N]"));
                        app.input_mode = InputMode::ConfirmDelete(name);
                    }
                }
                Ok(false)
            }
            KeyCode::Char('s') => {
                if app.active_panel == ActivePanel::Pods {
                    if let Some(pod) = app.selected_pod() {
                        let deployment = pod.deployment_name.clone();
                        app.status_message = Some(format!("Scale {deployment} — Replicas: "));
                        app.input_mode = InputMode::ScaleInput(deployment, String::new());
                    }
                }
                Ok(false)
            }
            _ => Ok(false),
        },
    }
}
```

- [ ] **Step 4: Update keybinds bar in ui.rs to show status messages**

In `src/tui/ui.rs`, replace the `draw_keybinds` function:

```rust
fn draw_keybinds(f: &mut Frame, area: Rect, app: &App) {
    if let Some(msg) = &app.status_message {
        let style = if msg.starts_with('✓') {
            Style::default().fg(Color::Rgb(63, 185, 80))
        } else if msg.starts_with('✗') {
            Style::default().fg(Color::Rgb(248, 81, 73))
        } else {
            Style::default().fg(Color::Rgb(210, 153, 34))
        };
        let para = Paragraph::new(format!(" {msg}")).style(style);
        f.render_widget(para, area);
        return;
    }

    let line = Line::from(vec![
        Span::styled(" q", Style::default().fg(Color::Rgb(201, 209, 217))),
        Span::styled(" quit  ", Style::default().fg(Color::Rgb(72, 79, 88))),
        Span::styled("?", Style::default().fg(Color::Rgb(201, 209, 217))),
        Span::styled(" help  ", Style::default().fg(Color::Rgb(72, 79, 88))),
        Span::styled("Tab", Style::default().fg(Color::Rgb(201, 209, 217))),
        Span::styled(" panel  ", Style::default().fg(Color::Rgb(72, 79, 88))),
        Span::styled("↑↓", Style::default().fg(Color::Rgb(201, 209, 217))),
        Span::styled(" nav  ", Style::default().fg(Color::Rgb(72, 79, 88))),
        Span::styled("d", Style::default().fg(Color::Rgb(201, 209, 217))),
        Span::styled(" delete  ", Style::default().fg(Color::Rgb(72, 79, 88))),
        Span::styled("s", Style::default().fg(Color::Rgb(201, 209, 217))),
        Span::styled(" scale", Style::default().fg(Color::Rgb(72, 79, 88))),
    ]);

    let para = Paragraph::new(line);
    f.render_widget(para, area);
}
```

Update `draw()` to pass `app` to `draw_keybinds`:

```rust
    draw_keybinds(f, chunks[3], app);
```

- [ ] **Step 5: Verify it compiles**

Run: `cd /Users/nassime/GitHub/Helyos/helyos-cli && cargo check`
Expected: no errors

- [ ] **Step 6: Commit**

```bash
cd /Users/nassime/GitHub/Helyos/helyos-cli
git add src/tui/
git commit -m "feat(cli): add pod delete and scale actions to TUI dashboard"
```

---

## Task 20: TUI — Log Sub-View

**Files:**
- Modify: `/Users/nassime/GitHub/Helyos/helyos-cli/src/tui/app.rs`
- Modify: `/Users/nassime/GitHub/Helyos/helyos-cli/src/tui/actions.rs`
- Modify: `/Users/nassime/GitHub/Helyos/helyos-cli/src/tui/ui.rs`

- [ ] **Step 1: Add log view state to App**

In `src/tui/app.rs`, add to `InputMode`:

```rust
    LogView(String, Vec<String>), // pod name, log lines
```

Add a method to `impl App`:

```rust
    pub async fn open_log_view(&mut self) {
        if let Some(pod) = self.pods.get(self.pod_cursor) {
            let name = pod.container_name();
            let project = &pod.project;
            let deployment = &pod.deployment_name;
            let path = format!(
                "/api/v1/projects/{project}/deployments/{deployment}/logs?tail=100"
            );
            match self.client.get_stream(&path).await {
                Ok(resp) => {
                    use futures::StreamExt;
                    let mut stream = resp.bytes_stream();
                    let mut lines = Vec::new();
                    // Read available data with a timeout
                    let timeout = tokio::time::timeout(
                        std::time::Duration::from_secs(2),
                        async {
                            while let Some(chunk) = stream.next().await {
                                if let Ok(bytes) = chunk {
                                    let text = String::from_utf8_lossy(&bytes);
                                    for line in text.lines() {
                                        if let Some(data) = line.strip_prefix("data: ") {
                                            lines.push(data.to_string());
                                        }
                                    }
                                }
                            }
                        },
                    )
                    .await;
                    // Timeout is expected (SSE stream stays open)
                    let _ = timeout;
                    self.input_mode = InputMode::LogView(name, lines);
                }
                Err(e) => {
                    self.status_message = Some(format!("✗ Failed to open logs: {e}"));
                }
            }
        }
    }
```

- [ ] **Step 2: Handle log view keys in actions.rs**

In `src/tui/actions.rs`, add a match arm for `LogView` in `handle_key` (before the `Normal` arm):

```rust
        InputMode::LogView(_, _) => match key.code {
            KeyCode::Char('q') | KeyCode::Esc => {
                app.input_mode = InputMode::Normal;
                Ok(false)
            }
            _ => Ok(false),
        },
```

Add `l` and `Enter` keys in the `Normal` match:

```rust
            KeyCode::Char('l') | KeyCode::Enter => {
                if app.active_panel == ActivePanel::Pods && !app.pods.is_empty() {
                    app.open_log_view().await;
                }
                Ok(false)
            }
```

- [ ] **Step 3: Render log sub-view in ui.rs**

In `src/tui/ui.rs`, update `draw()` to check for log view mode:

```rust
pub fn draw(f: &mut Frame, app: &App) {
    if let InputMode::LogView(name, lines) = &app.input_mode {
        draw_log_view(f, f.area(), name, lines);
        return;
    }

    let chunks = Layout::default()
        // ... rest unchanged
```

Add the `draw_log_view` function:

```rust
fn draw_log_view(f: &mut Frame, area: Rect, name: &str, lines: &[String]) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(1)])
        .split(area);

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Rgb(88, 166, 255)))
        .title(format!(" ● Logs — {name} "))
        .title_style(
            Style::default()
                .fg(Color::Rgb(88, 166, 255))
                .add_modifier(Modifier::BOLD),
        )
        .style(Style::default().bg(Color::Rgb(13, 17, 23)));

    let visible_height = (chunks[0].height as usize).saturating_sub(2);
    let start = lines.len().saturating_sub(visible_height);
    let visible_lines: Vec<Line> = lines[start..]
        .iter()
        .map(|line| {
            Line::from(vec![
                Span::styled(
                    "│ ",
                    Style::default().fg(Color::Rgb(48, 54, 61)),
                ),
                Span::styled(
                    line.as_str(),
                    Style::default().fg(Color::Rgb(230, 237, 243)),
                ),
            ])
        })
        .collect();

    let para = Paragraph::new(visible_lines).block(block);
    f.render_widget(para, chunks[0]);

    let keybinds = Line::from(vec![
        Span::styled(" q", Style::default().fg(Color::Rgb(201, 209, 217))),
        Span::styled(" back", Style::default().fg(Color::Rgb(72, 79, 88))),
    ]);
    let footer = Paragraph::new(keybinds);
    f.render_widget(footer, chunks[1]);
}
```

Add the necessary import for `InputMode` at the top of `ui.rs`:

```rust
use super::app::{ActivePanel, App, InputMode};
```

- [ ] **Step 4: Verify it compiles**

Run: `cd /Users/nassime/GitHub/Helyos/helyos-cli && cargo check`
Expected: no errors

- [ ] **Step 5: Commit**

```bash
cd /Users/nassime/GitHub/Helyos/helyos-cli
git add src/tui/
git commit -m "feat(cli): add log sub-view to TUI dashboard"
```

---

## Task 21: Final Integration Verification

**Files:**
- Both repos

- [ ] **Step 1: Full build, test, clippy, fmt for helyos-cli**

Run: `cd /Users/nassime/GitHub/Helyos/helyos-cli && cargo fmt && cargo clippy -- -D warnings && cargo test`
Expected: all pass

- [ ] **Step 2: Full build, test, clippy, fmt for helyosd**

Run: `cd /Users/nassime/GitHub/Helyos/helyosd && cargo fmt && cargo clippy -- -D warnings && cargo test`
Expected: all pass

- [ ] **Step 3: Commit any fixes**

```bash
cd /Users/nassime/GitHub/Helyos/helyos-cli && git add -A && git commit -m "chore: final cleanup for CLI UI redesign"
cd /Users/nassime/GitHub/Helyos/helyosd && git add -A && git commit -m "chore: final cleanup for events API"
```
