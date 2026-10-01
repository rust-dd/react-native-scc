//! Opening may repair a torn tail in place, so point it at a copy of the store files.

use std::path::Path;
use std::process::ExitCode;

use kv_core::{OpenOptions, Store, Value};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [dir, id, key] = args.as_slice() else {
        eprintln!("usage: dump_value <dir> <id> <key>");
        return ExitCode::from(2);
    };
    let store = match Store::open(Path::new(dir), id, OpenOptions::default()) {
        Ok(store) => store,
        Err(error) => {
            eprintln!("cannot open {id} in {dir}: {error}");
            return ExitCode::FAILURE;
        }
    };
    match store.get(key) {
        Some(Value::Str(text) | Value::Json(text)) => println!("{text}"),
        Some(Value::Num(number)) => println!("{number}"),
        Some(Value::Bool(flag)) => println!("{flag}"),
        Some(Value::Bytes(bytes)) => println!("<{} bytes>", bytes.len()),
        None => {
            eprintln!("{key} not found");
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}
