mod service;
mod proxy;

use syn::{FnArg, Pat, Ident, Type, ReturnType, Signature};
use quote::{quote, format_ident};

use service::service_macro;
use proxy::proxy_macro;
use proc_macro2::{TokenStream, Literal};


#[proc_macro_attribute]
pub fn service(args: proc_macro::TokenStream, input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    service_macro(args, input)
}

#[proc_macro_attribute]
pub fn proxy(args: proc_macro::TokenStream, input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    proxy_macro(args, input)
}


pub(crate) struct RpcMethod<'a> {
    signature: &'a Signature,
    return_ty: TokenStream,
    args: Vec<(&'a Ident, &'a Type)>,
    method_name_literal: Literal,
    args_struct_ident: Ident,
}


impl<'a> RpcMethod<'a> {
    pub fn new(signature: &'a Signature, args_struct_prefix: &'a str) -> Self {
        let return_ty = match &signature.output {
            ReturnType::Default => quote! { () },
            ReturnType::Type(_, ty) => quote! { #ty },
        };
        //println!("return_ty = {:?}", return_ty);

        let mut has_self = false;
        let mut args = vec![];

        for arg in &signature.inputs {
            match arg {
                FnArg::Receiver(_) => {
                    has_self = true;
                },
                FnArg::Typed(arg) => {
                    let ident = match &*arg.pat {
                        Pat::Ident(ty) => &ty.ident,
                        _ => panic!("Arguments must not be patterns."),
                    };
                    args.push((ident, &*arg.ty));
                },
            }
        }

        if !has_self {
            panic!("Method signature doesn't take self");
        }

        let method_name_literal = Literal::string(&signature.ident.to_string());
        let args_struct_ident = format_ident!("{}_{}", args_struct_prefix, signature.ident);

        Self {
            signature,
            return_ty,
            args,
            method_name_literal,
            args_struct_ident,
        }
    }

    pub fn generate_args_struct(&self) -> TokenStream {
        let struct_fields = self.args.iter()
            .map(|(ident, ty)| quote! { #ident: #ty, })
            .collect::<Vec<TokenStream>>();
        let args_struct_ident = &self.args_struct_ident;

        let tokens = quote! {
            #[derive(Debug, Serialize, Deserialize)]
            #[allow(non_camel_case_types)]
            struct #args_struct_ident {
                #(#struct_fields)*
            }
        };

        //println!("struct tokens: {}", tokens);

        tokens
    }

    pub fn generate_dispatcher_match_arm(&self) -> TokenStream {
        let method_args = self.args.iter()
            .map(|(ident, _)| quote! { params.#ident })
            .collect::<Vec<TokenStream>>();
        let args_struct_ident = &self.args_struct_ident;
        let return_ty = &self.return_ty;
        let method_ident = &self.signature.ident;
        let method_name_literal = &self.method_name_literal;

        if method_args.is_empty() {
            quote! {
                #method_name_literal => {
                    return ::nimiq_jsonrpc_server::dispatch_method_without_args(
                        request,
                        || async {
                            Ok::<#return_ty, ()>(self.#method_ident().await)
                        }
                    ).await
                },
            }
        }
        else {
            quote! {
                #method_name_literal => {
                    return ::nimiq_jsonrpc_server::dispatch_method_with_args::<#args_struct_ident, #return_ty, (), _, _>(
                        request,
                        |params: #args_struct_ident| async {
                            Ok::<#return_ty, ()>(self.#method_ident(#(#method_args),*).await)
                        }
                    ).await
                }
            }
        }
    }

    pub fn generate_proxy_method(&self) -> TokenStream {
        let method_ident = &self.signature.ident;
        let return_ty = &self.return_ty;
        let args_struct_ident = &self.args_struct_ident;
        let method_name_literal = &self.method_name_literal;

        if self.args.is_empty() {
            quote! {
                async fn #method_ident(&mut self) -> #return_ty {
                    self.client.call_method::<(), #return_ty>(
                        #method_name_literal,
                        &()
                    ).await
                }
            }
        }
        else {
            let method_args = self.args.iter()
                .map(|(ident, ty)| quote! { #ident: #ty })
                .collect::<Vec<TokenStream>>();

            let struct_fields = self.args.iter()
                .map(|(ident, _)| quote! { #ident })
                .collect::<Vec<TokenStream>>();

            quote! {
                async fn #method_ident(&mut self, #(#method_args),*) -> #return_ty {
                    let args = #args_struct_ident {
                        #(#struct_fields),*
                    };
                    self.client.call_method::<#args_struct_ident, #return_ty>(
                        #method_name_literal,
                        &args
                    ).await
                }
            }
        }
    }
}