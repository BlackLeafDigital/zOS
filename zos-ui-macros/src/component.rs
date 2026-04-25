//! Expansion logic for `#[component]` and friends.
//!
//! Input shape:
//! ```ignore
//! pub fn Clock(class: String, hour_format: bool) -> impl View {
//!     // body
//! }
//! ```
//!
//! Output:
//! - A `pub struct Clock { pub class: String, pub hour_format: bool }`.
//! - `impl Clock { fn new(...) -> Self; fn class(self, ...) -> Self; ... }`.
//! - `impl ::zos_ui::Component for Clock { fn view(self) -> impl ::zos_ui::View { ... } }`
//!   where the body is the original function body, prefixed by a destructure
//!   `let Self { class, hour_format } = self;` so the user's body sees the
//!   args by their original names.

use proc_macro2::TokenStream;
use quote::{format_ident, quote, quote_spanned};
use syn::spanned::Spanned;
use syn::{FnArg, Ident, ItemFn, Pat, PatIdent, PatType, Type, parse2};

/// Which flavor of component we're expanding.
///
/// All variants currently produce identical output; `PanelModule` and
/// `TaskbarIcon` are stubbed as `Plain` plus a doc-comment TODO marker for
/// the trait impl that will land once those traits are defined.
pub enum Variant {
    Plain,
    PanelModule,
    TaskbarIcon,
}

pub fn expand(_attr: TokenStream, item: TokenStream, variant: Variant) -> TokenStream {
    let func: ItemFn = match parse2(item) {
        Ok(f) => f,
        Err(e) => return e.to_compile_error(),
    };

    if !func.sig.generics.params.is_empty() {
        return quote_spanned! { func.sig.generics.span() =>
            compile_error!("#[component] does not yet support generic functions");
        };
    }
    if let Some(where_clause) = &func.sig.generics.where_clause {
        return quote_spanned! { where_clause.span() =>
            compile_error!("#[component] does not yet support where clauses");
        };
    }
    if let Some(asyncness) = &func.sig.asyncness {
        return quote_spanned! { asyncness.span() =>
            compile_error!("#[component] functions must not be async");
        };
    }

    let vis = &func.vis;
    let name = &func.sig.ident;
    let body = &func.block;
    let attrs = &func.attrs;

    // Collect (ident, type) pairs from each function arg.
    let mut field_idents: Vec<Ident> = Vec::new();
    let mut field_types: Vec<Type> = Vec::new();

    for arg in &func.sig.inputs {
        match arg {
            FnArg::Receiver(r) => {
                return quote_spanned! { r.span() =>
                    compile_error!("#[component] functions must not take self");
                };
            }
            FnArg::Typed(PatType { pat, ty, .. }) => match pat.as_ref() {
                Pat::Ident(PatIdent {
                    ident,
                    by_ref,
                    mutability,
                    subpat,
                    ..
                }) => {
                    if let Some(by_ref) = by_ref {
                        return quote_spanned! { by_ref.span() =>
                            compile_error!("#[component] argument patterns must be plain `name: Type` (no `ref`)");
                        };
                    }
                    if let Some(mutability) = mutability {
                        return quote_spanned! { mutability.span() =>
                            compile_error!("#[component] argument patterns must be plain `name: Type` (no `mut`)");
                        };
                    }
                    if let Some((_, subpat)) = subpat {
                        return quote_spanned! { subpat.span() =>
                            compile_error!("#[component] argument patterns must be plain `name: Type` (no sub-patterns)");
                        };
                    }
                    field_idents.push(ident.clone());
                    field_types.push((**ty).clone());
                }
                other => {
                    return quote_spanned! { other.span() =>
                        compile_error!("#[component] argument patterns must be plain `name: Type` (no destructuring)");
                    };
                }
            },
        }
    }

    let setter_idents: Vec<Ident> = field_idents
        .iter()
        .map(|f| format_ident!("{}", f, span = f.span()))
        .collect();

    // Generate one builder method per field. Skip generation if there are
    // zero fields — `new()` alone is fine in that case.
    let builder_methods = field_idents.iter().zip(field_types.iter()).map(|(ident, ty)| {
        quote! {
            #[allow(clippy::missing_const_for_fn)]
            pub fn #ident(mut self, #ident: #ty) -> Self {
                self.#ident = #ident;
                self
            }
        }
    });

    // Doc-comment marker for the specialized variants. The actual trait impls
    // (`PanelModule`, `TaskbarIconWidget`) will be wired up once those traits
    // are defined in zos-ui.
    let variant_marker = match variant {
        Variant::Plain => quote! {},
        Variant::PanelModule => quote! {
            // TODO(panel-module-impl): wire to PanelModule trait once defined in zos-ui.
        },
        Variant::TaskbarIcon => quote! {
            // TODO(taskbar-icon-impl): wire to TaskbarIconWidget trait once defined in zos-ui.
        },
    };

    let destructure = if field_idents.is_empty() {
        quote! { let Self {} = self; }
    } else {
        quote! { let Self { #(#field_idents),* } = self; }
    };

    quote! {
        #variant_marker

        #(#attrs)*
        #vis struct #name {
            #( pub #field_idents: #field_types, )*
        }

        impl #name {
            #[allow(clippy::too_many_arguments)]
            pub fn new( #( #field_idents: #field_types ),* ) -> Self {
                Self { #( #setter_idents ),* }
            }

            #( #builder_methods )*
        }

        impl ::zos_ui::Component for #name {
            fn view(self) -> impl ::zos_ui::View {
                #destructure
                #body
            }
        }
    }
}
