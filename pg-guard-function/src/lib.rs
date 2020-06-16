
extern crate proc_macro;

use std::mem::replace;

use proc_macro::TokenStream;
use quote::quote;
use syn::{
    parse_macro_input,
    parse_quote,
    FnArg,
    ForeignItem,
    Item,
    ItemFn,
    ItemForeignMod,
    ItemMod,
    Token,
    VisPublic,
};

#[proc_macro_attribute]
pub fn pg_guard_function(args: TokenStream, module: TokenStream) -> TokenStream {
    assert!(args.is_empty());
    let mut module = parse_macro_input!(module as ItemMod);
    match &mut module.content {
        None => (),
        Some((_, items)) => {
            for item in items {
                let i = replace(item, Item::Verbatim(Default::default()));
                *item = match i {
                    Item::ForeignMod(function) => wrap_function(function),
                    _ => i,
                };
            }
        }
    }
    TokenStream::from(quote!(#module))
}

fn wrap_function(foreign: ItemForeignMod) -> Item {
    let mut signature = match &foreign.items[..] {
        [ForeignItem::Fn(function)] => {
            // we can't make rust varadic functions, so just bail
            if function.sig.variadic.is_some() {
                return Item::ForeignMod(foreign)
            }
            function.sig.clone()
        },

        // TODO multi-function blocks require duplicating the mod
        items
            if items.iter().any(|i| matches!(i, ForeignItem::Fn(..)))
            => todo!("multi-fn `extern` blocks are not yet implemented\n{}", foreign.to_token_stream()),
        _ => return Item::ForeignMod(foreign),
    };

    signature.unsafety = Some(<Token![unsafe]>::default());
    let name = signature.ident.clone();
    let args: Vec<_> = signature.inputs.iter().map(|arg| match arg {
        FnArg::Receiver(..) => unreachable!(),
        FnArg::Typed(arg) => arg.pat.clone(),
    }).collect();
    use quote::ToTokens;

    let i = ItemFn {
        attrs: vec![],
        vis: VisPublic{pub_token: <Token!(pub)>::default()}.into(),
        sig: signature,
        block: Box::new(parse_quote!({
            #foreign;
            crate::guard_pg(|| #name(#(#args),*) )
        })),
    };

    Item::Fn(i)
}
