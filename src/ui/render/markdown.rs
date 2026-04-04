use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use syntect::highlighting::{Theme, ThemeSet};
use syntect::parsing::SyntaxSet;

/// Parse a markdown string into styled ratatui `Line`s for transcript rendering.
/// Handles headings, bold, italic, code spans, and fenced code blocks with
/// syntax highlighting via `syntect`.
pub fn markdown_to_lines(input: &str) -> Vec<Line<'static>> {
    let ss = SyntaxSet::load_defaults_newlines();
    let ts = ThemeSet::load_defaults();
    let theme = &ts.themes["base16-ocean.dark"];

    let parser = Parser::new(input);
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut current_spans: Vec<Span<'static>> = Vec::new();
    let mut style_stack: Vec<Style> = vec![Style::default().fg(Color::White)];
    let mut in_code_block = false;
    let mut code_lang = String::new();
    let mut code_buffer = String::new();

    for event in parser {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                flush_line(&mut current_spans, &mut lines);
                let color = match level {
                    pulldown_cmark::HeadingLevel::H1 => Color::Yellow,
                    pulldown_cmark::HeadingLevel::H2 => Color::Cyan,
                    _ => Color::Green,
                };
                style_stack.push(Style::default().fg(color).add_modifier(Modifier::BOLD));
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
                highlight_code_block(&ss, theme, &code_lang, &code_buffer, &mut lines);
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
                current_spans.push(Span::styled(
                    format!("`{code}`"),
                    Style::default()
                        .fg(Color::Magenta)
                        .add_modifier(Modifier::BOLD),
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
                current_spans.push(Span::styled("  • ", Style::default().fg(Color::Yellow)));
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

fn current_style(stack: &[Style]) -> Style {
    stack.last().copied().unwrap_or_default()
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
                    Style::default().fg(Color::Gray),
                ));
            }
        }
    }
}

fn syntect_to_ratatui_style(style: syntect::highlighting::Style) -> Style {
    let fg = Color::Rgb(style.foreground.r, style.foreground.g, style.foreground.b);
    Style::default().fg(fg)
}
