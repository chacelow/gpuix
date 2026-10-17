use super::*;

type Events = Arc<parking_lot::Mutex<Vec<(u64, String)>>>;

fn add_input(tree: &mut RetainedTree, id: u64, auto_focus: bool) {
    tree.create_element(id, "input".into());
    tree.set_custom_prop(id, "autoFocus".into(), serde_json::json!(auto_focus));
    tree.set_event_listener(id, "focus".into(), true);
    tree.set_event_listener(id, "blur".into(), true);
    tree.append_child(1, id);
}

fn fixture(cx: &mut gpui::TestAppContext, input: bool) -> (gpui::WindowHandle<GpuixView>, Events) {
    cx.update(crate::custom_elements::input::init);
    let mut tree = RetainedTree::new();
    tree.create_element(1, "div".into());
    tree.set_root(1);
    let events: Events = Arc::default();
    let recorded = events.clone();
    let callback: EventCallback = Arc::new(move |event| {
        recorded
            .lock()
            .push((event.element_id as u64, event.event_type));
    });
    let window = cx.add_window(move |window, _| {
        window.activate_window();
        GpuixView::new(
            Arc::new(Mutex::new(tree)),
            Some(callback),
            "focus regression".into(),
            SharedSelection::default(),
        )
    });
    cx.run_until_parked();
    if input {
        window
            .update(cx, |view, _, cx| {
                let mut tree = view.tree.lock().unwrap();
                add_input(&mut tree, 2, true);
                add_input(&mut tree, 3, false);
                cx.notify();
            })
            .unwrap();
        cx.run_until_parked();
    }
    (window, events)
}

fn assert_events(events: &Events, expected: &[(u64, &str)]) {
    // Different handles have independent subscriptions; only each handle's
    // event order is contractual, not blur-vs-focus order across handles.
    let actual = events.lock();
    let mut ids: std::collections::BTreeSet<_> = actual.iter().map(|(id, _)| *id).collect();
    ids.extend(expected.iter().map(|(id, _)| *id));
    for id in ids {
        let actual: Vec<_> = actual
            .iter()
            .filter(|(target, _)| *target == id)
            .map(|(_, event)| event.as_str())
            .collect();
        let expected: Vec<_> = expected
            .iter()
            .filter(|(target, _)| *target == id)
            .map(|(_, event)| *event)
            .collect();
        assert_eq!(actual, expected, "element {id}");
    }
}

fn tab_away(window: gpui::WindowHandle<GpuixView>, cx: &mut gpui::TestAppContext) {
    cx.simulate_keystrokes(window.into(), "tab");
    // The JS host owns Tab's default and invokes this native navigation API.
    window
        .update(cx, |_, window, cx| window.focus_next(cx))
        .unwrap();
    cx.run_until_parked();
}

#[gpui::test]
fn autofocus_emits_focus_and_tab_emits_blur(cx: &mut gpui::TestAppContext) {
    let (window, events) = fixture(cx, true);
    assert_events(&events, &[(2, "focus")]);
    tab_away(window, cx);
    assert_events(&events, &[(2, "focus"), (2, "blur"), (3, "focus")]);
}

#[gpui::test]
fn autofocus_in_a_later_batch_emits_focus(cx: &mut gpui::TestAppContext) {
    let (window, events) = fixture(cx, false);
    window
        .update(cx, |view, _, cx| {
            add_input(&mut view.tree.lock().unwrap(), 2, true);
            cx.notify();
        })
        .unwrap();
    cx.run_until_parked();
    assert_events(&events, &[(2, "focus")]);
}

#[gpui::test]
fn deferred_focus_element_emits_focus_when_created(cx: &mut gpui::TestAppContext) {
    let (window, events) = fixture(cx, false);
    window
        .update(cx, |view, window, cx| view.request_focus(2, window, cx))
        .unwrap();
    cx.run_until_parked();
    assert_events(&events, &[]);
    window
        .update(cx, |view, _, cx| {
            add_input(&mut view.tree.lock().unwrap(), 2, false);
            cx.notify();
        })
        .unwrap();
    cx.run_until_parked();
    assert_events(&events, &[(2, "focus")]);
}

#[gpui::test]
fn existing_focus_element_emits_once_and_autofocus_never_refocuses(cx: &mut gpui::TestAppContext) {
    let (window, events) = fixture(cx, true);
    events.lock().clear();
    window
        .update(cx, |view, window, cx| view.request_focus(3, window, cx))
        .unwrap();
    cx.run_until_parked();
    assert_events(&events, &[(2, "blur"), (3, "focus")]);
    for _ in 0..3 {
        window
            .update(cx, |view, window, cx| {
                view.request_focus(3, window, cx);
                window.refresh();
            })
            .unwrap();
        cx.run_until_parked();
    }
    assert_events(&events, &[(2, "blur"), (3, "focus")]);
    window
        .update(cx, |view, window, cx| {
            assert!(view.focus_handles[&3].is_focused(window));
            view.request_focus(2, window, cx);
        })
        .unwrap();
    cx.run_until_parked();
    assert_events(
        &events,
        &[(2, "blur"), (3, "focus"), (3, "blur"), (2, "focus")],
    );
}

#[gpui::test]
fn explicit_request_outranks_autofocus(cx: &mut gpui::TestAppContext) {
    let (window, events) = fixture(cx, false);
    window
        .update(cx, |view, window, cx| {
            view.request_focus(3, window, cx);
            let mut tree = view.tree.lock().unwrap();
            add_input(&mut tree, 2, true);
            add_input(&mut tree, 3, false);
        })
        .unwrap();
    cx.run_until_parked();
    assert_events(&events, &[(3, "focus")]);
}

#[gpui::test]
fn later_user_focus_cancels_deferred_request(cx: &mut gpui::TestAppContext) {
    let (window, events) = fixture(cx, true);
    events.lock().clear();
    window
        .update(cx, |view, window, cx| view.request_focus(4, window, cx))
        .unwrap();
    cx.run_until_parked();
    tab_away(window, cx);
    window
        .update(cx, |view, _, cx| {
            add_input(&mut view.tree.lock().unwrap(), 4, false);
            cx.notify();
        })
        .unwrap();
    cx.run_until_parked();
    assert_events(&events, &[(2, "blur"), (3, "focus")]);
}

#[gpui::test]
fn autofocus_survives_retiring_the_previous_focus_handle(cx: &mut gpui::TestAppContext) {
    let (window, events) = fixture(cx, false);
    window
        .update(cx, |view, _, cx| {
            let mut tree = view.tree.lock().unwrap();
            tree.create_element(2, "div".into());
            tree.set_custom_prop(2, "tabIndex".into(), serde_json::json!(0));
            tree.set_event_listener(2, "focus".into(), true);
            tree.set_event_listener(2, "blur".into(), true);
            tree.append_child(1, 2);
            cx.notify();
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |view, window, cx| view.request_focus(2, window, cx))
        .unwrap();
    cx.run_until_parked();
    assert_events(&events, &[(2, "focus")]);
    events.lock().clear();
    window
        .update(cx, |view, _, cx| {
            let mut tree = view.tree.lock().unwrap();
            tree.set_custom_prop(2, "tabIndex".into(), serde_json::Value::Null);
            tree.set_event_listener(2, "focus".into(), false);
            tree.set_event_listener(2, "blur".into(), false);
            add_input(&mut tree, 3, true);
            tree.append_child(2, 3);
            cx.notify();
        })
        .unwrap();
    cx.run_until_parked();
    assert_events(&events, &[(3, "focus")]);
    window
        .update(cx, |view, window, _| {
            assert!(!view.focus_handles.contains_key(&2));
            assert!(view.focus_handles[&3].is_focused(window));
        })
        .unwrap();
}

#[gpui::test]
fn existing_explicit_request_outranks_same_batch_autofocus(cx: &mut gpui::TestAppContext) {
    let (window, events) = fixture(cx, true);
    events.lock().clear();
    window
        .update(cx, |view, window, cx| {
            view.request_focus(3, window, cx);
            add_input(&mut view.tree.lock().unwrap(), 4, true);
        })
        .unwrap();
    cx.run_until_parked();
    assert_events(&events, &[(2, "blur"), (3, "focus")]);
    window
        .update(cx, |view, window, _| {
            assert!(view.focus_handles[&3].is_focused(window));
        })
        .unwrap();
}

#[gpui::test]
fn tab_after_existing_explicit_request_wins(cx: &mut gpui::TestAppContext) {
    let (window, events) = fixture(cx, true);
    events.lock().clear();
    window
        .update(cx, |view, window, cx| {
            view.request_focus(3, window, cx);
            window.focus_next(cx);
            add_input(&mut view.tree.lock().unwrap(), 4, true);
        })
        .unwrap();
    cx.run_until_parked();
    assert_events(&events, &[]);
    window
        .update(cx, |view, window, _| {
            assert!(view.focus_handles[&2].is_focused(window));
        })
        .unwrap();
}

#[gpui::test]
fn pending_request_survives_renderer_autofocus_in_an_earlier_batch(cx: &mut gpui::TestAppContext) {
    let (window, events) = fixture(cx, false);
    window
        .update(cx, |view, window, cx| {
            view.request_focus(3, window, cx);
            add_input(&mut view.tree.lock().unwrap(), 2, true);
        })
        .unwrap();
    cx.run_until_parked();
    assert_events(&events, &[(2, "focus")]);
    window
        .update(cx, |view, _, cx| {
            add_input(&mut view.tree.lock().unwrap(), 3, false);
            cx.notify();
        })
        .unwrap();
    cx.run_until_parked();
    assert_events(&events, &[(2, "focus"), (2, "blur"), (3, "focus")]);
}

#[gpui::test]
fn existing_input_with_new_focus_listeners_emits_once(cx: &mut gpui::TestAppContext) {
    let (window, events) = fixture(cx, false);
    window
        .update(cx, |view, _, cx| {
            let mut tree = view.tree.lock().unwrap();
            add_input(&mut tree, 2, true);
            add_input(&mut tree, 3, false);
            tree.set_event_listener(3, "focus".into(), false);
            tree.set_event_listener(3, "blur".into(), false);
            cx.notify();
        })
        .unwrap();
    cx.run_until_parked();
    assert_events(&events, &[(2, "focus")]);
    events.lock().clear();
    window
        .update(cx, |view, window, cx| {
            {
                let mut tree = view.tree.lock().unwrap();
                tree.set_event_listener(3, "focus".into(), true);
                tree.set_event_listener(3, "blur".into(), true);
            }
            view.request_focus(3, window, cx);
        })
        .unwrap();
    cx.run_until_parked();
    assert_events(&events, &[(2, "blur"), (3, "focus")]);
    tab_away(window, cx);
    assert_events(
        &events,
        &[(2, "blur"), (3, "focus"), (3, "blur"), (2, "focus")],
    );
}

#[gpui::test]
fn pending_request_survives_retiring_owner_before_target_creation(cx: &mut gpui::TestAppContext) {
    let (window, events) = fixture(cx, false);
    window
        .update(cx, |view, _, cx| {
            let mut tree = view.tree.lock().unwrap();
            tree.create_element(2, "div".into());
            tree.set_custom_prop(2, "tabIndex".into(), serde_json::json!(0));
            tree.set_event_listener(2, "focus".into(), true);
            tree.set_event_listener(2, "blur".into(), true);
            tree.append_child(1, 2);
            cx.notify();
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |view, window, cx| view.request_focus(2, window, cx))
        .unwrap();
    cx.run_until_parked();
    events.lock().clear();
    window
        .update(cx, |view, window, cx| view.request_focus(4, window, cx))
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |view, _, cx| {
            view.tree.lock().unwrap().destroy_element(2);
            cx.notify();
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |view, _, cx| {
            add_input(&mut view.tree.lock().unwrap(), 4, false);
            cx.notify();
        })
        .unwrap();
    cx.run_until_parked();
    assert_events(&events, &[(4, "focus")]);
}

#[gpui::test]
fn new_source_blur_listener_observes_existing_target_request(cx: &mut gpui::TestAppContext) {
    let (window, events) = fixture(cx, false);
    window
        .update(cx, |view, _, cx| {
            let mut tree = view.tree.lock().unwrap();
            add_input(&mut tree, 2, true);
            add_input(&mut tree, 3, false);
            tree.set_event_listener(2, "blur".into(), false);
            cx.notify();
        })
        .unwrap();
    cx.run_until_parked();
    assert_events(&events, &[(2, "focus")]);
    events.lock().clear();
    window
        .update(cx, |view, window, cx| {
            view.tree
                .lock()
                .unwrap()
                .set_event_listener(2, "blur".into(), true);
            view.request_focus(3, window, cx);
        })
        .unwrap();
    cx.run_until_parked();
    assert_events(&events, &[(2, "blur"), (3, "focus")]);
}

#[gpui::test]
fn closing_window_releases_pending_focus_owner(cx: &mut gpui::TestAppContext) {
    let (window, events) = fixture(cx, true);
    events.lock().clear();
    let view = window
        .update(cx, |view, window, cx| {
            view.request_focus(4, window, cx);
            let view = cx.weak_entity();
            window.remove_window();
            view
        })
        .unwrap();
    cx.run_until_parked();
    assert!(
        view.upgrade().is_none(),
        "pending focus must not keep a closed window's owner alive"
    );
    assert!(!events
        .lock()
        .iter()
        .any(|(id, event)| *id == 4 && event == "focus"));
}
