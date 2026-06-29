//! Tree-sitter AST chunking for Rust, Python, and JavaScript.

use tree_sitter::{Language, Node, Parser, Tree};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceLanguage {
    Rust,
    Python,
    JavaScript,
}

impl SourceLanguage {
    pub fn from_path(path: &std::path::Path) -> Option<Self> {
        match path.extension()?.to_str()? {
            "rs" => Some(Self::Rust),
            "py" => Some(Self::Python),
            "js" | "jsx" | "mjs" | "cjs" => Some(Self::JavaScript),
            _ => None,
        }
    }

    fn tree_sitter_language(self) -> Language {
        match self {
            Self::Rust => tree_sitter_rust::LANGUAGE.into(),
            Self::Python => tree_sitter_python::LANGUAGE.into(),
            Self::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
        }
    }

    fn chunk_node_kinds(self) -> &'static [&'static str] {
        match self {
            Self::Rust => &[
                "function_item",
                "impl_item",
                "struct_item",
                "enum_item",
                "trait_item",
                "mod_item",
            ],
            Self::Python => &["function_definition", "class_definition"],
            Self::JavaScript => &[
                "function_declaration",
                "class_declaration",
                "method_definition",
            ],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawChunk {
    pub symbol_name: Option<String>,
    pub symbol_kind: String,
    pub start_line: u32,
    pub end_line: u32,
    pub start_byte: u32,
    pub end_byte: u32,
    pub content: String,
}

pub fn chunk_source(lang: SourceLanguage, source: &str) -> Vec<RawChunk> {
    let mut parser = Parser::new();
    if parser.set_language(&lang.tree_sitter_language()).is_err() {
        return whole_file_chunk(source);
    }
    let tree = match parser.parse(source, None) {
        Some(t) => t,
        None => return whole_file_chunk(source),
    };
    let mut out = Vec::new();
    collect_chunks(
        lang,
        source,
        &tree,
        tree.root_node(),
        lang.chunk_node_kinds(),
        &mut out,
    );
    if out.is_empty() {
        whole_file_chunk(source)
    } else {
        out
    }
}

fn whole_file_chunk(source: &str) -> Vec<RawChunk> {
    let line_count = source.lines().count().max(1) as u32;
    vec![RawChunk {
        symbol_name: None,
        symbol_kind: "file".into(),
        start_line: 1,
        end_line: line_count,
        start_byte: 0,
        end_byte: source.len() as u32,
        content: source.to_string(),
    }]
}

fn collect_chunks(
    lang: SourceLanguage,
    source: &str,
    tree: &Tree,
    node: Node,
    kinds: &[&str],
    out: &mut Vec<RawChunk>,
) {
    if kinds.contains(&node.kind()) {
        if let Some(chunk) = node_to_chunk(lang, source, tree, node) {
            out.push(chunk);
        }
        // Fall through and keep descending so nested definitions — methods inside
        // `impl`/`mod`, methods inside Python/JS classes — get their own chunks too.
        // (Previously this `return`ed, folding every nested def into its parent and
        // making them invisible to find_symbol/search_code — see GAP-03 audit.)
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_chunks(lang, source, tree, child, kinds, out);
    }
}

fn node_to_chunk(
    lang: SourceLanguage,
    source: &str,
    _tree: &Tree,
    node: Node,
) -> Option<RawChunk> {
    let start = node.start_byte();
    let end = node.end_byte();
    if end <= start || end > source.len() {
        return None;
    }
    let content = source[start..end].to_string();
    let start_line = node.start_position().row as u32 + 1;
    let end_line = node.end_position().row as u32 + 1;
    let symbol_name = extract_symbol_name(lang, source, node);
    Some(RawChunk {
        symbol_name,
        symbol_kind: node.kind().to_string(),
        start_line,
        end_line,
        start_byte: start as u32,
        end_byte: end as u32,
        content,
    })
}

fn extract_symbol_name(lang: SourceLanguage, source: &str, node: Node) -> Option<String> {
    match lang {
        SourceLanguage::Rust => name_from_named_child(source, node, &["identifier", "type_identifier"]),
        SourceLanguage::Python => name_from_named_child(source, node, &["identifier"]),
        SourceLanguage::JavaScript => {
            name_from_named_child(source, node, &["identifier", "property_identifier"])
        }
    }
}

fn name_from_named_child(source: &str, node: Node, kinds: &[&str]) -> Option<String> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.is_named() && kinds.contains(&child.kind()) {
            let s = &source[child.start_byte()..child.end_byte()];
            if !s.is_empty() {
                return Some(s.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_function_chunk_has_name() {
        let src = "fn hello() {}\nfn world(x: i32) -> i32 { x }\n";
        let chunks = chunk_source(SourceLanguage::Rust, src);
        let names: Vec<_> = chunks
            .iter()
            .filter_map(|c| c.symbol_name.as_deref())
            .collect();
        assert!(names.contains(&"hello"));
        assert!(names.contains(&"world"));
    }

    #[test]
    fn python_class_chunk() {
        let src = "class Foo:\n    def bar(self):\n        pass\n";
        let chunks = chunk_source(SourceLanguage::Python, src);
        assert!(chunks.iter().any(|c| c.symbol_name.as_deref() == Some("Foo")));
    }

    #[test]
    fn js_function_chunk() {
        let src = "function add(a, b) { return a + b; }\n";
        let chunks = chunk_source(SourceLanguage::JavaScript, src);
        assert!(chunks.iter().any(|c| c.symbol_name.as_deref() == Some("add")));
    }

    // Regression for GAP-03 B1: nested definitions must get their own chunks.
    #[test]
    fn rust_impl_methods_are_chunked() {
        let src = "struct Foo;\nimpl Foo {\n    fn bar(&self) {}\n    fn baz(&self) {}\n}\n";
        let chunks = chunk_source(SourceLanguage::Rust, src);
        let names: Vec<_> = chunks.iter().filter_map(|c| c.symbol_name.as_deref()).collect();
        assert!(names.contains(&"bar"), "impl method `bar` must be chunked: {names:?}");
        assert!(names.contains(&"baz"), "impl method `baz` must be chunked: {names:?}");
    }

    #[test]
    fn rust_mod_inner_fn_is_chunked() {
        let src = "mod m {\n    fn inner() {}\n}\n";
        let chunks = chunk_source(SourceLanguage::Rust, src);
        let names: Vec<_> = chunks.iter().filter_map(|c| c.symbol_name.as_deref()).collect();
        assert!(names.contains(&"inner"), "mod inner fn must be chunked: {names:?}");
    }

    #[test]
    fn python_class_method_is_chunked() {
        let src = "class Foo:\n    def bar(self):\n        return 1\n";
        let chunks = chunk_source(SourceLanguage::Python, src);
        let names: Vec<_> = chunks.iter().filter_map(|c| c.symbol_name.as_deref()).collect();
        assert!(names.contains(&"bar"), "class method `bar` must be chunked: {names:?}");
    }
}
