use super::windows_protocol::{CloseProtocol, FrameKind, FrameProtocol, FrameTicket, Token};
use super::{GpuixView, UiCommand};
use futures::channel::mpsc;
use napi::bindgen_prelude::*;
use napi::threadsafe_function::{ThreadsafeFunction, ThreadsafeFunctionCallMode};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use parking_lot::Mutex;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::time::Instant;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{IsIconic, IsWindowVisible};

#[derive(Default)]
pub(super) struct Owner {
    pub frames: Mutex<FrameProtocol>,
    pub close: Mutex<CloseProtocol>,
    shutdown: AtomicBool,
}

impl Owner {
    pub fn shutdown(&self) {
        if self.shutdown.swap(true, Ordering::AcqRel) { return; }
        self.frames.lock().shutdown();
        self.close.lock().replace(false);
    }
}

pub(super) struct FrameDispatch {
    ticket: FrameTicket,
    timestamp_ms: f64,
}

type FrameTsfn = ThreadsafeFunction<FrameDispatch, (), (), napi::Status, false, true, 1>;
type CloseTsfn = ThreadsafeFunction<Token, (), (), napi::Status, false, true, 1>;

pub(super) struct FrameHandler {
    pub generation: u64,
    callback: FrameTsfn,
}

pub(super) struct CloseHandler {
    pub generation: u64,
    callback: CloseTsfn,
}

pub(super) fn frame_handler(
    env: &Env,
    callback: FunctionRef<f64, ()>,
    owner: Arc<Owner>,
    sender: mpsc::UnboundedSender<UiCommand>,
) -> Result<FrameHandler> {
    // The trampoline performs cancellation before invoking the real JS listener.
    // Neither its callback nor its completion ever waits for the UI thread.
    let noop = env.create_function_from_closure::<(), (), _>("gpuixFrameDispatch", |_| Ok(()))?;
    let callback = noop.build_threadsafe_function::<FrameDispatch>()
        .weak::<true>().max_queue_size::<1>()
        .build_callback(move |ctx| {
            let dispatch = ctx.value;
            let valid = owner.frames.lock().valid(dispatch.ticket);
            if valid {
                if let Err(error) = callback.borrow_back(&ctx.env)
                    .and_then(|callback| callback.call(dispatch.timestamp_ms)) {
                    log::error!("Windows frame callback failed: {error}");
                }
            }
            sender.unbounded_send(UiCommand::WindowsFrameComplete(dispatch.ticket)).ok();
            Ok(())
        })?;
    Ok(FrameHandler { generation: 0, callback })
}

pub(super) fn close_handler(
    env: &Env,
    callback: FunctionRef<(), bool>,
    owner: Arc<Owner>,
    sender: mpsc::UnboundedSender<UiCommand>,
) -> Result<CloseHandler> {
    let noop = env.create_function_from_closure::<(), (), _>("gpuixCloseDispatch", |_| Ok(()))?;
    let callback = noop.build_threadsafe_function::<Token>()
        .weak::<true>().max_queue_size::<1>()
        .build_callback(move |ctx| {
            let token = ctx.value;
            let valid = owner.close.lock().valid(token);
            let allow = valid && match callback.borrow_back(&ctx.env)
                .and_then(|callback| callback.call(())) {
                Ok(allow) => allow,
                Err(error) => {
                    log::error!("Windows close callback failed: {error}");
                    false
                }
            };
            sender.unbounded_send(UiCommand::WindowsCloseComplete { token, allow }).ok();
            Ok(())
        })?;
    Ok(CloseHandler { generation: 0, callback })
}

struct Bridge {
    owner: Arc<Owner>,
    animation: Option<FrameHandler>,
    resume: Option<FrameHandler>,
    close: Option<CloseHandler>,
    close_requested: bool,
    animation_scheduled: bool,
    origin: Instant,
    last_resume_generation: u64,
    last_active: bool,
    last_visible: bool,
    activation: Option<gpui::Subscription>,
}

thread_local! {
    static BRIDGE: RefCell<Option<Rc<RefCell<Bridge>>>> = const { RefCell::new(None) };
}

fn bridge() -> Option<Rc<RefCell<Bridge>>> {
    BRIDGE.with(|stored| stored.borrow().clone())
}

impl Bridge {
    fn observe(&mut self, window: &gpui::Window) {
        let generation = window.resume_generation();
        let active = window.is_window_active();
        let visible = visible(window);
        if (visible && (generation != self.last_resume_generation || !self.last_visible))
            || (active && !self.last_active) {
            self.owner.frames.lock().request_resume();
        }
        self.last_resume_generation = generation;
        self.last_active = active;
        self.last_visible = visible;
    }

    fn dispatch(&mut self, kind: FrameKind) {
        let handler = match kind {
            FrameKind::Animation => self.animation.as_ref(),
            FrameKind::Resume => self.resume.as_ref(),
        };
        let Some(handler) = handler else { return };
        let ticket = {
            let mut state = self.owner.frames.lock();
            if !state.current(kind, handler.generation) { return; }
            state.begin(kind)
        };
        let Some(ticket) = ticket else { return };
        let status = handler.callback.call(FrameDispatch {
            ticket,
            timestamp_ms: self.origin.elapsed().as_secs_f64() * 1000.0,
        }, ThreadsafeFunctionCallMode::NonBlocking);
        if status != napi::Status::Ok {
            // Closing/queue-full cannot leave a presentation gate waiting on an ack.
            let mut state = self.owner.frames.lock();
            state.complete(ticket);
            state.replace(kind, false);
            log::error!("Windows frame dispatch failed: {status}");
        }
    }
}
fn visible(window: &gpui::Window) -> bool {
    let Ok(handle) = HasWindowHandle::window_handle(window) else { return false };
    let RawWindowHandle::Win32(handle) = handle.as_raw() else { return false };
    let hwnd = HWND(handle.hwnd.get() as *mut std::ffi::c_void);
    unsafe { IsWindowVisible(hwnd).as_bool() && !IsIconic(hwnd).as_bool() }
}


pub(super) fn install(
    owner: Arc<Owner>,
    window: &mut gpui::Window,
    cx: &mut gpui::Context<GpuixView>,
) {
    let activation = cx.observe_window_activation(window, |_, window, _| {
        if let Some(bridge) = bridge() { bridge.borrow_mut().observe(window); }
        window.refresh();
    });
    let value = Rc::new(RefCell::new(Bridge {
        owner,
        animation: None,
        resume: None,
        close: None,
        close_requested: false,
        animation_scheduled: false,
        origin: Instant::now(),
        last_resume_generation: window.resume_generation(),
        last_active: window.is_window_active(),
        last_visible: visible(window),
        activation: Some(activation),
    }));
    BRIDGE.with(|stored| *stored.borrow_mut() = Some(value));
    window.on_before_present(Some(Box::new(|window, _cx| {
        let Some(bridge) = bridge() else { return true };
        let mut bridge = bridge.borrow_mut();
        bridge.observe(window);
        bridge.dispatch(FrameKind::Resume);
        let deferred = bridge.owner.frames.lock().defer_present();
        !deferred
    })));
    window.on_window_should_close(cx, |_, _| request_close());
}

pub(super) fn set_frame_handler(
    kind: FrameKind,
    handler: Option<FrameHandler>,
    generation: u64,
    window: &mut gpui::Window,
) {
    if let Some(bridge) = bridge() {
        let mut bridge = bridge.borrow_mut();
        if !bridge.owner.frames.lock().current(kind, generation) { return; }
        match kind {
            FrameKind::Animation => bridge.animation = handler,
            FrameKind::Resume => {
                bridge.resume = handler;
                bridge.last_resume_generation = window.resume_generation();
                bridge.last_active = window.is_window_active();
                bridge.last_visible = visible(window);
            }
        }
    }
    window.resume_presentation();
    schedule_animation(window);
}

pub(super) fn schedule_animation(window: &gpui::Window) {
    // Named `shared`, not `bridge`: the closure below calls the `bridge()` function again, and a
    // local named `bridge` would shadow it (E0618).
    let Some(shared) = bridge() else { return };
    let mut value = shared.borrow_mut();
    if value.animation_scheduled || !value.owner.frames.lock().needs_animation() { return; }
    value.animation_scheduled = true;
    drop(value);
    window.on_next_frame(|window, _cx| {
        let Some(bridge) = bridge() else { return };
        let mut bridge = bridge.borrow_mut();
        bridge.animation_scheduled = false;
        bridge.observe(window);
        // Resume always takes priority over the ordinary frame callback.
        bridge.dispatch(FrameKind::Resume);
        bridge.dispatch(FrameKind::Animation);
    });
}

pub(super) fn complete_frame(ticket: FrameTicket, window: &mut gpui::Window, cx: &mut gpui::App) {
    let completed = bridge().is_some_and(|bridge| {
        let value = bridge.borrow();
        let completed = value.owner.frames.lock().complete(ticket);
        completed
    });
    if completed {
        if let Some(root) = window.root::<GpuixView>().flatten() { cx.notify(root.entity_id()); }
        window.resume_presentation();
        schedule_animation(window);
    }
}

pub(super) fn set_close_handler(handler: CloseHandler) {
    let requested = if let Some(bridge) = bridge() {
        let mut bridge = bridge.borrow_mut();
        let current = bridge.owner.close.lock().current(handler.generation);
        if !current { return; }
        bridge.close = Some(handler);
        bridge.close_requested
    } else { false };
    if requested { request_close(); }
}

fn request_close() -> bool {
    let Some(bridge) = bridge() else { return true };
    let mut bridge = bridge.borrow_mut();
    let owner = bridge.owner.clone();
    let mut close = owner.close.lock();
    if !close.registered() { return true; }
    bridge.close_requested = true;
    let Some(handler) = bridge.close.as_ref() else { return false };
    if !close.current(handler.generation) { return false; }
    if let Some(token) = close.begin() {
        let status = handler.callback.call(token, ThreadsafeFunctionCallMode::NonBlocking);
        if status != napi::Status::Ok {
            close.complete(token, false);
            log::error!("Windows close dispatch failed: {status}");
        }
        bridge.close_requested = false;
    } else {
        bridge.close_requested = !close.pending_is_current();
    }
    false
}

pub(super) fn complete_close(token: Token, allow: bool, window: &mut gpui::Window) {
    let allowed = bridge().is_some_and(|bridge| {
        let value = bridge.borrow();
        let allowed = value.owner.close.lock().complete(token, allow);
        allowed
    });
    if allowed {
        shutdown(window);
        window.remove_window();
    } else if bridge().is_some_and(|bridge| bridge.borrow().close_requested) {
        request_close();
    }
}

pub(super) fn shutdown(window: &mut gpui::Window) {
    clear();
    window.on_before_present(None);
    window.resume_presentation();
}

pub(super) fn clear() {
    BRIDGE.with(|stored| {
        if let Some(bridge) = stored.borrow_mut().take() {
            let mut bridge = bridge.borrow_mut();
            bridge.owner.shutdown();
            bridge.animation = None;
            bridge.resume = None;
            bridge.close = None;
            bridge.activation = None;
        }
    });
}
