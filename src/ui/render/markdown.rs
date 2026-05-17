use palette::{Mix, Srgb};
use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use std::sync::OnceLock;
use syntect::highlighting::{Theme, ThemeSet};
use syntect::parsing::SyntaxSet;

use crate::ui::tui::{
    line, span,
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

static MARKDOWN_SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();
static MARKDOWN_THEME_SET: OnceLock<ThemeSet> = OnceLock::new();
static MARKDOWN_SEMANTIC_PALETTE: OnceLock<MarkdownSemanticPalette> = OnceLock::new();

#[derive(Clone, Copy)]
struct MarkdownSemanticPalette {
    heading_h1: Color,
    heading_h2: Color,
    heading_h3: Color,
    bullet: Color,
    inline_code: Color,
    code_fallback: Color,
}

impl MarkdownSemanticPalette {
    fn heading_color(self, level: pulldown_cmark::HeadingLevel) -> Color {
        match level {
            pulldown_cmark::HeadingLevel::H1 => self.heading_h1,
            pulldown_cmark::HeadingLevel::H2 => self.heading_h2,
            _ => self.heading_h3,
        }
    }
}

pub fn markdown_to_inline_line(input: &str) -> Option<Line<'static>> {
    let mut lines = markdown_to_lines(input)
        .into_iter()
        .filter(|line| line.spans.iter().any(|span| !span.content.is_empty()))
        .collect::<Vec<_>>();
    if lines.len() == 1 { lines.pop() } else { None }
}

pub fn markdown_to_lines(input: &str) -> Vec<Line<'static>> {
    let ss = markdown_syntax_set();
    let theme = markdown_theme();
    let palette = markdown_semantic_palette();

    let parser = Parser::new(input);
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut current_spans: Vec<Span<'static>> = Vec::new();
    let mut style_stack: Vec<Style> = vec![Style::new().fg(Color::White)];
    let mut in_code_block = false;
    let mut code_lang = String::new();
    let mut code_buffer = String::new();

    for node in parser {
        match node {
            Event::Start(Tag::Heading { level, .. }) => {
                flush_line(&mut current_spans, &mut lines);
                let color = palette.heading_color(level);
                style_stack.push(Style::new().fg(color).add_modifier(Modifier::BOLD));
            }
            Event::End(TagEnd::Heading(_)) => {
                style_stack.pop();
                flush_line(&mut current_spans, &mut lines);
            }
            Event::Start(Tag::Emphasis) => {
                style_stack.push(current_style(&style_stack).add_modifier(Modifier::ITALIC));
            }
            Event::End(TagEnd::Emphasis) => {
                style_stack.pop();
            }
            Event::Start(Tag::Strong) => {
                style_stack.push(current_style(&style_stack).add_modifier(Modifier::BOLD));
            }
            Event::End(TagEnd::Strong) => {
                style_stack.pop();
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                flush_line(&mut current_spans, &mut lines);
                in_code_block = true;
                code_buffer.clear();
                code_lang = match kind {
                    pulldown_cmark::CodeBlockKind::Fenced(lang) => lang.to_string(),
                    pulldown_cmark::CodeBlockKind::Indented => String::new(),
                };
            }
            Event::End(TagEnd::CodeBlock) => {
                if let Some(theme) = theme {
                    highlight_code_block(ss, theme, &code_lang, &code_buffer, &mut lines);
                } else {
                    for source_line in code_buffer.lines() {
                        lines.push(line![
                            span!(Style::new().fg(palette.code_fallback); "{source_line}")
                        ]);
                    }
                }
                in_code_block = false;
                code_buffer.clear();
            }
            Event::Text(text) => {
                if in_code_block {
                    code_buffer.push_str(&text);
                } else {
                    current_spans.push(Span::styled(text.to_string(), current_style(&style_stack)));
                }
            }
            Event::Code(code) => {
                current_spans.push(span!(
                    Style::new()
                        .fg(palette.inline_code)
                        .add_modifier(Modifier::BOLD);
                    "`{code}`"
                ));
            }
            Event::SoftBreak | Event::HardBreak => {
                flush_line(&mut current_spans, &mut lines);
            }
            Event::Start(Tag::Paragraph) => {}
            Event::End(TagEnd::Paragraph) => {
                flush_line(&mut current_spans, &mut lines);
                lines.push(Line::from(""));
            }
            Event::Start(Tag::Item) => {
                current_spans.push(span!(palette.bullet; "  • "));
            }
            Event::End(TagEnd::Item) => {
                flush_line(&mut current_spans, &mut lines);
            }
            _ => {}
        }
    }
    flush_line(&mut current_spans, &mut lines);
    lines
}

fn markdown_syntax_set() -> &'static SyntaxSet {
    MARKDOWN_SYNTAX_SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

fn markdown_theme() -> Option<&'static Theme> {
    let themes = MARKDOWN_THEME_SET.get_or_init(ThemeSet::load_defaults);
    themes
        .themes
        .get("base16-ocean.dark")
        .or_else(|| themes.themes.values().next())
}

fn markdown_semantic_palette() -> &'static MarkdownSemanticPalette {
    MARKDOWN_SEMANTIC_PALETTE.get_or_init(|| {
        let ocean = srgb(0x8f, 0xbc, 0xbb);
        let amber = srgb(0xeb, 0xcb, 0x8b);
        let sky = srgb(0x81, 0xa1, 0xc1);
        let moss = srgb(0xa3, 0xbe, 0x8c);
        let blossom = srgb(0xb4, 0x8e, 0xad);
        let slate = srgb(0x4c, 0x56, 0x6a);

        MarkdownSemanticPalette {
            heading_h1: ratatui_color(ocean.mix(amber, 0.8)),
            heading_h2: ratatui_color(ocean.mix(sky, 0.65)),
            heading_h3: ratatui_color(ocean.mix(moss, 0.6)),
            bullet: ratatui_color(ocean.mix(amber, 0.55)),
            inline_code: ratatui_color(ocean.mix(blossom, 0.7)),
            code_fallback: ratatui_color(slate.mix(sky, 0.35)),
        }
    })
}

fn current_style(stack: &[Style]) -> Style {
    stack.last().copied().unwrap_or_default()
}

fn srgb(red: u8, green: u8, blue: u8) -> Srgb<f32> {
    Srgb::new(
        red as f32 / 255.0,
        green as f32 / 255.0,
        blue as f32 / 255.0,
    )
}

fn ratatui_color(color: Srgb<f32>) -> Color {
    let bytes = color.into_format::<u8>();
    Color::Rgb(bytes.red, bytes.green, bytes.blue)
}

fn flush_line(spans: &mut Vec<Span<'static>>, lines: &mut Vec<Line<'static>>) {
    if !spans.is_empty() {
        lines.push(Line::from(std::mem::take(spans)));
    }
}

fn highlight_code_block(
    ss: &SyntaxSet,
    theme: &Theme,
    lang: &str,
    code: &str,
    lines: &mut Vec<Line<'static>>,
) {
    let syntax = ss
        .find_syntax_by_token(lang)
        .unwrap_or_else(|| ss.find_syntax_plain_text());
    let mut highlighter = syntect::easy::HighlightLines::new(syntax, theme);

    for source_line in code.lines() {
        match highlighter.highlight_line(source_line, ss) {
            Ok(ranges) => {
                let spans: Vec<Span<'static>> = ranges
                    .into_iter()
                    .map(|(style, text)| {
                        Span::styled(text.to_string(), syntect_to_ratatui_style(style))
                    })
                    .collect();
                lines.push(Line::from(spans));
            }
            Err(_) => {
                lines.push(Line::styled(
                    source_line.to_string(),
                    Style::new().fg(markdown_semantic_palette().code_fallback),
                ));
            }
        }
    }
}

fn syntect_to_ratatui_style(style: syntect::highlighting::Style) -> Style {
    let fg = Color::Rgb(style.foreground.r, style.foreground.g, style.foreground.b);
    Style::new().fg(fg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_to_inline_line_renders_heading_without_blank_trailer() {
        let line = markdown_to_inline_line("## Heading").expect("single heading line");
        let text: String = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();

        assert_eq!(text, "Heading");
        assert_eq!(
            line.spans[0].style.fg,
            Some(markdown_semantic_palette().heading_h2)
        );
    }

    #[test]
    fn markdown_to_inline_line_rejects_fenced_code_scaffold() {
        assert!(markdown_to_inline_line("```rust").is_none());
    }

    #[test]
    fn markdown_to_lines_keeps_code_blocks_renderable() {
        let lines = markdown_to_lines("```rust\nfn main() {}\n```");

        assert!(!lines.is_empty());
    }

    #[test]
    fn markdown_to_inline_line_uses_semantic_palette_for_inline_code() {
        let line = markdown_to_inline_line("Use `cargo fmt`").expect("single inline-code line");
        let code_span = line
            .spans
            .iter()
            .find(|span| span.content.as_ref().contains("`cargo fmt`"))
            .expect("inline code span");

        assert_eq!(
            code_span.style.fg,
            Some(markdown_semantic_palette().inline_code)
        );
    }
}
