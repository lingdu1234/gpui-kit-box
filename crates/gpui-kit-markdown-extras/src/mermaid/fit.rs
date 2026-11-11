//! 等比适配包装：宿主可用空间不足时，把整张图按同一比例缩小。
//!
//! Markdown 宿主把插件元素原样嵌进文档流，`render()` 拿不到可用宽高，
//! 所以由图自己在布局期测量：`request_measured_layout` 的回调能收到父容器
//! 的可用空间，据此算出 `scale = min(1, 可用/自然)`——只缩不放，哪个方向
//! 有确定约束就参与取 min（文档流纵向可滚动时高度无约束，只按宽缩）。
//! 回调返回缩放后的尺寸；`prepaint` 再按同一 scale 构建图元素，布局并定位
//! 到包装的原点。五种图共用这一个包装，空间充足时 scale=1，输出与自然
//! 尺寸逐像素一致。
//!
//! 缩放只发生在渲染映射层：各渲染器把全部坐标、字号、线宽同乘一个系数，
//! 布局算法不动，几何天然相似，不会变形。

use gpui_kit::{
    AnyElement, App, AvailableSpace, Bounds, Element, ElementId, GlobalElementId,
    InspectorElementId, IntoElement, LayoutId, Length, Pixels, Size, Style, Window, px, size,
};
use std::panic::Location;

/// 等比收缩系数：有确定约束的方向各取「可用/自然」，取最小并夹在 [0, 1]。
fn fit_scale(natural: Size<Pixels>, available: Size<AvailableSpace>) -> f32 {
    let mut scale = 1.0f32;
    if let AvailableSpace::Definite(w) = available.width
        && natural.width > px(0.)
    {
        scale = scale.min(f32::from(w) / f32::from(natural.width));
    }
    if let AvailableSpace::Definite(h) = available.height
        && natural.height > px(0.)
    {
        scale = scale.min(f32::from(h) / f32::from(natural.height));
    }
    scale.clamp(0., 1.)
}

/// 包在图元素外面的一层「测量 + 等比缩放」壳。
///
/// `build` 在 `prepaint` 期才调用：那时可用宽高已经确定，按算好的 scale
/// 构建一次图元素即可（与 `uniform_list` 在 prepaint 渲染子项同一模式）。
pub(super) struct FitDiagram {
    natural: Size<Pixels>,
    build: Option<Box<dyn FnOnce(f32, &mut Window, &mut App) -> AnyElement>>,
}

impl FitDiagram {
    pub(super) fn new(
        width: f32,
        height: f32,
        build: impl FnOnce(f32, &mut Window, &mut App) -> AnyElement + 'static,
    ) -> Self {
        Self {
            natural: size(px(width), px(height)),
            build: Some(Box::new(build)),
        }
    }
}

impl IntoElement for FitDiagram {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl Element for FitDiagram {
    type RequestLayoutState = ();
    type PrepaintState = Option<AnyElement>;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        _cx: &mut App,
    ) -> (LayoutId, ()) {
        let natural = self.natural;
        let mut style = Style::default();
        // 与自然尺寸容器的 mx_auto 一致：缩小后仍在文档流里水平居中。
        style.margin.left = Length::Auto;
        style.margin.right = Length::Auto;
        let layout_id = window.request_measured_layout(style, move |known, available, _, _| {
            let available = Size {
                width: known.width.map_or(available.width, AvailableSpace::Definite),
                height: known.height.map_or(available.height, AvailableSpace::Definite),
            };
            let scale = fit_scale(natural, available);
            size(natural.width * scale, natural.height * scale)
        });
        (layout_id, ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Option<AnyElement> {
        let available = size(
            AvailableSpace::Definite(bounds.size.width),
            AvailableSpace::Definite(bounds.size.height),
        );
        let scale = fit_scale(self.natural, available);
        let build = self.build.take().expect("prepaint 每帧只跑一次");
        let mut inner = build(scale, window, cx);
        inner.prepaint_as_root(bounds.origin, available, window, cx);
        Some(inner)
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        inner: &mut Option<AnyElement>,
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some(inner) = inner {
            inner.paint(window, cx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_only_shrinks_and_takes_the_tightest_axis() {
        let natural = size(px(800.), px(400.));
        // 空间充足：不放大。
        assert_eq!(
            fit_scale(
                natural,
                size(AvailableSpace::Definite(px(1000.)), AvailableSpace::MaxContent)
            ),
            1.
        );
        // 宽不足：按宽缩。
        assert_eq!(
            fit_scale(
                natural,
                size(AvailableSpace::Definite(px(400.)), AvailableSpace::MaxContent)
            ),
            0.5
        );
        // 高不足：按高缩。
        assert_eq!(
            fit_scale(
                natural,
                size(AvailableSpace::MaxContent, AvailableSpace::Definite(px(100.)))
            ),
            0.25
        );
        // 两向都不足：取更紧的那个，保持宽高比。
        assert_eq!(
            fit_scale(
                natural,
                size(
                    AvailableSpace::Definite(px(400.)),
                    AvailableSpace::Definite(px(100.))
                )
            ),
            0.25
        );
        // 无确定约束（可滚动文档流的常态）：自然尺寸。
        assert_eq!(
            fit_scale(
                natural,
                size(AvailableSpace::MaxContent, AvailableSpace::MinContent)
            ),
            1.
        );
    }
}