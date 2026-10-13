# egui-tab-menu

A **Tab menu** for egui, the "press Tab, type, create" menu of node-based DCCs: Houdini's layout
with Nuke's search ranking, nested submenus of any depth. The same item list also renders as
plain nested egui menus (an "Add" button, a right-click menu), so every place an application
creates things shows one list.

```rust
use egui_tab_menu::{MenuItem, TabMenu};

// Once, kept across frames.
let mut menu = TabMenu::new("add_menu");

// Every frame: the host's flat list; `group` is a "/"-separated path of submenus.
let items = vec![
    MenuItem::new(Kind::Box, "Box").group("Geometry/Primitives").icon(ph::CUBE),
    MenuItem::new(Kind::Sun, "Sun").group("Lighting").keywords("directional light"),
];
if tab_pressed {
    menu.open(pointer_pos);
}
if let Some(kind) = menu.show(ctx, &items) {
    create(kind);
}
// The same tree inside any open egui menu:
ui.menu_button("Add", |ui| {
    if let Some(kind) = menu.menu_ui(ui, &items) {
        create(kind);
    }
});
```

`MenuTree::build` / `TabMenu::tree` expose the tree for hosts with their own menu type (an
outliner's context items). History, favorites and usage counts belong to the host to save:
`history` / `set_history`, `favorites` / `set_favorites`, `usage` / `set_usage`.

## Requirements (gathered from Houdini and Nuke)

The widget implements these; a change must keep them.

### Opening and layout (Houdini)

1. Tab opens the menu **at the cursor**: the search field sits under the pointer.
2. A title ("TAB Menu") over a **search field with a magnifier**.
3. The **search field always has the keyboard**: typing goes straight into it, no click first.
4. The first column lists the **categories** (groups, with a right caret) and top-level entries,
   then a **separator**, then **`All ▸`** (every entry, flat, sorted) and the **History** section
   listed in place under its own header (`(no entries)` when empty). **Favorites** get a section
   the same way when there are any.
5. **Hovering a group opens its submenu** in a column to the right, **to any depth**. Moving to a
   sibling group replaces the deeper columns.
6. A side column stays **unlit until the cursor or the keyboard enters it**; the open group in a
   column the cursor left shows a weaker highlight.
7. The menu is **anchored once** (moved inside the screen on its first frame) and **never moves
   while open**: rows must not reflow under the cursor.
8. Columns nearly touch (Houdini's cascade); long columns scroll, keeping the lit row in view.
9. Categories and entries are **sorted by name** by default (Houdini); a host may keep its own
   order (`sorted(false)`).

### Searching

10. Typing **keeps the categories column** and shows the matches as a **column to its right**;
    the **best match is lit** and **Enter takes it** at once.
11. **Multi-word, order-free AND** (Houdini): every word of the query must match the entry.
12. A word matches as, best first:
    - the **start of a word** of the name, in the name's word order ranking higher:
      `e p` finds *Edit Poly*, `p e` prefers *Poly Extrude* (Houdini);
    - the **initials** of consecutive words, **CamelCase starting a word**: `dm` finds
      *DeepMerge* (Nuke);
    - a **substring** of the name (Nuke 11.2+: non-consecutive letters across words);
    - a word of the **menu path** or the entry's **keywords** (Nuke's `[Color]` search).
13. A **single letter** only ever matches a word start or initial, never a substring: `e p`
    must not list every name with an "e" and a "p".
14. The **whole name** ranks first.
15. Ties go to **favorites** (Nuke's star, pinned first), then to the entries **taken most
    often** (Nuke's weighting), then **by name**.

### Keyboard and mouse

16. **Up / Down** move within a column, skipping headers and separators; a group opens as it is
    reached. In a search they move through the results.
17. **Right** (or **Enter**) on a group enters its column; **Left** goes back.
18. **Enter** on an entry takes it; **Escape** closes; a **click outside** closes.
19. A **click** takes an entry; a moving cursor lights the row under it, a still cursor leaves the
    keyboard's choice alone.
20. A **star** on an entry (shown on hover, lit when set) toggles it as a **favorite** (Nuke).

### Memory

21. Taking an entry puts it at the front of the **History** (deduplicated, 10 by default) and
    counts one **use** for the ranking; the nested egui menus count the same way.
22. History, favorites and usage are **plain data the host saves and restores**; the widget keeps
    no files.

### One model

23. The host passes **one flat list** (key, label, icon, `/`-separated group path, hint,
    keywords); the Tab menu, the nested egui menus and any host menu built from `MenuTree` all
    show it, so no two menus of an application can drift apart.
24. No application types: the key is any `Clone + PartialEq` value.

## References

- Houdini: [The Tab menu](https://www.sidefx.com/docs/houdini/basics/tabmenu.html).
- Nuke: [Using the Tab Menu](https://learn.foundry.com/nuke/content/getting_started/using_interface/tab_menu.html)
  (weighting, favorites, substring matching since 11.2).
- nodes-rs `nodes-chrome/src/tab_menu_wgpu.rs`: the wgpu node editor's Houdini Tab menu this
  widget's behaviour follows (cascade, History / All, anchoring, unlit side columns).
