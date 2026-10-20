//! Parser-backed attribute scopes, including otherwise opaque macro tokens.
use super::{Policy, Suppression, bind};
use crate::Result;
use proc_macro2::{Delimiter, TokenStream, TokenTree};
use quote::ToTokens;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};
use syn::{Meta, Token, punctuated::Punctuated, spanned::Spanned, visit::Visit};

pub(super) fn check(
    root: &Path,
    file: &str,
    source: &str,
    paths: &BTreeSet<String>,
    policy: &Policy,
    matched: &mut BTreeMap<String, usize>,
) -> Result<()> {
    let tree = syn::parse_file(source)
        .map_err(|error| format!("{file}: cannot parse authored Rust: {error}"))?;
    let mut visitor = Inspector {
        root,
        paths,
        file,
        policy,
        matched,
        scope: vec!["crate".into()],
        attributes: BTreeSet::new(),
        errors: Vec::new(),
    };
    visitor.visit_file(&tree);
    let tokens: TokenStream = source.parse()?;
    let mut token_attributes = BTreeSet::new();
    token_suppressions(tokens, &mut token_attributes);
    if token_attributes != visitor.attributes {
        visitor.errors.push(format!(
            "{file}: lint/formatter attribute in an opaque macro or unsupported scope"
        ));
    }
    if !visitor.errors.is_empty() {
        return Err(visitor.errors.join("\n").into());
    }
    Ok(())
}
struct Inspector<'a> {
    root: &'a Path,
    paths: &'a BTreeSet<String>,
    file: &'a str,
    policy: &'a Policy,
    matched: &'a mut BTreeMap<String, usize>,
    scope: Vec<String>,
    attributes: BTreeSet<(usize, usize)>,
    errors: Vec<String>,
}
impl Inspector<'_> {
    fn scope(&self) -> String {
        self.scope.join("/")
    }
    fn attribute(&mut self, attribute: &syn::Attribute) -> Result<()> {
        if attribute.path().is_ident("cfg_attr") {
            if contains_suppression(attribute.meta.to_token_stream()) {
                return Err(format!("{}: conditional lint-level attributes are prohibited; use a directly scoped registered exception", self.file).into());
            }
            return Ok(());
        }
        if !is_level(attribute.path()) && !is_format_skip(attribute.path()) {
            return Ok(());
        }
        let position = attribute.span().start();
        self.attributes.insert((position.line, position.column));
        if is_format_skip(attribute.path()) {
            return Err(format!("{}: rustfmt skipping is prohibited", self.file).into());
        }
        let observed = parse_suppression(attribute, self.scope())?;
        bind(self.policy, self.file, observed, self.matched)
    }
    fn command(&mut self, node: &syn::ExprMethodCall) {
        let name = node.method.to_string();
        if !matches!(name.as_str(), "arg" | "args" | "env" | "envs") {
            return;
        }
        let mut strings = Strings::default();
        for argument in &node.args {
            strings.visit_expr(argument);
        }
        for argument in strings.0 {
            if !weakening(&argument) {
                continue;
            }
            let entry = self.policy.command_exceptions.iter().find(|entry| {
                entry.file == self.file && entry.scope == self.scope() && entry.argument == argument
            });
            if let Some(entry) = entry {
                *self.matched.entry(entry.id.clone()).or_default() += 1;
            } else {
                self.errors.push(format!(
                    "{}: unregistered lint-weakening command argument {argument:?} at {}",
                    self.file,
                    self.scope()
                ));
            }
        }
    }
}
impl<'ast> Visit<'ast> for Inspector<'_> {
    fn visit_item(&mut self, item: &'ast syn::Item) {
        let name = item_name(item);
        if let Some(name) = &name {
            self.scope.push(name.clone());
        }
        syn::visit::visit_item(self, item);
        if name.is_some() {
            self.scope.pop();
        }
    }
    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        self.scope.push(format!("fn:{}", item.sig.ident));
        syn::visit::visit_impl_item_fn(self, item);
        self.scope.pop();
    }
    fn visit_trait_item_fn(&mut self, item: &'ast syn::TraitItemFn) {
        self.scope.push(format!("fn:{}", item.sig.ident));
        syn::visit::visit_trait_item_fn(self, item);
        self.scope.pop();
    }
    fn visit_local(&mut self, local: &'ast syn::Local) {
        self.scope
            .push(format!("let:{}", local.pat.to_token_stream()));
        syn::visit::visit_local(self, local);
        self.scope.pop();
    }
    fn visit_attribute(&mut self, attribute: &'ast syn::Attribute) {
        if let Err(error) = self.attribute(attribute) {
            self.errors.push(error.to_string());
        }
    }
    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        if let Err(error) =
            super::references::check(self.root, self.file, self.paths, item, &self.scope)
        {
            self.errors.push(error.to_string());
        }
        syn::visit::visit_item_mod(self, item);
    }
    fn visit_macro(&mut self, node: &'ast syn::Macro) {
        if let Err(error) = super::references::include(self.root, self.file, self.paths, node) {
            self.errors.push(error.to_string());
        }
        syn::visit::visit_macro(self, node);
    }
    fn visit_expr_method_call(&mut self, node: &'ast syn::ExprMethodCall) {
        self.command(node);
        syn::visit::visit_expr_method_call(self, node);
    }
}
fn item_name(item: &syn::Item) -> Option<String> {
    Some(match item {
        syn::Item::Fn(value) => format!("fn:{}", value.sig.ident),
        syn::Item::Mod(value) => format!("mod:{}", value.ident),
        syn::Item::Struct(value) => format!("struct:{}", value.ident),
        syn::Item::Enum(value) => format!("enum:{}", value.ident),
        syn::Item::Trait(value) => format!("trait:{}", value.ident),
        syn::Item::Impl(value) => format!("impl:{}", value.self_ty.to_token_stream()),
        syn::Item::Type(value) => format!("type:{}", value.ident),
        syn::Item::Const(value) => format!("const:{}", value.ident),
        syn::Item::Static(value) => format!("static:{}", value.ident),
        _ => return None,
    })
}
fn parse_suppression(attribute: &syn::Attribute, scope: String) -> Result<Suppression> {
    let arguments = attribute.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)?;
    let mut lints = Vec::new();
    let mut id = None;
    for argument in arguments {
        match argument {
            Meta::Path(path) => lints.push(path.to_token_stream().to_string().replace(' ', "")),
            Meta::NameValue(value) if value.path.is_ident("reason") => {
                if id.is_some() {
                    return Err("Duplicate exception reason".into());
                }
                if let syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(value),
                    ..
                }) = value.value
                {
                    id = value.value().strip_prefix("quality:").map(str::to_owned);
                }
            }
            _ => return Err("Unsupported lint exception syntax".into()),
        }
    }
    lints.sort();
    let id = id.ok_or("Every lint-level exception must reference reason = \"quality:<id>\"")?;
    Ok(Suppression { id, scope, lints })
}
fn is_level(path: &syn::Path) -> bool {
    ["allow", "expect", "warn"]
        .iter()
        .any(|name| path.is_ident(name))
}
fn is_format_skip(path: &syn::Path) -> bool {
    path.segments
        .first()
        .is_some_and(|segment| segment.ident == "rustfmt")
        && path.segments.iter().any(|segment| segment.ident == "skip")
}
fn contains_suppression(tokens: TokenStream) -> bool {
    tokens.into_iter().any(|token| match token {
        TokenTree::Ident(ident) => matches!(
            ident.to_string().as_str(),
            "allow" | "expect" | "warn" | "rustfmt"
        ),
        TokenTree::Group(group) => contains_suppression(group.stream()),
        _ => false,
    })
}
fn token_suppressions(tokens: TokenStream, found: &mut BTreeSet<(usize, usize)>) {
    let tokens: Vec<_> = tokens.into_iter().collect();
    for (index, token) in tokens.iter().enumerate() {
        if let TokenTree::Punct(hash) = token
            && hash.as_char() == '#'
        {
            let offset = usize::from(
                matches!(tokens.get(index + 1), Some(TokenTree::Punct(p)) if p.as_char() == '!'),
            );
            if let Some(TokenTree::Group(group)) = tokens.get(index + 1 + offset)
                && group.delimiter() == Delimiter::Bracket
                && contains_suppression(group.stream())
            {
                let position = hash.span().start();
                found.insert((position.line, position.column));
            }
        }
        if let TokenTree::Group(group) = token {
            token_suppressions(group.stream(), found);
        }
    }
}
#[derive(Default)]
struct Strings(Vec<String>);
impl<'ast> Visit<'ast> for Strings {
    fn visit_lit_str(&mut self, literal: &'ast syn::LitStr) {
        self.0.push(literal.value());
    }
}
fn weakening(argument: &str) -> bool {
    argument == "-A"
        || argument.starts_with("-A")
        || argument.starts_with("--allow")
        || argument.starts_with("--cap-lints")
        || argument.starts_with("--force-warn")
        || argument.contains(" -A")
        || argument.contains("--cap-lints")
}
