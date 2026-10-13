//! The row's value actions: reset to default, typed copy / paste, randomise.
//!
//! One switch, [`crate::AttrGridConfig::actions`] (default: none), decides which actions a host
//! gets, as small buttons in the row's action gutter (`0 C P Rs Ra`) AND as entries of the label's
//! context menu. Both go through [`run`], so there is ONE implementation. A row whose editor the
//! host draws itself ([`crate::AttrGridHooks::editor`]) offers only Copy: the grid does not know
//! that editor's invariants (an index combo, a partial multi-selection), and the host may refuse
//! any value through [`crate::AttrGridHooks::accepts`]. Each action is a plain function over
//! the field, so the tests drive the same code.
//!
//! An action reports an edit only when the value really changed (compared after clamping), so a
//! reset on a row already at its default, or a paste of an equal value, is not an edit.

use std::borrow::Cow;
use std::sync::Arc;

use egui::{Context, Id, Ui};

use crate::random::Pcg;
use crate::{AttrField, AttrValue};

/// egui-memory key of the process-wide typed clipboard: ONE slot that survives frames, so
/// copy-then-paste works whichever rows are involved.
fn clipboard_id() -> Id {
    Id::new("egui_attr_grid_clipboard_v2")
}

/// The clipboard slot without copying the value: rows ask every frame whether a paste would be
/// accepted, and egui's store hands back a CLONE of whatever it holds, so the slot holds an `Arc`.
fn clipboard_arc(ctx: &Context) -> Option<Arc<AttrValue>> {
    ctx.data(|d| d.get_temp::<Arc<AttrValue>>(clipboard_id()))
}

/// Put `value` on the clipboard (replacing the single slot).
pub fn clipboard_set(ctx: &Context, value: AttrValue) {
    ctx.data_mut(|d| d.insert_temp(clipboard_id(), Arc::new(value)));
}

/// Do `a` and `b` have the same type, so one can be written over the other? A list must also
/// match element by element: pasting a 3-list over a 5-list would change the attribute's shape.
pub fn same_kind(a: &AttrValue, b: &AttrValue) -> bool {
    match (a, b) {
        (AttrValue::List(x), AttrValue::List(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| same_kind(p, q))
        }
        _ => std::mem::discriminant(a) == std::mem::discriminant(b),
    }
}

/// The hard `[min, max]` of a ranged scalar row (a `soft` range holds nothing back).
fn hard_range(ui_options: &[String]) -> Option<(f64, f64)> {
    crate::slider_range(ui_options).filter(|_| !crate::is_soft(ui_options))
}

/// Hold a scalar inside the row's hard slider range, so a reset or paste never leaves a value
/// the slider itself could not produce.
pub(crate) fn clamp_value(value: &mut AttrValue, ui_options: &[String]) {
    let Some((lo, hi)) = hard_range(ui_options) else {
        return;
    };
    if let Some(v) = value.as_f64() {
        value.set_f64(v.clamp(lo, hi));
    }
}

/// May `value` be written into `field`: same type, and a string choice must be one of the row's
/// choices (a combo cannot show anything else).
fn accepts_value(field: &AttrField, value: &AttrValue) -> bool {
    if matches!(field.value, AttrValue::Label(_)) || !same_kind(&field.value, value) {
        return false;
    }
    match value {
        AttrValue::Str(s) if crate::is_choice_row(&field.ui_options) => {
            crate::choices(&field.ui_options).any(|c| c == s)
        }
        _ => true,
    }
}

/// The field's default as a reset would write it: the same type as the value (a mismatched one is
/// refused, a field can never change type) and held in the hard range. Borrowed unless clamped.
pub(crate) fn resolved_default<'a>(
    value: &AttrValue,
    default: Option<&'a AttrValue>,
    ui_options: &[String],
) -> Option<Cow<'a, AttrValue>> {
    let default = default?;
    if matches!(value, AttrValue::Label(_)) || !same_kind(value, default) {
        return None;
    }
    let mut out = Cow::Borrowed(default);
    if hard_range(ui_options).is_some() && default.as_f64().is_some() {
        let mut clamped = default.clone();
        clamp_value(&mut clamped, ui_options);
        out = Cow::Owned(clamped);
    }
    Some(out)
}

/// Write `default` over `value` (or just component `part` of it): the ONE reset implementation,
/// behind the menu, the gutter button and the Ctrl / Cmd-click gesture. The default must be the
/// value's type and is held in the hard range ([`resolved_default`]); a part is a vector axis, a
/// list item, or the scalar itself. False when there is no usable default or nothing changes.
pub fn reset_to(
    value: &mut AttrValue,
    default: Option<&AttrValue>,
    ui_options: &[String],
    part: Option<usize>,
) -> bool {
    let Some(default) = resolved_default(value, default, ui_options) else {
        return false;
    };
    let mut next = value.clone();
    match (&mut next, part) {
        (AttrValue::Vec3(v), Some(i)) => set_axis(v.get_mut(i), default.component(i)),
        (AttrValue::Vec4(v), Some(i)) => set_axis(v.get_mut(i), default.component(i)),
        (AttrValue::List(items), Some(i)) => {
            if let (Some(item), Some(d)) = (items.get_mut(i), default.component(i)) {
                item.set_f64(d);
            }
        }
        _ => next = default.into_owned(),
    }
    if next == *value {
        return false;
    }
    *value = next;
    true
}

/// Write one vector axis from the default's matching component, when both exist.
fn set_axis(slot: Option<&mut f32>, default: Option<f64>) {
    if let (Some(slot), Some(d)) = (slot, default) {
        *slot = d as f32;
    }
}

/// Write the field's default over its value. False when there is none or nothing changes.
#[cfg(test)]
pub(crate) fn reset(field: &mut AttrField) -> bool {
    reset_to(
        &mut field.value,
        field.default.as_ref(),
        &field.ui_options,
        None,
    )
}

/// egui-memory key of the pending reset request (see [`request_reset`]).
fn reset_id() -> Id {
    Id::new("egui_attr_grid_reset_request")
}

/// An editor saw the reset gesture (Ctrl / Cmd-click) on component `part` (None: the whole
/// value). Editors never write the default themselves: the caller takes the request
/// ([`take_reset`]) and applies it through [`reset_to`] / [`run_part`], so a host's veto
/// ([`crate::AttrGridHooks::accepts`]) holds for the gesture as it does for the menu.
pub(crate) fn request_reset(ctx: &Context, part: Option<usize>) {
    ctx.data_mut(|d| d.insert_temp(reset_id(), part));
}

/// Take the reset request an editor made this frame: `Some(part)` once per request. A caller
/// that renders editors calls it before (to drop a stale request) and after them.
pub fn take_reset(ctx: &Context) -> Option<Option<usize>> {
    ctx.data_mut(|d| d.remove_temp::<Option<usize>>(reset_id()))
}

/// Write the clipboard over the field's value when the types match. False when it cannot.
pub(crate) fn paste(ctx: &Context, field: &mut AttrField) -> bool {
    let Some(clip) = clipboard_arc(ctx).filter(|c| accepts_value(field, c)) else {
        return false;
    };
    let mut value = (*clip).clone();
    clamp_value(&mut value, &field.ui_options);
    if value == field.value {
        return false;
    }
    field.value = value;
    true
}

/// The distribution a field is randomised from: its own [`crate::Random`] policy, else one made
/// from what the row is (the slider range of a number, 0..1 for a colour, a bool or a choice), as
/// the old section-level randomise did. None when the row has nothing to randomise.
pub(crate) fn policy(field: &AttrField) -> Option<Cow<'_, crate::Random>> {
    policy_of(&field.value, field.random.as_ref(), &field.ui_options)
}

/// [`policy`] over the field's parts, so the draw can borrow the policy and write the value.
fn policy_of<'a>(
    value: &AttrValue,
    random: Option<&'a crate::Random>,
    ui_options: &[String],
) -> Option<Cow<'a, crate::Random>> {
    if let Some(own) = random {
        return Some(Cow::Borrowed(own));
    }
    let unit = || Some(Cow::Owned(crate::Random::uniform(0.0..=1.0)));
    match value {
        AttrValue::Bool(_) => unit(),
        AttrValue::Str(_) if crate::is_choice_row(ui_options) => unit(),
        AttrValue::Vec3(_) | AttrValue::Vec4(_) if crate::is_color_hint(ui_options) => unit(),
        v if v.as_f64().is_some() => crate::slider_range(ui_options)
            .map(|(lo, hi)| Cow::Owned(crate::Random::uniform(lo..=hi))),
        _ => None,
    }
}

/// Draw a fresh random value from the field's policy ([`policy`]). Numbers are held in the
/// hard slider range; a colour keeps its alpha. False when the field has no policy, its type has
/// nothing to randomise, or the draw happened to equal the old value.
pub(crate) fn randomize(field: &mut AttrField, rng: &mut Pcg) -> bool {
    let before = field.value.clone();
    draw(field, rng) && field.value != before
}

/// The draw behind [`randomize`]; true when the field's type was randomisable.
fn draw(field: &mut AttrField, rng: &mut Pcg) -> bool {
    let Some(policy) = policy_of(&field.value, field.random.as_ref(), &field.ui_options) else {
        return false;
    };
    let range = hard_range(&field.ui_options);
    let num = |rng: &mut Pcg| {
        let s = policy.sample(rng);
        range.map_or(s, |(lo, hi)| s.clamp(lo, hi))
    };
    // Float-to-int `as` saturates, so a wide Normal sample cannot wrap.
    match &mut field.value {
        AttrValue::Float(v) => *v = num(rng) as f32,
        AttrValue::Int(v) => *v = num(rng).round() as i32,
        AttrValue::UInt(v) => *v = num(rng).round() as u32,
        AttrValue::Int64(v) => *v = num(rng).round() as i64,
        AttrValue::Int8(v) => *v = num(rng).round() as i8,
        AttrValue::Bool(v) => *v = policy.sample(rng) >= 0.5,
        AttrValue::Vec3(a) => a.iter_mut().for_each(|c| *c = num(rng) as f32),
        AttrValue::Vec4(a) => {
            // A colour's alpha is not part of "a random colour".
            let n = if crate::is_color_hint(&field.ui_options) {
                3
            } else {
                4
            };
            a[..n].iter_mut().for_each(|c| *c = num(rng) as f32);
        }
        AttrValue::Str(s) if crate::is_choice_row(&field.ui_options) => {
            let n = crate::choices(&field.ui_options).count();
            let i = ((rng.next_f64() * n as f64) as usize).min(n.saturating_sub(1));
            let Some(pick) = crate::choices(&field.ui_options).nth(i) else {
                return false;
            };
            *s = pick.to_owned();
        }
        _ => return false,
    }
    true
}

/// Seed for one randomise click: the frame clock (so successive clicks differ) mixed with the
/// row key (so rows randomised in one frame are decorrelated).
fn seed(ctx: &Context, key: &str) -> u64 {
    ((ctx.input(|i| i.time) * 1_000_000.0) as u64) ^ Id::new(key).value()
}

/// A value action of a row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueAction {
    /// Write the field's default over its value.
    Reset,
    /// Put the value on the typed clipboard.
    Copy,
    /// Write the clipboard over the value (same type only).
    Paste,
    /// Draw a fresh random value for this row.
    RandomOne,
    /// Like [`Self::RandomOne`], and tell the host ([`crate::AttrGridHooks::random_all`]) to do the
    /// same on its other selected objects: the grid never sees the object set.
    RandomAll,
}

impl ValueAction {
    /// Every action, in gutter order.
    pub const ALL: [ValueAction; 5] = [
        ValueAction::Reset,
        ValueAction::Copy,
        ValueAction::Paste,
        ValueAction::RandomOne,
        ValueAction::RandomAll,
    ];

    /// The gutter button's icon.
    pub fn glyph(self) -> &'static str {
        use egui_widgets_config::icons;
        match self {
            ValueAction::Reset => icons::RESET,
            ValueAction::Copy => icons::COPY,
            ValueAction::Paste => icons::PASTE,
            ValueAction::RandomOne => icons::RANDOM,
            ValueAction::RandomAll => icons::RANDOM_ALL,
        }
    }

    /// The word an assistive technology reads for the button.
    pub fn name(self) -> &'static str {
        match self {
            ValueAction::Reset => "Reset",
            ValueAction::Copy => "Copy",
            ValueAction::Paste => "Paste",
            ValueAction::RandomOne => "Randomise",
            ValueAction::RandomAll => "Randomise all",
        }
    }

    /// The menu entry's text, also the button's tooltip.
    pub fn label(self) -> &'static str {
        match self {
            ValueAction::Reset => "Reset to default",
            ValueAction::Copy => "Copy value",
            ValueAction::Paste => "Paste value",
            ValueAction::RandomOne => "Randomise",
            ValueAction::RandomAll => "Randomise across all objects",
        }
    }
}

/// Which value actions the grid draws as buttons in each row's action gutter. The row menu offers
/// them regardless. Default: none, so a host that never asks keeps its gutters as they were.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ValueActions {
    pub reset: bool,
    pub copy: bool,
    pub paste: bool,
    pub random_one: bool,
    pub random_all: bool,
}

impl ValueActions {
    /// Reset, copy and paste: what WarpBro and Playa turn on.
    pub const BASIC: ValueActions = ValueActions {
        reset: true,
        copy: true,
        paste: true,
        random_one: false,
        random_all: false,
    };

    /// Is `action` enabled?
    pub fn has(self, action: ValueAction) -> bool {
        match action {
            ValueAction::Reset => self.reset,
            ValueAction::Copy => self.copy,
            ValueAction::Paste => self.paste,
            ValueAction::RandomOne => self.random_one,
            ValueAction::RandomAll => self.random_all,
        }
    }

    /// The actions enabled in BOTH: a row's own set can only narrow the grid's.
    pub fn and(self, other: ValueActions) -> ValueActions {
        ValueActions {
            reset: self.reset && other.reset,
            copy: self.copy && other.copy,
            paste: self.paste && other.paste,
            random_one: self.random_one && other.random_one,
            random_all: self.random_all && other.random_all,
        }
    }

    /// How many buttons the gutter reserves (one slot each, so columns line up across rows).
    pub fn count(self) -> usize {
        ValueAction::ALL.iter().filter(|a| self.has(**a)).count()
    }
}

/// The host's veto over a value (its invariants): `(field, candidate)`.
pub(crate) type Accepts<'a> = &'a dyn Fn(&AttrField, &AttrValue) -> bool;

/// Does `action` make sense for this field at all (a reset needs a default, a randomise something
/// to draw from)? `owned`: the host draws this row's editor, so only Copy applies. Inapplicable
/// actions are not drawn; applicable ones may still be disabled ([`enabled`]).
pub(crate) fn applicable(field: &AttrField, action: ValueAction, owned: bool) -> bool {
    match action {
        ValueAction::Copy => !matches!(field.value, AttrValue::Label(_)),
        _ if owned => false,
        ValueAction::Reset => field.default.is_some(),
        ValueAction::Paste => !matches!(field.value, AttrValue::Label(_)),
        ValueAction::RandomOne | ValueAction::RandomAll => policy(field).is_some(),
    }
}

/// Can `action` run now? `editable` is false on an inactive row: it may still be copied.
pub(crate) fn enabled(
    ctx: &Context,
    field: &AttrField,
    editable: bool,
    action: ValueAction,
    accepts: Accepts,
) -> bool {
    match action {
        ValueAction::Copy => true,
        ValueAction::Reset => {
            editable
                && resolved_default(&field.value, field.default.as_ref(), &field.ui_options)
                    .is_some_and(|d| *d != field.value && accepts(field, &d))
        }
        ValueAction::Paste => {
            editable
                && clipboard_arc(ctx)
                    .is_some_and(|v| accepts_value(field, &v) && accepts(field, &v))
        }
        ValueAction::RandomOne | ValueAction::RandomAll => editable,
    }
}

/// Run `action` on the field; true when it edited the value. `part`: only a reset uses it.
fn fire(ctx: &Context, field: &mut AttrField, action: ValueAction, part: Option<usize>) -> bool {
    match action {
        ValueAction::Reset => reset_to(
            &mut field.value,
            field.default.as_ref(),
            &field.ui_options,
            part,
        ),
        ValueAction::Copy => {
            clipboard_set(ctx, field.value.clone());
            false
        }
        ValueAction::Paste => paste(ctx, field),
        ValueAction::RandomOne => randomize(field, &mut Pcg::new(seed(ctx, &field.key))),
        // A different stream from RandomOne, so the two buttons do not repeat each other.
        ValueAction::RandomAll => randomize(field, &mut Pcg::new(seed(ctx, &field.key) ^ 0xA11)),
    }
}

/// [`fire`] under the host's veto: a value the host does not accept is rolled back and is no edit.
pub(crate) fn run(
    ctx: &Context,
    field: &mut AttrField,
    action: ValueAction,
    accepts: Accepts,
) -> bool {
    run_part(ctx, field, action, None, accepts)
}

/// [`run`] on one component of the value: what a Ctrl / Cmd-click reset of a vector axis is.
pub(crate) fn run_part(
    ctx: &Context,
    field: &mut AttrField,
    action: ValueAction,
    part: Option<usize>,
    accepts: Accepts,
) -> bool {
    if action == ValueAction::Copy {
        return fire(ctx, field, action, part);
    }
    let before = field.value.clone();
    let edited = fire(ctx, field, action, part);
    if edited && !accepts(field, &field.value) {
        field.value = before;
        return false;
    }
    edited
}

/// Randomise every field that can be ([`policy`]: its own [`crate::Random`], else its slider range
/// or 0..1 for colours, bools and choices), as one section-level "Rnd" button would; returns the
/// `(key, value)` edits, like the grid. `seed` should change per click.
pub fn randomize_fields(fields: &mut [AttrField], seed: u64) -> Vec<(String, AttrValue)> {
    let mut rng = Pcg::new(seed);
    fields
        .iter_mut()
        .filter_map(|f| randomize(f, &mut rng).then(|| (f.key.clone(), f.value.clone())))
        .collect()
}

/// The built-in entries of a row's context menu: the enabled `flags` that apply to the row; the
/// one clicked, for the caller to [`run`] (it owns the hooks and the edit list).
pub(crate) fn menu(
    ui: &mut Ui,
    field: &AttrField,
    flags: ValueActions,
    editable: bool,
    owned: bool,
    accepts: Accepts,
) -> Option<ValueAction> {
    let mut clicked = None;
    for action in ValueAction::ALL {
        if !flags.has(action) || !applicable(field, action, owned) {
            continue;
        }
        let on = enabled(ui.ctx(), field, editable, action, accepts);
        if ui
            .add_enabled(on, egui::Button::new(action.label()))
            .clicked()
        {
            clicked = Some(action);
            ui.close();
        }
    }
    clicked
}
