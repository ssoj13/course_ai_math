//! Differential-test helper: dump `range_of_child_at_index` for every top-level
//! track in a timeline `.otio` file.
//!
//! Usage: `algo_dump <file.otio>` — prints `trackIdx<TAB>childIdx<TAB>result`,
//! where result is `start_val,start_rate,dur_val,dur_rate` or `ERR`. Compared
//! against the reference Python OTIO by `tests/diff_algo.py`.

use std::env;
use std::fs;

use otio_algo::range_of_child_at_index;
use otio_core::Document;

fn main() {
    let path = env::args().nth(1).expect("usage: algo_dump <file.otio>");
    let text = fs::read_to_string(&path).expect("read file");
    let doc = match Document::from_json_str(&text) {
        Ok(d) => d,
        Err(_) => return, // non-standard JSON (e.g. big_int) — nothing to dump
    };
    let Some(root) = doc.root() else { return };
    if root.schema_name() != "Timeline" {
        return;
    }
    let Some(stack) = root.child("tracks") else { return };
    let track_ids: Vec<_> = doc.node(stack).expect("stack").children("children").to_vec();

    for (ti, &tid) in track_ids.iter().enumerate() {
        let tnode = doc.node(tid).expect("track node");
        if otio_core::canonical_schema(tnode.schema_name()) != "Track" {
            continue;
        }
        let n = tnode.children("children").len();
        for ci in 0..n {
            let res = match range_of_child_at_index(&doc, tid, ci as i64) {
                Ok(r) => format!(
                    "{},{},{},{}",
                    r.start_time().value(),
                    r.start_time().rate(),
                    r.duration().value(),
                    r.duration().rate()
                ),
                Err(_) => "ERR".to_string(),
            };
            println!("{ti}\t{ci}\t{res}");
        }
    }
}
