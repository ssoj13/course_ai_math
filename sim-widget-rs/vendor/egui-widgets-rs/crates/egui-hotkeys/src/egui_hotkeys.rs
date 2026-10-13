//! Panel-scoped keyboard routing for egui apps.
//!
//! The model is the one of Blender, Houdini, Nuke and Qt: a key is looked up in a CONTEXT
//! CHAIN, most specific first. The chain starts at the active panel (the visible panel under the
//! pointer; keyboard focus survives the pointer leaving, so the last clicked panel keeps the keys
//! until a click lands outside every panel), continues through its [`Keymap::parent`]s and ends
//! at the global table. The FIRST table that binds the chord wins, so a panel may override a
//! global key. Text editors keep their keys: while the host is `typing` only Ctrl/Cmd chords are
//! looked up at all. Overlapping chords inside one table are a bug, found by [`conflicts`].
//!
//! The app describes itself with a [`Keymap`]: its scope type (one value per panel), its
//! command type and the binding tables. Every panel calls [`register`] each frame, even a
//! panel without bindings, so it blocks the previous owner; an overlay drawn on a layer of its
//! own (a floating strip) that belongs to a panel calls [`register_at`] with that layer. The
//! retained owner counts only while its panel keeps registering. Keys are read through
//! [`pressed`] (a whole frame) or [`consume`] (one command); both share [`resolve`], so there
//! is exactly one matching codepath.
//!
//! State lives in egui temp data under ids namespaced by the keymap TYPE, so two keymaps in
//! one app (or one app and an embedded library) never see each other's panels or owner.

use std::any::TypeId;

use egui::{Context, Id, Key, Modifiers, Rect, Ui};

/// One key binding of command `C`.
#[derive(Clone, Copy, Debug)]
pub struct Binding<C> {
    /// What the chord runs.
    pub command: C,
    /// The key, as the layout reports it (or the position, for a [`Binding::physical`]).
    pub key: Key,
    /// The exact modifier set: an extra Shift or Alt does NOT match.
    pub modifiers: Modifiers,
    additive: bool,
    physical: bool,
    repeat: bool,
}

impl<C> Binding<C> {
    /// An exact chord: `key` with exactly `modifiers` held.
    pub const fn new(command: C, key: Key, modifiers: Modifiers) -> Self {
        Self { command, key, modifiers, additive: false, physical: false, repeat: false }
    }

    /// Matches the key POSITION, not the character: Shift+1 types "!" on a US layout and another
    /// character (sometimes one egui has no [`Key`] for) elsewhere, but is always the physical
    /// 1 key. For digits.
    pub const fn physical(command: C, key: Key, modifiers: Modifiers) -> Self {
        Self { command, key, modifiers, additive: false, physical: true, repeat: false }
    }

    /// An exact chord that ignores Shift: a held Shift neither blocks nor is required. For
    /// property filters (U) and keys whose layout produces them with Shift ("+" is Shift+=).
    /// `modifiers` must not contain Shift.
    pub const fn additive(command: C, key: Key, modifiers: Modifiers) -> Self {
        Self { command, key, modifiers, additive: true, physical: false, repeat: false }
    }

    /// Also fire on the OS auto-repeat events of a held key. The default is one press, one fire
    /// (a held toggle must not flicker); stepping and zooming want the repeats.
    pub const fn repeats(mut self) -> Self {
        self.repeat = true;
        self
    }

    /// Whether this binding's CHORD is the pressed `key` (`physical_key` is the key position the
    /// event reports) with `modifiers` held, whatever the repeat flag.
    ///
    /// Exact chords (Ctrl / Cmd equivalent): the pressed modifiers are `self`, the binding the
    /// pattern. egui's `matches_logically` lets a Shift binding fire without Shift, so plain 3
    /// would hit "set mark 3", and table order alone would keep Insert apart from Shift+Insert.
    pub fn is_chord(&self, key: Key, physical_key: Option<Key>, mut modifiers: Modifiers) -> bool {
        if self.additive {
            modifiers.shift = false;
        }
        let key = if self.physical { physical_key.unwrap_or(key) } else { key };
        self.key == key && modifiers.matches_exact(self.modifiers)
    }

    /// Whether this binding fires for a pressed `key` with `modifiers` held; `repeat` marks an
    /// auto-repeat event, which only a [`Binding::repeats`] binding accepts.
    pub fn matches(&self, key: Key, physical_key: Option<Key>, modifiers: Modifiers, repeat: bool) -> bool {
        (!repeat || self.repeat) && self.is_chord(key, physical_key, modifiers)
    }

    /// Whether one key event can match both bindings: the same key and a modifier set both
    /// accept (an additive binding also accepts its set plus Shift). The position-or-character
    /// choice of [`Binding::physical`] is ignored on purpose: two bindings of one key in one
    /// table are suspect on any layout.
    pub fn overlaps<D>(&self, other: &Binding<D>) -> bool {
        fn accepted<C>(b: &Binding<C>) -> [Option<Modifiers>; 2] {
            [Some(b.modifiers), b.additive.then(|| b.modifiers.plus(Modifiers::SHIFT))]
        }
        self.key == other.key && accepted(self).into_iter().flatten().any(|m| accepted(other).contains(&Some(m)))
    }
}

/// The app's binding tables. Implement it on a marker type; the type is also the namespace of
/// the egui temp data this crate keeps for the app.
pub trait Keymap: 'static {
    /// One value per panel (a dock tab, the timeline, ...).
    type Scope: Copy + Eq + Send + Sync + 'static;
    /// What a binding runs; small and copyable (an id, an enum).
    type Cmd: Copy + Eq;

    /// Bindings that fire in every scope, the end of every context chain: a panel table that
    /// binds the same chord overrides them.
    fn global(&self) -> &[Binding<Self::Cmd>];

    /// Bindings of one panel; empty for a panel without keys (it still blocks the owner).
    fn bindings(&self, scope: Self::Scope) -> &[Binding<Self::Cmd>];

    /// The scope searched after `scope` and before the global table. `None` (the default): the
    /// global table follows directly, which is all a flat set of panels needs. A cycle is a host
    /// bug (the lookup stops after [`MAX_DEPTH`] scopes).
    fn parent(&self, _scope: Self::Scope) -> Option<Self::Scope> {
        None
    }

    /// The scope that owns the keys when no panel is under the pointer and none was clicked
    /// (at startup, and after a click outside every panel). `None`: those keys go nowhere.
    fn fallback(&self) -> Option<Self::Scope> {
        None
    }
}

/// Which table a resolved chord came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route<S> {
    /// [`Keymap::global`]: fires in any scope.
    Global,
    /// The bindings of this panel.
    Scope(S),
}

/// The longest chain of [`Keymap::parent`]s followed.
pub const MAX_DEPTH: usize = 8;

/// The context chain of `scope`: the scope, its parents, then the global table.
fn chain<K: Keymap>(km: &K, scope: Option<K::Scope>) -> impl Iterator<Item = Route<K::Scope>> + '_ {
    let mut next = scope;
    let mut depth = 0;
    std::iter::from_fn(move || {
        let current = next?;
        depth += 1;
        next = km.parent(current).filter(|_| depth < MAX_DEPTH);
        Some(Route::Scope(current))
    })
    .chain(std::iter::once(Route::Global))
}

/// Context-chain lookup, shared by every consumer and independent of panel draw order: the first
/// table of the chain (`scope`, its parents, global) with a binding for the chord decides.
/// `scope` is the active panel (see [`active`]); `physical_key` is the key position the event
/// reports (see [`Binding::physical`]); `repeat`: the event is an auto-repeat (see
/// [`Binding::repeats`]). A binding without `repeats()` in a nearer table shadows a farther
/// repeating binding of the same chord: the repeat resolves to nothing (and stays for the
/// widgets) instead of leaking to the parent.
pub fn resolve<K: Keymap>(
    km: &K,
    scope: Option<K::Scope>,
    key: Key,
    physical_key: Option<Key>,
    modifiers: Modifiers,
    repeat: bool,
) -> Option<(Route<K::Scope>, K::Cmd)> {
    let (route, b) = chain(km, scope).find_map(|route| {
        let table = match route {
            Route::Global => km.global(),
            Route::Scope(s) => km.bindings(s),
        };
        table.iter().find(|b| b.is_chord(key, physical_key, modifiers)).map(|b| (route, b))
    })?;
    (!repeat || b.repeat).then_some((route, b.command))
}

/// Two bindings of one table that a single key press can both match: the first listed would
/// silently win and the second never fire.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Conflict<S, C> {
    /// The table that holds both.
    pub route: Route<S>,
    /// The earlier binding's command (the one that wins).
    pub first: C,
    /// The later binding's command (unreachable by that chord).
    pub second: C,
}

/// Every overlap of chords INSIDE one table: the global table and each of `scopes` (all the
/// host's scopes). For a host test: `assert!(conflicts(&km, &Scope::ALL).is_empty())`. A chord
/// shared between a scope and its ancestors is NOT a conflict: that is an override.
pub fn conflicts<K: Keymap>(km: &K, scopes: &[K::Scope]) -> Vec<Conflict<K::Scope, K::Cmd>> {
    let tables = std::iter::once((Route::Global, km.global())).chain(scopes.iter().map(|&s| (Route::Scope(s), km.bindings(s))));
    let mut out = Vec::new();
    for (route, table) in tables {
        for (i, a) in table.iter().enumerate() {
            let clashes = table.iter().skip(i + 1).filter(|b| a.overlaps(b));
            out.extend(clashes.map(|b| Conflict { route, first: a.command, second: b.command }));
        }
    }
    out
}

/// One registered panel, as of the frame it was drawn.
#[derive(Clone)]
struct PanelRegion<S> {
    id: Id,
    scope: S,
    rect: Rect,
    layer: egui::LayerId,
    viewport: egui::ViewportId,
    frame: u64,
}

/// A temp-data id private to keymap `K`.
fn id_of<K: Keymap>(tag: &'static str) -> Id {
    Id::new((tag, TypeId::of::<K>()))
}

/// The panel that owns the keys this frame: the one under the pointer, else the one last
/// clicked, else [`Keymap::fallback`].
///
/// Resolved from PREVIOUS-frame geometry, so it is available before any panel draws (keys are
/// read at the top of the frame, and panel draw order cannot change who gets them). The hover
/// test is egui's layer hit test, so a window or popup over a panel hides it. A pointer over a
/// panel's rectangle that the layer test rejects (an unregistered overlay on top) owns nothing:
/// the keys go nowhere rather than to the panel underneath. The retained owner counts only while
/// its panel registered last frame (a closed tab or hidden panels give the keys back to the
/// fallback instead of leaving them dead until the next click).
pub fn active<K: Keymap>(km: &K, ctx: &Context) -> Option<K::Scope> {
    let regions = ctx.data(|d| d.get_temp::<Vec<PanelRegion<K::Scope>>>(id_of::<K>("regions"))).unwrap_or_default();
    let frame = ctx.cumulative_frame_nr();
    let viewport = ctx.viewport_id();
    let live = |r: &&PanelRegion<K::Scope>| r.viewport == viewport && r.frame.saturating_add(1) >= frame;
    let hovered = regions.iter().rev().filter(live).find(|r| ctx.rect_contains_pointer(r.layer, r.rect));
    hovered.map(|r| r.scope).or_else(|| {
        let obscured = ctx.pointer_hover_pos().is_some_and(|pointer| {
            regions.iter().filter(live).any(|r| {
                let rect = ctx.layer_transform_to_global(r.layer).map_or(r.rect, |t| t * r.rect);
                rect.contains(pointer)
            })
        });
        // A press outside every panel drops the retained owner at once.
        if obscured || ctx.input(|i| i.pointer.any_pressed()) {
            return None;
        }
        match ctx.data(|d| d.get_temp::<K::Scope>(id_of::<K>("owner"))) {
            Some(owner) if regions.iter().filter(live).any(|r| r.scope == owner) => Some(owner),
            Some(_) => {
                // Its panel is gone: forget it, so it does not come back on its own when the
                // panel is reopened.
                ctx.data_mut(|d| d.remove::<K::Scope>(id_of::<K>("owner")));
                km.fallback()
            }
            None => km.fallback(),
        }
    })
}

/// Declare a panel for this frame; call it for EVERY panel, including panels without
/// bindings (they must block the previous owner). Clipping the rectangle to the Ui's clip rect
/// and the layer test of [`active`] keep an obscured or scrolled-away panel from taking the
/// keys. A press inside the rectangle makes the panel the retained owner.
pub fn register<K: Keymap>(km: &K, ui: &Ui, scope: K::Scope, rect: Rect) {
    put(km, ui.ctx(), ui.id(), ui.layer_id(), scope, rect.intersect(ui.clip_rect()));
}

/// [`register`] for an overlay that belongs to a panel but is drawn on a layer of its own (a
/// floating strip is an `egui::Area`). The pointer over such an overlay hits the overlay's
/// layer, not the panel's, so without this the panel would own nothing there. `rect` is the
/// overlay's already-clipped area; call it once per `(layer, scope)`.
pub fn register_at<K: Keymap>(km: &K, ctx: &Context, layer: egui::LayerId, scope: K::Scope, rect: Rect) {
    put(km, ctx, layer.id, layer, scope, rect);
}

/// The one implementation behind [`register`] and [`register_at`]: store the region for the next
/// frame's [`active`], and let a press inside it (or outside, for the owner) move the owner.
fn put<K: Keymap>(km: &K, ctx: &Context, id: Id, layer: egui::LayerId, scope: K::Scope, rect: Rect) {
    let frame = ctx.cumulative_frame_nr();
    let viewport = ctx.viewport_id();
    let region = PanelRegion { id, scope, rect, layer, viewport, frame };
    ctx.data_mut(|d| {
        let regions = d.get_temp_mut_or_default::<Vec<PanelRegion<K::Scope>>>(id_of::<K>("regions"));
        regions.retain(|r| r.viewport != viewport || r.frame.saturating_add(1) >= frame);
        match regions.iter_mut().find(|r| r.id == region.id && r.scope == scope && r.viewport == viewport) {
            Some(previous) => *previous = region,
            None => regions.push(region),
        }
    });
    let inside = ctx.rect_contains_pointer(layer, rect);
    let pressed = ctx.input(|i| i.pointer.any_pressed());
    let owner = ctx.data(|d| d.get_temp::<K::Scope>(id_of::<K>("owner")));
    if inside && pressed {
        ctx.data_mut(|d| d.insert_temp(id_of::<K>("owner"), scope));
    } else if pressed && !inside && owner == Some(scope) && active(km, ctx).is_none() {
        ctx.data_mut(|d| d.remove::<K::Scope>(id_of::<K>("owner")));
    }
}

/// While a text editor (or a modal) has the keyboard only chords with Ctrl/Cmd may fire:
/// typing never produces them, while bare letters, Space and arrows are text.
fn may_fire(typing: bool, m: Modifiers) -> bool {
    !typing || m.command || m.ctrl
}

/// The one key-event pass behind [`pressed`] and [`consume`]: every key press (an auto-repeat
/// only for a [`Binding::repeats`] binding) that resolves (in the active scope) to a command
/// `pick` accepts is REMOVED from the frame's events and returned; with `once`, only the first.
/// A press `pick` refuses stays for the widgets.
fn take<K: Keymap>(
    km: &K,
    ctx: &Context,
    typing: bool,
    mut pick: impl FnMut(Route<K::Scope>, K::Cmd) -> bool,
    once: bool,
) -> Vec<K::Cmd> {
    let scope = active(km, ctx);
    ctx.input_mut(|input| {
        let mut out = Vec::new();
        let mut i = 0;
        while let Some(event) = input.events.get(i) {
            // Remove the exact event: a physical binding's logical key may be any character.
            let hit = match event {
                egui::Event::Key { key, physical_key, pressed: true, repeat, modifiers } if may_fire(typing, *modifiers) => {
                    resolve(km, scope, *key, *physical_key, *modifiers, *repeat).filter(|&(route, cmd)| pick(route, cmd))
                }
                _ => None,
            };
            let Some((_, cmd)) = hit else {
                i += 1;
                continue;
            };
            input.events.remove(i);
            out.push(cmd);
            if once {
                break;
            }
        }
        out
    })
}

/// Consume exactly one press (or auto-repeat, for a repeating binding) of `command`'s chord,
/// only when `route` is the table it resolves from: a panel command in its owning scope, a
/// global one always. `typing`: see [`pressed`].
pub fn consume<K: Keymap>(km: &K, ctx: &Context, route: Route<K::Scope>, command: K::Cmd, typing: bool) -> bool {
    !take(km, ctx, typing, |r, c| r == route && c == command, true).is_empty()
}

/// Every command pressed this frame in the active scope (global ones always), in event order;
/// their events are consumed. A command `enabled` refuses is not consumed at all, so its key
/// still reaches the widgets.
///
/// `typing` (the host's "a text field or dialog has the keyboard", typically
/// `ctx.egui_wants_keyboard_input()`): while set, only chords with Ctrl/Cmd fire. warpbro's
/// original `consume` blocked everything while typing; Ctrl chords are let through here since
/// they are never text, which is what a viewer's Open / Save need.
pub fn pressed<K: Keymap>(km: &K, ctx: &Context, typing: bool, enabled: impl Fn(K::Cmd) -> bool) -> Vec<K::Cmd> {
    take(km, ctx, typing, |_, c| enabled(c), false)
}

/// The platform Copy request (Ctrl+C / Cmd+C, Ctrl+Insert). egui-winit turns it into
/// `Event::Copy`, not a key event. Consumed only when `wanted` and no text editor has focus,
/// so text fields and selectable labels keep their own copy.
pub fn take_copy(ctx: &Context, wanted: bool) -> bool {
    if !wanted || ctx.text_edit_focused() {
        return false;
    }
    ctx.input_mut(|input| {
        let before = input.events.len();
        input.events.retain(|event| !matches!(event, egui::Event::Copy));
        input.events.len() != before
    })
}

/// The platform Paste (`Event::Paste`, clipboard text) when `accept` claims it and no text
/// editor has focus; other text stays for the widgets.
pub fn take_paste(ctx: &Context, accept: impl Fn(&str) -> bool) -> Option<String> {
    if ctx.text_edit_focused() {
        return None;
    }
    ctx.input_mut(|input| {
        let index = input.events.iter().position(|event| matches!(event, egui::Event::Paste(text) if accept(text)))?;
        match input.events.remove(index) {
            egui::Event::Paste(text) => Some(text),
            _ => None,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Event, PointerButton, Pos2, RawInput, pos2, vec2};

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Sc {
        Viewport,
        Timeline,
        Export,
        Settings,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Cm {
        Undo,
        Redo,
        Fit,
        Preview,
        CachePreview,
        Keyed,
        Mark(u8),
        SetMark(u8),
        Shadowed,
        Override,
        Step,
        Jump,
    }
    use Cm::*;

    /// A plain (non-repeat) press: the common case of [`super::resolve`].
    fn resolve<K: Keymap>(km: &K, scope: Option<K::Scope>, key: Key, physical: Option<Key>, mods: Modifiers) -> Option<(Route<K::Scope>, K::Cmd)> {
        super::resolve(km, scope, key, physical, mods, false)
    }

    const GLOBAL: &[Binding<Cm>] = &[
        Binding::new(Undo, Key::Z, Modifiers::COMMAND),
        Binding::new(Redo, Key::Z, Modifiers::COMMAND.plus(Modifiers::SHIFT)),
        // Q is overridden by the viewport (see below).
        Binding::new(Shadowed, Key::Q, Modifiers::NONE),
    ];
    const VIEWPORT: &[Binding<Cm>] = &[
        Binding::new(Fit, Key::F, Modifiers::NONE),
        Binding::new(Fit, Key::G, Modifiers::NONE),
        // The same chord as the global Shadowed: the nearer table wins.
        Binding::new(Override, Key::Q, Modifiers::NONE),
    ];
    const TIMELINE: &[Binding<Cm>] = &[
        Binding::new(Fit, Key::F, Modifiers::NONE),
        Binding::additive(Keyed, Key::U, Modifiers::NONE),
        Binding::new(Step, Key::ArrowRight, Modifiers::NONE).repeats(),
        Binding::new(Preview, Key::Insert, Modifiers::NONE),
        Binding::new(CachePreview, Key::Insert, Modifiers::SHIFT),
        Binding::physical(SetMark(3), Key::Num3, Modifiers::SHIFT),
        Binding::physical(Mark(3), Key::Num3, Modifiers::NONE),
    ];

    /// No fallback owner.
    struct Km;
    impl Keymap for Km {
        type Scope = Sc;
        type Cmd = Cm;
        fn global(&self) -> &[Binding<Cm>] {
            GLOBAL
        }
        fn bindings(&self, scope: Sc) -> &[Binding<Cm>] {
            match scope {
                Sc::Viewport => VIEWPORT,
                Sc::Timeline => TIMELINE,
                Sc::Export | Sc::Settings => &[],
            }
        }
    }

    /// The same tables and types as [`Km`], with a fallback owner.
    struct KmFb;
    impl Keymap for KmFb {
        type Scope = Sc;
        type Cmd = Cm;
        fn global(&self) -> &[Binding<Cm>] {
            GLOBAL
        }
        fn bindings(&self, scope: Sc) -> &[Binding<Cm>] {
            Km.bindings(scope)
        }
        fn fallback(&self) -> Option<Sc> {
            Some(Sc::Viewport)
        }
    }

    const VP: Rect = Rect { min: pos2(20.0, 20.0), max: pos2(300.0, 200.0) };
    const TL: Rect = Rect { min: pos2(20.0, 220.0), max: pos2(600.0, 400.0) };
    const OUTSIDE: Pos2 = pos2(700.0, 500.0);

    /// One egui frame with `events`; `f` runs inside a CentralPanel (the layer the regions use).
    fn frame(ctx: &Context, events: Vec<Event>, f: impl FnMut(&mut Ui)) {
        frame_in(ctx, egui::ViewportId::ROOT, events, f);
    }

    /// [`frame`] in the viewport `id` (egui keeps a frame counter per viewport).
    fn frame_in(ctx: &Context, id: egui::ViewportId, events: Vec<Event>, mut f: impl FnMut(&mut Ui)) {
        let input = RawInput {
            viewport_id: id,
            viewports: std::iter::once((id, egui::ViewportInfo::default())).collect(),
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))),
            events,
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |root| {
            egui::CentralPanel::default().show(root, |ui| f(ui));
        });
        // No renderer here: the font atlas upload egui asks for is dropped on purpose.
        output.textures_delta.clear();
    }

    fn press(key: Key, physical_key: Option<Key>, modifiers: Modifiers) -> Event {
        Event::Key { key, physical_key, pressed: true, repeat: false, modifiers }
    }

    fn release(key: Key) -> Event {
        Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE }
    }

    fn button(pos: Pos2, pressed: bool) -> Event {
        Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }
    }

    /// Register both panels of the layout on every frame, like an app does.
    fn both(ui: &Ui) {
        register(&Km, ui, Sc::Viewport, VP);
        register(&Km, ui, Sc::Timeline, TL);
    }

    /// A context in which the pointer rests over `at` and `scope`'s panels have been drawn once.
    fn hovering(at: Pos2) -> Context {
        let ctx = Context::default();
        frame(&ctx, vec![Event::PointerMoved(at)], |ui| both(ui));
        ctx
    }

    #[test]
    fn pointer_routes_commands_before_the_hovered_panel_draws() {
        for order in [[Sc::Viewport, Sc::Timeline], [Sc::Timeline, Sc::Viewport]] {
            let ctx = Context::default();
            let draw = |pointer, key: bool, expected: Sc| {
                let events = vec![Event::PointerMoved(pointer), if key { press(Key::F, None, Modifiers::NONE) } else { release(Key::F) }];
                frame(&ctx, events, |ui| {
                    for scope in order {
                        register(&Km, ui, scope, if scope == Sc::Viewport { VP } else { TL });
                        assert_eq!(consume(&Km, ui.ctx(), Route::Scope(scope), Fit, false), key && scope == expected);
                    }
                });
            };
            draw(VP.center(), false, Sc::Viewport);
            // Move and press in one frame, without clicking to switch ownership.
            draw(TL.center(), true, Sc::Timeline);
            draw(TL.center(), false, Sc::Timeline);
            draw(VP.center(), true, Sc::Viewport);
        }
    }

    #[test]
    fn overlays_and_panels_without_bindings_block_the_previous_owner() {
        for registered in [false, true] {
            let ctx = Context::default();
            let draw = |events: Vec<Event>, covered: bool| {
                let mut fired = false;
                frame(&ctx, events, |ui| {
                    register(&Km, ui, Sc::Viewport, ui.max_rect());
                    fired = consume(&Km, ui.ctx(), Route::Scope(Sc::Viewport), Fit, false);
                    if covered {
                        egui::Area::new(Id::new("hotkey-overlay")).order(egui::Order::Foreground).fixed_pos(pos2(40.0, 40.0)).show(ui.ctx(), |ui| {
                            let (rect, _) = ui.allocate_exact_size(vec2(200.0, 200.0), egui::Sense::hover());
                            if registered {
                                register(&Km, ui, Sc::Settings, rect);
                            }
                        });
                    }
                });
                fired
            };
            let point = pos2(80.0, 80.0);
            draw(vec![Event::PointerMoved(point)], false);
            draw(vec![button(point, true)], false);
            draw(vec![button(point, false)], true);
            draw(vec![], true);
            assert!(!draw(vec![press(Key::F, None, Modifiers::NONE)], true));
            assert_eq!(active(&Km, &ctx), if registered { Some(Sc::Settings) } else { None });
        }
    }

    #[test]
    fn one_press_routes_only_to_the_active_panel_and_is_consumed_once() {
        let ctx = Context::default();
        const EXPORT: Rect = Rect { min: pos2(620.0, 20.0), max: pos2(780.0, 200.0) };
        let all = |ui: &Ui| {
            both(ui);
            register(&Km, ui, Sc::Export, EXPORT);
        };
        // An owner counts only while its panel registers: draw all three once first.
        frame(&ctx, vec![], |ui| all(ui));
        let draw = |owner: Sc, expected: Sc| {
            let mut count = 0;
            frame(&ctx, vec![press(Key::F, None, Modifiers::NONE)], |ui| {
                all(ui);
                ui.ctx().data_mut(|d| d.insert_temp(id_of::<Km>("owner"), owner));
                for scope in [Sc::Viewport, Sc::Timeline, Sc::Export] {
                    let fired = consume(&Km, ui.ctx(), Route::Scope(scope), Fit, false);
                    assert_eq!(fired, scope == expected && expected != Sc::Export);
                    count += usize::from(fired);
                    assert!(!consume(&Km, ui.ctx(), Route::Scope(scope), Fit, false));
                }
            });
            assert_eq!(count, usize::from(expected != Sc::Export));
        };
        draw(Sc::Timeline, Sc::Timeline);
        frame(&ctx, vec![release(Key::F)], |ui| all(ui));
        draw(Sc::Viewport, Sc::Viewport);
        frame(&ctx, vec![release(Key::F)], |ui| all(ui));
        draw(Sc::Export, Sc::Export);
    }

    #[test]
    fn chords_match_exactly_and_digits_by_key_position() {
        let t = Some(Sc::Timeline);
        let r = |key, physical, mods| resolve(&Km, t, key, physical, mods);
        let tl = |c| Some((Route::Scope(Sc::Timeline), c));
        // A Shift binding never fires without Shift, a plain one never with it.
        assert_eq!(r(Key::Insert, None, Modifiers::NONE), tl(Preview));
        assert_eq!(r(Key::Insert, None, Modifiers::SHIFT), tl(CachePreview));
        assert_eq!(r(Key::Insert, None, Modifiers::ALT), None);
        assert_eq!(r(Key::Num3, Some(Key::Num3), Modifiers::NONE), tl(Mark(3)));
        // Shift + 3 types "!" (US) or a character with no egui key, but is the physical 3.
        assert_eq!(r(Key::Exclamationmark, Some(Key::Num3), Modifiers::SHIFT), tl(SetMark(3)));
        // Without a physical key the logical one stands in, which is not Num3 here.
        assert_eq!(r(Key::Exclamationmark, None, Modifiers::SHIFT), None);
        // Property filters keep their additive Shift.
        assert_eq!(r(Key::U, None, Modifiers::SHIFT), tl(Keyed));
        assert_eq!(r(Key::U, None, Modifiers::NONE), tl(Keyed));
        assert_eq!(r(Key::U, None, Modifiers::CTRL), None);
    }

    #[test]
    fn the_nearest_table_of_the_chain_decides() {
        let scope = |s, c| Some((Route::Scope(s), c));
        let global = |c| Some((Route::Global, c));
        // Global chords answer in a scope that does not bind them.
        assert_eq!(resolve(&Km, Some(Sc::Timeline), Key::Z, None, Modifiers::COMMAND), global(Undo));
        assert_eq!(resolve(&Km, Some(Sc::Export), Key::Z, None, Modifiers::COMMAND.plus(Modifiers::SHIFT)), global(Redo));
        assert_eq!(resolve(&Km, Some(Sc::Timeline), Key::Q, None, Modifiers::NONE), global(Shadowed));
        // The viewport binds Q itself: its table is nearer, so it overrides the global one.
        assert_eq!(resolve(&Km, Some(Sc::Viewport), Key::Q, None, Modifiers::NONE), scope(Sc::Viewport, Override));
        assert_eq!(resolve(&Km, Some(Sc::Timeline), Key::F, None, Modifiers::NONE), scope(Sc::Timeline, Fit));
        assert_eq!(resolve(&Km, Some(Sc::Viewport), Key::F, None, Modifiers::NONE), scope(Sc::Viewport, Fit));
        assert_eq!(resolve(&Km, Some(Sc::Export), Key::F, None, Modifiers::NONE), None);
        assert_eq!(resolve(&Km, Some(Sc::Timeline), Key::F, None, Modifiers::COMMAND), None);
        // No active scope: only the global table answers.
        assert_eq!(resolve(&Km, None, Key::F, None, Modifiers::NONE), None);
        assert_eq!(resolve(&Km, None, Key::Z, None, Modifiers::COMMAND), global(Undo));
        assert_eq!(resolve(&Km, None, Key::Q, None, Modifiers::NONE), global(Shadowed));
    }

    /// A scope's `parent` is searched after it and before the global table, transitively.
    struct KmTree;
    impl Keymap for KmTree {
        type Scope = Sc;
        type Cmd = Cm;
        fn global(&self) -> &[Binding<Cm>] {
            GLOBAL
        }
        fn bindings(&self, scope: Sc) -> &[Binding<Cm>] {
            match scope {
                // Overrides its parent's F.
                Sc::Settings => const { &[Binding::new(Jump, Key::F, Modifiers::NONE)] },
                other => Km.bindings(other),
            }
        }
        fn parent(&self, scope: Sc) -> Option<Sc> {
            match scope {
                Sc::Export => Some(Sc::Viewport),
                Sc::Settings => Some(Sc::Export),
                _ => None,
            }
        }
    }

    #[test]
    fn parents_are_searched_between_the_scope_and_the_global_table() {
        let r = |s, key, mods| resolve(&KmTree, Some(s), key, None, mods);
        let at = |s, c| Some((Route::Scope(s), c));
        // Export has no table: Viewport's F, G and its Q override come through it.
        assert_eq!(r(Sc::Export, Key::G, Modifiers::NONE), at(Sc::Viewport, Fit));
        assert_eq!(r(Sc::Export, Key::Q, Modifiers::NONE), at(Sc::Viewport, Override));
        // Two levels up, and the global table last.
        assert_eq!(r(Sc::Settings, Key::G, Modifiers::NONE), at(Sc::Viewport, Fit));
        assert_eq!(r(Sc::Settings, Key::Z, Modifiers::COMMAND), Some((Route::Global, Undo)));
        // A scope's own binding beats its ancestors'.
        assert_eq!(r(Sc::Settings, Key::F, Modifiers::NONE), at(Sc::Settings, Jump));
        assert_eq!(r(Sc::Export, Key::F, Modifiers::NONE), at(Sc::Viewport, Fit));
        // The timeline is not under the viewport.
        assert_eq!(r(Sc::Timeline, Key::G, Modifiers::NONE), None);
    }

    /// The repeat of a chord a nearer table binds without `repeats()` does not fall through to a
    /// farther repeating binding: the nearer table owns the chord.
    struct KmRep;
    impl Keymap for KmRep {
        type Scope = Sc;
        type Cmd = Cm;
        fn global(&self) -> &[Binding<Cm>] {
            const { &[Binding::new(Step, Key::ArrowRight, Modifiers::NONE).repeats()] }
        }
        fn bindings(&self, scope: Sc) -> &[Binding<Cm>] {
            match scope {
                Sc::Export => const { &[Binding::new(Jump, Key::ArrowRight, Modifiers::NONE)] },
                _ => &[],
            }
        }
    }

    #[test]
    fn a_nearer_non_repeating_binding_shadows_a_repeating_one() {
        let r = |scope, repeat| super::resolve(&KmRep, Some(scope), Key::ArrowRight, None, Modifiers::NONE, repeat);
        assert_eq!(r(Sc::Export, false), Some((Route::Scope(Sc::Export), Jump)));
        assert_eq!(r(Sc::Export, true), None);
        assert_eq!(r(Sc::Viewport, true), Some((Route::Global, Step)));
    }

    /// A keymap with chords that overlap inside one table.
    struct KmBad;
    impl Keymap for KmBad {
        type Scope = Sc;
        type Cmd = Cm;
        fn global(&self) -> &[Binding<Cm>] {
            const { &[Binding::new(Fit, Key::F, Modifiers::NONE), Binding::new(Jump, Key::F, Modifiers::NONE)] }
        }
        fn bindings(&self, scope: Sc) -> &[Binding<Cm>] {
            match scope {
                // An additive U also answers Shift+U, which the exact one binds.
                Sc::Timeline => const {
                    &[
                        Binding::additive(Keyed, Key::U, Modifiers::NONE),
                        Binding::new(Preview, Key::U, Modifiers::SHIFT),
                        Binding::new(Step, Key::U, Modifiers::ALT),
                    ]
                },
                // Fine: F is bound in the global table too, which is an override, not a conflict.
                Sc::Viewport => const { &[Binding::new(Fit, Key::F, Modifiers::NONE)] },
                _ => &[],
            }
        }
    }

    #[test]
    fn conflicts_lists_overlaps_inside_one_table_only() {
        assert_eq!(
            conflicts(&KmBad, &[Sc::Viewport, Sc::Timeline, Sc::Export]),
            [
                Conflict { route: Route::Global, first: Fit, second: Jump },
                Conflict { route: Route::Scope(Sc::Timeline), first: Keyed, second: Preview },
            ]
        );
        // The shipped test tables are clean, overrides and all.
        let all = [Sc::Viewport, Sc::Timeline, Sc::Export, Sc::Settings];
        assert_eq!(conflicts(&Km, &all), []);
        assert_eq!(conflicts(&KmTree, &all), []);
    }

    #[test]
    fn consume_takes_exactly_one_event_and_skips_repeats() {
        let ctx = hovering(VP.center());
        let repeat = Event::Key { key: Key::F, physical_key: None, pressed: true, repeat: true, modifiers: Modifiers::NONE };
        // Two chords of one command (egui drops a second press of a key that is still down).
        let events = vec![press(Key::F, None, Modifiers::NONE), press(Key::G, None, Modifiers::NONE), repeat];
        frame(&ctx, events, |ui| {
            both(ui);
            let c = ui.ctx();
            let one = || consume(&Km, c, Route::Scope(Sc::Viewport), Fit, false);
            assert!(one());
            // Two presses came in: one is left. The repeat is never a press.
            assert!(one());
            assert!(!one());
        });
    }

    #[test]
    fn auto_repeat_fires_only_repeating_bindings() {
        let ctx = hovering(TL.center());
        let repeat = |key| Event::Key { key, physical_key: None, pressed: true, repeat: true, modifiers: Modifiers::NONE };
        // A held key: the press, then repeats. The step repeats, the plain binding does not.
        // (egui turns the repeat of a key that is not down into a press, hence F is pressed first.)
        let events = vec![
            press(Key::ArrowRight, None, Modifiers::NONE),
            press(Key::F, None, Modifiers::NONE),
            repeat(Key::ArrowRight),
            repeat(Key::ArrowRight),
            repeat(Key::F),
        ];
        let mut left = 0;
        frame(&ctx, events, |ui| {
            both(ui);
            assert_eq!(pressed(&Km, ui.ctx(), false, |_| true), [Step, Fit, Step, Step]);
            left = ui.input(|i| i.events.iter().filter(|e| matches!(e, Event::Key { key: Key::F, .. })).count());
        });
        // The repeat of a non-repeating binding stays for the widgets.
        assert_eq!(left, 1);
        frame(&ctx, vec![repeat(Key::ArrowRight), repeat(Key::F)], |ui| {
            both(ui);
            let c = ui.ctx();
            assert!(!consume(&Km, c, Route::Scope(Sc::Timeline), Fit, false));
            assert!(consume(&Km, c, Route::Scope(Sc::Timeline), Step, false));
            assert!(!consume(&Km, c, Route::Scope(Sc::Timeline), Step, false));
        });
    }

    #[test]
    fn pressed_batches_the_active_scope_and_consumes_what_it_takes() {
        let ctx = hovering(TL.center());
        let events = vec![
            press(Key::Insert, None, Modifiers::NONE),
            press(Key::F, None, Modifiers::NONE),
            press(Key::Z, None, Modifiers::COMMAND),
            press(Key::Num3, Some(Key::Num3), Modifiers::NONE),
            // Not bound anywhere in the timeline scope.
            press(Key::G, None, Modifiers::NONE),
        ];
        let mut left = Vec::new();
        frame(&ctx, events, |ui| {
            both(ui);
            let got = pressed(&Km, ui.ctx(), false, |c| c != Fit);
            assert_eq!(got, [Preview, Undo, Mark(3)]);
            left = ui.input(|i| i.events.iter().filter_map(|e| if let Event::Key { key, .. } = e { Some(*key) } else { None }).collect());
        });
        // A refused command's key and an unbound key both stay for the widgets.
        assert_eq!(left, [Key::F, Key::G]);
    }

    /// The override is what `pressed` returns and what `consume` accepts: the global command it
    /// shadows neither fires nor is consumable over the overriding panel.
    #[test]
    fn an_override_wins_through_pressed_and_consume() {
        let q = || press(Key::Q, None, Modifiers::NONE);
        let ctx = hovering(VP.center());
        frame(&ctx, vec![q()], |ui| {
            both(ui);
            assert_eq!(pressed(&Km, ui.ctx(), false, |_| true), [Override]);
        });
        frame(&ctx, vec![release(Key::Q)], |ui| both(ui));
        frame(&ctx, vec![q()], |ui| {
            both(ui);
            let c = ui.ctx();
            assert!(!consume(&Km, c, Route::Global, Shadowed, false));
            assert!(consume(&Km, c, Route::Scope(Sc::Viewport), Override, false));
        });
        // Over a panel that does not override it, the global command answers.
        let ctx = hovering(TL.center());
        frame(&ctx, vec![q()], |ui| {
            both(ui);
            assert_eq!(pressed(&Km, ui.ctx(), false, |_| true), [Shadowed]);
        });
    }

    #[test]
    fn pressed_in_another_scope_reads_that_scopes_table() {
        let ctx = hovering(VP.center());
        frame(&ctx, vec![press(Key::Insert, None, Modifiers::NONE), press(Key::F, None, Modifiers::NONE)], |ui| {
            both(ui);
            assert_eq!(pressed(&Km, ui.ctx(), false, |_| true), [Fit]);
        });
    }

    #[test]
    fn typing_lets_only_ctrl_chords_through() {
        let ctx = hovering(VP.center());
        let events = || vec![press(Key::F, None, Modifiers::NONE), press(Key::Z, None, Modifiers::COMMAND)];
        frame(&ctx, events(), |ui| {
            both(ui);
            assert_eq!(pressed(&Km, ui.ctx(), true, |_| true), [Undo]);
        });
        frame(&ctx, vec![release(Key::F), release(Key::Z)], |_| {});
        frame(&ctx, events(), |ui| {
            both(ui);
            let c = ui.ctx();
            assert!(!consume(&Km, c, Route::Scope(Sc::Viewport), Fit, true));
            assert!(consume(&Km, c, Route::Global, Undo, true));
            // The bare key was left alone while typing, and fires once typing ends.
            assert!(consume(&Km, c, Route::Scope(Sc::Viewport), Fit, false));
        });
    }

    #[test]
    fn a_click_outside_every_panel_drops_the_owner_and_the_fallback_takes_over() {
        let ctx = Context::default();
        let both_fb = |ui: &Ui| {
            register(&KmFb, ui, Sc::Viewport, VP);
            register(&KmFb, ui, Sc::Timeline, TL);
        };
        let run = |events: Vec<Event>| {
            let mut seen = (None, None);
            frame(&ctx, events, |ui| {
                both(ui);
                both_fb(ui);
                seen = (active(&Km, ui.ctx()), active(&KmFb, ui.ctx()));
            });
            seen
        };
        // Startup: no panel hovered, nothing clicked. Only the keymap with a fallback has an owner.
        assert_eq!(run(vec![Event::PointerMoved(OUTSIDE)]), (None, Some(Sc::Viewport)));
        // Click in the timeline, move away: the click is remembered by both.
        run(vec![Event::PointerMoved(TL.center())]);
        run(vec![button(TL.center(), true)]);
        run(vec![button(TL.center(), false), Event::PointerMoved(OUTSIDE)]);
        assert_eq!(run(vec![]), (Some(Sc::Timeline), Some(Sc::Timeline)));
        // Press outside every panel: ownership is dropped, now and for the frames after.
        assert_eq!(run(vec![button(OUTSIDE, true)]), (None, None));
        run(vec![button(OUTSIDE, false)]);
        assert_eq!(run(vec![]), (None, Some(Sc::Viewport)));
    }

    /// A strip drawn as its own `Area` over the viewport: registered at its layer it is the
    /// viewport's, for hover and for clicks; unregistered it would hide the viewport.
    #[test]
    fn an_overlay_registered_at_its_layer_belongs_to_its_panel() {
        let ctx = Context::default();
        let strip = Rect::from_min_size(pos2(40.0, 40.0), vec2(100.0, 20.0));
        let layer = egui::LayerId::new(egui::Order::Middle, Id::new("strip"));
        let run = |events: Vec<Event>| {
            let mut seen = None;
            frame(&ctx, events, |ui| {
                both(ui);
                egui::Area::new(Id::new("strip")).order(egui::Order::Middle).fixed_pos(strip.min).show(ui.ctx(), |ui| {
                    ui.allocate_exact_size(strip.size(), egui::Sense::hover());
                });
                register_at(&Km, ui.ctx(), layer, Sc::Viewport, strip);
                seen = active(&Km, ui.ctx());
            });
            seen
        };
        run(vec![Event::PointerMoved(TL.center())]);
        run(vec![button(TL.center(), true)]);
        // Over the strip the viewport owns the keys, though the pointer is over the strip's layer.
        assert_eq!(run(vec![button(TL.center(), false), Event::PointerMoved(strip.center())]), Some(Sc::Viewport));
        assert_eq!(run(vec![Event::PointerMoved(OUTSIDE)]), Some(Sc::Timeline));
        // A click on the strip makes the viewport the retained owner.
        run(vec![Event::PointerMoved(strip.center()), button(strip.center(), true)]);
        assert_eq!(run(vec![button(strip.center(), false), Event::PointerMoved(OUTSIDE)]), Some(Sc::Viewport));
    }

    /// The retained owner is honoured only while its panel registers: a closed tab hands the
    /// keys to the fallback, and reopening the tab does not bring the stale owner back.
    #[test]
    fn an_owner_whose_panel_stopped_registering_falls_back() {
        let ctx = Context::default();
        let run = |timeline: bool, events: Vec<Event>| {
            let mut seen = (None, None);
            frame(&ctx, events, |ui| {
                both(ui);
                register(&KmFb, ui, Sc::Viewport, VP);
                if timeline {
                    register(&KmFb, ui, Sc::Timeline, TL);
                }
                seen = (active(&Km, ui.ctx()), active(&KmFb, ui.ctx()));
            });
            seen
        };
        run(true, vec![Event::PointerMoved(TL.center())]);
        run(true, vec![button(TL.center(), true)]);
        run(true, vec![button(TL.center(), false), Event::PointerMoved(OUTSIDE)]);
        assert_eq!(run(true, vec![]).1, Some(Sc::Timeline));
        // `Km` keeps drawing the timeline, `KmFb` stops: its owner is gone, the fallback answers.
        run(false, vec![]);
        assert_eq!(run(false, vec![]), (Some(Sc::Timeline), Some(Sc::Viewport)));
        // Back again without a click: the forgotten owner does not return.
        run(true, vec![]);
        assert_eq!(run(true, vec![]).1, Some(Sc::Viewport));
    }

    /// A region registered in another viewport (a detached window) is not a panel of this one.
    #[test]
    fn regions_of_another_viewport_are_ignored() {
        let ctx = Context::default();
        let other = egui::ViewportId::from_hash_of("hotkeys-other");
        let at = vec![Event::PointerMoved(TL.center())];
        frame(&ctx, at.clone(), |_| {});
        // Several frames, so its frame counter is as far along as the root's will be.
        for _ in 0..4 {
            frame_in(&ctx, other, at.clone(), |ui| register(&Km, ui, Sc::Timeline, TL));
        }
        let mut seen = Some(Sc::Export);
        frame(&ctx, at, |ui| seen = active(&Km, ui.ctx()));
        assert_eq!(seen, None);
    }

    /// A panel scrolled out of its clip rectangle does not own the pointer outside the clip.
    #[test]
    fn a_clipped_panel_owns_the_pointer_only_inside_its_clip() {
        let ctx = Context::default();
        let clip = Rect::from_min_max(pos2(20.0, 20.0), pos2(300.0, 100.0));
        let run = |at: Pos2| {
            let mut seen = None;
            frame(&ctx, vec![Event::PointerMoved(at)], |ui| {
                ui.scope(|ui| {
                    ui.set_clip_rect(clip.intersect(ui.clip_rect()));
                    register(&Km, ui, Sc::Viewport, VP);
                });
                seen = active(&Km, ui.ctx());
            });
            seen
        };
        let inside = clip.center();
        let below = pos2(100.0, 150.0);
        assert!(VP.contains(below) && !clip.contains(below));
        run(inside);
        assert_eq!(run(inside), Some(Sc::Viewport));
        assert_eq!(run(below), None);
    }

    #[test]
    fn keymaps_keep_separate_state() {
        let ctx = Context::default();
        frame(&ctx, vec![Event::PointerMoved(TL.center())], |ui| register(&Km, ui, Sc::Timeline, TL));
        frame(&ctx, vec![Event::PointerMoved(TL.center())], |ui| {
            register(&Km, ui, Sc::Timeline, TL);
            // KmFb has the same scope and command types but registered nothing.
            assert_eq!(active(&Km, ui.ctx()), Some(Sc::Timeline));
            assert_eq!(active(&KmFb, ui.ctx()), Some(Sc::Viewport));
        });
    }

    #[test]
    fn copy_and_paste_are_taken_only_outside_text_editors() {
        let ctx = Context::default();
        frame(&ctx, vec![Event::Copy, Event::Paste("a".into()), Event::Paste("b".into())], |ui| {
            let c = ui.ctx();
            assert!(!take_copy(c, false));
            assert!(take_copy(c, true));
            assert!(!take_copy(c, true));
            assert_eq!(take_paste(c, |t| t == "b").as_deref(), Some("b"));
            assert_eq!(take_paste(c, |t| t == "b"), None);
            assert_eq!(take_paste(c, |_| true).as_deref(), Some("a"));
        });
    }
}
