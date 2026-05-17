# Ratatui 0.30 API Surface Map — Vexcoder Wiring Inventory

Reference inventory of ratatui 0.30 APIs against the active codebase.
Generated from a source-verified audit of the repository at HEAD.

Consumed by: PR-400 Batch G checklist (see
`TASKS/PR-400-ratatui-crossterm-tty-contract-plan.md`).

## Legend

| Status | Meaning |
| :--- | :--- |
| ✅ Active | Called in production source; location verified |
| ⚠️ Partial | Used but via legacy form or incomplete pattern |
| ❌ Gap | Not used; feature flag may be enabled |
| 🚫 Declared-only feature | Feature flag declared in Cargo.toml; API never called |

---

## 1. Text Layer — `ratatui::text`

### 1.1 Text

| # | API | Status | Source file(s) | Notes |
| :--- | :--- | :--- | :--- | :--- |
| 1 | `Text::raw(str)` | ❌ Gap | — | Preferred constructor for unstyled multi-line text. Not used; `Text::from` used instead. |
| 2 | `Text::styled(str, style)` | ❌ Gap | — | Top-level styled constructor. Not used. |
| 3 | `Text::from(Vec<Line>)` | ✅ Active | `src/ui/render/mod.rs`, `src/ui/render/transcript.rs` | Primary assembly pattern throughout the renderer. |
| 4 | `Text::from_iter(iter)` | ❌ Gap | — | Iterator-based construction. Not used; explicit Vec used. |
| 5 | `text.style(style)` | ❌ Gap | — | Fluent top-level style setter. Not used. |
| 6 | `text.patch_style(style)` | ❌ Gap | — | Additive style merge. Not used. |
| 7 | `text.reset_style()` | ❌ Gap | — | Style clear. Not used. |
| 8 | `text.left_aligned()` / `.centered()` / `.right_aligned()` | ❌ Gap | — | Shorthand alignment setters (0.26+). Not used. |
| 9 | `text.push_line(line)` | ❌ Gap | — | In-place append. Streaming transcript assembly uses Vec + Text::from instead. Migration target. |
| 10 | `text.push_span(span)` | ❌ Gap | — | Append span to last line. Not used. |
| 11 | `text.extend(iter<Line>)` | ❌ Gap | — | Implements Extend<Line>. Useful for merging transcript chunks. Not used. |
| 12 | `text.width()` | ❌ Gap | — | Max unicode width across lines. Not used. |
| 13 | `text.height()` | ❌ Gap | — | Line count. Not used. |
| 14 | `text.iter()` / `iter_mut()` | ❌ Gap | — | Line iterators. Not used. |

### 1.2 Line

| # | API | Status | Source file(s) | Notes |
| :--- | :--- | :--- | :--- | :--- |
| 15 | `Line::raw(str)` | ❌ Gap | — | Unstyled single-line constructor. Not used. |
| 16 | `Line::styled(str, style)` | ⚠️ Partial | `src/ui/render/mod.rs` | Used sparsely (approval modal body). Not the dominant pattern. |
| 17 | `Line::from(Vec<Span>)` | ✅ Active | `src/ui/render/mod.rs`, `src/ui/render/transcript.rs` | Dominant multi-style line pattern. Used extensively. |
| 18 | `Line::from_iter(iter<Span>)` | ❌ Gap | — | Iterator-based construction. Not used. |
| 19 | `line.style(style)` | ⚠️ Partial | `src/ui/render/mod.rs` | Used via `.fg(Color::X)` method chain on Style only. |
| 20 | `line.patch_style(style)` | ❌ Gap | — | Additive style on line. Not used. |
| 21 | `line.left_aligned()` / `.centered()` / `.right_aligned()` | ❌ Gap | — | Per-line alignment overrides. Not used. |
| 22 | `line.push_span(span)` | ❌ Gap | — | In-place append. Not used. |
| 23 | `line.width()` | ❌ Gap | — | Unicode-width aware measurement. Not used. |
| 24 | `line.styled_graphemes(base_style)` | ❌ Gap | — | Low-level render pipeline. Not used. |
| 25 | `span.into_left_aligned_line()` | ❌ Gap | — | Stylize-to-Line conversion shortcut. Not used. |

### 1.3 Span

| # | API | Status | Source file(s) | Notes |
| :--- | :--- | :--- | :--- | :--- |
| 26 | `Span::raw(str)` | ✅ Active | `src/ui/render/mod.rs`, `src/ui/render/transcript.rs` | Used for unstyled span segments. |
| 27 | `Span::styled(str, style)` | ✅ Active | `src/ui/render/mod.rs`, `src/ui/render/transcript.rs` | Primary span construction pattern (46+ call sites). |
| 28 | `span.style(style)` | ❌ Gap | — | Fluent setter on existing span. Not used. |
| 29 | `span.patch_style(style)` | ❌ Gap | — | Additive style merge. Not used. |
| 30 | `span.width()` | ❌ Gap | — | Unicode-aware width. Not used. |

### 1.4 Stylize Trait

| # | API | Status | Source file(s) | Notes |
| :--- | :--- | :--- | :--- | :--- |
| 31 | `"text".red()` / `.green()` / `.blue()` | ❌ Gap | — | Ergonomic color shortcuts on &str/String/Span/Line/Text. Not used anywhere. |
| 32 | `"text".yellow()` / `.cyan()` / `.magenta()` | ❌ Gap | — | Named color shortcuts. Not used. |
| 33 | `"text".gray()` / `.dark_gray()` | ❌ Gap | — | Neutral color shortcuts. Not used. |
| 34 | `"text".bold()` / `.dim()` / `.italic()` | ❌ Gap | — | Modifier shortcuts. Not used; `add_modifier(Modifier::X)` used instead (44 call sites). |
| 35 | `"text".underlined()` | ❌ Gap | — | Not used. |
| 36 | `"text".on_red()` / `.on_blue()` / etc. | ❌ Gap | — | Background color via Stylize. Not used. |
| 37 | `const MY_STYLE: Style = Style::new().blue()` | ❌ Gap | — | 0.30: Style::new() is const-compatible; enables compile-time style constants. Not used. |

### 1.5 Style Struct

| # | API | Status | Source file(s) | Notes |
| :--- | :--- | :--- | :--- | :--- |
| 38 | `Style::default()` | ✅ Active | `src/ui/render/mod.rs`, `src/ui/render/transcript.rs` | Used 68+ times. Legacy form — prefer `Style::new()` in new code. |
| 39 | `Style::new()` | ❌ Gap | — | Modern const-compatible form. Zero usage. Migration target. |
| 40 | `style.add_modifier(Modifier::X)` | ✅ Active | `src/ui/render/mod.rs`, `src/ui/render/transcript.rs` | 44 call sites. `Modifier::BOLD`, `DIM`, `ITALIC` all used. |
| 41 | `style.remove_modifier(Modifier::X)` | ❌ Gap | — | Sub-modifier. Not used. |
| 42 | `style.patch(other)` | ❌ Gap | — | Layered style composition. Not used. |
| 43 | `Color::Rgb(r, g, b)` | ✅ Active | `src/ui/render/mod.rs` | `Color::Rgb(24, 24, 24)` used for composer background. |
| 44 | `Color::Indexed(u8)` | ❌ Gap | — | 256-color palette. Not used. |
| 45 | `style.underline_color(Color)` | ❌ Gap | — | Kitty underline color extension. Not used. |

### 1.6 Text Utilities

| # | API | Status | Source file(s) | Notes |
| :--- | :--- | :--- | :--- | :--- |
| 46 | `Masked::new(str, char)` | ❌ Gap | — | Password masking. Not used. |
| 47 | `IntoText` / `ToText` trait | ✅ Active | `src/ui/render/transcript.rs` | `ansi_to_tui::IntoText` used for ANSI-to-ratatui conversion. |
| 48 | `palette::tailwind::*` | ❌ Gap | — | Built-in Tailwind color palette (0.28+). Not used. |

---

## 2. Widget Layer — `ratatui::widgets`

### 2.1 Paragraph

| # | API | Status | Source file(s) | Notes |
| :--- | :--- | :--- | :--- | :--- |
| 49 | `Paragraph::new(text)` | ✅ Active | `src/ui/render/mod.rs`, `src/ui/render/transcript.rs` | Primary render widget. Used throughout. |
| 50 | `paragraph.block(Block)` | ✅ Active | `src/ui/render/mod.rs` | Wraps paragraph in bordered block. |
| 51 | `paragraph.wrap(Wrap { trim })` | ✅ Active | `src/ui/render/mod.rs` | `trim: false` used in composer and modal body renders. |
| 52 | `paragraph.scroll((row, col))` | ✅ Active | `src/ui/render/mod.rs`, `src/app/scroll.rs` | Manual vertical scroll. Primary scroll mechanism via `app/scroll.rs` math. |
| 53 | `paragraph.alignment(Alignment)` / `.centered()` | ✅ Active | `src/ui/render/mod.rs` | `Alignment::Left` and `Alignment::Center` used. |
| 54 | `paragraph.style(style)` | ✅ Active | `src/ui/render/mod.rs` | Applied to full widget including status line and picker. |
| 55 | `paragraph.line_count(width)` [unstable] | 🚫 Declared-only feature | — | Feature `unstable-rendered-line-info` enabled in Cargo.toml. `line_count()` never called. Should be used for scroll-max calculation. |

### 2.2 Block

| # | API | Status | Source file(s) | Notes |
| :--- | :--- | :--- | :--- | :--- |
| 56 | `Block::bordered()` | ❌ Gap | — | 0.27+ shorthand. Not used; `Block::default().borders(Borders::ALL)` used instead everywhere. |
| 57 | `Block::default().borders(Borders::ALL)` | ✅ Active | `src/ui/render/mod.rs` | Legacy form. Used for picker overlay and approval modal borders. |
| 58 | `block.title(line)` / `.title_top()` | ⚠️ Partial | `src/ui/render/mod.rs` | `.title("Body")` used (string form). `title_top()` / `title_bottom()` with `Line` alignment not used. |
| 59 | `block.padding(Padding)` | ❌ Gap | — | Inner padding. Not used. |
| 60 | `block.border_type(BorderType)` | ❌ Gap | — | Visual border style variants. Not used. |
| 61 | `block.border_style(style)` | ❌ Gap | — | Separate style for border chars. Not used. |
| 62 | `block.inner(area)` | ✅ Active | `src/ui/render/mod.rs` | Gets usable area inside border. Used correctly in modal render. |

### 2.3 Clear

| # | API | Status | Source file(s) | Notes |
| :--- | :--- | :--- | :--- | :--- |
| 63 | `frame.render_widget(Clear, area)` | ✅ Active | `src/ui/render/mod.rs` | Used before modal/picker overlays. Correct pattern. Two call sites. |

### 2.4 List / ListItem / ListState

| # | API | Status | Source file(s) | Notes |
| :--- | :--- | :--- | :--- | :--- |
| 64 | `List::new(items)` | ❌ Gap | — | StatefulWidget. Not used; file picker and history views use custom Paragraph rendering instead. |
| 65 | `ListItem::new(text)` | ❌ Gap | — | Not used. |
| 66 | `ListState::default().select(i)` | ❌ Gap | — | Not used. |

### 2.5 Table / Row / Cell / TableState

| # | API | Status | Source file(s) | Notes |
| :--- | :--- | :--- | :--- | :--- |
| 67 | `Table::new(rows, widths)` | ❌ Gap | — | Not used; evidence/ledger data rendered as Paragraph. |
| 68 | `Cell::from(text)` | ❌ Gap | — | Not used. Note: `Cell::new()` appears in `src/app/ctor.rs` but is `std::cell::Cell`, not `ratatui::widgets::Cell`. |
| 69 | `TableState` | ❌ Gap | — | Not used. |

### 2.6 Scrollbar / ScrollbarState

| # | API | Status | Source file(s) | Notes |
| :--- | :--- | :--- | :--- | :--- |
| 70 | `Scrollbar::new(ScrollbarOrientation)` | ❌ Gap | — | Visual scrollbar widget. Not used. Long transcripts have no visible scroll indicator. |
| 71 | `ScrollbarState::new(content_len).position(offset)` | ❌ Gap | — | Not used. Scroll position tracked manually in `src/app/scroll.rs`. |

### 2.7 Other Widgets

| # | API | Status | Source file(s) | Notes |
| :--- | :--- | :--- | :--- | :--- |
| 72 | `Tabs::new(titles).select(i)` | ❌ Gap | — | Not used. |
| 73 | `Gauge::default().ratio(f64)` | ❌ Gap | — | Not used. |
| 74 | `Span` as `Widget` (direct render) | ❌ Gap | — | Not used; always wrapped in Paragraph. |
| 75 | `Line` as `Widget` (direct render) | ❌ Gap | — | Not used. |

---

## 3. Widget Traits

| # | API | Status | Source file(s) | Notes |
| :--- | :--- | :--- | :--- | :--- |
| 76 | `impl Widget for MyWidget` (consuming) | ❌ Gap | — | No custom widget structs. All rendering is direct Paragraph/Block calls. |
| 77 | `impl Widget for &MyWidget` (reference, 0.26+) | ❌ Gap | — | Preferred pattern for reusable widgets. Not used. |
| 78 | `impl StatefulWidget` | ❌ Gap | — | No stateful widgets. |
| 79 | `WidgetRef` / `render_widget_ref` [unstable] | 🚫 Declared-only feature | — | Feature `unstable-widget-ref` enabled in Cargo.toml. `render_widget_ref` / `WidgetRef` never called. Key gap: avoids cloning for transcript widgets. |
| 80 | `StatefulWidgetRef` [unstable] | 🚫 Declared-only feature | — | Feature enabled; never used. |

---

## 4. Frame

| # | API | Status | Source file(s) | Notes |
| :--- | :--- | :--- | :--- | :--- |
| 81 | `frame.render_widget(w, area)` | ✅ Active | `src/ui/render/mod.rs`, `src/ui/render/transcript.rs` | Primary render call. 14+ call sites. |
| 82 | `frame.render_stateful_widget(w, area, state)` | ❌ Gap | — | Not used. No stateful widgets. |
| 83 | `frame.render_widget_ref(w, area)` [unstable] | 🚫 Declared-only feature | — | Feature enabled; never called. |
| 84 | `frame.area()` | ✅ Active | `src/ui/render/mod.rs` | Replaces deprecated `frame.size()`. Used correctly. |
| 85 | `frame.set_cursor_position((x, y))` | ✅ Active | `src/ui/render/mod.rs:118` | Used for composer cursor placement. Single call site in `render_input_with_actions`. Crossterm-backed Frame method. |

---

## 5. Layout

| # | API | Status | Source file(s) | Notes |
| :--- | :--- | :--- | :--- | :--- |
| 86 | `Layout::default().direction().constraints().split(area)` | ⚠️ Partial | `src/ui/layout.rs`, `src/ui/render/mod.rs` | Used throughout layout module and render. Legacy `.split()` form returns `Vec<Rect>`. Migration target: use `.areas()` which returns a typed array. |
| 87 | `Layout::vertical(constraints).areas(area)` | ❌ Gap | — | 0.28+ shorthand with typed array return. Not used. |
| 88 | `Layout::horizontal(constraints).areas(area)` | ❌ Gap | — | Not used. |
| 89 | `.flex(Flex::SpaceBetween)` / etc. | ❌ Gap | — | 0.30 CSS-flexbox distribution. Not used. |
| 90 | `Constraint::Fill(weight)` | ❌ Gap | — | 0.28+ proportional fill. Not used; `Min`/`Length` used instead. |
| 91 | `area.inner(Margin { v, h })` | ❌ Gap | — | Inset for scrollbar placement. Not used. |
| 92 | `Layout::try_areas()` | ❌ Gap | — | 0.30 compile-time count check. Not used. |
| 93 | `Rect::split_evenly(n)` | ❌ Gap | — | 0.30 Rect method. Not used. |

---

## 6. Buffer (low-level)

| # | API | Status | Source file(s) | Notes |
| :--- | :--- | :--- | :--- | :--- |
| 94 | `buf.set_string(x, y, str, style)` | ❌ Gap | — | Direct buffer write for custom Widget impls. Not used; no custom Widget impls exist. |
| 95 | `buf.set_line(x, y, line, width)` | ❌ Gap | — | Not used. |
| 96 | `buf.set_style(area, style)` | ❌ Gap | — | Rectangular style fill. Not used. |

---

## 7. Display Lifecycle

| # | API | Status | Source file(s) | Notes |
| :--- | :--- | :--- | :--- | :--- |
| 97 | `ratatui::try_init_with_options(TerminalOptions { viewport })` | ✅ Active | `src/tui_handle.rs` | Inline viewport init. Vexcoder's deliberate primary-screen choice. |
| 98 | `ratatui::try_restore()` | ✅ Active | `src/tui_handle.rs` | Paired with try_init. Installed in panic hook. |
| 99 | `Viewport::Inline(height)` | ✅ Active | `src/tui_handle.rs` | Primary-screen inline rendering preserving host scrollback. |
| 100 | `insert_before(height, fn)` | ✅ Active | `src/tui_handle.rs:45` | Core streaming insert API. Wrapped in `insert_before_lines()`. |
| 101 | `frame.set_cursor_position((x, y))` | ✅ Active | `src/ui/render/mod.rs:118` | See item 85. Listed here for cross-reference with PR-400 Batch E (input-side claims). Crossterm-bridge method on Frame; cursor placement within the inline composer is the only call site. |

---

## 8. Summary Statistics

| Status | Count |
| :--- | :--- |
| ✅ Active | 17 |
| ⚠️ Partial | 5 |
| 🚫 Declared-only feature | 5 |
| ❌ Gap | 74 |
| **Total** | **101** |

### Declared-only feature gaps (highest priority — feature already declared, zero cost to use)

| Feature in Cargo.toml | API | What it unlocks |
| :--- | :--- | :--- |
| `unstable-widget-ref` | `render_widget_ref` / `WidgetRef` | Reference-based rendering; avoids clone on every frame for transcript widgets |
| `unstable-widget-ref` | `StatefulWidgetRef` | Reference stateful rendering |
| `unstable-rendered-line-info` | `paragraph.line_count(width)` | Correct scroll-max calculation without manual line-count math |
| `unstable-backend-writer` | Direct backend writer access | Custom ANSI passthrough if needed |

### Top migration targets (style modernisation, no behaviour change)

| Current pattern | Target pattern | Call sites |
| :--- | :--- | :--- |
| `Style::default().fg(Color::X)` | `Style::new().fg(Color::X)` or `"text".cyan()` | 68 |
| `Style::default().add_modifier(Modifier::BOLD)` | `"text".bold()` via Stylize | 44 |
| `Block::default().borders(Borders::ALL)` | `Block::bordered()` | 5 |
| `Layout::default()...split(area)` | `Layout::vertical([...]).areas(area)` | 6 (layout.rs + render/mod.rs) |
| `Vec<Line>` + `Text::from()` in transcript | `text.push_line()` / `text.extend()` | transcript.rs assembly |