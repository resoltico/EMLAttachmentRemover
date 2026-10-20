//! Authored Rust source references must participate in checkout discovery.
use crate::Result;
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};
use syn::{Token, parse::Parser, punctuated::Punctuated};

pub(super) fn check(
    root: &Path,
    file: &str,
    paths: &BTreeSet<String>,
    item: &syn::ItemMod,
    scope: &[String],
) -> Result<()> {
    let source = root.join(file);
    for attribute in &item.attrs {
        if attribute.path().is_ident("path") {
            if let syn::Meta::NameValue(value) = &attribute.meta
                && let syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(value),
                    ..
                }) = &value.value
            {
                return selected(
                    root,
                    paths,
                    source
                        .parent()
                        .ok_or("Source has no parent")?
                        .join(value.value()),
                );
            }
            return Err(format!("{file}: unsupported Rust source path expression").into());
        }
    }
    if item.content.is_some() {
        return Ok(());
    }
    let mut base = source.parent().ok_or("Source has no parent")?.to_owned();
    let stem = source
        .file_stem()
        .and_then(|value| value.to_str())
        .ok_or("Invalid Rust filename")?;
    if !matches!(stem, "main" | "lib" | "mod") {
        base.push(stem);
    }
    for component in scope
        .iter()
        .filter_map(|value| value.strip_prefix("mod:"))
        .filter(|value| item.ident != *value)
    {
        base.push(component);
    }
    let simple = base.join(format!("{}.rs", item.ident));
    let nested = base.join(item.ident.to_string()).join("mod.rs");
    selected(root, paths, if simple.exists() { simple } else { nested })
}
pub(super) fn include(
    root: &Path,
    file: &str,
    paths: &BTreeSet<String>,
    node: &syn::Macro,
) -> Result<()> {
    if !node.path.is_ident("include") {
        return Ok(());
    }
    let source = root.join(file);
    let manifest = source
        .ancestors()
        .find(|directory| directory.join("Cargo.toml").is_file())
        .ok_or("Included source has no Cargo owner")?;
    let expression: syn::Expr = syn::parse2(node.tokens.clone())?;
    let value = evaluate(&expression, manifest)?;
    let path = Path::new(&value);
    let input = if path.is_absolute() {
        path.to_owned()
    } else {
        source.parent().ok_or("Source has no parent")?.join(path)
    };
    selected(root, paths, input)
}
fn evaluate(expression: &syn::Expr, manifest: &Path) -> Result<String> {
    match expression {
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Str(value),
            ..
        }) => Ok(value.value()),
        syn::Expr::Macro(value) if value.mac.path.is_ident("concat") => {
            let values = Punctuated::<syn::Expr, Token![,]>::parse_terminated
                .parse2(value.mac.tokens.clone())?;
            values
                .iter()
                .map(|value| evaluate(value, manifest))
                .collect::<Result<Vec<_>>>()
                .map(|values| values.concat())
        }
        syn::Expr::Macro(value) if value.mac.path.is_ident("env") => {
            let name: syn::LitStr = syn::parse2(value.mac.tokens.clone())?;
            if name.value() != "CARGO_MANIFEST_DIR" {
                return Err("Authored include! cannot escape discovery through arbitrary environment/generated source; declare a specific ownership boundary before adding one".into());
            }
            Ok(manifest
                .to_str()
                .ok_or("Non-UTF8 source manifest path")?
                .to_owned())
        }
        _ => {
            Err("Unsupported authored include! path; source coverage cannot be established".into())
        }
    }
}
fn selected(root: &Path, paths: &BTreeSet<String>, input: PathBuf) -> Result<()> {
    let metadata = std::fs::symlink_metadata(&input).map_err(|error| {
        format!(
            "Referenced source {} is unavailable: {error}",
            input.display()
        )
    })?;
    let canonical = input.canonicalize()?;
    let canonical_root = root.canonicalize()?;
    let relative = canonical
        .strip_prefix(canonical_root)
        .map_err(|_| "Referenced source escapes checkout")?;
    let relative = relative
        .to_str()
        .ok_or("Non-UTF8 source reference")?
        .replace('\\', "/");
    if !metadata.is_file() || !paths.contains(&relative) {
        return Err(format!(
            "Referenced source {} is not a discovered ordinary authored source",
            input.display()
        )
        .into());
    }
    Ok(())
}
