use gpui_kit::{
    AnyElement, App, Div, IntoElement, ParentElement, RenderOnce, SharedString, StyleRefinement,
    Styled, Window, div, relative, rems,
};
use gpui_kit::component::{
    ActiveTheme,
    group_box::{GroupBox, GroupBoxVariants},
    h_flex, v_flex,
};

#[derive(IntoElement)]
pub struct PageSection {
    base: Div,
    title: SharedString,
    sub_title: Vec<AnyElement>,
    children: Vec<AnyElement>,
    fill: bool,
}

impl PageSection {
    pub fn sub_title(mut self, sub_title: impl IntoElement) -> Self {
        self.sub_title.push(sub_title.into_any_element());
        self
    }

    /// 标题在上、卡片吃掉剩余高度：表格类页面用它撑满视口，
    /// 默认的 GroupBox 卡片会把内容按固有高度垂直居中。
    pub fn fill(mut self) -> Self {
        self.fill = true;
        self
    }

    #[allow(unused)]
    pub(crate) fn max_w_md(mut self) -> Self {
        self.base = self.base.max_w(rems(48.));
        self
    }

    #[allow(unused)]
    pub(crate) fn max_w_lg(mut self) -> Self {
        self.base = self.base.max_w(rems(64.));
        self
    }

    #[allow(unused)]
    pub(crate) fn max_w_xl(mut self) -> Self {
        self.base = self.base.max_w(rems(80.));
        self
    }

    #[allow(unused)]
    pub(crate) fn max_w_2xl(mut self) -> Self {
        self.base = self.base.max_w(rems(96.));
        self
    }
}

impl ParentElement for PageSection {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl Styled for PageSection {
    fn style(&mut self) -> &mut StyleRefinement {
        self.base.style()
    }
}

impl RenderOnce for PageSection {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        if self.fill {
            let theme = cx.theme();
            return v_flex()
                .w_full()
                .h_full()
                .gap_3()
                .child(
                    div()
                        .text_color(theme.muted_foreground)
                        .line_height(relative(1.25))
                        .child(
                            h_flex()
                                .justify_center()
                                .w_full()
                                .gap_4()
                                .child(self.title)
                                .children(self.sub_title),
                        ),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .min_h_0()
                        .border_1()
                        .border_color(theme.border)
                        .rounded(theme.radius_lg)
                        .overflow_x_hidden()
                        .p_4()
                        .gap_4()
                        .text_color(theme.group_box_foreground)
                        .children(self.children),
                )
                .into_any_element();
        }

        GroupBox::new()
            .id(self.title.clone())
            .outline()
            .title(
                h_flex()
                    .justify_center()
                    .w_full()
                    .gap_4()
                    .child(self.title)
                    .children(self.sub_title),
            )
            .content_style(
                StyleRefinement::default()
                    .rounded(cx.theme().radius_lg)
                    .overflow_x_hidden(),
            )
            .child(self.base.children(self.children))
            .into_any_element()
    }
}

/// A card-like section used to group page content.
pub fn section(title: impl Into<SharedString>) -> PageSection {
    PageSection {
        title: title.into(),
        sub_title: vec![],
        base: h_flex()
            .flex_wrap()
            .justify_center()
            .items_center()
            .w_full()
            .gap_4(),
        children: vec![],
        fill: false,
    }
}
