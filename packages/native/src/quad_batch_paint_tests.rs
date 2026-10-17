use crate::{
    renderer::{apply_batch_to_tree, GpuixView},
    retained_tree::RetainedTree,
    text::SharedSelection,
};
use std::sync::{Arc, Mutex};

#[test]
fn malformed_paint_is_atomic() {
    for (key, value) in [
        ("strokeWidth", serde_json::json!(-1)),
        ("strokeWidth", serde_json::json!(-1e-100)),
        ("strokeWidth", serde_json::json!("1")),
        ("strokeWidth", serde_json::json!(1e100)),
        ("strokeColor", serde_json::json!(42)),
        ("strokeColor", serde_json::json!("not-a-color")),
    ] {
        let mut tree = RetainedTree::new();
        let ops = serde_json::json!([
            ["createElement", 1, "quad-batch"],
            ["setCustomProp", 1, key, value],
            ["setRoot", 1]
        ]);
        assert!(
            apply_batch_to_tree(&mut tree, &serde_json::to_vec(&ops).unwrap()).is_err(),
            "accepted {key}"
        );
        assert!(
            tree.elements.is_empty(),
            "invalid metadata partly applied the batch"
        );
    }
}

#[gpui::test]
fn bordered_quad_matches_div_metrics_at_all_scales(cx: &mut gpui::TestAppContext) {
    for origin in [[20.0, 10.0], [20.5, 10.75]] {
        for rect in [[3.5, 4.75, 30.5, 22.75], [-0.5, -1.75, 12.5, 8.25]] {
            for width in [0.0, 1.0, 1.25] {
                let make = |batch| {
                    let mut tree = RetainedTree::new();
                    let ops = serde_json::json!([
                        ["createElement", 1, "div"],
                        ["setStyle", 1, {"width":200,"height":200}],
                        ["createElement", 2, if batch {"quad-batch"} else {"div"}],
                        ["setStyle", 2, {"position":"absolute","left":origin[0],"top":origin[1],"width":100,"height":100,"overflow":"hidden","color":"rgba(164,181,198,0.63)"}],
                        ["appendChild", 1, 2],
                        ["setRoot", 1]
                    ]);
                    apply_batch_to_tree(&mut tree, &serde_json::to_vec(&ops).unwrap()).unwrap();
                    let ops = if batch {
                        tree.set_quads(2, &rect).unwrap();
                        serde_json::json!([
                            ["setCustomProp", 2, "strokeWidth", width],
                            ["setCustomProp", 2, "strokeColor", "rgba(50,69,103,0.47)"]
                        ])
                    } else {
                        serde_json::json!([
                            ["createElement",3,"div"],
                            ["setStyle",3,{"position":"absolute","left":rect[0],"top":rect[1],"width":rect[2],"height":rect[3],"backgroundColor":"rgba(164,181,198,0.63)","borderWidth":width,"borderColor":"rgba(50,69,103,0.47)"}],
                            ["appendChild",2,3]
                        ])
                    };
                    apply_batch_to_tree(&mut tree, &serde_json::to_vec(&ops).unwrap()).unwrap();
                    Arc::new(Mutex::new(tree))
                };
                let batch_tree = make(true);
                let div_tree = make(false);
                let batch = cx.open_window(gpui::size(gpui::px(200.0), gpui::px(200.0)), |_, _| {
                    GpuixView::new(batch_tree, None, "batch".into(), SharedSelection::default())
                });
                let div = cx.open_window(gpui::size(gpui::px(200.0), gpui::px(200.0)), |_, _| {
                    GpuixView::new(div_tree, None, "div".into(), SharedSelection::default())
                });
                for scale in [1.0, 1.25, 1.5, 2.0] {
                    for handle in [batch, div] {
                        handle
                            .update(cx, |_, window, cx| {
                                window.set_scale_factor(scale);
                                cx.notify();
                            })
                            .unwrap();
                    }
                    cx.run_until_parked();
                    let actual = batch
                        .update(cx, |_, window, _| window.painted_quads())
                        .unwrap();
                    let expected = div
                        .update(cx, |_, window, _| window.painted_quads())
                        .unwrap();
                    assert_eq!(actual.len(), expected.len(), "div-equivalent paint count");
                    for (actual, expected) in actual.iter().zip(&expected) {
                        assert_eq!(actual.bounds, expected.bounds, "bounds at scale {scale}");
                        assert_eq!(actual.background.as_solid(), expected.background.as_solid());
                        assert_eq!(
                            actual.border_widths, expected.border_widths,
                            "stroke metric at scale {scale}"
                        );
                        assert_eq!(
                            actual.border_color.a, expected.border_color.a,
                            "stroke alpha at scale {scale}"
                        );
                        if actual.border_color.a > 0.0 {
                            assert_eq!(actual.border_color, expected.border_color);
                        }
                    }
                }
            }
        }
    }
}
