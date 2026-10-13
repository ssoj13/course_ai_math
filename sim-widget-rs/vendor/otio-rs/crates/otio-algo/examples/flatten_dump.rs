//! Differential helper: flatten a timeline's stack and dump the resulting track's
//! child layout. Compared against Python `otio.algorithms.flatten_stack` by
//! `tests/diff_flatten.py`.
//!
//! Usage: `flatten_dump <file.otio>` — prints `childIdx<TAB>schema<TAB>result`,
//! where result is `start_val,start_rate,dur_val,dur_rate` or `ERR`. Prints a
//! single `ERR` line if flattening itself fails.

use std::env;
use std::fs;

use otio_algo::{flatten_stack, range_of_child_at_index};
use otio_core::{canonical_schema, Document};

fn main() {
    let path = env::args().nth(1).expect("usage: flatten_dump <file.otio>");
    let text = fs::read_to_string(&path).unwrap_or_default();
    let mut doc = match Document::from_json_str(&text) {
        Ok(d) => d,
        Err(_) => return,
    };

    // Resolve the tracks stack, then drop the shared borrow before mutating.
    let stack = match doc.root() {
        Some(r) if r.schema_name() == "Timeline" => match r.child("tracks") {
            Some(s) => s,
            None => return,
        },
        _ => return,
    };

    let flat = match flatten_stack(&mut doc, stack) {
        Ok(f) => f,
        Err(_) => {
            println!("ERR");
            return;
        }
    };

    let children = doc.node(flat).expect("flat track").children("children").to_vec();
    for (ci, &child) in children.iter().enumerate() {
        let schema = canonical_schema(doc.node(child).expect("child").schema_name());
        let res = match range_of_child_at_index(&doc, flat, ci as i64) {
            Ok(r) => format!(
                "{},{},{},{}",
                r.start_time().value(),
                r.start_time().rate(),
                r.duration().value(),
                r.duration().rate()
            ),
            Err(_) => "ERR".to_string(),
        };
        println!("{ci}\t{schema}\t{res}");
    }
}
