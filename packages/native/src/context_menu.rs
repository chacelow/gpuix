//! Context-menu notification crosses interactive descendants, not later layers.
use std::{cell::RefCell, rc::Rc};

use gpui::{
    prelude::*, AnyElement, App, Bounds, DispatchPhase, Element, ElementId, GlobalElementId,
    Hitbox, HitboxBehavior, InspectorElementId, LayoutId, MouseButton, MouseDownEvent, Pixels,
    Window,
};

use crate::renderer::{emit_event_full, point_to_xy, EventCallback};

#[derive(Clone, Default)]
pub(crate) struct ContextMenus(Rc<RefCell<Vec<(u64, Hitbox)>>>);

impl ContextMenus {
    pub fn scope(&self, id: u64, child: AnyElement) -> AnyElement {
        Scope {
            id,
            child,
            menus: self.clone(),
        }
        .into_any_element()
    }

    pub fn frame_start(&self, callback: Option<EventCallback>) -> impl IntoElement {
        let menus = self.clone();
        gpui::canvas(
            |_, _, _| (),
            move |_, _, window, _| {
                menus.0.borrow_mut().clear();
                let menus = menus.clone();
                let callback = callback.clone();
                window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
                    if phase != DispatchPhase::Capture || event.button != MouseButton::Right {
                        return;
                    }
                    let target = menus.0.borrow().iter().rev().find_map(|(id, probe)| {
                        (probe
                            .bounds
                            .intersect(&probe.content_mask.bounds)
                            .contains(&event.position)
                            && probe.is_hovered(window))
                        .then_some(*id)
                    });
                    if let Some(id) = target {
                        let callback = callback.clone();
                        let event = event.clone();
                        // Keep mouseDownOutside and descendant mouseDown ahead of contextMenu.
                        cx.defer(move |_| {
                            emit_event_full(&callback, id, "contextMenu", |payload| {
                                let (x, y) = point_to_xy(event.position);
                                payload.x = Some(x);
                                payload.y = Some(y);
                                payload.button = Some(2);
                                payload.modifiers = Some(event.modifiers.into());
                                payload.click_count = Some(event.click_count as u32);
                                payload.is_right_click = Some(true);
                            });
                        });
                    }
                });
            },
        )
        .absolute()
        .size(gpui::px(0.0))
    }
}

struct Scope {
    id: u64,
    child: AnyElement,
    menus: ContextMenus,
}

impl IntoElement for Scope {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for Scope {
    type RequestLayoutState = ();
    type PrepaintState = Hitbox;

    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (self.child.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Hitbox {
        self.child.prepaint(window, cx);
        // After ordinary descendants, before later siblings and deferred layers.
        // Normal changes neither their hover eligibility nor wheel handling.
        // A separate identity cannot replace an ancestor's captured hitbox on repaint.
        window.with_global_id("context-menu-probe".into(), |_, window| {
            window.insert_hitbox(bounds, HitboxBehavior::Normal)
        })
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        probe: &mut Hitbox,
        window: &mut Window,
        cx: &mut App,
    ) {
        // Paint is non-speculative. Pre-order makes the last eligible scope deepest/frontmost.
        self.menus.0.borrow_mut().push((self.id, probe.clone()));
        self.child.paint(window, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::element_tree::EventPayload;
    use gpui::{point, px, size, MouseUpEvent, TestAppContext, VisualTestContext};
    use parking_lot::Mutex;
    use std::sync::Arc;

    struct Fixture {
        menus: ContextMenus,
        callback: Option<EventCallback>,
        block_scroll: bool,
        nested: bool,
        overlay: bool,
        clip: bool,
    }

    impl gpui::Render for Fixture {
        fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
            let callback = self.callback.clone();
            let mut child = gpui::div().id("button").w(px(40.0)).h(px(30.0));
            child = if self.block_scroll {
                child.occlude()
            } else {
                child.block_mouse_except_scroll()
            };
            for button in [MouseButton::Left, MouseButton::Right] {
                let callback = callback.clone();
                child = child.on_mouse_down(button, move |event, _, _| {
                    emit_event_full(&callback, 30, "mouseDown", |p| {
                        p.button = Some(crate::renderer::mouse_button_to_u32(event.button));
                    });
                });
            }
            let click_callback = callback.clone();
            child = child.on_mouse_up(MouseButton::Left, move |_, _, _| {
                emit_event_full(&click_callback, 30, "click", |_| {});
            });
            let child = if self.nested {
                self.menus.scope(
                    20,
                    gpui::div()
                        .w(px(80.0))
                        .h(px(60.0))
                        .child(child)
                        .into_any_element(),
                )
            } else {
                child.into_any_element()
            };
            let scope = self.menus.scope(
                10,
                gpui::div()
                    .absolute()
                    .left(px(20.0))
                    .top(px(20.0))
                    .w(px(120.0))
                    .h(px(100.0))
                    .child(child)
                    .into_any_element(),
            );
            let body = if self.clip {
                gpui::div()
                    .relative()
                    .w(px(80.0))
                    .h(px(80.0))
                    .overflow_hidden()
                    .child(scope)
                    .into_any_element()
            } else {
                scope
            };
            let mut root = gpui::div()
                .size_full()
                .child(self.menus.frame_start(callback))
                .child(body);
            if self.overlay {
                root = root.child(gpui::deferred(
                    gpui::anchored()
                        .position(point(px(20.0), px(20.0)))
                        .child(gpui::div().w(px(80.0)).h(px(60.0)).occlude()),
                ));
            }
            root
        }
    }

    fn fixture(
        cx: &mut TestAppContext,
        block_scroll: bool,
        nested: bool,
        overlay: bool,
        clip: bool,
    ) -> (gpui::WindowHandle<Fixture>, Arc<Mutex<Vec<EventPayload>>>) {
        let events = Arc::new(Mutex::new(Vec::new()));
        let collected = events.clone();
        let callback: EventCallback = Arc::new(move |event| collected.lock().push(event));
        let window = cx.open_window(size(px(320.0), px(240.0)), move |_, _| Fixture {
            menus: ContextMenus::default(),
            callback: Some(callback),
            block_scroll,
            nested,
            overlay,
            clip,
        });
        VisualTestContext::from_window(window.into(), cx).run_until_parked();
        (window, events)
    }

    fn press(
        cx: &mut TestAppContext,
        window: gpui::WindowHandle<Fixture>,
        x: f32,
        y: f32,
        button: MouseButton,
    ) {
        VisualTestContext::from_window(window.into(), cx).simulate_event(MouseDownEvent {
            position: point(px(x), px(y)),
            button,
            click_count: 1,
            modifiers: gpui::Modifiers {
                shift: true,
                ..Default::default()
            },
            first_mouse: false,
        });
        cx.run_until_parked();
    }

    fn context_ids(events: &Arc<Mutex<Vec<EventPayload>>>) -> Vec<u64> {
        events
            .lock()
            .iter()
            .filter(|event| event.event_type == "contextMenu")
            .map(|event| event.element_id as u64)
            .collect()
    }

    fn assert_descendant_delivery(cx: &mut TestAppContext, block_scroll: bool) {
        let (window, events) = fixture(cx, block_scroll, false, false, false);
        press(cx, window, 30.0, 30.0, MouseButton::Right);
        let events = events.lock();
        assert_eq!(
            events
                .iter()
                .map(|event| event.event_type.as_str())
                .collect::<Vec<_>>(),
            vec!["mouseDown", "contextMenu"]
        );
        assert_eq!(events[0].button, Some(2));
        assert_eq!(events[1].element_id, 10.0);
        assert_eq!((events[1].x, events[1].y), (Some(30.0), Some(30.0)));
        assert!(events[1].modifiers.as_ref().unwrap().shift);
        assert_eq!(events[1].button, Some(2));
        assert_eq!(events[1].is_right_click, Some(true));
        assert!(events[1].key.is_none());
        assert!(events[1].keystroke_id.is_none());
    }

    #[gpui::test]
    fn button_descendant_preserves_mouse_down_and_notifies_once(cx: &mut TestAppContext) {
        assert_descendant_delivery(cx, false);
    }

    #[gpui::test]
    fn block_mouse_descendant_preserves_mouse_down_and_notifies_once(cx: &mut TestAppContext) {
        assert_descendant_delivery(cx, true);
    }

    #[gpui::test]
    fn nearest_scope_wins_and_each_press_notifies_again(cx: &mut TestAppContext) {
        let (window, events) = fixture(cx, true, true, false, false);
        press(cx, window, 30.0, 30.0, MouseButton::Right);
        press(cx, window, 30.0, 30.0, MouseButton::Right);
        assert_eq!(context_ids(&events), vec![20, 20]);
    }

    #[gpui::test]
    fn primary_click_retains_mouse_down_and_click_without_context_menu(cx: &mut TestAppContext) {
        let (window, events) = fixture(cx, false, false, false, false);
        press(cx, window, 30.0, 30.0, MouseButton::Left);
        VisualTestContext::from_window(window.into(), cx).simulate_event(MouseUpEvent {
            position: point(px(30.0), px(30.0)),
            button: MouseButton::Left,
            click_count: 1,
            modifiers: Default::default(),
        });
        cx.run_until_parked();
        assert_eq!(context_ids(&events), Vec::<u64>::new());
        assert_eq!(
            events
                .lock()
                .iter()
                .map(|event| event.event_type.as_str())
                .collect::<Vec<_>>(),
            vec!["mouseDown", "click"]
        );
    }

    #[gpui::test]
    fn point_outside_scope_bounds_notifies_nothing(cx: &mut TestAppContext) {
        let (window, events) = fixture(cx, true, false, false, false);
        press(cx, window, 200.0, 200.0, MouseButton::Right);
        assert_eq!(context_ids(&events), Vec::<u64>::new());
        press(cx, window, 130.0, 100.0, MouseButton::Right);
        assert_eq!(context_ids(&events), vec![10]);
    }

    #[gpui::test]
    fn deferred_occluding_layer_blocks_covered_scope_only(cx: &mut TestAppContext) {
        let (window, events) = fixture(cx, false, true, true, false);
        press(cx, window, 30.0, 30.0, MouseButton::Right);
        assert_eq!(context_ids(&events), Vec::<u64>::new());
        press(cx, window, 130.0, 100.0, MouseButton::Right);
        assert_eq!(context_ids(&events), vec![10]);
    }

    #[gpui::test]
    fn ancestor_content_mask_clips_the_context_scope(cx: &mut TestAppContext) {
        let (window, events) = fixture(cx, true, false, false, true);
        press(cx, window, 100.0, 30.0, MouseButton::Right);
        assert_eq!(context_ids(&events), Vec::<u64>::new());
        press(cx, window, 30.0, 30.0, MouseButton::Right);
        assert_eq!(context_ids(&events), vec![10]);
    }
}
