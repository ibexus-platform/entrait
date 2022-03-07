//!
//! inputs to procedural macros
//!

use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::spanned::Spanned;

///
/// The `entrait` invocation
///
pub struct EntraitAttr {
    pub trait_ident: syn::Ident,
    pub impl_target_type: Option<syn::Type>,
    pub debug: bool,
    pub async_trait: bool,
    pub mockable: bool,
    pub mock_deps_as: Option<syn::Ident>,
}

///
/// "keyword args" to `entrait`.
///
pub enum Extension {
    Debug(bool),
    AsyncTrait(bool),
    Mockable(bool),
    MockDepsAs(syn::Ident),
}

///
/// The "body" that is decorated with entrait.
///
pub struct EntraitFn {
    pub fn_attrs: Vec<syn::Attribute>,
    pub fn_vis: syn::Visibility,
    pub fn_sig: syn::Signature,
    // don't try to parse fn_body, just pass through the tokens:
    pub fn_body: proc_macro2::TokenStream,

    pub trait_fn_inputs: proc_macro2::TokenStream,
    pub call_param_list: proc_macro2::TokenStream,
}

impl Parse for EntraitAttr {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let trait_ident = input.parse()?;

        let impl_target_type = if input.peek(syn::token::For) {
            input.parse::<syn::token::For>()?;
            Some(input.parse()?)
        } else {
            None
        };

        let mut debug = false;
        let mut async_trait = false;
        let mut mockable = false;
        let mut mock_deps = None;

        while input.peek(syn::token::Comma) {
            input.parse::<syn::token::Comma>()?;

            match input.parse::<Extension>()? {
                Extension::Debug(enabled) => debug = enabled,
                Extension::AsyncTrait(enabled) => async_trait = enabled,
                Extension::Mockable(enabled) => mockable = enabled,
                Extension::MockDepsAs(ident) => mock_deps = Some(ident),
            };
        }

        Ok(EntraitAttr {
            trait_ident,
            impl_target_type,
            debug,
            async_trait,
            mockable,
            mock_deps_as: mock_deps,
        })
    }
}

impl Parse for Extension {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let ident: syn::Ident = input.parse()?;
        let span = ident.span();
        let ident_string = ident.to_string();

        input.parse::<syn::token::Eq>()?;

        match ident_string.as_str() {
            "debug" => Ok(Extension::Debug(input.parse::<syn::LitBool>()?.value())),
            "async_trait" => Ok(Extension::AsyncTrait(
                input.parse::<syn::LitBool>()?.value(),
            )),
            "mockable" => Ok(Extension::Mockable(input.parse::<syn::LitBool>()?.value())),
            "mock_deps_as" => Ok(Extension::MockDepsAs(input.parse()?)),
            _ => Err(syn::Error::new(
                span,
                format!("Unkonwn entrait extension \"{ident_string}\""),
            )),
        }
    }
}

impl Parse for EntraitFn {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let fn_attrs = input.call(syn::Attribute::parse_outer)?;
        let fn_vis = input.parse()?;
        let fn_sig: syn::Signature = input.parse()?;
        let fn_body = input.parse()?;

        let trait_fn_inputs = extract_trait_fn_inputs(&fn_sig)?;
        let call_param_list = extract_call_param_list(&fn_sig)?;

        Ok(EntraitFn {
            fn_attrs,
            fn_vis,
            fn_sig,
            fn_body,
            trait_fn_inputs,
            call_param_list,
        })
    }
}

fn extract_trait_fn_inputs(sig: &syn::Signature) -> syn::Result<proc_macro2::TokenStream> {
    let mut inputs = sig.inputs.clone();

    if inputs.is_empty() {
        return Err(syn::Error::new(
            sig.span(),
            "Function must take at least one parameter",
        ));
    }

    let first_mut = inputs.first_mut().unwrap();
    *first_mut = syn::parse_quote! { &self };

    Ok(quote! {
        #inputs
    })
}

fn extract_call_param_list(sig: &syn::Signature) -> syn::Result<proc_macro2::TokenStream> {
    let params = sig
        .inputs
        .iter()
        .enumerate()
        .map(|(index, arg)| {
            if index == 0 {
                Ok(quote! { self })
            } else {
                match arg {
                    syn::FnArg::Receiver(_) => {
                        Err(syn::Error::new(arg.span(), "Unexpected receiver arg"))
                    }
                    syn::FnArg::Typed(pat_typed) => match pat_typed.pat.as_ref() {
                        syn::Pat::Ident(pat_ident) => {
                            let ident = &pat_ident.ident;
                            Ok(quote! { #ident })
                        }
                        _ => Err(syn::Error::new(
                            arg.span(),
                            "Expected ident for function argument",
                        )),
                    },
                }
            }
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(quote! {
        #(#params),*
    })
}

///
/// Input to `entrait::generate_mock`.
/// its purpose is to output an `mockall::mock!` invocation.
///
/// `mockall::mock` is supposed to output `C {} impl T for C {..}..`,
/// This input receives trait items instead of impl items.
/// The reason for that is to handle a hygiene issue involving `self` when
/// outputting impl items in macro_rules.
/// `generate_mock` will rewrite trait items to impl items automatically.
///
pub struct EntraitGenerateMockInput {
    pub mock_ident: syn::Ident,
    pub trait_items: Vec<syn::ItemTrait>,
}

impl Parse for EntraitGenerateMockInput {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mock_ident = input.parse()?;
        let mut trait_items: Vec<syn::ItemTrait> = Vec::new();

        while !input.is_empty() {
            trait_items.push(input.parse()?);
        }

        Ok(EntraitGenerateMockInput {
            mock_ident,
            trait_items,
        })
    }
}
