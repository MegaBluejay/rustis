use proc_macro::TokenStream;
use quote::{ToTokens, format_ident, quote};
use syn::{
    Block, DeriveInput, FnArg, GenericArgument, Ident, ImplItem, Item, LitStr, Pat, PathArguments,
    Signature, Token, TraitItem, Type, TypeParamBound, parse_macro_input, parse_quote,
    punctuated::Punctuated, spanned::Spanned, visit_mut::VisitMut,
};

#[proc_macro_attribute]
pub fn arg(_attr: TokenStream, input: TokenStream) -> TokenStream {
    let mut item = parse_macro_input!(input as Item);

    match &mut item {
        Item::Trait(it) => {
            for ti in &mut it.items {
                let TraitItem::Fn(tif) = ti else {
                    continue;
                };

                if let Err(err) = patch_fn(&mut tif.sig, tif.default.as_mut()) {
                    return err.into_compile_error().into();
                }
            }
        }
        Item::Impl(itim) => {
            for imit in &mut itim.items {
                let ImplItem::Fn(iif) = imit else {
                    continue;
                };

                if let Err(err) = patch_fn(&mut iif.sig, Some(&mut iif.block)) {
                    return err.into_compile_error().into();
                }
            }
        }
        _ => {
            return syn::Error::new_spanned(item, "unsupported item for arg macro")
                .into_compile_error()
                .into();
        }
    }

    item.into_token_stream().into()
}

fn patch_fn(sig: &mut Signature, mut body: Option<&mut Block>) -> syn::Result<()> {
    for arg in &mut sig.inputs {
        if let Some(name) = patch_arg(arg)?
            && let Some(ref mut body) = body
        {
            body.stmts
                .insert(0, parse_quote! { let #name = crate::resp::Serde(#name); });
        }
    }

    Ok(())
}

fn patch_arg(arg: &mut FnArg) -> syn::Result<Option<Ident>> {
    let FnArg::Typed(pt) = arg else {
        return Ok(None);
    };

    let tit = match &mut *pt.ty {
        Type::ImplTrait(tit) => tit,
        Type::Path(tp) => {
            if tp.path.segments.len() == 1
                && tp.path.segments[0].ident == "Option"
                && let PathArguments::AngleBracketed(ab) = &mut tp.path.segments[0].arguments
                && ab.args.len() == 1
                && let GenericArgument::Type(ty) = &mut ab.args[0]
                && let Type::ImplTrait(tit) = ty
            {
                tit
            } else {
                return Ok(None);
            }
        }
        _ => return Ok(None),
    };

    let Pat::Ident(pi) = &*pt.pat else {
        return Ok(None);
    };

    let attr = pt
        .attrs
        .extract_if(.., |attr| attr.path().is_ident("arg"))
        .next();

    let mut many = false;
    let mut forward = false;

    if let Some(attr) = attr {
        let items = attr.parse_args_with(Punctuated::<Ident, Token![,]>::parse_terminated)?;

        for item in items {
            if item == "many" {
                many = true;
            } else if item == "forward" {
                forward = true;
            } else {
                return Err(syn::Error::new_spanned(item, "invalid arg"));
            }
        }
    }

    for bound in &mut tit.bounds {
        let TypeParamBound::Trait(tb) = bound else {
            continue;
        };

        if !tb.path.is_ident("Serialize") {
            continue;
        }

        tb.path = if many {
            parse_quote! { crate::resp::Args }
        } else {
            parse_quote! { crate::resp::Arg }
        };

        if forward {
            return Ok(None);
        } else {
            return Ok(Some(pi.ident.clone()));
        }
    }

    Ok(None)
}

#[proc_macro_derive(Serialize, attributes(serde))]
pub fn derive_args(input: TokenStream) -> TokenStream {
    let mut input = parse_macro_input!(input as DeriveInput);

    let name = input.ident.clone();
    let mod_name = format_ident!("__private_{}", name);
    let inner_ident = format_ident!("{}Inner", name);
    let name_str = LitStr::new(&name.to_string(), name.span());

    input.ident = inner_ident.clone();
    input.vis = syn::Visibility::Public(Token![pub](input.vis.span()));
    Visitor.visit_derive_input_mut(&mut input);

    let (imp, ty, whr) = input.generics.split_for_impl();

    quote! {
        #[allow(non_snake_case)]
        mod #mod_name {
            use super::*;

            #[derive(::serde::original::Serialize)]
            #[serde(remote = #name_str)]
            #input
        }

        impl #imp ::serde::Serialize for #name #ty #whr {
            fn serialize<S: ::serde::Serializer>(&self, serializer: S) -> ::std::result::Result<S::Ok, S::Error> {
                #mod_name::#inner_ident::serialize(self, serializer)
            }
        }

        impl #imp crate::resp::Args for #name #ty #whr {
            fn serialize_args<S: ::serde::Serializer>(&self, serializer: S) -> ::std::result::Result<S::Ok, S::Error> {
                ::serde::Serialize::serialize(self, serializer)
            }
        }
    }.into()
}

struct Visitor;

impl VisitMut for Visitor {
    fn visit_attributes_mut(&mut self, i: &mut Vec<syn::Attribute>) {
        i.retain(|attr| attr.path().is_ident("serde"));
    }
}
