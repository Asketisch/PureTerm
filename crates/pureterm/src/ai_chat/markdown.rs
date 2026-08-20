//! Markdown → 用于面板渲染的结构化块。
//!
//! 由 pulldown-cmark 解析 CommonMark;我们把事件流投影为扁平的
//! 块列表(内联 [`MarkdownSpan`] 组成的 [`Block::Paragraph`],或
//! [`Block::CodeBlock`]),面板直接渲染而无需重新解析。代码块
//! 保持为原始等宽文本(经 `syntect` 的语法高亮延后到
//! 后续 GAP 实现)。
//!
//! **与 UI 无关**:只产出普通结构体,由面板映射为 iced 富
//! 文本。这样解析逻辑完全不依赖 iced 类型,可完整单元测试。

/// 内联文本样式。`Code` 表示行内 `` `code` ``;代码块内容
/// 由 [`Block::CodeBlock`] 承载,不作为 span 出现。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SpanStyle {
    #[default]
    Normal,
    Bold,
    Italic,
    BoldItalic,
    Code,
}

/// 段落内的一段带样式的文本。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkdownSpan {
    pub text: String,
    pub style: SpanStyle,
}

/// 顶层块。面板将段落渲染为带样式文本,代码块
/// 渲染为等宽盒子。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    Paragraph(Vec<MarkdownSpan>),
    CodeBlock { lang: Option<String>, code: String },
}

/// 将 CommonMark `md` 解析为可渲染的块。空输入 → 无块。
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
                // 新段落——丢弃来自未处理块(标题/列表)的残留
                // 行内内容,避免其渗入。
                para.clear();
            }
            Event::End(TagEnd::Paragraph) => flush_into(&mut para, &mut blocks),

            Event::Start(Tag::CodeBlock(kind)) => {
                // 代码块会先结束进行中的段落。
                flush_into(&mut para, &mut blocks);
                in_code_block = true;
                code_buf.clear();
                code_lang = match kind {
                    CodeBlockKind::Fenced(info) => {
                        // 信息字符串是 `lang` 可选地后跟更多内容;取
                        // 第一个以空白分隔的 token 作为语言。
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

            // 标题、列表、引用、表格、链接等——v1 尽力而为:
            // 其行内文本统一收集进 `para`,并在下一个段落边界
            // (或输入结束)时输出。
            _ => {}
        }
    }
    flush_into(&mut para, &mut blocks);
    blocks
}

/// 将累积的行内 span 移入 `Paragraph` 块(为空时无操作)。
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
