//! Syntax-aware ownership guards and controls that must defeat textual bypasses.
use super::*;
use i_slint_compiler::{
    diagnostics::BuildDiagnostics,
    parser::{self, SyntaxKind, SyntaxNode},
};
use syn::visit::Visit;

fn readable(value: &str) -> bool {
    let text: String = value
        .split(['{', '}'])
        .enumerate()
        .filter(|(index, _)| index % 2 == 0)
        .map(|(_, part)| part)
        .collect();
    // Native identifiers and filename components are technical, not interface words.
    let technical = [
        "C",
        "MacOS",
        "Contents",
        "app",
        "eml",
        "jsonl",
        "EMLAttachmentRemover",
        "NSObject",
        "WinitApplicationDelegate",
        "EmlAboutMenuTarget",
    ];
    !technical.contains(&text.as_str())
        && (text
            .split_whitespace()
            .filter(|word| word.chars().any(char::is_alphabetic))
            .count()
            > 1
            || (!text.is_empty() && text.chars().all(|c| c.is_alphabetic() || c == '…')))
}

#[derive(Default)]
struct RustWords(Vec<String>);
impl RustWords {
    fn metadata(&mut self, meta: &syn::Meta) {
        // Compiler conditions/documentation and actual lint reasons are not UI text.
        if ["cfg", "doc", "expect"]
            .iter()
            .any(|name| meta.path().is_ident(name))
        {
            return;
        }
        match meta {
            syn::Meta::List(list) if list.path.is_ident("cfg_attr") => {
                use syn::parse::Parser;
                let attributes =
                    syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated
                        .parse2(list.tokens.clone())
                        .expect("valid cfg_attr metadata");
                for attribute in attributes.iter().skip(1) {
                    self.metadata(attribute);
                }
            }
            syn::Meta::List(list) => self.tokens(list.tokens.clone()),
            syn::Meta::NameValue(value) => self.visit_expr(&value.value),
            syn::Meta::Path(_) => {}
        }
    }
    fn tokens(&mut self, stream: proc_macro2::TokenStream) {
        for token in stream {
            match token {
                proc_macro2::TokenTree::Group(group) => self.tokens(group.stream()),
                proc_macro2::TokenTree::Literal(literal) => {
                    if let Ok(syn::Lit::Str(value)) = syn::parse_str(&literal.to_string()) {
                        self.visit_lit_str(&value);
                    }
                }
                _ => {}
            }
        }
    }
}
impl<'ast> Visit<'ast> for RustWords {
    fn visit_lit_str(&mut self, value: &'ast syn::LitStr) {
        if readable(&value.value()) {
            self.0.push(value.value());
        }
    }
    fn visit_attribute(&mut self, attribute: &'ast syn::Attribute) {
        self.metadata(&attribute.meta);
    }
    fn visit_macro(&mut self, value: &'ast syn::Macro) {
        if !value.path.is_ident("cfg") {
            self.tokens(value.tokens.clone());
        }
    }
    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        self.visit_expr(&call.receiver);
        for (index, argument) in call.args.iter().enumerate() {
            // org.freedesktop.FileManager1's method name is an external D-Bus ABI.
            let file_manager_method = index == 0
                && call.method == "call"
                && matches!(argument, syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(name), .. }) if name.value() == "ShowItems");
            if !file_manager_method {
                self.visit_expr(argument);
            }
        }
    }
    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        if !item.attrs.iter().any(|attr| {
            attr.path().is_ident("cfg")
                && attr
                    .parse_args::<syn::Path>()
                    .is_ok_and(|path| path.is_ident("test"))
        }) {
            syn::visit::visit_item_mod(self, item);
        }
    }
}
fn rust_words(source: &str) -> Vec<String> {
    let mut words = RustWords::default();
    words.visit_file(&syn::parse_file(source).expect("valid Rust source"));
    words.0
}
fn inspect_rust(directory: &Path) {
    for entry in fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            if path.file_name().unwrap() != "tests" {
                inspect_rust(&path);
            }
        } else if path.extension().is_some_and(|ext| ext == "rs")
            && !["wording.rs", "tests.rs"].contains(&path.file_name().unwrap().to_str().unwrap())
        {
            let words = rust_words(&fs::read_to_string(&path).unwrap());
            assert!(
                words.is_empty(),
                "Readable Rust literal in {}: {words:?}",
                path.display()
            );
        }
    }
}
fn slint(source: &str) -> SyntaxNode {
    let mut diagnostics = BuildDiagnostics::default();
    let node = parser::parse(source.to_owned(), None, &mut diagnostics);
    assert!(
        !diagnostics.has_errors(),
        "invalid Slint fixture/source: {source}"
    );
    node
}
fn slint_words(node: &SyntaxNode) -> Vec<String> {
    node.node
        .descendants_with_tokens()
        .filter_map(|item| item.into_token())
        .filter(|token| {
            token.kind() == SyntaxKind::StringLiteral
                && token.text().chars().any(char::is_alphabetic)
                && !token.parent_ancestors().any(|parent| {
                    matches!(
                        parent.kind(),
                        SyntaxKind::ImportSpecifier
                            | SyntaxKind::ExportModule
                            | SyntaxKind::AtImageUrl
                            | SyntaxKind::AtKeys
                    )
                })
        })
        .map(|token| token.text().to_owned())
        .collect()
}
fn visual_literals(node: &SyntaxNode) -> Vec<String> {
    let mut found = Vec::new();
    for item in node
        .node
        .descendants_with_tokens()
        .filter_map(|item| item.into_token())
    {
        let colour = item.kind() == SyntaxKind::ColorLiteral
            || (item.kind() == SyntaxKind::Identifier
                && i_slint_compiler::lookup::named_colors().contains_key(item.text())
                && item.parent().is_some_and(|parent| {
                    parent.kind() == SyntaxKind::QualifiedName && parent.text() == item.text()
                }));
        let size = item.kind() == SyntaxKind::NumberLiteral
            && ((item.text().ends_with("px") && !matches!(item.text(), "0px" | "1px"))
                || item.parent_ancestors().any(|parent| {
                    parent.kind() == SyntaxKind::Binding
                        && parent.first_token().is_some_and(|name| {
                            matches!(name.text(), "font-size" | "border-radius")
                        })
                }));
        if colour || size {
            found.push(item.text().to_owned());
        }
    }
    found
}
fn references(node: &SyntaxNode) -> Vec<String> {
    node.descendants()
        .filter(|node| node.kind() == SyntaxKind::QualifiedName)
        .map(|node| {
            node.children_with_tokens()
                .filter_map(|item| item.into_token())
                .filter(|token| {
                    token.kind() != SyntaxKind::Whitespace && token.kind() != SyntaxKind::Comment
                })
                .map(|token| token.text().to_owned())
                .collect()
        })
        .collect()
}
fn unused_tokens(theme: &SyntaxNode, sources: &[SyntaxNode]) -> Vec<String> {
    let external: Vec<_> = sources.iter().flat_map(references).collect();
    let internal = references(theme);
    theme
        .descendants()
        .filter(|node| {
            node.kind() == SyntaxKind::PropertyDeclaration
                && node
                    .children_with_tokens()
                    .filter_map(|item| item.into_token())
                    .any(|token| token.text() == "out")
        })
        .map(|node| {
            node.child_node(SyntaxKind::DeclaredIdentifier)
                .unwrap()
                .text()
                .to_string()
        })
        .filter(|name| {
            !external
                .iter()
                .any(|reference| reference == &format!("Theme.{name}"))
                && !internal
                    .iter()
                    .any(|reference| reference == name || reference == &format!("root.{name}"))
        })
        .collect()
}
fn ui_sources() -> Vec<(PathBuf, SyntaxNode)> {
    fs::read_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join("ui"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "slint"))
        .map(|path| {
            let node = slint(&fs::read_to_string(&path).unwrap());
            (path, node)
        })
        .collect()
}
#[test]
fn u1_rust_words_have_one_owner() {
    inspect_rust(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"));
}
#[test]
fn u1_slint_has_no_readable_literals() {
    for (path, node) in ui_sources() {
        assert!(
            slint_words(&node).is_empty(),
            "Readable Slint literal: {}",
            path.display()
        );
    }
}
#[test]
fn u4_visual_literals_are_rejected() {
    for (path, node) in ui_sources() {
        assert!(
            !node
                .node
                .descendants_with_tokens()
                .filter_map(|item| item.into_token())
                .any(|token| token.kind() == SyntaxKind::Identifier
                    && matches!(token.text(), "ui-size" | "UiMetrics"))
        );
        if path.file_name().unwrap() != "theme.slint" {
            assert!(
                visual_literals(&node).is_empty(),
                "Visual literal: {}: {:?}",
                path.display(),
                visual_literals(&node)
            );
        }
    }
}
#[test]
fn u4_unused_tokens_are_rejected() {
    let sources: Vec<_> = ui_sources().into_iter().map(|(_, node)| node).collect();
    let theme = slint(
        &fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("ui/theme.slint")).unwrap(),
    );
    assert_eq!(unused_tokens(&theme, &sources), Vec::<String>::new());
}
#[test]
fn u1_controls_reject_comment_raw_multiline_and_macro_bypasses() {
    for source in [
        r#"const S: &str = "Copy created"; // reason = bypass"#,
        "const S: &str = \"copy\ncreated\";",
        r##"const S: &str = r#"retry"#;"##,
        r#"fn show() { display!("Copy created"); }"#,
        r#"#[cfg_attr(feature = "native", message("Copy created"))] fn show() {}"#,
        r#"fn show() { proxy.call("Copy created", ()); }"#,
    ] {
        assert!(!rust_words(source).is_empty(), "accepted {source}");
    }
    assert!(
        rust_words(
            r#"#[expect(dead_code, reason = "Not user wording")] const N: u8 = 0;
        #[cfg(test)] mod tests { const S: &str = "Test wording"; }
        #[cfg_attr(feature = "native", expect(dead_code, reason = "Not UI wording"))] fn cfg_reason() {}
        fn technical() { let _ = cfg!(feature = "native"); proxy.call("ShowItems", ()); }"#
        )
        .is_empty()
    );
    assert!(
        !slint_words(&slint(
            r#"export component X inherits Text { text: "Show all"; }"#
        ))
        .is_empty()
    );
    assert!(slint_words(&slint(r#"import { TextEdit } from "std-widgets.slint"; export component X inherits Text { text: "⚠"; }"#)).is_empty());
}
#[test]
fn u4_controls_reject_expression_colours_and_comment_only_references() {
    for binding in [
        "width: 175px;",
        "font-size: 12px;",
        "font-size: (12px);",
        "border-radius: 1px + 2px;",
        "background: #fff;",
        "background: red;",
    ] {
        assert!(
            !visual_literals(&slint(&format!(
                "export component X inherits Rectangle {{ {binding} }}"
            )))
            .is_empty(),
            "accepted {binding}"
        );
    }
    let theme = slint("export global Theme { out property <length> unused: 12px; }");
    let comments = slint("// Theme.unused\nexport component X inherits Rectangle {}");
    assert_eq!(
        unused_tokens(&theme, &[theme.clone(), comments]),
        ["unused"]
    );
    let unrelated = slint(
        "export component X inherits Rectangle { property <length> unused: 1px; height: unused; }",
    );
    assert_eq!(unused_tokens(&theme, &[unrelated]), ["unused"]);
    let used = slint("export component X inherits Rectangle { height: Theme.unused; }");
    assert!(unused_tokens(&theme, &[theme.clone(), used]).is_empty());
}

#[test]
fn v1_no_toolkit_fork() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let text = fs::read_to_string(root.join("Cargo.toml")).unwrap();
    let patches = text.split("[patch.crates-io]").nth(1).unwrap_or("");
    for line in patches
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
    {
        assert!(
            line.starts_with("spin_on ="),
            "Unsupported toolkit patch: {line}"
        );
    }
    for entry in fs::read_dir(root.join("crates")).unwrap() {
        assert!(
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("slint-")
        );
    }
}
