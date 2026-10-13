//! Uniform-grid acceleration for CPU pick (same instances as GPU shapes).

use std::collections::HashMap;

use super::shape2d::{PickLayer, PickTarget, ShapeInstance};

const DEFAULT_CELL: f32 = 48.0;

#[derive(Debug, Clone, Default)]
pub struct PickIndex {
    cell_size: f32,
    cells: HashMap<(i32, i32), Vec<u32>>,
    entries: Vec<PickEntry>,
}

#[derive(Debug, Clone)]
struct PickEntry {
    shape: super::shape2d::Shape2d,
    layer: PickLayer,
    pick: PickTarget,
    hollow_border: Option<f32>,
    pickable: bool,
}

impl PickIndex {
    pub fn build(instances: &[ShapeInstance], cell_size: f32) -> Self {
        let cell_size = cell_size.max(8.0);
        let mut cells: HashMap<(i32, i32), Vec<u32>> = HashMap::new();
        let mut entries = Vec::with_capacity(instances.len());

        for inst in instances {
            let idx = entries.len() as u32;
            entries.push(PickEntry {
                shape: inst.shape.clone(),
                layer: inst.layer,
                pick: inst.pick.clone(),
                hollow_border: inst.hollow_border,
                pickable: inst.pickable,
            });
            let aabb = inst.shape.aabb();
            insert_aabb(&mut cells, idx, aabb, cell_size);
        }

        Self {
            cell_size,
            cells,
            entries,
        }
    }

    pub fn default_cell() -> f32 {
        DEFAULT_CELL
    }

    /// Highest pick layer wins; within a layer, later instance index wins (draw order).
    /// Pins drawn before a later body/title are occluded (plan2 socket_is_occluded).
    pub fn pick_topmost(&self, world: [f32; 2], padding: f32) -> Option<PickTarget> {
        let hits = self.hits_at(world, padding);
        resolve_pick(hits)
    }

    fn hits_at(&self, world: [f32; 2], padding: f32) -> Vec<(u32, PickLayer, PickTarget)> {
        let (cx, cy) = cell_of(world, self.cell_size);
        let mut hits = Vec::new();

        for dx in -1..=1 {
            for dy in -1..=1 {
                // saturating: a non-finite / extreme cursor world coord makes `cell_of`
                // return i32::MAX/MIN, and `+ dx` would overflow in debug builds. Pick
                // must never panic on any pointer position.
                let key = (cx.saturating_add(dx), cy.saturating_add(dy));
                let Some(list) = self.cells.get(&key) else {
                    continue;
                };
                for &idx in list {
                    let entry = &self.entries[idx as usize];
                    if !entry.pickable {
                        continue;
                    }
                    let hit = match entry.hollow_border {
                        Some(b) => entry.shape.contains_point_border(world, padding, b),
                        None => entry.shape.contains_point(world, padding),
                    };
                    if hit {
                        hits.push((idx, entry.layer, entry.pick.clone()));
                    }
                }
            }
        }

        hits
    }
}

#[cfg(test)]
mod overflow_tests {
    use super::*;

    #[test]
    fn pick_does_not_overflow_on_extreme_world() {
        let idx = PickIndex::build(&[], PickIndex::default_cell());
        // Extreme / non-finite cursor coords map to saturated grid cells; must not panic.
        let _ = idx.pick_topmost([f32::INFINITY, f32::NEG_INFINITY], 0.0);
        let _ = idx.pick_topmost([f32::MAX, f32::MIN], 4.0);
        let _ = idx.pick_topmost([f32::NAN, f32::NAN], 0.0);
    }
}

fn cell_of(p: [f32; 2], cell: f32) -> (i32, i32) {
    ((p[0] / cell).floor() as i32, (p[1] / cell).floor() as i32)
}

fn layer_ord(layer: PickLayer) -> u8 {
    match layer {
        PickLayer::Body => 0,
        PickLayer::Title => 1,
        PickLayer::Outline => 2,
        PickLayer::Field => 3,
        PickLayer::Pin => 4,
    }
}

fn better_hit(
    a: (u32, PickLayer, PickTarget),
    b: (u32, PickLayer, PickTarget),
) -> (u32, PickLayer, PickTarget) {
    let (ia, la, _) = a;
    let (ib, lb, _) = b;
    let oa = layer_ord(la);
    let ob = layer_ord(lb);
    if oa > ob || (oa == ob && ia > ib) {
        a
    } else {
        b
    }
}

/// Resolve hits: pin priority unless a later body/title occludes it.
fn resolve_pick(hits: Vec<(u32, PickLayer, PickTarget)>) -> Option<PickTarget> {
    if hits.is_empty() {
        return None;
    }

    let mut best_pin: Option<(u32, PickTarget)> = None;
    let mut best_opaque: Option<(u32, PickLayer, PickTarget)> = None;
    let mut best_any: Option<(u32, PickLayer, PickTarget)> = None;

    for (idx, layer, pick) in hits {
        best_any = Some(match best_any {
            None => (idx, layer, pick.clone()),
            Some(prev) => better_hit((idx, layer, pick.clone()), prev),
        });
        if layer == PickLayer::Pin {
            best_pin = Some(match best_pin {
                None => (idx, pick.clone()),
                Some((pi, _)) if idx > pi => (idx, pick.clone()),
                Some(prev) => prev,
            });
        } else if matches!(layer, PickLayer::Body | PickLayer::Title | PickLayer::Field) {
            best_opaque = Some(match best_opaque {
                None => (idx, layer, pick.clone()),
                Some(prev) => better_hit((idx, layer, pick.clone()), prev),
            });
        }
    }

    if let Some((pin_idx, pin_pick)) = best_pin {
        if let Some((_, _, occ_pick)) = best_opaque.filter(|(i, _, _)| *i > pin_idx) {
            return Some(occ_pick);
        }
        return Some(pin_pick);
    }

    best_any.map(|(_, _, p)| p)
}

fn insert_aabb(cells: &mut HashMap<(i32, i32), Vec<u32>>, idx: u32, aabb: [f32; 4], cell: f32) {
    let x0 = (aabb[0] / cell).floor() as i32;
    let x1 = (aabb[2] / cell).floor() as i32;
    let y0 = (aabb[1] / cell).floor() as i32;
    let y1 = (aabb[3] / cell).floor() as i32;
    for cx in x0..=x1 {
        for cy in y0..=y1 {
            cells.entry((cx, cy)).or_default().push(idx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::shape2d::{Shape2d, ShapeInstance, ShapeStyle};
    use nodes_core::{NodeId, PinSide};

    #[test]
    fn pin_beats_body_overlap() {
        let id = NodeId::new();
        let body = ShapeInstance {
            shape: Shape2d::RoundedRect {
                min: [0.0, 0.0],
                max: [100.0, 80.0],
                radius: 4.0,
            },
            style: ShapeStyle::default(),
            layer: PickLayer::Body,
            pick: PickTarget::NodeBody(id),
            hollow_border: None,
            pickable: true,
            pick_id: 0,
            depth_bias: 0.0,
            rotation: 0.0,
            pivot: [0.0, 0.0],
        };
        let pin = ShapeInstance {
            shape: Shape2d::Circle {
                center: [50.0, 40.0],
                radius: 8.0,
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
            pivot: [0.0, 0.0],
        };
        let index = PickIndex::build(&[body, pin], 32.0);
        let hit = index.pick_topmost([50.0, 40.0], 0.0).unwrap();
        assert!(matches!(hit, PickTarget::Pin { .. }));
    }

    #[test]
    fn later_body_wins_same_layer() {
        let a = NodeId::new();
        let b = NodeId::new();
        let first = ShapeInstance {
            shape: Shape2d::RoundedRect {
                min: [0.0, 0.0],
                max: [80.0, 60.0],
                radius: 2.0,
            },
            style: ShapeStyle::default(),
            layer: PickLayer::Body,
            pick: PickTarget::NodeBody(a),
            hollow_border: None,
            pickable: true,
            pick_id: 0,
            depth_bias: 0.0,
            rotation: 0.0,
            pivot: [0.0, 0.0],
        };
        let second = ShapeInstance {
            shape: Shape2d::RoundedRect {
                min: [20.0, 10.0],
                max: [100.0, 70.0],
                radius: 2.0,
            },
            style: ShapeStyle::default(),
            layer: PickLayer::Body,
            pick: PickTarget::NodeBody(b),
            hollow_border: None,
            pickable: true,
            pick_id: 0,
            depth_bias: 0.0,
            rotation: 0.0,
            pivot: [0.0, 0.0],
        };
        let index = PickIndex::build(&[first, second], 32.0);
        let hit = index.pick_topmost([50.0, 40.0], 0.0).unwrap();
        assert!(matches!(hit, PickTarget::NodeBody(id) if id == b));
    }

    #[test]
    fn pin_occluded_by_later_body() {
        let pin_node = NodeId::new();
        let cover_node = NodeId::new();
        let pin = ShapeInstance {
            shape: Shape2d::Circle {
                center: [50.0, 40.0],
                radius: 8.0,
            },
            style: ShapeStyle::default(),
            layer: PickLayer::Pin,
            pick: PickTarget::Pin {
                node: pin_node,
                side: PinSide::Output,
                index: 0,
            },
            hollow_border: None,
            pickable: true,
            pick_id: 0,
            depth_bias: 0.0,
            rotation: 0.0,
            pivot: [0.0, 0.0],
        };
        let cover = ShapeInstance {
            shape: Shape2d::RoundedRect {
                min: [10.0, 10.0],
                max: [90.0, 70.0],
                radius: 4.0,
            },
            style: ShapeStyle::default(),
            layer: PickLayer::Body,
            pick: PickTarget::NodeBody(cover_node),
            hollow_border: None,
            pickable: true,
            pick_id: 0,
            depth_bias: 0.0,
            rotation: 0.0,
            pivot: [0.0, 0.0],
        };
        let index = PickIndex::build(&[pin, cover], 32.0);
        let hit = index.pick_topmost([50.0, 40.0], 0.0).unwrap();
        assert!(matches!(hit, PickTarget::NodeBody(id) if id == cover_node));
    }

    #[test]
    fn stress_grid_many_instances() {
        let mut instances = Vec::new();
        for row in 0..20 {
            for col in 0..25 {
                let id = NodeId::new();
                let x = col as f32 * 120.0;
                let y = row as f32 * 90.0;
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
                    rotation: 0.0,
                    pivot: [0.0, 0.0],
                });
            }
        }
        let index = PickIndex::build(&instances, PickIndex::default_cell());
        assert!(index.pick_topmost([60.0, 45.0], 0.0).is_some());
        assert!(index.pick_topmost([5000.0, 5000.0], 0.0).is_none());
    }
}
