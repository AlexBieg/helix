use helix_core::syntax::OverlayHighlights;
use helix_core::RopeSlice;
use helix_view::Theme;

use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use std::ops::Range;

/// Track open tags to know what style to apply to text content.
#[derive(Debug, Clone)]
enum InlineTag {
    Heading(HeadingLevel),
    Emphasis,
    Strong,
    Strikethrough,
    Link,
    Image,
    CodeBlock,
    Blockquote,
    List,
    Item,
    Paragraph,
}

fn tag_to_inline(tag: &Tag) -> InlineTag {
    match tag {
        Tag::Heading { level, .. } => InlineTag::Heading(*level),
        Tag::BlockQuote(_) => InlineTag::Blockquote,
        Tag::CodeBlock(_) => InlineTag::CodeBlock,
        Tag::List(_) => InlineTag::List,
        Tag::Item => InlineTag::Item,
        Tag::Emphasis => InlineTag::Emphasis,
        Tag::Strong => InlineTag::Strong,
        Tag::Strikethrough => InlineTag::Strikethrough,
        Tag::Link { .. } => InlineTag::Link,
        Tag::Image { .. } => InlineTag::Image,
        Tag::Paragraph => InlineTag::Paragraph,
        _ => InlineTag::Paragraph,
    }
}

/// Parse markdown text and produce overlay highlights that style markdown
/// constructs inline on the source text.
///
/// Syntax markers (like `#`, `**`, `` ` ``) are rendered dimmed, while
/// content (headings, bold/italic text, code, links) receives appropriate
/// styling from the theme.
///
/// Returns overlay highlights and a set of document line numbers that are
/// inside fenced code blocks (for full-width background decorations).
pub fn inline_markdown_overlays(
    text: RopeSlice,
    theme: &Theme,
) -> (Option<OverlayHighlights>, Vec<std::ops::Range<usize>>) {
    let mut highlights: Vec<(helix_core::syntax::Highlight, Range<usize>)> = Vec::new();
    let mut code_block_line_ranges: Vec<std::ops::Range<usize>> = Vec::new();

    // Helper: try scopes in order. All but the last are exact-match only
    // (to prevent unwanted hierarchical fallback like heading.marker → heading).
    // The last scope uses hierarchical fallback as a safety net.
    let try_scope = |scopes: &[&str]| -> Option<helix_core::syntax::Highlight> {
        let (last, rest) = scopes.split_last()?;
        for s in rest {
            if let Some(h) = theme.find_highlight_exact(s) {
                return Some(h);
            }
        }
        theme.find_highlight(last)
    };

    // Marker scopes — try user scope first, then tree-sitter scopes, then generic fallbacks
    let heading_marker = try_scope(&[
        "markup.inline.marker",
        "markup.heading.marker",
        "ui.virtual",
    ]);
    let bracket_marker = try_scope(&[
        "markup.inline.marker",
        "punctuation.bracket",
        "ui.virtual",
        "punctuation.delimiter",
    ]);
    let special_marker = try_scope(&[
        "markup.inline.marker",
        "punctuation.special",
        "ui.virtual",
        "punctuation.delimiter",
    ]);

    let heading_highlights: [Option<helix_core::syntax::Highlight>; 6] = [
        try_scope(&["markup.heading.1", "markup.heading"]),
        try_scope(&["markup.heading.2", "markup.heading"]),
        try_scope(&["markup.heading.3", "markup.heading"]),
        try_scope(&["markup.heading.4", "markup.heading"]),
        try_scope(&["markup.heading.5", "markup.heading"]),
        try_scope(&["markup.heading.6", "markup.heading"]),
    ];
    let bold_highlight = try_scope(&["markup.bold"]);
    let italic_highlight = try_scope(&["markup.italic"]);
    let strikethrough_highlight = try_scope(&["markup.strikethrough"]);
    let code_highlight = try_scope(&["markup.raw.inline", "markup.raw"]);
    let link_text_highlight = try_scope(&["markup.link.text", "markup.link"]);
    let link_url_highlight = try_scope(&["markup.link.url", "markup.link"]);
    let list_highlight = try_scope(&["markup.list.unnumbered", "markup.list"]);
    let quote_highlight = try_scope(&["markup.quote"]);

    let source = text.to_string();
    let byte_to_char = {
        let mut map = vec![0usize; source.len() + 1];
        let mut char_idx = 0;
        for (byte_idx, _) in source.char_indices() {
            map[byte_idx] = char_idx;
            char_idx += 1;
        }
        map[source.len()] = char_idx;
        map
    };

    let byte_range_to_char_range = |byte_range: Range<usize>| -> Range<usize> {
        let start = byte_to_char[byte_range.start.min(byte_to_char.len() - 1)];
        let end = byte_to_char[byte_range.end.min(byte_to_char.len() - 1)];
        start..end
    };

    // Build byte-to-line mapping for code block background decoration
    let byte_to_line: Vec<usize> = {
        let mut map = vec![0usize; source.len() + 1];
        let mut line = 0;
        for (byte_idx, ch) in source.char_indices() {
            map[byte_idx] = line;
            if ch == '\n' {
                line += 1;
            }
        }
        map[source.len()] = line;
        map
    };
    let byte_range_to_line_range = |byte_range: std::ops::Range<usize>| -> std::ops::Range<usize> {
        let start = *byte_to_line.get(byte_range.start).unwrap_or(&0);
        let end = *byte_to_line.get(byte_range.end.min(byte_to_line.len() - 1)).unwrap_or(&0);
        start..end
    };

    let push_highlight = |highlights: &mut Vec<_>, opt_hl: Option<helix_core::syntax::Highlight>, range: Range<usize>| {
        if let Some(hl) = opt_hl {
            if range.start < range.end {
                highlights.push((hl, range));
            }
        }
    };

    let mut tag_stack: Vec<InlineTag> = Vec::new();
    let mut marker_len_stack: Vec<usize> = Vec::new();

    /// Detect the length of an emphasis/strong/strikethrough marker at a
    /// given byte position in the source.
    fn marker_len_at(source: &str, pos: usize) -> usize {
        let bytes = source.as_bytes();
        if pos + 1 >= bytes.len() {
            return 0;
        }
        match bytes[pos] {
            b'*' | b'_' => {
                if bytes[pos + 1] == bytes[pos] { 2 } else { 1 }
            }
            b'~' => {
                if bytes[pos + 1] == b'~' { 2 } else { 0 }
            }
            _ => 0,
        }
    }

    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    let parser = Parser::new_ext(&source, options);

    for (event, byte_range) in parser.into_offset_iter() {
        // Skip empty ranges
        if byte_range.is_empty() {
            // But still track tag stack changes
            match &event {
                Event::Start(tag) => {
                    tag_stack.push(tag_to_inline(tag));
                    if matches!(tag, Tag::Emphasis | Tag::Strong | Tag::Strikethrough) {
                        marker_len_stack.push(0); // empty range, marker length unknown
                    }
                }
                Event::End(tag) => {
                    if matches!(tag, TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough) {
                        marker_len_stack.pop();
                    }
                    if matches!(tag, TagEnd::Heading(..) | TagEnd::Paragraph | TagEnd::Item) {
                        tag_stack.retain(|t| !matches!(t, InlineTag::Heading(..) | InlineTag::Paragraph | InlineTag::Item));
                    } else {
                        tag_stack.pop();
                    }
                }
                _ => {}
            }
            continue;
        }

        let char_range = byte_range_to_char_range(byte_range.clone());

        match event {
            Event::Start(tag) => {
                match &tag {
                    // Opening markers are dimmed
                    Tag::Heading { .. } => {
                        push_highlight(&mut highlights, heading_marker, char_range.clone());
                    }
                    Tag::BlockQuote(_) => {
                        push_highlight(&mut highlights, special_marker, char_range.clone());
                        push_highlight(&mut highlights, quote_highlight, char_range.clone());
                    }
                    Tag::CodeBlock(_) => {
                        // pulldown-cmark gives the FULL block range, not just
                        // the opening fence. Extract the first line.
                        let fence_end = source[byte_range.start..]
                            .find('\n')
                            .map(|p| byte_range.start + p)
                            .unwrap_or(byte_range.end);
                        if byte_range.start < fence_end {
                            let fence_char_range =
                                byte_range_to_char_range(byte_range.start..fence_end);
                            push_highlight(&mut highlights, bracket_marker, fence_char_range);
                        }
                    }
                    Tag::List(_) => {
                    }
                    Tag::Item => {
                        push_highlight(&mut highlights, list_highlight, char_range.clone());
                    }
                    Tag::Emphasis | Tag::Strong | Tag::Strikethrough => {
                        // pulldown-cmark gives the FULL element range for Start,
                        // not just the opening marker. Extract the marker portion.
                        let marker_len = marker_len_at(&source, byte_range.start);
                        if marker_len > 0 {
                            let marker_byte_range =
                                byte_range.start..byte_range.start + marker_len;
                            let marker_char_range =
                                byte_range_to_char_range(marker_byte_range);
                            push_highlight(&mut highlights, bracket_marker, marker_char_range);
                        }
                        marker_len_stack.push(marker_len);
                    }
                    Tag::Link { .. } | Tag::Image { .. } => {
                        push_highlight(&mut highlights, bracket_marker, char_range);
                    }
                    _ => {}
                }
                tag_stack.push(tag_to_inline(&tag));
            }

            Event::End(tag) => {
                match tag {
                    TagEnd::Heading(..) | TagEnd::Paragraph | TagEnd::Item => {
                        tag_stack.retain(|t| {
                            !matches!(
                                t,
                                InlineTag::Heading(..)
                                    | InlineTag::Paragraph
                                    | InlineTag::Item
                            )
                        });
                    }
                    _ => {
                        tag_stack.pop();
                    }
                }

                match tag {
                    TagEnd::Heading(..) => {
                        push_highlight(&mut highlights, heading_marker, char_range);
                    }
                    TagEnd::BlockQuote(_) => {
                        push_highlight(&mut highlights, special_marker, char_range);
                    }
                    TagEnd::CodeBlock => {
                        // Extract the closing fence (last line) from the full block range.
                        let fence_start = source[..byte_range.end]
                            .rfind('\n')
                            .map(|p| p + 1)
                            .unwrap_or(byte_range.start);
                        if fence_start < byte_range.end {
                            let fence_char_range =
                                byte_range_to_char_range(fence_start..byte_range.end);
                            push_highlight(&mut highlights, bracket_marker, fence_char_range);
                        }
                    }
                    TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough => {
                        // pulldown-cmark gives the FULL element range for End too.
                        // The closing marker is at the end of the range.
                        let marker_len = marker_len_stack.pop().unwrap_or(0);
                        if marker_len > 0 {
                            let marker_start = byte_range.end.saturating_sub(marker_len);
                            if marker_start < byte_range.end {
                                let marker_char_range =
                                    byte_range_to_char_range(marker_start..byte_range.end);
                                push_highlight(&mut highlights, bracket_marker, marker_char_range);
                            }
                        }
                    }
                    TagEnd::Link | TagEnd::Image => {
                        // Split "](url)" into non-overlapping ranges:
                        // "]" bracket_marker, "(" bracket_marker, "url" link_url_highlight, ")" bracket_marker
                        let byte_slice = &source[byte_range.clone()];
                        if let Some(url_start) = byte_slice.find('(') {
                            let close_bracket_end = byte_range.start + url_start; // "]"
                            let open_paren_start = close_bracket_end; // "("
                            let url_byte_start = open_paren_start + 1;
                            let url_byte_end = byte_range.end.saturating_sub(1); // before ")"
                            let close_paren_start = url_byte_end; // ")" start

                            if byte_range.start < close_bracket_end {
                                push_highlight(&mut highlights, bracket_marker,
                                    byte_range_to_char_range(byte_range.start..close_bracket_end));
                            }
                            if close_bracket_end < open_paren_start + 1 {
                                push_highlight(&mut highlights, bracket_marker,
                                    byte_range_to_char_range(open_paren_start..open_paren_start + 1));
                            }
                            if url_byte_start < url_byte_end {
                                push_highlight(&mut highlights, link_url_highlight,
                                    byte_range_to_char_range(url_byte_start..url_byte_end));
                            }
                            if close_paren_start < byte_range.end {
                                push_highlight(&mut highlights, bracket_marker,
                                    byte_range_to_char_range(close_paren_start..byte_range.end));
                            }
                        } else {
                            // No URL — just dim the whole thing
                            push_highlight(&mut highlights, bracket_marker, char_range);
                        }
                    }
                    _ => {}
                }
            }

            Event::Text(_text) => {
                let current_style = tag_stack.last();
                match current_style {
                    Some(InlineTag::Heading(level)) => {
                        let idx = match level {
                            HeadingLevel::H1 => 0, HeadingLevel::H2 => 1,
                            HeadingLevel::H3 => 2, HeadingLevel::H4 => 3,
                            HeadingLevel::H5 => 4, HeadingLevel::H6 => 5,
                        };
                        push_highlight(&mut highlights, heading_highlights[idx], char_range);
                    }
                    Some(InlineTag::Strong) => {
                        push_highlight(&mut highlights, bold_highlight, char_range);
                    }
                    Some(InlineTag::Emphasis) => {
                        push_highlight(&mut highlights, italic_highlight, char_range);
                    }
                    Some(InlineTag::Strikethrough) => {
                        push_highlight(&mut highlights, strikethrough_highlight, char_range);
                    }
                    Some(InlineTag::Link) | Some(InlineTag::Image) => {
                        // Link text — style as link text
                        push_highlight(&mut highlights, link_text_highlight, char_range);
                    }
                    Some(InlineTag::CodeBlock) => {
                        // Let tree-sitter's language injection handle syntax
                        // highlighting — don't override with a single color.
                        // Track line ranges for full-width background decoration.
                        // pulldown-cmark's Text range may include the closing
                        // fence's first byte; trim the last line to exclude it.
                        let mut line_range =
                            byte_range_to_line_range(byte_range.clone());
                        if line_range.end > line_range.start {
                            line_range.end = line_range.end.saturating_sub(1);
                        }
                        if line_range.start < line_range.end {
                            code_block_line_ranges.push(line_range);
                        }
                    }
                    _ => {
                        // Plain text — no special styling
                    }
                }
            }

            Event::Code(_text) => {
                // Inline code: `` `code` `` — the whole range includes backticks.
                // Style backticks as marker, content as code.
                let byte_start = byte_range.start;
                let byte_end = byte_range.end;

                // Find backtick boundaries: count leading backticks
                let source_bytes = source.as_bytes();
                let mut bt_count = 0;
                while byte_start + bt_count < byte_end
                    && source_bytes[byte_start + bt_count] == b'`'
                {
                    bt_count += 1;
                }

                let mut trailing_bt = 0;
                while byte_end > byte_start + bt_count + trailing_bt
                    && source_bytes[byte_end - 1 - trailing_bt] == b'`'
                {
                    trailing_bt += 1;
                }

                if bt_count > 0 {
                    let open_range = byte_range_to_char_range(byte_start..byte_start + bt_count);
                    push_highlight(&mut highlights, bracket_marker, open_range);
                }
                if trailing_bt > 0 {
                    let close_range = byte_range_to_char_range(byte_end - trailing_bt..byte_end);
                    push_highlight(&mut highlights, bracket_marker, close_range);
                }

                let content_start = byte_start + bt_count;
                let content_end = byte_end - trailing_bt;
                if content_start < content_end {
                    let content_char_range = byte_range_to_char_range(content_start..content_end);
                    push_highlight(&mut highlights, code_highlight, content_char_range);
                }
            }

            Event::Html(_text) | Event::InlineHtml(_text) => {
                push_highlight(&mut highlights, special_marker, char_range);
            }

            Event::Rule => {
                push_highlight(&mut highlights, special_marker, char_range);
            }

            Event::SoftBreak | Event::HardBreak => {
            }

            Event::TaskListMarker(_checked) => {
                push_highlight(&mut highlights, bracket_marker, char_range);
            }

            Event::FootnoteReference(_) => {
                push_highlight(&mut highlights, bracket_marker, char_range);
            }

            Event::InlineMath(_) | Event::DisplayMath(_) => {
                push_highlight(&mut highlights, special_marker, char_range);
            }
        }
    }

    if highlights.is_empty() {
        return (None, code_block_line_ranges);
    }

    // Merge overlapping/adjacent ranges for the same highlight to reduce
    // overhead.
    highlights.sort_by(|a, b| a.1.start.cmp(&b.1.start).then_with(|| a.0.idx().cmp(&b.0.idx())));

    let mut merged: Vec<(helix_core::syntax::Highlight, Range<usize>)> = Vec::new();
    for (hl, range) in highlights {
        if let Some(last) = merged.last_mut() {
            if last.0 == hl && last.1.end >= range.start {
                last.1.end = last.1.end.max(range.end);
                continue;
            }
        }
        merged.push((hl, range));
    }

    (Some(OverlayHighlights::Heterogenous { highlights: merged }), code_block_line_ranges)
}

/// Check if a document language is markdown.
pub fn is_markdown_language(language_id: Option<&str>) -> bool {
    language_id == Some("markdown") || language_id == Some("md")
}

#[cfg(test)]
mod tests {
    use super::*;
    use helix_core::Rope;
    use helix_view::theme::DEFAULT_THEME;

    fn test_theme() -> Theme {
        // The DEFAULT_THEME has basic UI scopes but not tree-sitter highlight
        // scopes (those are loaded dynamically). Our function gracefully
        // returns None when scopes aren't available.
        DEFAULT_THEME.clone()
    }

    #[test]
    fn test_empty() {
        let text = RopeSlice::from("");
        let theme = test_theme();
        assert!(inline_markdown_overlays(text, &theme).0.is_none());
    }

    #[test]
    fn test_is_markdown_language() {
        assert!(is_markdown_language(Some("markdown")));
        assert!(is_markdown_language(Some("md")));
        assert!(!is_markdown_language(Some("rust")));
        assert!(!is_markdown_language(None));
    }

    #[test]
    fn test_parse_does_not_crash_headings() {
        let rope = Rope::from("# Hello\n## World\n");
        let text = rope.slice(..);
        let theme = test_theme();
        // With default theme (no tree-sitter scopes), returns None without crashing
        let _result = inline_markdown_overlays(text, &theme);
    }

    #[test]
    fn test_parse_does_not_crash_bold_italic() {
        let rope = Rope::from("This is **bold** and *italic* text.\n");
        let text = rope.slice(..);
        let theme = test_theme();
        let _result = inline_markdown_overlays(text, &theme);
    }

    #[test]
    fn test_parse_does_not_crash_code_blocks() {
        let rope = Rope::from("```rust\nfn main() {}\n```\n");
        let text = rope.slice(..);
        let theme = test_theme();
        let _result = inline_markdown_overlays(text, &theme);
    }

    #[test]
    fn test_parse_does_not_crash_links() {
        let rope = Rope::from("[click here](https://example.com)\n");
        let text = rope.slice(..);
        let theme = test_theme();
        let _result = inline_markdown_overlays(text, &theme);
    }

    #[test]
    fn test_parse_does_not_crash_mixed() {
        let rope = Rope::from("# Heading\n\nThis is **bold** and *italic*.\n\n```rust\nlet x = 1;\n```\n\n> A blockquote\n\n- List item 1\n- List item 2\n\n[link](https://example.com)\n");
        let text = rope.slice(..);
        let theme = test_theme();
        let _result = inline_markdown_overlays(text, &theme);
    }
}
