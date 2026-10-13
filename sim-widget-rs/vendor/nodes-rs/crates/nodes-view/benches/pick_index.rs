use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use nodes_core::{NodeId, PinSide};
use nodes_view::engine::pick_index::PickIndex;
use nodes_view::engine::shape2d::{PickLayer, PickTarget, Shape2d, ShapeInstance, ShapeStyle};

fn bench_instances(count: usize) -> Vec<ShapeInstance> {
    let mut instances = Vec::with_capacity(count);
    let side = (count as f32).sqrt().ceil() as usize;
    for i in 0..count {
        let row = i / side;
        let col = i % side;
        let x = col as f32 * 120.0;
        let y = row as f32 * 90.0;
        let id = NodeId::new();
        instances.push(ShapeInstance {
            shape: Shape2d::RoundedRect {
                min: [x, y],
                max: [x + 100.0, y + 70.0],
                radius: 4.0,
            },
            style: ShapeStyle::default(),
            layer: PickLayer::Body,
            pick: PickTarget::NodeBody(id),
            hollow_border: None,
            pickable: true,
            pick_id: 0,
            depth_bias: 0.0,
            // Axis-aligned: the pick index this bench measures is rotation-agnostic.
            rotation: 0.0,
            pivot: [x + 50.0, y + 35.0],
        });
        if i % 7 == 0 {
            instances.push(ShapeInstance {
                shape: Shape2d::Circle {
                    center: [x + 50.0, y + 35.0],
                    radius: 6.0,
                },
                style: ShapeStyle::default(),
                layer: PickLayer::Pin,
                pick: PickTarget::Pin {
                    node: id,
                    side: PinSide::Output,
                    index: 0,
                },
                hollow_border: None,
                pickable: true,
                pick_id: 0,
                depth_bias: 0.0,
                rotation: 0.0,
                pivot: [x + 50.0, y + 35.0],
            });
        }
    }
    instances
}

fn pick_bench(c: &mut Criterion) {
    let instances = bench_instances(50_000);
    let index = PickIndex::build(&instances, PickIndex::default_cell());
    let point = [600.0, 450.0];

    c.bench_function("pick_topmost_50k", |b| {
        b.iter(|| black_box(index.pick_topmost(black_box(point), 0.0)))
    });
}

criterion_group!(benches, pick_bench);
criterion_main!(benches);
