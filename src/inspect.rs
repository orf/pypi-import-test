// use std::error::Error;
// use std::path::PathBuf;
// use git2::{ObjectType, Repository, TreeWalkMode};
// use std::str;
// use std::str::Utf8Error;
// use std::time::Duration;
// use indicatif::{ProgressBar, ProgressStyle};
//
// pub fn parse_index(repo: PathBuf) -> usize {
//     let repo = Repository::open(repo).unwrap();
//     let head = repo.head().unwrap().peel_to_tree().unwrap();
//     let odb = repo.odb().unwrap();
//     let repo_idx = repo.index().unwrap();
//
//     let pbar = ProgressBar::new_spinner();
//     pbar.set_message("Counting entries");
//     pbar.set_style(
//         ProgressStyle::with_template("{spinner} {msg} ({per_sec})").unwrap()
//     );
//     pbar.enable_steady_tick(Duration::from_secs(1));
//
//     let mut total = 0;
//     head.walk(TreeWalkMode::PreOrder, |f, v| {
//         if let Some(ObjectType::Blob) = v.kind() {
//             if v.name().unwrap().ends_with(".py") {
//                 total += 1;
//             }
//         }
//         pbar.inc(1);
//         0
//     }).unwrap();
//     pbar.finish();
//
//
//     let pbar = ProgressBar::new(total);
//     pbar.set_message("Parsing");
//     pbar.set_style(
//         ProgressStyle::with_template("{wide_bar} {pos}/{len} {msg} ({per_sec})").unwrap()
//     );
//     pbar.enable_steady_tick(Duration::from_secs(1));
//     let mut total = 0;
//     head.walk(TreeWalkMode::PreOrder, |f, v| {
//         if let Some(ObjectType::Blob) = v.kind() {
//             if v.name().unwrap().ends_with(".py") {
//                 let f = PathBuf::new().join(f).join(v.name().unwrap());
//                 let res = repo_idx.get_path(f.as_path(), 0).unwrap();
//                 let obj = odb.read(v.id()).unwrap();
//                 let contents = match str::from_utf8(obj.data()) {
//                     Ok(c) => c,
//                     Err(_) => return 0,
//                 };
//                 let x = rustpython_parser::parser::parse_program(&contents, "foo");
//                 // total += rustpython_parser::lexer::make_tokenizer(&contents).count();
//                 // let x = libcst_native::parse_module(, Some("utf8"));
//                 pbar.inc(1);
//                 match x {
//                     Ok(b) => {
//                         total += b.len();
//                     }
//                     Err(e) => {
//                         println!("Error: {}", e.to_string());
//                         // https://github.com/orf/pypi-code-repo/blob/master/code/cipher5/0.0.1/tar.gz/Cipher5-0.0.1/setup.py
//                         // println!("Error in file {}{}", f, v.name().unwrap());
//                         // println!("https://github.com/orf/pypi-code-repo/blob/master/{}{}", f, v.name().unwrap());
//                     }
//                 }
//             }
//         }
//         0
//     }).unwrap();
//     pbar.finish();
//     total
// }
