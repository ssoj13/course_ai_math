//! Transient "toast" notifications for egui — a top-right, auto-expiring stack.
//!
//! This is the *ephemeral* complement to the persistent `egui-logbuffer` panel:
//! where the log buffer keeps a scrollback you browse, toasts are fire-and-forget
//! pop-ups that appear, linger a few seconds, fade, and remove themselves.
//!
//! ## Ownership / shape
//! The [`ToastStack`] is **caller-owned state** (hold one in your app struct).
//! Push messages whenever something noteworthy happens, then call
//! [`ToastStack::show`] **once per frame**. `show` is the only mutator that
//! matters at render time: it stamps each toast's first-shown timestamp, drops
//! expired ones, and applies the fade. No app types — `Toast` carries a `String`
//! and a [`ToastLevel`].
//!
//! ## Timing model (the "why")
//! A toast's lifetime is measured from the **first frame it is displayed**, not
//! from when it was pushed. We deliberately stamp `created_at` lazily inside
//! `show` (from `ctx.input(|i| i.time)`) rather than at `push` time so that:
//! - toasts queued while the window was unfocused / not repainting do not
//!   silently "expire" before the user ever sees them, and
//! - we depend only on egui's frame clock, never `std::time` (consistent with
//!   the rest of this widget set and correct under egui's time source).
//!
//! While any toast is live we call `ctx.request_repaint()` so expiry and the
//! fade animate smoothly even when nothing else is driving the UI.

use egui::{Align2, Area, Color32, Context, Frame, Id, Order, RichText, Vec2};

/// Severity of a toast, mapped to an accent colour by [`ToastLevel::accent`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastLevel {
    Info,
    Success,
    Warn,
    Error,
}

impl ToastLevel {
    /// Accent colour for the toast's box (border + tint).
    pub fn accent(self) -> Color32 {
        match self {
            ToastLevel::Info => Color32::from_rgb(80, 160, 255),
            ToastLevel::Success => Color32::from_rgb(90, 200, 110),
            ToastLevel::Warn => Color32::from_rgb(235, 180, 60),
            ToastLevel::Error => Color32::from_rgb(235, 90, 90),
        }
    }

    /// A small leading status glyph, from the shared [`egui_widgets_config::icons`]
    /// vocabulary. The host app must register the phosphor font
    /// (`egui_phosphor::add_to_fonts`) for these to render.
    pub fn icon(self) -> &'static str {
        use egui_widgets_config::icons;
        match self {
            ToastLevel::Info => icons::INFO,
            ToastLevel::Success => icons::SUCCESS,
            ToastLevel::Warn => icons::WARNING,
            ToastLevel::Error => icons::ERROR,
        }
    }
}

/// One transient notification.
///
/// `created_at` is `None` until the first frame [`ToastStack::show`] paints it;
/// see the module-level timing note for why stamping is lazy. `ttl` is the
/// lifetime in seconds *after* that first show.
pub struct Toast {
    pub text: String,
    pub level: ToastLevel,
    /// Stable per-toast id assigned by the owning [`ToastStack`] on push. Used as
    /// the floating `Area` id so a toast keeps its identity (no flicker) when an
    /// EARLIER toast is removed and the rest shift position.
    id: u64,
    /// Frame-clock time (seconds, from egui) of the first display; lazily set.
    created_at: Option<f64>,
    /// Lifetime in seconds after first display.
    ttl: f64,
}

/// Default lifetime of a toast, in seconds.
const DEFAULT_TTL: f64 = 4.0;
/// Duration of the trailing fade-out, in seconds.
const FADE: f64 = 0.5;
/// Duration of the leading fade-in, in seconds (toasts ease in rather than pop).
const FADE_IN: f64 = 0.18;

impl Toast {
    /// A new toast with the default lifetime (~4 s). Not shown until it is in a
    /// [`ToastStack`] and that stack's `show` runs.
    pub fn new(text: impl Into<String>, level: ToastLevel) -> Self {
        // `id` is a placeholder; the owning `ToastStack` assigns a stable one on push.
        Self {
            text: text.into(),
            level,
            id: 0,
            created_at: None,
            ttl: DEFAULT_TTL,
        }
    }

    /// Override the lifetime (seconds after first display).
    pub fn ttl(mut self, secs: f64) -> Self {
        self.ttl = secs;
        self
    }

    /// Opacity in `0.0..=1.0` for `now`: full until the last [`FADE`] seconds,
    /// then linearly to 0. `None` `created_at` means "not shown yet" ⇒ fully
    /// opaque (it is about to be stamped this frame).
    fn alpha(&self, now: f64) -> f32 {
        // Not stamped yet (about to be this frame) ⇒ start transparent so the
        // very first painted frame is the bottom of the fade-in, not a pop.
        let Some(t0) = self.created_at else {
            return 0.0;
        };
        let elapsed = now - t0;
        let remaining = self.ttl - elapsed;
        // Ease in over the first FADE_IN s, hold at full, ease out the last FADE s.
        let fade_in = (elapsed / FADE_IN).clamp(0.0, 1.0) as f32;
        let fade_out = if remaining >= FADE {
            1.0
        } else {
            (remaining / FADE).clamp(0.0, 1.0) as f32
        };
        fade_in.min(fade_out)
    }

    /// Whether this toast has outlived its ttl as of `now` (only meaningful once
    /// `created_at` is stamped).
    fn expired(&self, now: f64) -> bool {
        matches!(self.created_at, Some(t0) if now - t0 > self.ttl)
    }
}

/// A caller-owned stack of live toasts. `Default` is an empty stack.
#[derive(Default)]
pub struct ToastStack {
    toasts: Vec<Toast>,
    /// Monotonic id source so every toast gets a stable `Area` key (see `Toast::id`).
    next_id: u64,
}

impl ToastStack {
    /// Empty stack.
    pub fn new() -> Self {
        Self::default()
    }

    /// Push a message with the default lifetime.
    pub fn push(&mut self, text: impl Into<String>, level: ToastLevel) {
        self.push_toast(Toast::new(text, level));
    }

    /// Push a pre-built [`Toast`] (e.g. with a custom `.ttl(..)`).
    pub fn push_toast(&mut self, mut toast: Toast) {
        toast.id = self.next_id;
        self.next_id += 1;
        self.toasts.push(toast);
    }

    /// True if no toasts are live.
    pub fn is_empty(&self) -> bool {
        self.toasts.is_empty()
    }

    /// Paint the live toasts as a top-right anchored floating stack, advancing
    /// their lifecycle. Call **once per frame**.
    ///
    /// Per call this: stamps `created_at` on any not-yet-shown toast (from
    /// `ctx.input(|i| i.time)`), paints each as a rounded accent-tinted box with
    /// its text, fades the last [`FADE`] seconds, dismisses any clicked toast,
    /// removes expired ones, and — while any remain — requests a repaint so the
    /// expiry/fade animate.
    pub fn show(&mut self, ctx: &Context) {
        if self.toasts.is_empty() {
            return;
        }

        let now = ctx.input(|i| i.time);

        // Stamp first-show time lazily so lifetimes start when the user can
        // actually see the toast (see module timing note).
        for toast in &mut self.toasts {
            if toast.created_at.is_none() {
                toast.created_at = Some(now);
            }
        }

        // Stack downward from the top-right corner. Each toast is its own Area so
        // it floats above panels; we offset successive toasts by their height.
        let mut y_offset = 8.0;
        // Per-toast keep flags (false = remove this frame). Indexed by position;
        // computed during paint, applied in one retain pass after the loop.
        let mut keep = vec![true; self.toasts.len()];

        for (idx, toast) in self.toasts.iter().enumerate() {
            let alpha = toast.alpha(now);
            let accent = toast.level.accent();
            // Pre-multiplied-ish tint: scale all channels by alpha for the fade.
            let a = (alpha * 255.0) as u8;
            let border = Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), a);
            let bg = Color32::from_rgba_unmultiplied(28, 28, 32, (alpha * 235.0) as u8);
            let text_col = Color32::from_rgba_unmultiplied(235, 235, 235, (alpha * 255.0) as u8);

            // Per-toast stable unique id (see `Toast::id` / `ToastStack::next_id`).
            let area_id = Id::new(("egui_toast", toast.id));
            let inner = Area::new(area_id)
                .order(Order::Foreground)
                .anchor(Align2::RIGHT_TOP, Vec2::new(-8.0, y_offset))
                .show(ctx, |ui| {
                    let frame = Frame::default()
                        .fill(bg)
                        .stroke(egui::Stroke::new(1.0, border))
                        .corner_radius(8.0)
                        .inner_margin(egui::Margin::symmetric(12, 9))
                        .shadow(egui::Shadow {
                            offset: [0, 3],
                            blur: 12,
                            spread: 0,
                            color: Color32::from_black_alpha((alpha * 90.0) as u8),
                        })
                        .show(ui, |ui| {
                            ui.set_max_width(280.0);
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(toast.level.icon()).color(border).strong());
                                ui.add_space(2.0);
                                ui.label(RichText::new(&toast.text).color(text_col));
                            });
                        });
                    // Sense the dismiss-click with a CHILD id (area_id.with(..)), NOT
                    // by re-`interact`ing the Area response: re-interacting reuses the
                    // Area's own id a second time in the same frame, which is exactly
                    // what triggered egui's "Double use of widget id" on every toast.
                    ui.interact(
                        frame.response.rect,
                        area_id.with("dismiss"),
                        egui::Sense::click(),
                    )
                    .clicked()
                });

            // Advance the next toast below this one (plus a small gap).
            y_offset += inner.response.rect.height() + 6.0;

            // Click anywhere on the toast dismisses it; expiry also removes it.
            keep[idx] = !inner.inner && !toast.expired(now);
        }

        // Single retain pass driven by the per-toast keep flags.
        let mut idx = 0;
        self.toasts.retain(|_| {
            let k = keep[idx];
            idx += 1;
            k
        });

        // Keep animating while anything is still live.
        if !self.toasts.is_empty() {
            ctx.request_repaint();
        }
    }
}
