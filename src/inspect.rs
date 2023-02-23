// use std::fs;
//
// use std::collections::HashMap;
//
// use std::io::Read;
// use std::path::PathBuf;
//
// use git2::{ObjectType, Repository, TreeWalkMode};
// use indicatif::ParallelProgressIterator;
// use indicatif::{ProgressIterator, ProgressStyle};
// use itertools::Itertools;
// use rayon::iter::ParallelIterator;
// use rayon::prelude::*;
// use rustpython_parser::ast::{ExprKind, Stmt, StmtKind};
//
// #[derive(Default, Clone, Debug, Copy)]
// pub struct State {
//     // pub statements_to_find: HashSet<TypeId>,
//     // pub counts: HashMap<TypeId, u64>
//     pub standard_classes: usize,
//     pub dataclasses: usize,
// }
//
// pub fn is_dataclass(expr: &ExprKind) -> bool {
//     match expr {
//         ExprKind::Attribute {
//             value: _,
//             attr,
//             ctx: _,
//         } => attr == "dataclass",
//         ExprKind::Call {
//             func,
//             args: _,
//             keywords: _,
//         } => is_dataclass(&func.node),
//         ExprKind::Name { id, ctx: _ } => {
//             if id == "dataclasses" || id == "dataclass" {
//                 return true;
//             }
//             false
//         }
//         _ => false,
//     }
// }
//
// pub fn find_stuff(statement: &Stmt, state: &mut State) {
//     match &statement.node {
//         StmtKind::FunctionDef {
//             name: _,
//             args: _,
//             body,
//             decorator_list: _,
//             returns: _,
//             type_comment: _,
//         } => {
//             for item in body {
//                 find_stuff(item, state);
//             }
//         }
//         StmtKind::AsyncFunctionDef {
//             name: _,
//             args: _,
//             body,
//             decorator_list: _,
//             returns: _,
//             type_comment: _,
//         } => {
//             for item in body {
//                 find_stuff(item, state);
//             }
//         }
//         StmtKind::ClassDef {
//             name: _,
//             bases: _,
//             keywords: _,
//             body,
//             decorator_list,
//         } => {
//             if decorator_list.iter().any(|d| is_dataclass(&d.node)) {
//                 state.dataclasses += 1;
//             } else {
//                 state.standard_classes += 1;
//             }
//
//             for item in body {
//                 find_stuff(item, state);
//             }
//         }
//
//         StmtKind::Expr { value: _ } => {}
//         _ => {}
//     }
// }
//
// pub fn parse_file(item: PathBuf) {
//     // let mut contents = File::open(item).unwrap();
//     // let total = contents.read_to_end().unwrap();
//     let contents = fs::read_to_string(item).unwrap();
//     let res = rustpython_parser::parse_program(&contents, "foo").unwrap();
//
//     let mut state = State::default();
//
//     for statement in res {
//         find_stuff(&statement, &mut state);
//     }
//     println!("State: {state:#?}");
//     // println!("{:#?}", res);
//     // let item = serde_json::to_string(&res).unwrap();
//     // println!("{item}");
// }
//
// pub fn parse(repo: PathBuf) {
//     let repo = Repository::open(repo).unwrap();
//     let odb = repo.odb().unwrap();
//     let mut oids = vec![];
//     let tree = repo.head().unwrap().peel_to_tree().unwrap();
//
//     tree.walk(TreeWalkMode::PostOrder, |v, item| {
//         if let Some(ObjectType::Blob) = item.kind() {
//             if v.contains('/') {
//                 let path: String = v.split('/').take(3).join("/");
//                 oids.push((path, item.id()));
//             }
//         }
//         0
//     })
//     .unwrap();
//
//     // odb.foreach(|oid| {
//     //     // let entry = tree.get_path(Path)
//     //     // let obj = repo.find_object(*oid, None).unwrap();
//     //     // repo.commit
//     //     // obj.peel_to_tree().unwrap();
//     //     // obj.as_blob().unwrap().
//     //     // let walk = repo.revwalk().unwrap();
//     //
//     //     oids.push(oid.clone());
//     //     true
//     // }).unwrap();
//
//     // let tree_thing = repo.head().unwrap().peel_to_tree().unwrap();
//     // tree_thing.walk(TreeWalkMode::PostOrder, |x, y| {
//     //     if let Some(ObjectType::Blob) = y.kind() {
//     //         oids.push(y.id());
//     //     }
//     //     0
//     // })
//     // .unwrap();
//
//     let oids: Vec<_> = oids.into_iter().take(1_000_000).collect();
//
//     // let pbar = ProgressBar::new(total);
//     // pbar.set_message("Parsing");
//     // pbar.set_style(
//     //     ProgressStyle::with_template("{wide_bar} {pos}/{len} {msg} ({per_sec})").unwrap()
//     // );
//     // pbar.enable_steady_tick(Duration::from_secs(1));
//
//     let style =
//         ProgressStyle::with_template("{wide_bar} {pos}/{len} {msg} ({per_sec:>5})").unwrap();
//
//     // let total = oids.len() as u64;
//     let totals: Vec<_> = oids
//         .into_par_iter()
//         .progress_with_style(style)
//         .flat_map(|(path, oid)| {
//             let mut state = State::default();
//             let obj = odb.read(oid).unwrap();
//             // println!("kind: {}", obj.kind());
//             match obj.kind() {
//                 ObjectType::Blob => {
//                     let data = obj.data();
//                     let text = std::str::from_utf8(data);
//                     match text {
//                         Ok(data) => {
//                             // let res = libcst::parse_module(data, Some("utf8"));
//                             let res = rustpython_parser::parse_program(data, "foo");
//                             match res {
//                                 Ok(parsed) => {
//                                     // println!("{:#?}", parsed);
//                                     // let locked = io::stdout().lock();
//                                     for statement in &parsed {
//                                         find_stuff(statement, &mut state);
//                                     }
//                                     return Some((path, state));
//                                     // let item = serde_json::to_string(&parsed).unwrap();
//                                     // println!("{item}");
//                                     // serde_json::to_writer(locked, &parsed).unwrap();
//                                     // let x = serde_json::to_string(&parsed).unwrap();
//                                     // return parsed.body.len();
//                                 }
//                                 Err(_) => {
//                                     // total_errors += 1;
//                                 }
//                             }
//                         }
//                         Err(_e) => {
//                             // println!("Error decoding utf8: {e}");
//                         }
//                     }
//                 }
//                 _ => {}
//             };
//             None
//         })
//         .collect();
//
//     let grouped: HashMap<String, _> = totals
//         .into_iter()
//         .progress()
//         .group_by(|v| v.0.clone())
//         .into_iter()
//         .map(|(k, v)| {
//             (
//                 k,
//                 v.into_iter()
//                     .map(|i| i.1)
//                     .reduce(|mut acc, el| {
//                         acc.standard_classes += el.standard_classes;
//                         acc.dataclasses += el.dataclasses;
//                         acc
//                     })
//                     .unwrap(),
//             )
//         })
//         .collect();
//     for (key, value) in grouped.iter().sorted_by_cached_key(|(_k, v)| v.dataclasses) {
//         println!("Item: {key}");
//         println!("Value: {value:?}");
//     }
//
//     // .reduce_with(|mut a, b| {
//     //     // println!("a {a:?} b {b:?}");
//     //     // (a.0 + b.0, a.1 + b.1)
//     //     a.dataclasses += b.dataclasses;
//     //     a.standard_classes += b.standard_classes;
//     //     a
//     // });
//
//     // println!("total {totals:#?}");
//     //
//     // let mut total_body = 0;
//     // let mut total_errors = 0;
//     //
//     // odb.foreach(|v| {
//     //     let obj = odb.read(*v).unwrap();
//     //     pbar.inc(1);
//     //
//     //     return match obj.kind() {
//     //         ObjectType::Blob => {
//     //             let data = obj.data();
//     //             let text = std::str::from_utf8(data);
//     //             match text {
//     //                 Ok(data) => {
//     //                     match libcst::parse_module(data, Some("utf8")) {
//     //                         Ok(parsed) => {
//     //                             total_body += parsed.body.len();
//     //                         }
//     //                         Err(_) => {
//     //                             total_errors += 1;
//     //                             pbar.set_message(format!("errors: {total_errors}"));
//     //                         }
//     //                     }
//     //                 }
//     //                 Err(e) => {
//     //                     println!("Error decoding utf8: {e}");
//     //                 }
//     //             }
//     //             true
//     //         }
//     //         _ => {
//     //             true
//     //         }
//     //     };
//     // }).unwrap();
//     // // let x = libcst::parse_module(include_str!("foo.py"), Some("utf8")).unwrap();
//     // // let x = rustpython_parser::parse_program(include_str!("foo.py"), "foo").unwrap();
//     // // println!("{x:#?}");
//     // println!("Total body len: {total_body}")
// }
//
// // use std::error::Error;
// // use std::path::PathBuf;
// // use git2::{ObjectType, Repository, TreeWalkMode};
// // use std::str;
// // use std::str::Utf8Error;
// // use std::time::Duration;
// // use indicatif::{ProgressBar, ProgressStyle};
// //
// // pub fn parse_index(repo: PathBuf) -> usize {
// //     let repo = Repository::open(repo).unwrap();
// //     let head = repo.head().unwrap().peel_to_tree().unwrap();
// //     let odb = repo.odb().unwrap();
// //     let repo_idx = repo.index().unwrap();
// //
// //     let pbar = ProgressBar::new_spinner();
// //     pbar.set_message("Counting entries");
// //     pbar.set_style(
// //         ProgressStyle::with_template("{spinner} {msg} ({per_sec})").unwrap()
// //     );
// //     pbar.enable_steady_tick(Duration::from_secs(1));
// //
// //     let mut total = 0;
// //     head.walk(TreeWalkMode::PreOrder, |f, v| {
// //         if let Some(ObjectType::Blob) = v.kind() {
// //             if v.name().unwrap().ends_with(".py") {
// //                 total += 1;
// //             }
// //         }
// //         pbar.inc(1);
// //         0
// //     }).unwrap();
// //     pbar.finish();
// //
// //
// //     let pbar = ProgressBar::new(total);
// //     pbar.set_message("Parsing");
// //     pbar.set_style(
// //         ProgressStyle::with_template("{wide_bar} {pos}/{len} {msg} ({per_sec})").unwrap()
// //     );
// //     pbar.enable_steady_tick(Duration::from_secs(1));
// //     let mut total = 0;
// //     head.walk(TreeWalkMode::PreOrder, |f, v| {
// //         if let Some(ObjectType::Blob) = v.kind() {
// //             if v.name().unwrap().ends_with(".py") {
// //                 let f = PathBuf::new().join(f).join(v.name().unwrap());
// //                 let res = repo_idx.get_path(f.as_path(), 0).unwrap();
// //                 let obj = odb.read(v.id()).unwrap();
// //                 let contents = match str::from_utf8(obj.data()) {
// //                     Ok(c) => c,
// //                     Err(_) => return 0,
// //                 };
// //                 let x = rustpython_parser::parser::parse_program(&contents, "foo");
// //                 // total += rustpython_parser::lexer::make_tokenizer(&contents).count();
// //                 // let x = libcst_native::parse_module(, Some("utf8"));
// //                 pbar.inc(1);
// //                 match x {
// //                     Ok(b) => {
// //                         total += b.len();
// //                     }
// //                     Err(e) => {
// //                         println!("Error: {}", e.to_string());
// //                         // https://github.com/orf/pypi-code-repo/blob/master/code/cipher5/0.0.1/tar.gz/Cipher5-0.0.1/setup.py
// //                         // println!("Error in file {}{}", f, v.name().unwrap());
// //                         // println!("https://github.com/orf/pypi-code-repo/blob/master/{}{}", f, v.name().unwrap());
// //                     }
// //                 }
// //             }
// //         }
// //         0
// //     }).unwrap();
// //     pbar.finish();
// //     total
// // }
