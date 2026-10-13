# egui-viewport-hud

Записная книжка для этого крейта. Дополняй при изменениях.

## Что это
Generic viewport **HUD** for egui 0.34 — semi-transparent corner stat panels
(usdview style: gold monospace on a translucent black plate). The host formats
its stats into `Vec<String>` lines, tags each block with a corner, and calls
`draw_hud`; the widget lays out + paints and returns the painted `HudExtents`
(so the host can keep a hover read-out clear of the HUD). Owns NO data.

Stateless painter (no `show -> Action`, no interaction) — like
`egui-viewport-overlays`, it is a drawing helper, not a model-panel.

## Откуда взято
Gathered from `vfx-rs/crates/view/vfx-view/src/panels/hud.rs` (references usdviewq
`stageView.py` HUD drawing).

## Decoupling decisions
- **Data stays in the app.** vfx-view's `HudState` traverses the USD stage for
  prim-type counts and caches FPS / GPU-timing / complexity / VRAM. None of that
  is generic — only the corner-anchored translucent-plate text layout is. So the
  widget takes pre-formatted `HudBlock { corner, lines }` and the host keeps
  `HudState` + the stage traversal.
- **Stacking rule preserved.** Top corners grow downward, bottom corners grow
  upward, blocks pile in order — matching the vfx-view layout (perf line lowest,
  GPU-timing / VBO blocks above). The per-line translucent-plate painter
  (`draw_hud_line`) is a verbatim port of `draw_hud_text`.
- **Style is a param.** `HudStyle::default()` reproduces the usdview gold / 160-α
  black / pad 6 / font 12; callers can theme it.

## Public API (src/egui_viewport_hud.rs)
- `enum HudCorner { TopLeft, TopRight, BottomLeft, BottomRight }`
- `struct HudStyle { text_color, bg_color, pad, font_size, line_gap }` (`Default`)
- `struct HudBlock { corner, lines: Vec<String> }` + `new`
- `struct HudExtents { top_left_bottom, top_right_bottom, bottom_left_top, bottom_right_top: f32 }`
- `fn draw_hud(ui, rect, blocks: &[HudBlock], style: &HudStyle) -> HudExtents`

## Gotchas
- Right-corner lines are right-aligned per line (x = right − galley width − pad),
  so a longer line pushes left, matching usdview.
- `HudExtents.bottom_left_top` is what vfx-view used to position the pixel-probe
  hover label above the bottom-left perf block.

## TODO
- [ ] Demo tab in `egui-widgets-demo` (four corners of sample stats).
- [ ] Consume from vfx-view (`HudState::draw` builds `HudBlock`s → `draw_hud`).
