use tree_sitter::{Language, Parser, Tree};

/// A line range (1-based, inclusive) representing the scope an annotation covers,
/// along with a human-readable label like "fn parse_token" or "class Foo".
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Scope {
    pub start: usize,
    pub end: usize,
    pub label: Option<String>,
}

/// Node kind names that represent definition-level constructs.
/// When we find a [check] comment, we look for the nearest enclosing or
/// following node whose kind is one of these. The annotation's scope
/// becomes that node's full line range.
///
/// These are intentionally broad — we want to catch functions, methods,
/// classes, structs, impls, modules, etc.
const SCOPE_NODE_KINDS: &[&str] = &[
    // Rust
    "function_item",
    "impl_item",
    "struct_item",
    "enum_item",
    "trait_item",
    "mod_item",
    "static_item",
    "const_item",
    "type_item",
    // Python
    "function_definition",
    "class_definition",
    // Go
    "function_declaration",
    "method_declaration",
    "type_declaration",
    // JavaScript/TypeScript
    "function_declaration",
    "class_declaration",
    "method_definition",
    "arrow_function",
    "lexical_declaration",
    "export_statement",
    // C
    "function_definition",
    "struct_specifier",
    "enum_specifier",
    "declaration",
    // Haskell
    "function",
    "signature",
    "data_type",
    "newtype",
    "type_alias",
    "class",
    "instance",
    // Nix
    "binding",
    "inherit",
];

fn is_scope_node(kind: &str) -> bool {
    SCOPE_NODE_KINDS.contains(&kind)
}

/// Get the tree-sitter Language for a file extension, if we have a grammar.
pub fn language_for_extension(ext: &str) -> Option<Language> {
    match ext {
        "rs" => Some(tree_sitter_rust::LANGUAGE.into()),
        "py" | "pyi" | "pyw" => Some(tree_sitter_python::LANGUAGE.into()),
        "go" => Some(tree_sitter_go::LANGUAGE.into()),
        "js" | "mjs" | "cjs" | "jsx" => Some(tree_sitter_javascript::LANGUAGE.into()),
        "ts" | "mts" | "cts" => Some(tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()),
        "tsx" => Some(tree_sitter_typescript::LANGUAGE_TSX.into()),
        "c" | "h" => Some(tree_sitter_c::LANGUAGE.into()),
        "hs" => Some(tree_sitter_haskell::LANGUAGE.into()),
        "nix" => Some(tree_sitter_nix::LANGUAGE.into()),
        _ => None,
    }
}

/// Parse the source and return the tree. Returns None if parsing fails.
pub fn parse(source: &str, language: Language) -> Option<Tree> {
    let mut parser = Parser::new();
    parser.set_language(&language).ok()?;
    parser.parse(source, None)
}

/// Find the scope for a comment at the given line (1-based).
///
/// Strategy:
/// 1. Find the smallest named node that contains the comment's line
/// 2. Look at next siblings for a scope node (the annotation precedes what it's about)
/// 3. Walk up ancestors to find an enclosing scope node
///
/// Returns the scope node's line range, or None if no scope node is found.
pub fn find_scope(tree: &Tree, comment_line: usize, source: &str) -> Option<Scope> {
    let root = tree.root_node();
    let line_0 = comment_line.saturating_sub(1);

    let node = find_named_node_at_line(root, line_0)?;

    // Walk from this node and its ancestors looking for a scope node
    let mut cursor = node;
    loop {
        // Check next siblings (the annotation typically precedes what it's about)
        let mut sib = cursor;
        while let Some(next) = sib.next_named_sibling() {
            if is_scope_node(next.kind()) {
                return Some(extend_scope(next, source));
            }
            if let Some(scope) = find_scope_in_children(next, source) {
                return Some(scope);
            }
            sib = next;
            if sib.start_position().row > line_0 + 5 {
                break;
            }
        }

        // Check if the parent itself is a scope node (we're inside it)
        if let Some(parent) = cursor.parent() {
            if is_scope_node(parent.kind()) {
                return Some(node_to_scope(parent, source));
            }
            cursor = parent;
        } else {
            break;
        }
    }

    None
}

/// Find the smallest named node whose line range contains the given line (0-based).
fn find_named_node_at_line(node: tree_sitter::Node, line_0: usize) -> Option<tree_sitter::Node> {
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        let start = child.start_position().row;
        let end = child.end_position().row;
        if start <= line_0 && line_0 <= end {
            // Try to go deeper
            return find_named_node_at_line(child, line_0).or(Some(child));
        }
    }
    // No named child contains this line — if the node itself does, return it
    if node.start_position().row <= line_0 && line_0 <= node.end_position().row {
        Some(node)
    } else {
        None
    }
}

fn node_to_scope(node: tree_sitter::Node, source: &str) -> Scope {
    Scope {
        start: node.start_position().row + 1,
        end: node.end_position().row + 1,
        label: node_label(node, source),
    }
}

/// Extend a scope node forward through consecutive scope-node siblings.
/// This handles cases like Haskell's signature + function body being separate nodes.
fn extend_scope(node: tree_sitter::Node, source: &str) -> Scope {
    let start = node.start_position().row + 1;
    let mut end = node.end_position().row + 1;
    let mut sib = node;
    while let Some(next) = sib.next_named_sibling() {
        if next.start_position().row <= sib.end_position().row + 1
            && is_scope_node(next.kind())
        {
            end = next.end_position().row + 1;
            sib = next;
        } else {
            break;
        }
    }
    // Use the first node for the label
    Scope {
        start,
        end,
        label: node_label(node, source),
    }
}

/// Short human-readable prefix for a node kind.
fn kind_prefix(kind: &str) -> &'static str {
    match kind {
        "function_item" => "fn",
        "function_definition" | "function_declaration" | "function" => "fn",
        "method_declaration" | "method_definition" => "method",
        "impl_item" => "impl",
        "struct_item" | "struct_specifier" => "struct",
        "enum_item" | "enum_specifier" => "enum",
        "trait_item" => "trait",
        "mod_item" => "mod",
        "static_item" => "static",
        "const_item" => "const",
        "type_item" | "type_declaration" | "type_alias" => "type",
        "class_definition" | "class_declaration" | "class" => "class",
        "instance" => "instance",
        "arrow_function" => "fn",
        "lexical_declaration" | "declaration" => "let",
        "export_statement" => "export",
        "signature" => "sig",
        "data_type" => "data",
        "newtype" => "newtype",
        "binding" => "binding",
        "inherit" => "inherit",
        _ => "",
    }
}

/// Extract a human-readable label like "fn parse_token" from a scope node.
fn node_label(node: tree_sitter::Node, source: &str) -> Option<String> {
    let prefix = kind_prefix(node.kind());
    if prefix.is_empty() {
        return None;
    }

    // Try to find a name child node
    let name = find_name_child(node, source);

    match name {
        Some(name) => Some(format!("{} {}", prefix, name)),
        None => Some(prefix.to_string()),
    }
}

/// Find the name/identifier child of a node.
fn find_name_child(node: tree_sitter::Node, source: &str) -> Option<String> {
    // Try the "name" field first (most grammars use this)
    if let Some(name_node) = node.child_by_field_name("name") {
        return Some(name_node.utf8_text(source.as_bytes()).ok()?.to_string());
    }

    // Try common name-like child node kinds
    let name_kinds = &["name", "identifier", "variable", "type_identifier"];
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if name_kinds.contains(&child.kind()) {
            return Some(child.utf8_text(source.as_bytes()).ok()?.to_string());
        }
    }

    None
}

fn find_scope_in_children(node: tree_sitter::Node, source: &str) -> Option<Scope> {
    let mut cursor = node.walk();
    let children: Vec<_> = node.named_children(&mut cursor).collect();

    for (i, child) in children.iter().enumerate() {
        if is_scope_node(child.kind()) {
            let start = child.start_position().row + 1;
            let mut end = child.end_position().row + 1;
            let label = node_label(*child, source);

            // Extend through consecutive scope-node siblings (e.g. Haskell
            // signature followed by function body)
            for subsequent in &children[i + 1..] {
                if subsequent.start_position().row <= end
                    && is_scope_node(subsequent.kind())
                {
                    end = subsequent.end_position().row + 1;
                } else {
                    break;
                }
            }

            return Some(Scope { start, end, label });
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_function_scope() {
        let source = "\
// [check] Verify bounds
fn process(x: usize) -> usize {
    x + 1
}

fn other() {}
";
        let lang = language_for_extension("rs").unwrap();
        let tree = parse(source, lang).unwrap();
        let scope = find_scope(&tree, 1, source).unwrap();
        // The function_item spans lines 2-4
        assert_eq!(scope.start, 2);
        assert_eq!(scope.end, 4);
    }

    #[test]
    fn rust_impl_block_scope() {
        let source = "\
struct Foo;

// [check] Verify all trait methods
impl Foo {
    fn bar(&self) {}
    fn baz(&self) {}
}
";
        let lang = language_for_extension("rs").unwrap();
        let tree = parse(source, lang).unwrap();
        let scope = find_scope(&tree, 3, source).unwrap();
        assert_eq!(scope.start, 4);
        assert_eq!(scope.end, 7);
    }

    #[test]
    fn python_function_scope() {
        let source = "\
# [check] Ensure rate limiting
def call_service(payload):
    for attempt in range(3):
        try_call(payload)
";
        let lang = language_for_extension("py").unwrap();
        let tree = parse(source, lang).unwrap();
        let scope = find_scope(&tree, 1, source).unwrap();
        assert_eq!(scope.start, 2);
        assert_eq!(scope.end, 4);
    }

    #[test]
    fn python_class_scope() {
        let source = "\
# [check:file] Check all validators
class UserValidator:
    def validate_email(self):
        pass
    def validate_name(self):
        pass
";
        let lang = language_for_extension("py").unwrap();
        let tree = parse(source, lang).unwrap();
        let scope = find_scope(&tree, 1, source).unwrap();
        assert_eq!(scope.start, 2);
        assert_eq!(scope.end, 6);
    }

    #[test]
    fn go_function_scope() {
        let source = "\
// [check] Verify error handling
func Process(x int) (int, error) {
\treturn x + 1, nil
}
";
        let lang = language_for_extension("go").unwrap();
        let tree = parse(source, lang).unwrap();
        let scope = find_scope(&tree, 1, source).unwrap();
        assert_eq!(scope.start, 2);
        assert_eq!(scope.end, 4);
    }

    #[test]
    fn comment_inside_function() {
        let source = "\
fn outer() {
    // [check] Verify this inner logic
    let x = dangerous_call();
    let y = x + 1;
}
";
        let lang = language_for_extension("rs").unwrap();
        let tree = parse(source, lang).unwrap();
        let scope = find_scope(&tree, 2, source).unwrap();
        // Should find the enclosing function
        assert_eq!(scope.start, 1);
        assert_eq!(scope.end, 5);
    }

    #[test]
    fn no_grammar_returns_none() {
        assert!(language_for_extension("xyz").is_none());
    }

    #[test]
    fn haskell_function_scope() {
        let source = "\
-- [check] Verify purity
process :: Int -> Int
process x = x + 1
";
        let lang = language_for_extension("hs").unwrap();
        let tree = parse(source, lang).unwrap();
        let scope = find_scope(&tree, 1, source).unwrap();
        // Should cover at least the function signature and body
        assert!(scope.start <= 2);
        assert!(scope.end >= 3);
    }
}
