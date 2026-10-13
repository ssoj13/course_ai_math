use super::*;

fn items() -> Vec<MenuItem<&'static str>> {
    vec![
        MenuItem::new("edit_poly", "Edit Poly").group("Geometry"),
        MenuItem::new("deep_merge", "DeepMerge").group("Deep"),
        MenuItem::new("poly_extrude", "Poly Extrude").group("Geometry"),
        MenuItem::new("render", "Render").group("Settings/Output"),
        MenuItem::new("quality", "Quality").group("Settings/Output"),
        MenuItem::new("viewport", "Viewport").group("Settings"),
        MenuItem::new("fractal", "Fractal").keywords("mandelbulb"),
    ]
}

fn key(key: Key) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    }
}

/// One frame of the menu with these events; what it took.
fn frame(
    ctx: &Context,
    menu: &mut TabMenu<&'static str>,
    items: &[MenuItem<&'static str>],
    events: Vec<egui::Event>,
) -> Option<&'static str> {
    let mut taken = None;
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 800.0))),
            events,
            ..Default::default()
        },
        |ui| {
            if let Some(k) = menu.show(ui.ctx(), items) {
                taken = Some(k);
            }
        },
    );
    // No painter here: the frame's texture uploads are dropped on purpose.
    output.textures_delta.clear();
    taken
}

#[test]
fn words_split_at_separators_and_camel_case() {
    assert_eq!(words("DeepMerge"), ["deep", "merge"]);
    assert_eq!(words("Edit Poly"), ["edit", "poly"]);
    assert_eq!(words("rgb_to_hsv2"), ["rgb", "to", "hsv", "2"]);
    assert_eq!(words("HDRView"), ["hdrview"]);
}

#[test]
fn the_search_matches_word_starts_initials_substrings_and_paths() {
    let items = items();
    let find = |q: &str| {
        let menu = TabMenu::new("t");
        menu.search(&items, q)
            .into_iter()
            .map(|i| items[i].key)
            .collect::<Vec<_>>()
    };
    // Houdini: word starts, in order first.
    assert_eq!(find("e p"), ["edit_poly", "poly_extrude"]);
    assert_eq!(find("p e")[0], "poly_extrude", "word order counts");
    // Nuke: initials, CamelCase included.
    assert_eq!(find("dm"), ["deep_merge"]);
    // The group path and keywords match too, after label matches.
    assert_eq!(find("mandel"), ["fractal"]);
    assert_eq!(find("output"), ["quality", "render"]);
    // The whole label first.
    assert_eq!(find("render")[0], "render");
    assert!(find("zzz").is_empty());
}

#[test]
fn ties_go_to_favorites_then_usage_then_name() {
    let items = items();
    let mut menu = TabMenu::new("t");
    let keys = |menu: &TabMenu<&'static str>| {
        menu.search(&items, "output")
            .into_iter()
            .map(|i| items[i].key)
            .collect::<Vec<_>>()
    };
    assert_eq!(keys(&menu), ["quality", "render"]);
    menu.set_usage([("render", 3)]);
    assert_eq!(keys(&menu), ["render", "quality"]);
    menu.set_favorites(["quality"]);
    assert_eq!(keys(&menu), ["quality", "render"]);
}

#[test]
fn the_tree_nests_groups_sorted_by_name() {
    let items = items();
    let tree = MenuTree::build(&items, true);
    let names: Vec<_> = tree.groups.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["Deep", "Geometry", "Settings"]);
    let settings = &tree.groups[2].1;
    assert_eq!(settings.groups[0].0, "Output");
    let output: Vec<_> = settings.groups[0]
        .1
        .leaves
        .iter()
        .map(|&i| items[i].key)
        .collect();
    assert_eq!(output, ["quality", "render"]);
    assert_eq!(
        tree.leaves
            .iter()
            .map(|&i| items[i].key)
            .collect::<Vec<_>>(),
        ["fractal"]
    );
    // Unsorted: the items' own order.
    let tree = MenuTree::build(&items, false);
    assert_eq!(tree.groups[0].0, "Geometry");
}

#[test]
fn typing_searches_and_enter_takes_the_best_match() {
    let ctx = Context::default();
    let items = items();
    let mut menu = TabMenu::new("t");
    menu.open(Pos2::new(100.0, 100.0));
    // The first frame focuses the search field.
    assert_eq!(frame(&ctx, &mut menu, &items, vec![]), None);
    frame(
        &ctx,
        &mut menu,
        &items,
        vec![egui::Event::Text("e p".into())],
    );
    assert_eq!(
        frame(&ctx, &mut menu, &items, vec![key(Key::Enter)]),
        Some("edit_poly")
    );
    assert!(!menu.is_open());
    assert_eq!(menu.history().copied().collect::<Vec<_>>(), ["edit_poly"]);
    assert_eq!(menu.usage(), [("edit_poly", 1)]);
}

#[test]
fn arrows_walk_the_nested_groups_and_enter_takes_an_entry() {
    let ctx = Context::default();
    let items = items();
    let mut menu = TabMenu::new("t");
    menu.open(Pos2::new(100.0, 100.0));
    frame(&ctx, &mut menu, &items, vec![]);
    // Down: Deep (opens), Down: Geometry, Down: Settings; Right: Settings' first row, the
    // Output group; Right: its first entry, Quality; Down: Render; Enter takes it.
    let walk = [
        Key::ArrowDown,
        Key::ArrowDown,
        Key::ArrowDown,
        Key::ArrowRight,
        Key::ArrowRight,
        Key::ArrowDown,
    ];
    for k in walk {
        assert_eq!(frame(&ctx, &mut menu, &items, vec![key(k)]), None);
    }
    assert_eq!(
        frame(&ctx, &mut menu, &items, vec![key(Key::Enter)]),
        Some("render")
    );
}

#[test]
fn escape_closes_without_taking() {
    let ctx = Context::default();
    let items = items();
    let mut menu = TabMenu::new("t");
    menu.open(Pos2::new(100.0, 100.0));
    frame(&ctx, &mut menu, &items, vec![]);
    assert_eq!(frame(&ctx, &mut menu, &items, vec![key(Key::Escape)]), None);
    assert!(!menu.is_open());
    assert_eq!(menu.history().count(), 0);
}
