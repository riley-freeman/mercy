// use std::{
//     collections::HashMap,
//     sync::{LazyLock, Mutex},
// };
//
// use syn::{Expr, ExprLet, Ident, Pat, Token, parse::Parse};
//
// struct State {
//     pub ident: Ident,
//     pub expr: Box<Expr>,
// }
//
// unsafe impl Send for State {}
// unsafe impl Sync for State {}
//
// static STATES: LazyLock<Mutex<HashMap<String, State>>> =
//     LazyLock::new(|| Mutex::new(HashMap::new()));
//
// pub struct States {
//     pub expressions: Vec<Expr>,
// }
//
// impl Parse for States {
//     fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
//         let let_expressions = input.parse_terminated(ExprLet::parse, Token![;])?;
//
//         for let_expression in let_expressions {
//             if let Pat::Ident(ident) = *let_expression.pat {
//                 let expr = let_expression.expr;
//                 let name = ident.ident.to_string();
//                 let state = State {
//                     ident: ident.ident,
//                     expr,
//                 };
//                 STATES.lock().unwrap().insert(name, state);
//             }
//         }
//
//         todo!()
//     }
// }
//
// #[proc_macro]
// pub fn use_states(input: TokenStream) -> TokenStream {
//     let mut expressions = Vec::new();
// }
