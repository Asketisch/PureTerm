//! Markdown → structured blocks for panel rendering.
//!
//! pulldown-cmark parses CommonMark; we project its event stream into a flat
//! block list ([`Block::Paragraph`] of inline [`MarkdownSpan`]s, or
//! [`Block::CodeBlock`]) that the panel renders without re-parsing. Code blocks
//! stay as raw monospace text (syntax highlighting via `syntect` is deferred to
//! a later GAP).
//!
//! **UI-agnostic**: produces plain structs; the panel maps them onto iced rich
//! text. This keeps the parsing logic fully unit-testable with no iced types.

/// Inline text style. `Code` is inline `` `code` ``; code-*block* content is
/// carried in [`Block::CodeBlock`], not as a span.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SpanStyle {
    #[default]
    Normal,
    Bold,
    Italic,
    BoldItalic,
    Code,
}

/// One styled text run within a paragraph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkdownSpan {
    pub text: String,
    pub style: SpanStyle,
}

/// A top-level block. The panel renders paragraphs as styled text and code
/// blocks as a monospace box.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    Paragraph(Vec<MarkdownSpan>),
    CodeBlock { lang: Option<String>, code: String },
}

/// Parse CommonMark `md` into renderable blocks. Empty input → no blocks.
pub fn parse(md: &str) -> Vec<Block> {
    use pulldown_cmark::{CodeBlockKind, Event, Parser, Tag, TagEnd};

    let mut blocks = Vec::new();
    let mut para: Vec<MarkdownSpan> = Vec::new();
    let mut bold = false;
    let mut italic = false;
    let mut in_code_block = false;
    let mut code_buf = String::new();
    let mut code_lang: Option<String> = None;

    for event in Parser::new(md) {
        match event {
            Event::Start(Tag::Paragraph) => {
                // Fresh paragraph — discard any stray inline content from an
                // unhandled block (heading/list) so it can't bleed in.
                para.clear();
            }
            Event::End(TagEnd::Paragraph) => flush_into(&mut para, &mut blocks),

            Event::Start(Tag::CodeBlock(kind)) => {
                // A code block closes any in-flight paragraph first.
                flush_into(&mut para, &mut blocks);
                in_code_block = true;
                code_buf.clear();
                code_lang = match kind {
                    CodeBlockKind::Fenced(info) => {
                        // Info string is `lang` optionally followed by more; take
                        // the first whitespace-delimited token as the language.
                        let lang = info.as_ref().split_whitespace().next();
                        lang.map(|s| s.to_string())
                    }
                    CodeBlockKind::Indented => None,
                };
            }
            Event::End(TagEnd::CodeBlock) => {
                in_code_block = false;
                blocks.push(Block::CodeBlock {
                    lang: code_lang.take(),
                    code: std::mem::take(&mut code_buf),
                });
            }

            Event::Start(Tag::Strong) => bold = true,
            Event::End(TagEnd::Strong) => bold = false,
            Event::Start(Tag::Emphasis) => italic = true,
            Event::End(TagEnd::Emphasis) => italic = false,

            Event::Text(t) => {
                if in_code_block {
                    code_buf.push_str(t.as_ref());
                } else {
                    let style = match (bold, italic) {
                        (true, true) => SpanStyle::BoldItalic,
                        (true, false) => SpanStyle::Bold,
                        (false, true) => SpanStyle::Italic,
                        (false, false) => SpanStyle::Normal,
                    };
                    para.push(MarkdownSpan {
                        text: t.to_string(),
                        style,
                    });
                }
            }
            Event::Code(t) => para.push(MarkdownSpan {
                text: t.to_string(),
                style: SpanStyle::Code,
            }),
            Event::SoftBreak | Event::HardBreak => {
                para.push(MarkdownSpan {
                    text: "\n".into(),
                    style: SpanStyle::Normal,
                })
            }

            // Headings, lists, blockquotes, tables, links, etc. — best-effort in
            // v1: their inline text is gathered into `para` and flushed at the
            // next paragraph boundary (or end of input).
            _ => {}
        }
    }
    flush_into(&mut para, &mut blocks);
    blocks
}

/// Move accumulated inline spans into a `Paragraph` block (no-op if empty).
fn flush_into(para: &mut Vec<MarkdownSpan>, blocks: &mut Vec<Block>) {
    if !para.is_empty() {
        blocks.push(Block::Paragraph(std::mem::take(para)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(text: &str, style: SpanStyle) -> MarkdownSpan {
        MarkdownSpan {
            text: text.into(),
            style,
        }
    }

    #[test]
    fn plain_text_becomes_one_normal_span() {
        assert_eq!(
            parse("hello world"),
            vec![Block::Paragraph(vec![span("hello world", SpanStyle::Normal)])],
        );
    }

    #[test]
    fn bold_text_is_marked_bold() {
        assert_eq!(
            parse("**important**"),
            vec![Block::Paragraph(vec![span("important", SpanStyle::Bold)])],
        );
    }

    #[test]
    fn italic_text_is_marked_italic() {
        assert_eq!(
            parse("*emphasis*"),
            vec![Block::Paragraph(vec![span("emphasis", SpanStyle::Italic)])],
        );
    }

    #[test]
    fn bold_italic_is_both() {
        assert_eq!(
            parse("***both***"),
            vec![Block::Paragraph(vec![span("both", SpanStyle::BoldItalic)])],
        );
    }

    #[test]
    fn inline_code_is_marked_code() {
        assert_eq!(
            parse("run `ls` now"),
            vec![Block::Paragraph(vec![
                span("run ", SpanStyle::Normal),
                span("ls", SpanStyle::Code),
                span(" now", SpanStyle::Normal),
            ])],
        );
    }

    #[test]
    fn fenced_code_block_without_lang() {
        assert_eq!(
            parse("```\nfn main() {}\n```"),
            vec![Block::CodeBlock {
                lang: None,
                code: "fn main() {}\n".into(),
            }],
        );
    }

    #[test]
    fn fenced_code_block_with_lang() {
        assert_eq!(
            parse("```rust\nlet x = 1;\n```"),
            vec![Block::CodeBlock {
                lang: Some("rust".into()),
                code: "let x = 1;\n".into(),
            }],
        );
    }

    #[test]
    fn paragraph_then_code_block_then_paragraph() {
        let md = "intro\n\n```\ncode\n```\n\noutro";
        assert_eq!(
            parse(md),
            vec![
                Block::Paragraph(vec![span("intro", SpanStyle::Normal)]),
                Block::CodeBlock {
                    lang: None,
                    code: "code\n".into(),
                },
                Block::Paragraph(vec![span("outro", SpanStyle::Normal)]),
            ],
        );
    }

    #[test]
    fn empty_input_yields_no_blocks() {
        assert!(parse("").is_empty());
    }
}
