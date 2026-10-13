//! The color-management context.
//!
//! In lcms2 a `cmsContext` owns everything that C would otherwise keep in globals:
//! per-context plugin registries, the memory manager, the error logger, the observer
//! adaptation state and the proofing alarm codes. It is threaded explicitly through the
//! `*THR` APIs; `ContextID == NULL` (id 0) means "use the process-global default".
//!
//! The Rust port keeps the *threading* model — an explicit `&Context` — but replaces the
//! `void* chunks[MemoryClientMax]` array with **typed fields**, and drops the pieces that
//! Rust provides natively (the memory manager / sub-allocator → Rust ownership; the mutex
//! plugin → std `Sync`), so there is no `static mut`, no `unsafe`, no allocator indirection.
//!
//! Plugin registries live in [`Context::plugins`]-shaped *chunks*, reached through the same
//! two-step as C's `_cmsContextGetClientChunk` (`cmsplugin.c:747-772`): the context's own
//! chunk if it has one, otherwise Context 0's. See [`crate::plugin`] for the lookup side;
//! this module owns the storage and the `*THR` mutators.

use core::any::Any;
use core::fmt;
use std::sync::{Arc, OnceLock, RwLock};

use crate::MAX_CHANNELS;
use crate::error::{ErrorCode, Result};
use crate::plugin::{self, Plugin, PluginChunk};

/// Error-logging callback (`cmsLogErrorHandlerFunction`).
///
/// lcms passes `(ContextID, code, text)`; the context is implicit here (you hold the
/// `&Context` that owns the handler), so the callback receives just the classification
/// and the formatted message. Most fallible paths return [`crate::Result`] instead — the
/// logger exists for parity with code that signals-and-continues.
pub type LogHandler = Arc<dyn Fn(ErrorCode, &str) + Send + Sync>;

/// Default observer adaptation state (`DEFAULT_OBSERVER_ADAPTATION_STATE` in `cmsxform.c`).
pub const DEFAULT_ADAPTATION_STATE: f64 = 1.0;

/// Default proofing alarm codes (`DEFAULT_ALARM_CODES_VALUE` in `cmsxform.c`): the first
/// three channels flag out-of-gamut with `0x7F00`, the rest are zero.
pub const DEFAULT_ALARM_CODES: [u16; MAX_CHANNELS] =
    [0x7F00, 0x7F00, 0x7F00, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];

/// Context 0's logger chunk (`globalContext.chunks[Logger]`), lazily created.
static DEFAULT_LOGGER: OnceLock<RwLock<Option<LogHandler>>> = OnceLock::new();

fn default_logger() -> &'static RwLock<Option<LogHandler>> {
    DEFAULT_LOGGER.get_or_init(|| RwLock::new(None))
}

/// Install the **Context-0** error logger (`cmsSetLogErrorHandler`, `cmserr.c:546-549`).
///
/// C defines this as `cmsSetLogErrorHandlerTHR(NULL, Fn)`, i.e. it writes the process-global
/// default that every context *without a logger chunk of its own* inherits. `None` restores
/// the silent `DefaultLogErrorHandlerFunction` (`cmserr.c:520-529`).
///
/// The per-context (`*THR`) form is [`Context::set_log_handler`]. The two do **not** collapse:
/// they target different chunks, and once a context owns a logger it stops consulting this one.
pub fn set_log_handler(handler: Option<LogHandler>) {
    let mut slot = default_logger().write().unwrap_or_else(|e| e.into_inner());
    *slot = handler;
}

/// Whether a context owns a logger chunk, and what is in it.
///
/// C models this with a pointer that is either NULL (inherit Context 0) or a chunk holding a
/// handler that may itself be the no-op default. Collapsing the two into a single
/// `Option<LogHandler>` would lose a real distinction: `cmsSetLogErrorHandlerTHR(ctx, NULL)`
/// **silences that context** by writing the default handler into its own chunk
/// (`cmserr.c:532-543`) — it does not re-expose the global one.
#[derive(Clone, Default)]
enum LoggerSlot {
    /// No chunk of its own: `_cmsContextGetClientChunk` falls back to Context 0.
    #[default]
    Inherit,
    /// This context owns a logger chunk; `None` is C's silent default handler.
    Own(Option<LogHandler>),
}

/// A color-management context (`cmsContext`).
///
/// Cheap to create; shareable behind `&` across threads (it is `Send + Sync`). Configure
/// it (logger, alarm codes, adaptation state, plugins) via the `&mut self` setters before
/// handing it to transforms, which borrow it immutably.
///
/// # `Clone` vs [`Context::dup`]
/// `Clone` copies the `Arc` to the plugin chunk, so a clone **is the same context**: later
/// registrations through either handle are seen by both. That mirrors C, where copying a
/// `cmsContext` copies a pointer. [`Context::dup`] is the other one — `cmsDupContext`, which
/// deep-copies the chunks so the two diverge from that moment on.
#[derive(Clone)]
pub struct Context {
    /// Observer adaptation state used by absolute-colorimetric intent (0.0..=1.0).
    adaptation_state: f64,
    /// Out-of-gamut marker codes for proofing transforms (16-bit encoded).
    alarm_codes: [u16; MAX_CHANNELS],
    /// This context's error logger, or `Inherit` to use Context 0's.
    logger: LoggerSlot,
    /// Opaque user data (`cmsCreateContext`'s `UserData`, `cmsGetContextUserData`).
    user_data: Option<Arc<dyn Any + Send + Sync>>,
    /// This context's plugin chunks, or `None` to read Context 0's.
    ///
    /// `None` is C's `ContextID == NULL`, which `_cmsGetContext` (`cmsplugin.c:714-721`)
    /// resolves to `globalContext`. Once this is `Some` — even holding an *empty* chunk set —
    /// Context 0 is shadowed entirely, exactly as in `_cmsContextGetClientChunk`.
    plugins: Option<PluginChunk>,
}

impl Context {
    /// Create a context with lcms2's default settings, deferring to Context 0.
    ///
    /// It owns neither a plugin chunk nor a logger chunk, so both resolve to the process-global
    /// defaults — the behaviour of every C call made with `ContextID == NULL`. Use
    /// [`Context::with_plugin`] for the `cmsCreateContext` semantics of an *own*, initially
    /// empty chunk that shadows Context 0.
    pub fn new() -> Self {
        Self {
            adaptation_state: DEFAULT_ADAPTATION_STATE,
            alarm_codes: DEFAULT_ALARM_CODES,
            logger: LoggerSlot::Inherit,
            user_data: None,
            plugins: None,
        }
    }

    /// Create a context carrying opaque user data.
    ///
    /// Kept as the light-weight convenience it has always been: like [`Context::new`] it owns
    /// no chunks and so still sees Context 0's plugins. For the full `cmsCreateContext`
    /// contract — own chunks, Context 0 shadowed — use [`Context::with_plugin`].
    pub fn with_user_data(data: Arc<dyn Any + Send + Sync>) -> Self {
        let mut ctx = Self::new();
        ctx.user_data = Some(data);
        ctx
    }

    /// `cmsCreateContext(Plugin, UserData)` (`cmsplugin.c:824-887`) — a context with its **own**
    /// plugin chunk, holding just `plugin`.
    ///
    /// # This does not inherit Context 0
    /// C allocates the new chunks with `_cmsAlloc*PluginChunk(ctx, NULL)`, and the `src == NULL`
    /// arm installs the *pristine empty* chunk (e.g. `cmsgamma.c:114-118`) rather than a copy of
    /// Context 0. Since a non-NULL chunk stops `_cmsContextGetClientChunk` from ever reaching
    /// `globalContext`, a context built here sees **only** the plugins registered into it —
    /// anything registered globally with [`plugin::register_plugin`] is invisible to it.
    /// [`Context::dup`] is the constructor that snapshots an existing chunk set.
    ///
    /// The logger chunk is likewise allocated fresh and silent (`_cmsAllocLogErrorChunk(ctx,
    /// NULL)`, `cmserr.c:501-517`), so this context does not hear [`set_log_handler`] either.
    pub fn with_plugin(
        plugin: Plugin,
        user_data: Option<Arc<dyn Any + Send + Sync>>,
    ) -> Result<Self> {
        let chunk = plugin::empty_chunk();
        {
            let mut reg = chunk.write().unwrap_or_else(|e| e.into_inner());
            plugin::push_plugin(&mut reg, plugin);
        }
        Ok(Self {
            adaptation_state: DEFAULT_ADAPTATION_STATE,
            alarm_codes: DEFAULT_ALARM_CODES,
            logger: LoggerSlot::Own(None),
            user_data,
            plugins: Some(chunk),
        })
    }

    /// `cmsDupContext(ctx, NewUserData)` (`cmsplugin.c:893-…`) — duplicate this context,
    /// **snapshotting** its plugin and logger chunks.
    ///
    /// C allocates the copy's chunks with `_cmsAlloc*Chunk(ctx, src)`, whose `src != NULL` arm
    /// walks and copies the plugin list (`DupPluginCurvesList`, `cmsgamma.c:70-102`). The copy
    /// is taken *now*: registrations made into either context afterwards do not reach the other.
    /// This is what distinguishes `dup` from `Clone`, which shares the chunk instead.
    ///
    /// When this context has no chunk of its own, the snapshot is taken from Context 0 — C
    /// resolves the source through `_cmsGetContext` first, which maps a NULL id to
    /// `globalContext` (`cmsplugin.c:714-721`), so the duplicate still ends up owning a chunk.
    ///
    /// `new_user_data` of `None` keeps this context's user data, matching C's
    /// `NewUserData != NULL ? NewUserData : src->chunks[UserPtr]`.
    pub fn dup(&self, new_user_data: Option<Arc<dyn Any + Send + Sync>>) -> Self {
        let snapshot = match &self.plugins {
            Some(chunk) => chunk.read().unwrap_or_else(|e| e.into_inner()).clone(),
            None => plugin::snapshot_global(),
        };
        // The logger is a chunk too, so the duplicate resolves and freezes it the same way.
        let logger = match &self.logger {
            LoggerSlot::Own(handler) => LoggerSlot::Own(handler.clone()),
            LoggerSlot::Inherit => LoggerSlot::Own(
                default_logger().read().unwrap_or_else(|e| e.into_inner()).clone(),
            ),
        };
        Self {
            adaptation_state: self.adaptation_state,
            alarm_codes: self.alarm_codes,
            logger,
            user_data: new_user_data.or_else(|| self.user_data.clone()),
            plugins: Some(plugin::chunk_from(snapshot)),
        }
    }

    /// The opaque user data, if any (`cmsGetContextUserData`).
    pub fn user_data(&self) -> Option<&Arc<dyn Any + Send + Sync>> {
        self.user_data.as_ref()
    }

    /// Replace the opaque user data — the settable half of `cmsDupContext`'s `NewUserData`.
    pub fn set_user_data(&mut self, data: Option<Arc<dyn Any + Send + Sync>>) {
        self.user_data = data;
    }

    /// This context's plugin chunk, if it owns one (`ctx->chunks[…]`, NULL when absent).
    ///
    /// The lookup side of `_cmsContextGetClientChunk` lives in [`crate::plugin::read_reg`];
    /// this is the accessor it resolves step 1 with.
    pub(crate) fn plugin_chunk(&self) -> Option<&PluginChunk> {
        self.plugins.as_ref()
    }

    /// `cmsPluginTHR(ctx, plugin)` (`cmsplugin.c:546`) — register an extension into the chunk
    /// **this context resolves to**.
    ///
    /// The target is chosen by the same two-step as every lookup: `_cmsRegisterXPlugin` opens
    /// with `_cmsContextGetClientChunk(ContextID, …)` (`cmsgamma.c:129`) and prepends into the
    /// result. So the effect depends on whether this context owns a chunk:
    ///
    /// * **owns one** (built by [`Context::with_plugin`] or [`Context::dup`]) — the plugin goes
    ///   there and stays private to this context;
    /// * **owns none** — this context *is* Context 0, since `_cmsGetContext` maps a NULL id
    ///   straight to `globalContext` ("On 0, use global settings", `cmsplugin.c:719-721`). The
    ///   plugin therefore lands in the global registry and is visible process-wide, exactly as
    ///   `cmsPlugin(p)` would — the two are literally the same call in C.
    ///
    /// Registering does **not** grow a chunk for a context that lacks one. C has no such
    /// promotion: a context gets its own chunks only at construction, so `cmsCreateContext` /
    /// [`Context::with_plugin`] is the sole way to become isolated. Promoting here would both
    /// scope the plugin more narrowly than C does and silently shadow every already-registered
    /// global plugin away from this context.
    pub fn register_plugin(&mut self, plugin: Plugin) -> Result<()> {
        plugin::with_reg_mut(self.plugins.as_ref(), |reg| plugin::push_plugin(reg, plugin));
        Ok(())
    }

    /// `cmsUnregisterPluginsTHR(ctx)` (`cmsplugin.c:780-796`) — return the chunk **this context
    /// resolves to** to its pristine, built-in-only state.
    ///
    /// C implements this as `_cmsRegisterXPlugin(ctx, NULL)`, whose `Data == NULL` arm clears
    /// the chunk that `_cmsContextGetClientChunk` just returned (`cmsgamma.c:133-137`) — the
    /// same resolution [`Context::register_plugin`] uses. A context owning a chunk clears only
    /// its own; a context owning none is Context 0 and so clears the global registry, which is
    /// why `cmsUnregisterPlugins()` is defined as `cmsUnregisterPluginsTHR(NULL)`.
    pub fn unregister_plugins(&mut self) {
        plugin::with_reg_mut(self.plugins.as_ref(), |reg| *reg = Default::default());
    }

    /// Current observer adaptation state (`cmsSetAdaptationStateTHR` query side).
    pub fn adaptation_state(&self) -> f64 {
        self.adaptation_state
    }

    /// `cmsSetAdaptationStateTHR` (`cmsxform.c:57-73`) — set the observer adaptation state and
    /// return the PREVIOUS one.
    ///
    /// A negative argument is a pure QUERY: C stores only `if (d >= 0.0)`, and always returns the
    /// prior value, so `cmsSetAdaptationStateTHR(ctx, -1)` reads without writing. Dropping that
    /// guard would let `-1` be stored as a real adaptation state, which C can never hold.
    pub fn set_adaptation_state(&mut self, state: f64) -> f64 {
        let prev = self.adaptation_state;
        if state >= 0.0 {
            self.adaptation_state = state;
        }
        prev
    }

    /// Current proofing alarm codes (`cmsGetAlarmCodesTHR`).
    pub fn alarm_codes(&self) -> [u16; MAX_CHANNELS] {
        self.alarm_codes
    }

    /// Set the proofing alarm codes (`cmsSetAlarmCodesTHR`).
    pub fn set_alarm_codes(&mut self, codes: [u16; MAX_CHANNELS]) {
        self.alarm_codes = codes;
    }

    /// Install an error logger on **this** context (`cmsSetLogErrorHandlerTHR`,
    /// `cmserr.c:532-543`).
    ///
    /// This gives the context its own logger chunk, so it stops inheriting the Context-0
    /// handler installed by the free [`set_log_handler`]. Passing `None` writes C's silent
    /// `DefaultLogErrorHandlerFunction` into that chunk, which **silences this context** — it
    /// does not restore the global handler, because the chunk still shadows Context 0.
    pub fn set_log_handler(&mut self, handler: Option<LogHandler>) {
        self.logger = LoggerSlot::Own(handler);
    }

    /// Report an error to the effective logger (`cmsSignalError`).
    ///
    /// This is the side-channel form; prefer returning [`crate::Error`]. Resolution follows
    /// `_cmsContextGetClientChunk`: this context's own handler if it has a logger chunk,
    /// otherwise Context 0's (see the free [`set_log_handler`]). With neither installed it is
    /// a no-op, exactly like C's default handler.
    pub fn signal(&self, code: ErrorCode, message: &str) {
        match &self.logger {
            LoggerSlot::Own(Some(handler)) => handler(code, message),
            LoggerSlot::Own(None) => {}
            LoggerSlot::Inherit => {
                // Clone out of the lock before calling: a handler is free to reinstall itself
                // via `set_log_handler`, and holding the read guard across that would deadlock.
                let handler =
                    default_logger().read().unwrap_or_else(|e| e.into_inner()).clone();
                if let Some(handler) = handler {
                    handler(code, message);
                }
            }
        }
    }
}

impl Default for Context {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for Context {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Context")
            .field("adaptation_state", &self.adaptation_state)
            .field("alarm_codes", &self.alarm_codes)
            .field(
                "logger",
                &match &self.logger {
                    LoggerSlot::Inherit => "<context-0>",
                    LoggerSlot::Own(Some(_)) => "<fn>",
                    LoggerSlot::Own(None) => "<silent>",
                },
            )
            .field("user_data", &self.user_data.as_ref().map(|_| "<any>"))
            .field("plugins", &self.plugins.as_ref().map(|_| "<own-chunk>"))
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// Serialises the tests that touch the process-global Context-0 logger.
    static LOGGER_LOCK: Mutex<()> = Mutex::new(());

    /// A handler that bumps `hits`, so a test can prove it did (or did not) fire.
    fn counting_handler(hits: &Arc<AtomicU32>) -> LogHandler {
        let hits = Arc::clone(hits);
        Arc::new(move |_code, _msg| {
            hits.fetch_add(1, Ordering::SeqCst);
        })
    }

    #[test]
    fn defaults_match_lcms() {
        let ctx = Context::new();
        assert_eq!(ctx.adaptation_state(), 1.0);
        assert_eq!(ctx.alarm_codes()[0], 0x7F00);
        assert_eq!(ctx.alarm_codes()[2], 0x7F00);
        assert_eq!(ctx.alarm_codes()[3], 0);
        assert_eq!(ctx.alarm_codes()[15], 0);
    }

    /// `cmsSetAdaptationStateTHR` (`cmsxform.c:57-73`) is BOTH a setter and a query: it always
    /// returns the previous value, and stores the new one only `if (d >= 0.0)`. A negative
    /// argument therefore reads without writing - the idiom C itself uses at `cmsxform.c:1306`
    /// to fill the adaptation array. Without the guard, `-1` would be stored as a real
    /// adaptation state, which C can never hold.
    #[test]
    fn negative_adaptation_state_queries_without_storing() {
        let mut ctx = Context::new();
        assert_eq!(ctx.adaptation_state(), 1.0);

        // A real value is stored, and the PREVIOUS one comes back.
        assert_eq!(ctx.set_adaptation_state(0.25), 1.0);
        assert_eq!(ctx.adaptation_state(), 0.25);

        // A negative argument reads only - the stored value must survive untouched.
        assert_eq!(ctx.set_adaptation_state(-1.0), 0.25, "must return the previous value");
        assert_eq!(ctx.adaptation_state(), 0.25, "a negative argument must not be stored");

        // Zero is a legitimate state (no adaptation), not a rejection.
        assert_eq!(ctx.set_adaptation_state(0.0), 0.25);
        assert_eq!(ctx.adaptation_state(), 0.0);
    }

    #[test]
    fn setters_roundtrip() {
        let mut ctx = Context::new();
        ctx.set_adaptation_state(0.0);
        assert_eq!(ctx.adaptation_state(), 0.0);
        let mut codes = [0u16; MAX_CHANNELS];
        codes[0] = 0x1234;
        ctx.set_alarm_codes(codes);
        assert_eq!(ctx.alarm_codes()[0], 0x1234);
    }

    #[test]
    fn logger_receives_signal() {
        let hits = Arc::new(AtomicU32::new(0));
        let mut ctx = Context::new();
        let hits_cb = Arc::clone(&hits);
        ctx.set_log_handler(Some(Arc::new(move |code, _msg| {
            assert_eq!(code, ErrorCode::Range);
            hits_cb.fetch_add(1, Ordering::SeqCst);
        })));
        ctx.signal(ErrorCode::Range, "out of range");
        // No-op when cleared.
        ctx.set_log_handler(None);
        ctx.signal(ErrorCode::Range, "ignored");
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn user_data_retrievable() {
        let ctx = Context::with_user_data(Arc::new(42u32));
        let got = ctx.user_data().unwrap().downcast_ref::<u32>().copied();
        assert_eq!(got, Some(42));
    }

    #[test]
    fn user_data_is_settable_after_construction() {
        let mut ctx = Context::new();
        assert!(ctx.user_data().is_none());
        ctx.set_user_data(Some(Arc::new(7u32)));
        assert_eq!(ctx.user_data().and_then(|d| d.downcast_ref::<u32>().copied()), Some(7));
        ctx.set_user_data(None);
        assert!(ctx.user_data().is_none());
    }

    /// `cmsDupContext`'s `NewUserData` is "keep the parent's when NULL"
    /// (`NewUserData != NULL ? NewUserData : src->chunks[UserPtr]`).
    #[test]
    fn dup_user_data_falls_back_to_the_parent() {
        let parent = Context::with_user_data(Arc::new(1u32));

        let kept = parent.dup(None);
        assert_eq!(kept.user_data().and_then(|d| d.downcast_ref::<u32>().copied()), Some(1));

        let replaced = parent.dup(Some(Arc::new(2u32)));
        assert_eq!(replaced.user_data().and_then(|d| d.downcast_ref::<u32>().copied()), Some(2));
    }

    /// `cmsSetLogErrorHandler` (`cmserr.c:546-549`) writes Context 0's logger chunk, and a
    /// context with no chunk of its own resolves through it — `_cmsContextGetClientChunk`'s
    /// fallback arm. Before this existed, such a context was permanently silent.
    #[test]
    fn global_logger_reaches_a_context_without_its_own() {
        let _g = LOGGER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let hits = Arc::new(AtomicU32::new(0));
        set_log_handler(Some(counting_handler(&hits)));

        Context::new().signal(ErrorCode::Range, "heard via Context 0");
        assert_eq!(hits.load(Ordering::SeqCst), 1);

        // Clearing it restores C's silent default handler.
        set_log_handler(None);
        Context::new().signal(ErrorCode::Range, "ignored");
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }

    /// A context that owns a logger chunk stops consulting Context 0, exactly as for plugins.
    #[test]
    fn own_logger_shadows_the_global() {
        let _g = LOGGER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let global_hits = Arc::new(AtomicU32::new(0));
        let own_hits = Arc::new(AtomicU32::new(0));
        set_log_handler(Some(counting_handler(&global_hits)));

        let mut ctx = Context::new();
        ctx.set_log_handler(Some(counting_handler(&own_hits)));
        ctx.signal(ErrorCode::Range, "mine");

        assert_eq!(own_hits.load(Ordering::SeqCst), 1);
        assert_eq!(global_hits.load(Ordering::SeqCst), 0, "Context 0 must not also fire");

        set_log_handler(None);
    }

    /// `cmsSetLogErrorHandlerTHR(ctx, NULL)` installs the silent default *into the context's own
    /// chunk* (`cmserr.c:532-543`). It silences that context; it does not re-expose Context 0's
    /// handler, because the chunk still shadows it.
    #[test]
    fn clearing_a_context_logger_silences_it_rather_than_unshadowing() {
        let _g = LOGGER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let global_hits = Arc::new(AtomicU32::new(0));
        set_log_handler(Some(counting_handler(&global_hits)));

        let mut ctx = Context::new();
        ctx.set_log_handler(None);
        ctx.signal(ErrorCode::Range, "must go nowhere");

        assert_eq!(
            global_hits.load(Ordering::SeqCst),
            0,
            "an own (silent) chunk must not fall back to Context 0"
        );

        set_log_handler(None);
    }

    /// `cmsCreateContext` allocates its logger chunk with `src == NULL`
    /// (`_cmsAllocLogErrorChunk`, `cmserr.c:501-517`), i.e. the silent default — so such a
    /// context does not inherit the Context-0 handler either.
    #[test]
    fn with_plugin_context_does_not_inherit_the_global_logger() {
        let _g = LOGGER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let hits = Arc::new(AtomicU32::new(0));
        set_log_handler(Some(counting_handler(&hits)));

        let ctx = Context::with_plugin(
            crate::plugin::Plugin::Optimization(|_, _, _, _, _| Ok(false)),
            None,
        )
        .unwrap();
        ctx.signal(ErrorCode::Range, "must go nowhere");
        assert_eq!(hits.load(Ordering::SeqCst), 0);

        set_log_handler(None);
    }

    /// The logger is a chunk like any other, so `cmsDupContext` freezes it: the duplicate keeps
    /// the handler that was effective at dup time, not whatever Context 0 holds later.
    #[test]
    fn dup_freezes_the_inherited_logger() {
        let _g = LOGGER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let first = Arc::new(AtomicU32::new(0));
        let second = Arc::new(AtomicU32::new(0));
        set_log_handler(Some(counting_handler(&first)));

        let snapshot = Context::new().dup(None);
        set_log_handler(Some(counting_handler(&second)));

        snapshot.signal(ErrorCode::Range, "to the frozen handler");
        assert_eq!(first.load(Ordering::SeqCst), 1);
        assert_eq!(second.load(Ordering::SeqCst), 0, "dup must not track Context 0");

        set_log_handler(None);
    }

    // Compile-time proof that a context is shareable across threads.
    const _: fn() = || {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Context>();
    };
}
