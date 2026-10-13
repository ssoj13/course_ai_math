use egui::{Area, Context, Id, Pos2, RawInput, Rect, Vec2, epaint::Shape};

#[test]
fn footer_stays_inside_pane_after_oversized_body() {
    for width in [80.0, 190.0, 420.0] {
        let ctx = Context::default();
        let pane = Rect::from_min_size(Pos2::ZERO, Vec2::new(width, 300.0));
        // Repeat to include egui's deferred sizing pass.
        for pass in 0..3 {
            ctx.begin_pass(RawInput {
                screen_rect: Some(pane),
                ..Default::default()
            });
            Area::new(Id::new("prefs_footer"))
                .fixed_pos(Pos2::ZERO)
                .show(&ctx, |ui| {
                    ui.set_max_size(pane.size());
                    ui.set_clip_rect(pane);
                    ui.allocate_exact_size(Vec2::new(2000.0, 20.0), egui::Sense::hover());
                    egui_prefs2::footer(ui, || panic!("no input should reset settings"));
                });
            let mut output = ctx.end_pass();
            output.textures_delta.clear();
            if pass == 0 {
                continue;
            }
            let mut found = false;
            for shape in output.shapes {
                if let Shape::Text(text) = shape.shape
                    && text.galley.job.text == "Reset to Defaults"
                {
                    found = true;
                    let bounds = text.galley.rect.translate(text.pos.to_vec2());
                    assert!(bounds.left() >= pane.left() - 0.1, "{width}: {bounds:?}");
                    assert!(bounds.right() <= pane.right() + 0.1, "{width}: {bounds:?}");
                }
            }
            assert!(found, "footer must still be rendered at width {width}");
        }
    }
}
