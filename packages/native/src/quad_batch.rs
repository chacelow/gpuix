#[cfg(test)]
#[path = "quad_batch_paint_tests.rs"]
mod paint_tests;

use crate::{retained_tree::RetainedElement, style::StyleDesc};
use gpui::prelude::*;

pub(crate) fn validate(values: &[f32]) -> Result<(), String> {
    if values.len() % 4 != 0 {
        return Err("quads length must be a multiple of four".into());
    }
    if values
        .chunks_exact(4)
        .any(|q| q.iter().any(|v| !v.is_finite()) || q[2] < 0.0 || q[3] < 0.0)
    {
        return Err("quads must contain finite coordinates and nonnegative dimensions".into());
    }
    Ok(())
}

fn stroke_width(value: &serde_json::Value) -> Result<f32, String> {
    if value.is_null() {
        return Ok(0.0);
    }
    let width = value
        .as_f64()
        .filter(|width| width.is_finite() && *width >= 0.0)
        .map(|width| width as f32)
        .filter(|width| width.is_finite())
        .ok_or_else(|| "quad-batch strokeWidth must be finite and nonnegative".to_string())?;
    Ok(width)
}

fn stroke_color(value: &serde_json::Value) -> Result<gpui::Hsla, String> {
    if value.is_null() {
        return Ok(gpui::transparent_black());
    }
    value
        .as_str()
        .and_then(crate::color::parse_color_rgba)
        .map(Into::into)
        .ok_or_else(|| "quad-batch strokeColor must be a supported color string".to_string())
}

pub(crate) fn validate_paint_prop(key: &str, value: &serde_json::Value) -> Result<(), String> {
    match key {
        "strokeWidth" => stroke_width(value).map(|_| ()),
        "strokeColor" => stroke_color(value).map(|_| ()),
        _ => Ok(()),
    }
}

pub(crate) fn render(element: &RetainedElement, style: Option<&StyleDesc>) -> gpui::Div {
    let values = element.quads();
    let color: gpui::Hsla = style
        .and_then(|s| s.color.as_deref())
        .and_then(crate::color::parse_color_rgba)
        .map(Into::into)
        .unwrap_or_else(gpui::white);
    let prop = |key| {
        element
            .custom_props()
            .get(key)
            .unwrap_or(&serde_json::Value::Null)
    };
    let stroke_width = stroke_width(prop("strokeWidth")).expect("validated quad-batch strokeWidth");
    let stroke_color = stroke_color(prop("strokeColor")).expect("validated quad-batch strokeColor");
    let mut stroke_background = stroke_color;
    stroke_background.a = 0.0;
    let canvas = gpui::canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
                window.paint_layer(bounds, |window| {
                    for q in values.as_deref().unwrap_or(&[]).chunks_exact(4) {
                        if q[2] == 0.0 || q[3] == 0.0 {
                            continue;
                        }
                        // Match Taffy's authored lengths; paint_quad snaps absolute edges.
                        let mut rect = window.pixel_snap_bounds(gpui::Bounds::new(
                            gpui::point(gpui::px(q[0]), gpui::px(q[1])),
                            gpui::size(gpui::px(q[2]), gpui::px(q[3])),
                        ));
                        rect.origin = bounds.origin + rect.origin;
                        window.paint_quad(gpui::fill(rect, color));
                        if stroke_width > 0.0 {
                            // Div composites the background into the render target before the
                            // border. A single filled/bordered quad changes 8-bit alpha rounding.
                            window.paint_quad(gpui::quad(
                                rect,
                                gpui::Corners::default(),
                                stroke_background,
                                gpui::Edges::all(gpui::px(stroke_width)),
                                stroke_color,
                                gpui::BorderStyle::default(),
                            ));
                        }
                    }
                });
            });
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full();
    let mut el = gpui::div().relative();
    if let Some(style) = style {
        el = crate::renderer::apply_styles(el, style);
    }
    el.style().mouse_cursor = None;
    el.child(canvas)
}

#[cfg(test)]
mod tests {
    use crate::{
        renderer::{apply_batch_to_tree, GpuixView},
        retained_tree::RetainedTree,
        text::SharedSelection,
    };
    use std::sync::{Arc, Mutex};

    fn assert_div_geometry(cx: &mut gpui::TestAppContext, origin: [f32; 2], rect: [f32; 4]) {
        let make_tree = |batch: bool| {
            let mut tree = RetainedTree::new();
            let mutations = serde_json::json!([
                ["createElement", 1, "div"],
                ["setStyle", 1, {"width": 200, "height": 200}],
                ["createElement", 2, if batch { "quad-batch" } else { "div" }],
                ["setStyle", 2, {"position": "absolute", "left": origin[0], "top": origin[1],
                    "width": 100, "height": 100, "color": "red"}],
                ["appendChild", 1, 2],
                ["setRoot", 1]
            ]);
            apply_batch_to_tree(&mut tree, &serde_json::to_vec(&mutations).unwrap()).unwrap();
            if batch {
                tree.set_quads(2, &rect).unwrap();
            } else {
                let mutations = serde_json::json!([
                    ["createElement", 3, "div"],
                    ["setStyle", 3, {"position": "absolute", "left": rect[0], "top": rect[1],
                        "width": rect[2], "height": rect[3], "backgroundColor": "red"}],
                    ["appendChild", 2, 3]
                ]);
                apply_batch_to_tree(&mut tree, &serde_json::to_vec(&mutations).unwrap()).unwrap();
            }
            Arc::new(Mutex::new(tree))
        };
        let batch_tree = make_tree(true);
        let div_tree = make_tree(false);
        let batch = cx.open_window(gpui::size(gpui::px(200.0), gpui::px(200.0)), |_, _| {
            GpuixView::new(
                batch_tree,
                None,
                "batch geometry".into(),
                SharedSelection::default(),
            )
        });
        let div = cx.open_window(gpui::size(gpui::px(200.0), gpui::px(200.0)), |_, _| {
            GpuixView::new(
                div_tree,
                None,
                "div geometry".into(),
                SharedSelection::default(),
            )
        });
        for scale in [1.0, 1.25, 1.5, 2.0] {
            for window in [batch, div] {
                window
                    .update(cx, |_, window, cx| {
                        window.set_scale_factor(scale);
                        cx.notify();
                    })
                    .unwrap();
            }
            cx.run_until_parked();
            let actual = batch
                .update(cx, |_, window, _| {
                    window
                        .painted_quads()
                        .iter()
                        .map(|quad| quad.bounds)
                        .collect::<Vec<_>>()
                })
                .unwrap();
            let expected = div
                .update(cx, |_, window, _| {
                    window
                        .painted_quads()
                        .iter()
                        .map(|quad| quad.bounds)
                        .collect::<Vec<_>>()
                })
                .unwrap();
            assert_eq!(
                expected.len(),
                1,
                "reference div did not paint at scale {scale}"
            );
            assert_eq!(
                actual, expected,
                "native div mismatch at scale {scale}, origin {origin:?}, rect {rect:?}"
            );
        }
    }

    #[gpui::test]
    fn fractional_position_matches_native_div(cx: &mut gpui::TestAppContext) {
        assert_div_geometry(cx, [20.0, 10.0], [3.5, 4.75, 10.0, 12.0]);
        assert_div_geometry(cx, [20.0, 10.0], [-0.5, -1.75, 10.0, 12.0]);
    }

    #[gpui::test]
    fn fractional_size_matches_native_div(cx: &mut gpui::TestAppContext) {
        assert_div_geometry(cx, [20.0, 10.0], [1.0, 1.0, 1.9, 1.9]);
    }

    #[gpui::test]
    fn stereo_lane_half_pixel_matches_native_div(cx: &mut gpui::TestAppContext) {
        assert_div_geometry(cx, [0.0, 10.0], [1.0, 5.5, 1.0, 4.5]);
    }

    #[gpui::test]
    fn fractional_host_origin_matches_native_div(cx: &mut gpui::TestAppContext) {
        assert_div_geometry(cx, [20.5, 10.75], [3.5, 4.75, 10.5, 12.75]);
    }

    struct HitboxProbe {
        element: gpui::Div,
        inserted: std::rc::Rc<std::cell::Cell<bool>>,
    }

    impl gpui::IntoElement for HitboxProbe {
        type Element = Self;
        fn into_element(self) -> Self {
            self
        }
    }

    impl gpui::Element for HitboxProbe {
        type RequestLayoutState = <gpui::Div as gpui::Element>::RequestLayoutState;
        type PrepaintState = Option<gpui::Hitbox>;
        fn id(&self) -> Option<gpui::ElementId> {
            None
        }
        fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
            None
        }
        fn request_layout(
            &mut self,
            id: Option<&gpui::GlobalElementId>,
            inspector: Option<&gpui::InspectorElementId>,
            window: &mut gpui::Window,
            cx: &mut gpui::App,
        ) -> (gpui::LayoutId, Self::RequestLayoutState) {
            self.element.request_layout(id, inspector, window, cx)
        }
        fn prepaint(
            &mut self,
            id: Option<&gpui::GlobalElementId>,
            inspector: Option<&gpui::InspectorElementId>,
            bounds: gpui::Bounds<gpui::Pixels>,
            state: &mut Self::RequestLayoutState,
            window: &mut gpui::Window,
            cx: &mut gpui::App,
        ) -> Self::PrepaintState {
            let hitbox = self
                .element
                .prepaint(id, inspector, bounds, state, window, cx);
            self.inserted.set(hitbox.is_some());
            hitbox
        }
        fn paint(
            &mut self,
            id: Option<&gpui::GlobalElementId>,
            inspector: Option<&gpui::InspectorElementId>,
            bounds: gpui::Bounds<gpui::Pixels>,
            state: &mut Self::RequestLayoutState,
            hitbox: &mut Self::PrepaintState,
            window: &mut gpui::Window,
            cx: &mut gpui::App,
        ) {
            self.element
                .paint(id, inspector, bounds, state, hitbox, window, cx);
        }
    }

    struct CursorFixture {
        tree: RetainedTree,
        inserted: std::rc::Rc<std::cell::Cell<bool>>,
    }

    impl gpui::Render for CursorFixture {
        fn render(
            &mut self,
            _: &mut gpui::Window,
            _: &mut gpui::Context<Self>,
        ) -> impl gpui::IntoElement {
            let element = &self.tree.elements[&1];
            HitboxProbe {
                element: super::render(element, element.style.as_deref()),
                inserted: self.inserted.clone(),
            }
        }
    }

    #[gpui::test]
    fn cursor_and_pointer_styles_cannot_insert_a_native_hitbox(cx: &mut gpui::TestAppContext) {
        let mut tree = RetainedTree::new();
        apply_batch_to_tree(&mut tree, br#"[["createElement",1,"quad-batch"],["setStyle",1,{"width":100,"height":100,"color":"red","cursor":"pointer","pointerEvents":"auto"}],["setRoot",1]]"#).unwrap();
        tree.set_quads(1, &[0.0, 0.0, 100.0, 100.0]).unwrap();
        let inserted = std::rc::Rc::new(std::cell::Cell::new(true));
        let observed = inserted.clone();
        cx.add_window(|_, _| CursorFixture { tree, inserted });
        cx.run_until_parked();
        assert!(
            !observed.get(),
            "pure visual batch registered a native hitbox"
        );
    }

    #[gpui::test]
    fn local_positions_masks_color_opacity_and_retained_buffer(cx: &mut gpui::TestAppContext) {
        let mut tree = RetainedTree::new();
        apply_batch_to_tree(&mut tree, br##"[["createElement",1,"div"],["setStyle",1,{"width":80,"height":60,"overflow":"hidden","opacity":0.5}],["createElement",2,"quad-batch"],["setStyle",2,{"position":"absolute","left":20,"top":10,"width":100,"height":100,"color":"#ff000080","opacity":0.5}],["appendChild",1,2],["setRoot",1]]"##).unwrap();
        tree.set_quads(
            2,
            &[
                3.0, 4.0, 10.0, 12.0, 55.0, 5.0, 20.0, 10.0, 90.0, 90.0, 10.0, 10.0,
            ],
        )
        .unwrap();
        let buffer = tree.elements[&2].quads().unwrap();
        let tree = Arc::new(Mutex::new(tree));
        let window = cx.open_window(gpui::size(gpui::px(200.0), gpui::px(200.0)), |_, _| {
            GpuixView::new(
                tree.clone(),
                None,
                "quad test".into(),
                SharedSelection::default(),
            )
        });
        cx.run_until_parked();
        window
            .update(cx, |_, window, _| {
                let quads = window.painted_quads();
                assert_eq!(quads.len(), 2);
                let scale = window.scale_factor();
                let first = &quads[0];
                assert_eq!(first.bounds.origin.x.as_f32() / scale, 23.0);
                assert_eq!(first.bounds.origin.y.as_f32() / scale, 14.0);
                assert_eq!(first.bounds.size.width.as_f32() / scale, 10.0);
                assert_eq!(first.bounds.size.height.as_f32() / scale, 12.0);
                assert_eq!(first.content_mask.bounds.right().as_f32() / scale, 80.0);
                assert_eq!(first.content_mask.bounds.bottom().as_f32() / scale, 60.0);
                let color = first.background.as_solid().unwrap();
                assert_eq!(color.h, 0.0);
                assert_eq!(color.s, 1.0);
                assert!((color.a - (128.0 / 255.0 * 0.25)).abs() < 0.001);
            })
            .unwrap();
        {
            let mut retained = tree.lock().unwrap();
            apply_batch_to_tree(&mut retained, br##"[["setStyle",2,{"position":"absolute","left":20,"top":10,"width":40,"height":30,"color":"#ff000080","opacity":0.5}]]"##).unwrap();
        }
        window
            .update(cx, |_, window, cx| {
                cx.notify();
                window.refresh();
            })
            .unwrap();
        cx.run_until_parked();
        window
            .update(cx, |_, window, _| {
                let quads = window.painted_quads();
                assert_eq!(quads.len(), 1);
                let scale = window.scale_factor();
                assert_eq!(quads[0].content_mask.bounds.right().as_f32() / scale, 60.0);
                assert_eq!(quads[0].content_mask.bounds.bottom().as_f32() / scale, 40.0);
            })
            .unwrap();
        {
            let mut retained = tree.lock().unwrap();
            apply_batch_to_tree(&mut retained, br##"[["setStyle",2,{"position":"absolute","left":20,"top":10,"width":100,"height":100,"color":"#ff000080","opacity":0.5}]]"##).unwrap();
        }
        let mut tree = tree.lock().unwrap();
        tree.take_dirty();
        apply_batch_to_tree(&mut tree, br##"[["setStyle",2,{"position":"absolute","left":20,"top":10,"width":100,"height":100,"color":"#ff000080","opacity":0.5}]]"##).unwrap();
        assert!(Arc::ptr_eq(&buffer, &tree.elements[&2].quads().unwrap()));
        assert!(!tree.take_dirty());
        assert!(!tree.elements[&2].needs_focus());
        tree.set_quads(2, &[0.0, 0.0, 2.0, 2.0]).unwrap();
        assert!(!Arc::ptr_eq(&buffer, &tree.elements[&2].quads().unwrap()));
        assert!(tree.take_dirty());
        for invalid in [
            &[0.0][..],
            &[0.0, 0.0, -1.0, 1.0],
            &[f32::NAN, 0.0, 1.0, 1.0],
        ] {
            assert!(tree.set_quads(2, invalid).is_err());
        }
        assert!(tree.set_quads(1, &[]).is_err());
        tree.destroy_element(1);
        assert!(tree.elements.is_empty());
    }
}
