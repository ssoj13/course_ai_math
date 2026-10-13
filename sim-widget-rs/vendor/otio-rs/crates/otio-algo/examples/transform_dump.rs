//! Differential helper: for each clip (depth-first), map its local start time
//! into the top stack's coordinate space via `transformed_time`. Compared against
//! Python `clip.transformed_time(start, timeline.tracks)` by `tests/diff_transform.py`.
//!
//! Usage: `transform_dump <file.otio>` — prints `clipIdx<TAB>value,rate` (or `ERR`).

use std::env;
use std::fs;

use otio_algo::{transformed_time, trimmed_range};
use otio_core::{canonical_schema, Document, NodeId};

/// Collect clip node ids depth-first, in document order (matching `find_clips`).
fn collect_clips(doc: &Document, node: NodeId, out: &mut Vec<NodeId>) {
    let Some(n) = doc.node(node) else { return };
    match canonical_schema(n.schema_name()) {
        "Clip" => out.push(node),
        "Track" | "Stack" => {
            for &c in n.children("children") {
                collect_clips(doc, c, out);
            }
        }
        _ => {}
    }
}

fn main() {
    let path = env::args().nth(1).expect("usage: transform_dump <file.otio>");
    let text = fs::read_to_string(&path).unwrap_or_default();
    let doc = match Document::from_json_str(&text) {
        Ok(d) => d,
        Err(_) => return,
    };
    let stack = match doc.root() {
        Some(r) if r.schema_name() == "Timeline" => match r.child("tracks") {
            Some(s) => s,
            None => return,
        },
        _ => return,
    };

    let mut clips = Vec::new();
    collect_clips(&doc, stack, &mut clips);

    for (i, &clip) in clips.iter().enumerate() {
        let res = match trimmed_range(&doc, clip)
            .and_then(|tr| transformed_time(&doc, clip, tr.start_time(), stack))
        {
            Ok(tt) => format!("{},{}", tt.value(), tt.rate()),
            Err(_) => "ERR".to_string(),
        };
        println!("{i}\t{res}");
    }
}
