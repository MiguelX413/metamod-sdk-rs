#![deny(unsafe_op_in_unsafe_fn)]

use proc_macro::TokenStream;
use quote::quote;
use std::{env, ffi::CString, fs, path::PathBuf};
use syn::{ImplItem, ItemImpl, LitCStr, LitStr, parse_macro_input};

/// Export a plugin's Metamod entry points and metadata.
///
/// Requires `name`, `version`, and `author`. Pass `cargo` to fill omitted
/// fields from the package name, version, authors, and repository URL.
/// `[package.metadata.metamod]` can set `name` and `log_tag` when using `cargo`.
/// `date` defaults to empty and `log_tag` defaults to the display name.
/// Use once per plugin library.
#[proc_macro_attribute]
pub fn metamod_plugin(args: TokenStream, input: TokenStream) -> TokenStream {
    let mut implementation = parse_macro_input!(input as ItemImpl);
    match register(args, &mut implementation) {
        Ok(item) => item,
        Err(error) => error.into_compile_error().into(),
    }
}

fn register(args: TokenStream, implementation: &mut ItemImpl) -> syn::Result<TokenStream> {
    if implementation.trait_.is_none() || !implementation.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &implementation.self_ty,
            "expected a concrete impl Plugin",
        ));
    }
    if let Some(item) = implementation
        .items
        .iter()
        .find(|item| matches!(item, ImplItem::Const(item) if item.ident == "METADATA"))
    {
        return Err(syn::Error::new_spanned(
            item,
            "metamod_plugin supplies METADATA",
        ));
    }

    let mut cargo = false;
    let mut name = None;
    let mut version = None;
    let mut author = None;
    let mut date = None;
    let mut url = None;
    let mut log_tag = None;
    let parser = syn::meta::parser(|meta| {
        if meta.path.is_ident("cargo") {
            if cargo {
                return Err(meta.error("duplicate cargo option"));
            }
            cargo = true;
            return Ok(());
        }
        let field = match meta.path.get_ident().map(ToString::to_string).as_deref() {
            Some("name") => &mut name,
            Some("version") => &mut version,
            Some("author") => &mut author,
            Some("date") => &mut date,
            Some("url") => &mut url,
            Some("log_tag") => &mut log_tag,
            _ => return Err(meta.error("unknown plugin metadata field")),
        };
        if field.is_some() {
            return Err(meta.error("duplicate plugin metadata field"));
        }
        let value: LitStr = meta.value()?.parse()?;
        let c_string = CString::new(value.value())
            .map_err(|_| syn::Error::new(value.span(), "metadata cannot contain NUL"))?;
        *field = Some(LitCStr::new(&c_string, value.span()));
        Ok(())
    });
    syn::parse::Parser::parse(parser, args)?;

    let span = implementation.impl_token.span;
    if cargo {
        let manifest_dir = env::var_os("CARGO_MANIFEST_DIR")
            .ok_or_else(|| syn::Error::new(span, "cargo metadata requires CARGO_MANIFEST_DIR"))?;
        let path = PathBuf::from(manifest_dir).join("Cargo.toml");
        let manifest = fs::read_to_string(&path)
            .map_err(|error| syn::Error::new(span, format!("{}: {error}", path.display())))?;
        let manifest = manifest
            .parse::<toml_edit::DocumentMut>()
            .map_err(|error| syn::Error::new(span, format!("{}: {error}", path.display())))?;
        if let Some(metadata) = manifest
            .get("package")
            .and_then(|package| package.get("metadata"))
            .and_then(|metadata| metadata.get("metamod"))
        {
            let metadata = metadata
                .as_table_like()
                .ok_or_else(|| syn::Error::new(span, "package.metadata.metamod must be a table"))?;
            for (key, field) in [("name", &mut name), ("log_tag", &mut log_tag)] {
                if field.is_none()
                    && let Some(value) = metadata.get(key)
                {
                    let value = value.as_str().ok_or_else(|| {
                        syn::Error::new(
                            span,
                            format!("package.metadata.metamod.{key} must be a string"),
                        )
                    })?;
                    let value = CString::new(value).map_err(|_| {
                        syn::Error::new(
                            span,
                            format!("package.metadata.metamod.{key} cannot contain NUL"),
                        )
                    })?;
                    *field = Some(LitCStr::new(&value, span));
                }
            }
        }
    }
    let cargo_default = |value: Option<LitCStr>, variable: &str| {
        if let Some(value) = value {
            quote!(#value)
        } else {
            // Expand env! in the plugin crate so Cargo tracks its package
            // metadata, including fields inherited from the workspace.
            quote! {
                match ::core::ffi::CStr::from_bytes_with_nul(
                    ::core::concat!(::core::env!(#variable), "\0").as_bytes()
                ) {
                    ::core::result::Result::Ok(value) => value,
                    ::core::result::Result::Err(_) => {
                        ::core::panic!(::core::concat!(#variable, " cannot contain NUL"))
                    }
                }
            }
        }
    };
    let field = |value: Option<LitCStr>, name, variable| {
        if cargo || value.is_some() {
            Ok(cargo_default(value, variable))
        } else {
            Err(syn::Error::new(
                span,
                format!("missing plugin metadata field {name}"),
            ))
        }
    };
    let name = field(name, "name", "CARGO_PKG_NAME")?;
    let version = field(version, "version", "CARGO_PKG_VERSION")?;
    let author = field(author, "author", "CARGO_PKG_AUTHORS")?;
    let url = if cargo {
        cargo_default(url, "CARGO_PKG_REPOSITORY")
    } else {
        let url = url.unwrap_or_else(|| LitCStr::new(c"", span));
        quote!(#url)
    };
    let date = date.unwrap_or_else(|| LitCStr::new(c"", span));
    let log_tag = log_tag.map_or_else(|| name.clone(), |value| quote!(#value));
    implementation.items.push(syn::parse_quote! {
        const METADATA: ::metamod_sdk_rs::Metadata = ::metamod_sdk_rs::Metadata {
            name: #name,
            version: #version,
            author: #author,
            date: #date,
            url: #url,
            log_tag: #log_tag,
        };
    });
    // Register the manifest as a rustc input so metadata-only edits rebuild.
    let track_manifest = cargo.then(|| {
        quote! {
            const _: &[u8] = ::core::include_bytes!(
                ::core::concat!(::core::env!("CARGO_MANIFEST_DIR"), "/Cargo.toml")
            );
        }
    });
    let plugin = &implementation.self_ty;
    Ok(quote! {
        #implementation
        #track_manifest
        ::metamod_sdk_rs::export_plugin!(#plugin);
    }
    .into())
}
