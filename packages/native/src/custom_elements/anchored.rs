/// Anchored custom element for deferred, trigger-relative floating layers.
use super::{CustomElement, CustomElementFactory, CustomRenderContext};

pub struct AnchoredFactory;

impl CustomElementFactory for AnchoredFactory {
    fn element_type(&self) -> &str {
        "anchored"
    }

    fn create(&self, _id: u64) -> Box<dyn CustomElement> {
        Box::new(AnchoredElement::default())
    }
}

#[derive(Debug, Clone, Copy)]
enum AnchorPoint {
    TopLeft,
    TopCenter,
    TopRight,
    RightCenter,
    BottomRight,
    BottomCenter,
    BottomLeft,
    LeftCenter,
}

impl AnchorPoint {
    fn from_str(value: &str) -> Option<Self> {
        Some(match value {
            "topLeft" => Self::TopLeft,
            "topCenter" => Self::TopCenter,
            "topRight" => Self::TopRight,
            "rightCenter" => Self::RightCenter,
            "bottomRight" => Self::BottomRight,
            "bottomCenter" => Self::BottomCenter,
            "bottomLeft" => Self::BottomLeft,
            "leftCenter" => Self::LeftCenter,
            _ => return None,
        })
    }

    fn as_gpui(self) -> gpui::Anchor {
        match self {
            Self::TopLeft => gpui::Anchor::TopLeft,
            Self::TopCenter => gpui::Anchor::TopCenter,
            Self::TopRight => gpui::Anchor::TopRight,
            Self::RightCenter => gpui::Anchor::RightCenter,
            Self::BottomRight => gpui::Anchor::BottomRight,
            Self::BottomCenter => gpui::Anchor::BottomCenter,
            Self::BottomLeft => gpui::Anchor::BottomLeft,
            Self::LeftCenter => gpui::Anchor::LeftCenter,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
enum Side {
    Top,
    Right,
    #[default]
    Bottom,
    Left,
}

impl Side {
    fn from_str(value: &str) -> Self {
        match value {
            "top" => Self::Top,
            "right" => Self::Right,
            "left" => Self::Left,
            _ => Self::Bottom,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
enum Alignment {
    #[default]
    Start,
    Center,
    End,
}

impl Alignment {
    fn from_str(value: &str) -> Self {
        match value {
            "center" => Self::Center,
            "end" => Self::End,
            _ => Self::Start,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
enum FitMode {
    Switch,
    #[default]
    Snap,
}

impl FitMode {
    fn from_str(value: &str) -> Self {
        if value == "switch" {
            Self::Switch
        } else {
            Self::Snap
        }
    }
}

#[derive(Debug, Clone)]
pub struct AnchoredElement {
    position: Option<(f32, f32)>,
    side: Side,
    align: Alignment,
    anchor: Option<AnchorPoint>,
    gap: f32,
    offset: (f32, f32),
    fit: FitMode,
    snap_margin: f32,
    deferred: bool,
    priority: usize,
    occlude: bool,
    /// `fill: "window"`: cover the whole viewport, like a dialog portal.
    fill_window: bool,
}

impl Default for AnchoredElement {
    fn default() -> Self {
        Self {
            position: None,
            side: Side::Bottom,
            align: Alignment::Start,
            anchor: None,
            gap: 0.0,
            offset: (0.0, 0.0),
            fit: FitMode::Snap,
            snap_margin: 8.0,
            deferred: true,
            priority: 1,
            occlude: true,
            fill_window: false,
        }
    }
}

impl AnchoredElement {
    fn resolved_anchor(&self) -> AnchorPoint {
        if let Some(anchor) = self.anchor {
            return anchor;
        }
        match (self.side, self.align) {
            (Side::Top, Alignment::Start) => AnchorPoint::BottomLeft,
            (Side::Top, Alignment::Center) => AnchorPoint::BottomCenter,
            (Side::Top, Alignment::End) => AnchorPoint::BottomRight,
            (Side::Right, Alignment::Start) => AnchorPoint::TopLeft,
            (Side::Right, Alignment::Center) => AnchorPoint::LeftCenter,
            (Side::Right, Alignment::End) => AnchorPoint::BottomLeft,
            (Side::Bottom, Alignment::Start) => AnchorPoint::TopLeft,
            (Side::Bottom, Alignment::Center) => AnchorPoint::TopCenter,
            (Side::Bottom, Alignment::End) => AnchorPoint::TopRight,
            (Side::Left, Alignment::Start) => AnchorPoint::TopRight,
            (Side::Left, Alignment::Center) => AnchorPoint::RightCenter,
            (Side::Left, Alignment::End) => AnchorPoint::BottomRight,
        }
    }

    fn resolved_offset(&self) -> gpui::Point<gpui::Pixels> {
        let (side_x, side_y) = match self.side {
            Side::Top => (0.0, -self.gap),
            Side::Right => (self.gap, 0.0),
            Side::Bottom => (0.0, self.gap),
            Side::Left => (-self.gap, 0.0),
        };
        gpui::point(
            gpui::px(side_x + self.offset.0),
            gpui::px(side_y + self.offset.1),
        )
    }

    fn wrap_at_trigger(&self, layer: gpui::AnyElement) -> gpui::AnyElement {
        use gpui::prelude::*;

        match (self.side, self.align) {
            (Side::Top, Alignment::Start) => gpui::div()
                .absolute()
                .top_0()
                .left_0()
                .size_0()
                .child(layer)
                .into_any_element(),
            (Side::Top, Alignment::Center) if matches!(self.fit, FitMode::Snap) => gpui::div()
                .absolute()
                .top_0()
                .left(gpui::relative(0.5))
                .size_0()
                .child(layer)
                .into_any_element(),
            (Side::Top, Alignment::Center) => gpui::div()
                .absolute()
                .top_0()
                .left_0()
                .h_0()
                .w_full()
                .flex()
                .justify_center()
                .child(layer)
                .into_any_element(),
            (Side::Top, Alignment::End) => gpui::div()
                .absolute()
                .top_0()
                .right_0()
                .size_0()
                .child(layer)
                .into_any_element(),
            (Side::Right, Alignment::Start) => gpui::div()
                .absolute()
                .top_0()
                .right_0()
                .size_0()
                .child(layer)
                .into_any_element(),
            (Side::Right, Alignment::Center) if matches!(self.fit, FitMode::Snap) => gpui::div()
                .absolute()
                .right_0()
                .top(gpui::relative(0.5))
                .size_0()
                .child(layer)
                .into_any_element(),
            (Side::Right, Alignment::Center) => gpui::div()
                .absolute()
                .top_0()
                .right_0()
                .w_0()
                .h_full()
                .flex()
                .items_center()
                .child(layer)
                .into_any_element(),
            (Side::Right, Alignment::End) => gpui::div()
                .absolute()
                .bottom_0()
                .right_0()
                .size_0()
                .child(layer)
                .into_any_element(),
            (Side::Bottom, Alignment::Start) => gpui::div()
                .absolute()
                .bottom_0()
                .left_0()
                .size_0()
                .child(layer)
                .into_any_element(),
            (Side::Bottom, Alignment::Center) if matches!(self.fit, FitMode::Snap) => gpui::div()
                .absolute()
                .bottom_0()
                .left(gpui::relative(0.5))
                .size_0()
                .child(layer)
                .into_any_element(),
            (Side::Bottom, Alignment::Center) => gpui::div()
                .absolute()
                .bottom_0()
                .left_0()
                .h_0()
                .w_full()
                .flex()
                .justify_center()
                .child(layer)
                .into_any_element(),
            (Side::Bottom, Alignment::End) => gpui::div()
                .absolute()
                .bottom_0()
                .right_0()
                .size_0()
                .child(layer)
                .into_any_element(),
            (Side::Left, Alignment::Start) => gpui::div()
                .absolute()
                .top_0()
                .left_0()
                .size_0()
                .child(layer)
                .into_any_element(),
            (Side::Left, Alignment::Center) if matches!(self.fit, FitMode::Snap) => gpui::div()
                .absolute()
                .left_0()
                .top(gpui::relative(0.5))
                .size_0()
                .child(layer)
                .into_any_element(),
            (Side::Left, Alignment::Center) => gpui::div()
                .absolute()
                .top_0()
                .left_0()
                .w_0()
                .h_full()
                .flex()
                .items_center()
                .child(layer)
                .into_any_element(),
            (Side::Left, Alignment::End) => gpui::div()
                .absolute()
                .bottom_0()
                .left_0()
                .size_0()
                .child(layer)
                .into_any_element(),
        }
    }
}

impl CustomElement for AnchoredElement {
    fn render(
        &mut self,
        ctx: CustomRenderContext,
        window: &mut gpui::Window,
        _cx: &mut gpui::Context<crate::renderer::GpuixView>,
    ) -> gpui::AnyElement {
        use gpui::prelude::*;

        // The overlay, not the trigger, is what a user clicks, so the recorded
        // box has to be this content div after `anchored` placed it. Only gpui
        // knows where that is, so it reports its own painted box instead of
        // carrying a `bounds_tracker` child.
        let mut content = gpui::div()
            .id(gpui::SharedString::from(format!(
                "__gpuix_anchored_{}",
                ctx.id
            )))
            .flex_col();
        content = crate::automation::track_own_bounds(content, ctx.id);
        if let Some(style) = ctx.style {
            content = crate::renderer::apply_interactive_styles(content, style);
        }
        if self.fill_window {
            // `anchored()` is absolute and sized by its content, so a percentage
            // size has nothing to resolve against. A resize runs gpui's
            // `Window::bounds_changed`, which refreshes the window, so reading
            // the viewport here tracks it in the same frame, with no JS polling.
            // Applied after the JS style so it always wins over width/height.
            let viewport = window.viewport_size();
            content = content.w(viewport.width).h(viewport.height);
        }
        content = crate::accessibility::apply_accessibility(content, ctx.props, None);
        content = super::wire_standard_events(content, &ctx);
        // Deferred overlays paint over the window blur. A missing fill lets the
        // page show through the card. Force an opaque surface when JS omitted
        // one. An explicit transparent fill opts out: gpui paints no quad for it,
        // and a full-window layer such as a dialog portal must paint nothing.
        let declares_fill = ctx
            .style
            .and_then(crate::style::StyleDesc::resolved_background)
            .is_some();
        if !declares_fill {
            content = content.bg(gpui::rgb(0x1A1A1A));
        }
        if self.occlude {
            content = content.occlude();
        }
        for child in ctx.children {
            content = content.child(child);
        }

        let mut anchored = gpui::anchored();
        if self.fill_window {
            // Already the window: no offset, no snapping.
            anchored = anchored
                .anchor(gpui::Anchor::TopLeft)
                .position(gpui::point(gpui::px(0.0), gpui::px(0.0)));
        } else {
            anchored = anchored
                .anchor(self.resolved_anchor().as_gpui())
                .offset(self.resolved_offset());
            if let Some((x, y)) = self.position {
                anchored = anchored.position(gpui::point(gpui::px(x), gpui::px(y)));
            }
            if matches!(self.fit, FitMode::Snap) {
                anchored = anchored.snap_to_window_with_margin(gpui::px(self.snap_margin));
            }
        }

        let anchored = anchored.child(content);
        let layer = if self.deferred {
            gpui::deferred(anchored)
                .with_priority(self.priority)
                .into_any_element()
        } else {
            anchored.into_any_element()
        };

        if self.position.is_some() || self.fill_window {
            layer
        } else {
            self.wrap_at_trigger(layer)
        }
    }

    fn set_prop(&mut self, key: &str, value: serde_json::Value) {
        match key {
            "position" => {
                self.position = value.as_object().and_then(|position| {
                    Some((
                        position.get("x")?.as_f64()? as f32,
                        position.get("y")?.as_f64()? as f32,
                    ))
                });
            }
            "side" => self.side = value.as_str().map(Side::from_str).unwrap_or_default(),
            "align" => self.align = value.as_str().map(Alignment::from_str).unwrap_or_default(),
            "anchor" => self.anchor = value.as_str().and_then(AnchorPoint::from_str),
            "gap" => self.gap = value.as_f64().unwrap_or(0.0) as f32,
            "offset" => {
                self.offset = value
                    .as_object()
                    .map(|offset| {
                        (
                            offset
                                .get("x")
                                .and_then(|value| value.as_f64())
                                .unwrap_or(0.0) as f32,
                            offset
                                .get("y")
                                .and_then(|value| value.as_f64())
                                .unwrap_or(0.0) as f32,
                        )
                    })
                    .unwrap_or_default();
            }
            "fit" => self.fit = value.as_str().map(FitMode::from_str).unwrap_or_default(),
            "snapMargin" => self.snap_margin = value.as_f64().unwrap_or(8.0) as f32,
            "deferred" => self.deferred = value.as_bool().unwrap_or(true),
            "priority" => {
                self.priority = value
                    .as_u64()
                    .and_then(|priority| usize::try_from(priority).ok())
                    .unwrap_or(1);
            }
            "occlude" => self.occlude = value.as_bool().unwrap_or(true),
            "fill" => self.fill_window = value.as_str() == Some("window"),
            _ => {}
        }
    }

    fn supported_props(&self) -> &'static [&'static str] {
        &[
            "position",
            "side",
            "align",
            "anchor",
            "gap",
            "offset",
            "fit",
            "snapMargin",
            "deferred",
            "priority",
            "occlude",
            "fill",
        ]
    }

    fn supported_events(&self) -> &'static [&'static str] {
        &["click", "mouseEnter", "mouseLeave", "fileDrop"]
    }

    fn destroy(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{prelude::*, px, size};

    struct AnchorFixture {
        element: AnchoredElement,
        trigger_width: f32,
        trigger_height: f32,
        width: f32,
        height: f32,
        padding: f32,
        left: f32,
        top: f32,
    }

    impl gpui::Render for AnchorFixture {
        fn render(
            &mut self,
            _: &mut gpui::Window,
            _: &mut gpui::Context<Self>,
        ) -> impl IntoElement {
            let mut anchored = gpui::anchored()
                .anchor(self.element.resolved_anchor().as_gpui())
                .offset(self.element.resolved_offset());
            if let Some((x, y)) = self.element.position {
                anchored = anchored.position(gpui::point(px(x), px(y)));
            }
            if matches!(self.element.fit, FitMode::Snap) {
                anchored = anchored.snap_to_window_with_margin(px(self.element.snap_margin));
            }
            let anchored = anchored.child(
                gpui::div()
                    .id("layer")
                    .debug_selector(|| "ANCHOR_LAYER".into())
                    .w(px(self.width))
                    .h(px(self.height))
                    .p(px(self.padding)),
            );
            let layer = if self.element.deferred {
                gpui::deferred(anchored).into_any_element()
            } else {
                anchored.into_any_element()
            };
            let layer = if self.element.position.is_some() {
                layer
            } else {
                self.element.wrap_at_trigger(layer)
            };
            gpui::div().size_full().p(px(self.padding)).child(
                gpui::div()
                    .id("trigger")
                    .debug_selector(|| "TRIGGER".into())
                    .absolute()
                    .left(px(self.left))
                    .top(px(self.top))
                    .w(px(self.trigger_width))
                    .h(px(self.trigger_height))
                    .child(layer),
            )
        }
    }

    #[gpui::test]
    fn all_alignment_and_size_combinations_keep_one_trigger_anchor(cx: &mut gpui::TestAppContext) {
        let window = cx.open_window(size(px(800.0), px(600.0)), |_, _| AnchorFixture {
            element: AnchoredElement::default(),
            trigger_width: 120.0,
            trigger_height: 100.0,
            width: 50.0,
            height: 50.0,
            padding: 0.0,
            left: 300.0,
            top: 220.0,
        });
        let mut seed = 0x146115_u32;
        for sample in 0..15 {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let (tw, th, width, height, padding) = match sample {
                0 => (120.0, 100.0, 50.0, 50.0, 0.0),
                1 => (74.0, 130.0, 68.0, 68.0, 7.0),
                2 => (63.0, 41.0, 130.0, 130.0, 11.0),
                _ => (
                    30.0 + (seed % 151) as f32,
                    20.0 + ((seed >> 8) % 141) as f32,
                    30.0 + ((seed >> 16) % 191) as f32,
                    20.0 + ((seed >> 24) % 171) as f32,
                    (seed % 10) as f32,
                ),
            };
            let gap = (sample % 8) as f32;
            for side in [Side::Top, Side::Right, Side::Bottom, Side::Left] {
                for align in [Alignment::Start, Alignment::Center, Alignment::End] {
                    for deferred in [false, true] {
                        window
                            .update(cx, |fixture, window, cx| {
                                fixture.element.side = side;
                                fixture.element.align = align;
                                fixture.element.gap = gap;
                                fixture.element.deferred = deferred;
                                fixture.trigger_width = tw;
                                fixture.trigger_height = th;
                                fixture.width = width;
                                fixture.height = height;
                                fixture.padding = padding;
                                cx.notify();
                                window.refresh();
                            })
                            .unwrap();
                        cx.run_until_parked();
                        let mut visual = gpui::VisualTestContext::from_window(window.into(), cx);
                        let trigger = visual.debug_bounds("TRIGGER").expect("trigger must paint");
                        let bounds = visual
                            .debug_bounds("ANCHOR_LAYER")
                            .expect("layer must paint");
                        let left = f32::from(trigger.left());
                        let top = f32::from(trigger.top());
                        let right = f32::from(trigger.right());
                        let bottom = f32::from(trigger.bottom());
                        let (x, y) = match side {
                            Side::Top | Side::Bottom => {
                                let x = match align {
                                    Alignment::Start => left,
                                    Alignment::Center => (left + right - width) / 2.0,
                                    Alignment::End => right - width,
                                };
                                (
                                    x,
                                    if matches!(side, Side::Top) {
                                        top - gap - height
                                    } else {
                                        bottom + gap
                                    },
                                )
                            }
                            Side::Right | Side::Left => {
                                let y = match align {
                                    Alignment::Start => top,
                                    Alignment::Center => (top + bottom - height) / 2.0,
                                    Alignment::End => bottom - height,
                                };
                                (
                                    if matches!(side, Side::Left) {
                                        left - gap - width
                                    } else {
                                        right + gap
                                    },
                                    y,
                                )
                            }
                        };
                        assert!(
                            (f32::from(bounds.origin.x) - x).abs() <= 0.51,
                            "{side:?}/{align:?}/{deferred}: {bounds:?}, expected x={x}"
                        );
                        assert!(
                            (f32::from(bounds.origin.y) - y).abs() <= 0.51,
                            "{side:?}/{align:?}/{deferred}: {bounds:?}, expected y={y}"
                        );
                        assert_eq!(bounds.size, size(px(width), px(height)));
                    }
                }
            }
        }
    }

    fn public_fixture(element: AnchoredElement) -> AnchorFixture {
        AnchorFixture {
            element,
            trigger_width: 40.0,
            trigger_height: 30.0,
            width: 80.0,
            height: 60.0,
            padding: 0.0,
            left: 100.0,
            top: 100.0,
        }
    }

    #[gpui::test]
    fn explicit_positions_keep_original_pixels_and_snap_margins(cx: &mut gpui::TestAppContext) {
        let window = cx.open_window(size(px(400.0), px(300.0)), |_, _| {
            public_fixture(AnchoredElement {
                align: Alignment::Center,
                gap: 6.0,
                snap_margin: 12.0,
                ..AnchoredElement::default()
            })
        });
        for (side, x, y) in [
            (Side::Top, 160.0, 34.0),
            (Side::Right, 206.0, 70.0),
            (Side::Bottom, 160.0, 106.0),
            (Side::Left, 114.0, 70.0),
        ] {
            for deferred in [false, true] {
                window
                    .update(cx, |fixture, window, cx| {
                        fixture.element.side = side;
                        fixture.element.position = Some((200.0, 100.0));
                        fixture.element.deferred = deferred;
                        cx.notify();
                        window.refresh();
                    })
                    .unwrap();
                cx.run_until_parked();
                let bounds = gpui::VisualTestContext::from_window(window.into(), cx)
                    .debug_bounds("ANCHOR_LAYER")
                    .expect("positioned layer must paint");
                assert_eq!(
                    bounds,
                    gpui::Bounds::new(gpui::point(px(x), px(y)), size(px(80.0), px(60.0)))
                );
            }
        }
        for (position, anchor, x, y) in [
            ((395.0, 295.0), Some(AnchorPoint::TopLeft), 308.0, 228.0),
            ((5.0, 5.0), None, 12.0, 11.0),
        ] {
            window
                .update(cx, |fixture, window, cx| {
                    fixture.element.side = Side::Bottom;
                    fixture.element.position = Some(position);
                    fixture.element.anchor = anchor;
                    fixture.element.deferred = true;
                    cx.notify();
                    window.refresh();
                })
                .unwrap();
            cx.run_until_parked();
            let bounds = gpui::VisualTestContext::from_window(window.into(), cx)
                .debug_bounds("ANCHOR_LAYER")
                .expect("snapped layer must paint");
            assert_eq!(
                bounds,
                gpui::Bounds::new(gpui::point(px(x), px(y)), size(px(80.0), px(60.0)))
            );
        }
    }

    #[gpui::test]
    fn switch_fit_keeps_original_main_axis_flip_and_cross_axis_clamp(
        cx: &mut gpui::TestAppContext,
    ) {
        let window = cx.open_window(size(px(400.0), px(300.0)), |_, _| {
            public_fixture(AnchoredElement {
                align: Alignment::Center,
                gap: 6.0,
                fit: FitMode::Switch,
                ..AnchoredElement::default()
            })
        });
        for (side, left, top, x, y) in [
            (Side::Bottom, 350.0, 250.0, 290.0, 220.0),
            (Side::Right, 350.0, 250.0, 310.0, 205.0),
            (Side::Top, 10.0, 10.0, 0.0, 10.0),
            (Side::Left, 10.0, 10.0, 10.0, 0.0),
        ] {
            window
                .update(cx, |fixture, window, cx| {
                    fixture.element.side = side;
                    fixture.left = left;
                    fixture.top = top;
                    cx.notify();
                    window.refresh();
                })
                .unwrap();
            cx.run_until_parked();
            let bounds = gpui::VisualTestContext::from_window(window.into(), cx)
                .debug_bounds("ANCHOR_LAYER")
                .expect("switched layer must paint");
            assert_eq!(
                bounds,
                gpui::Bounds::new(gpui::point(px(x), px(y)), size(px(80.0), px(60.0)))
            );
        }
    }
}
