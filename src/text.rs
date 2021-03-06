use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

use gfx::Encoder;
use gfx::handle::{DepthStencilView, RenderTargetView};
use gfx_glyph as g;
use mint;
use object;

use color::Color;
use hub::Operation as HubOperation;
use render::{BackendCommandBuffer, BackendFactory, BackendResources, ColorFormat, DepthFormat};

#[derive(Debug)]
pub(crate) enum Operation {
    Text(String),
    Font(Font),
    Scale(f32),
    Pos(mint::Point2<f32>),
    Size(mint::Vector2<f32>),
    Color(Color),
    Opacity(f32),
    Layout(Layout),
}

/// Describes the horizontal alignment preference for positioning & bounds.
/// See [`gfx_glyph::HorizontalAlign`](https://docs.rs/gfx_glyph/0.13.0/gfx_glyph/enum.HorizontalAlign.html)
/// for more.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Align {
    /// Leftmost character is immediately to the right of the render position.
    /// Bounds start from the render position and advance rightwards.
    Left,
    /// Leftmost & rightmost characters are equidistant to the render position.
    /// Bounds start from the render position and advance equally left & right.
    Center,
    /// Rightmost character is immediately to the left of the render position.
    /// Bounds start from the render position and advance leftwards.
    Right,
}

/// Describes text alignment & wrapping.
/// See [`gfx_glyph::Layout`](https://docs.rs/gfx_glyph/0.13.0/gfx_glyph/enum.Layout.html).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Layout {
    /// Renders a single line from left-to-right according to the inner alignment.
    SingleLine(Align),
    /// Renders multiple lines from left-to-right according to the inner alignment.
    Wrap(Align),
}

impl Default for Layout {
    fn default() -> Self {
        Layout::SingleLine(Align::Left)
    }
}

impl From<Align> for g::HorizontalAlign {
    fn from(align: Align) -> g::HorizontalAlign {
        match align {
            Align::Left => g::HorizontalAlign::Left,
            Align::Center => g::HorizontalAlign::Center,
            Align::Right => g::HorizontalAlign::Right,
        }
    }
}

impl From<Layout> for g::Layout<g::BuiltInLineBreaker> {
    fn from(layout: Layout) -> g::Layout<g::BuiltInLineBreaker> {
        match layout {
            Layout::Wrap(a) => g::Layout::Wrap {
                line_breaker: g::BuiltInLineBreaker::UnicodeLineBreaker,
                h_align: a.into(),
                v_align: g::VerticalAlign::Top,
            },
            Layout::SingleLine(a) => g::Layout::SingleLine {
                line_breaker: g::BuiltInLineBreaker::UnicodeLineBreaker,
                h_align: a.into(),
                v_align: g::VerticalAlign::Top,
            },
        }
    }
}

/// Smart pointer containing a font to draw text.
#[derive(Clone)]
pub struct Font {
    brush: Rc<RefCell<g::GlyphBrush<'static, BackendResources, BackendFactory>>>,
    pub(crate) id: String,
}

impl Font {
    pub(crate) fn new<T: Into<g::SharedBytes<'static>>>(
        buf: T,
        id: String,
        factory: BackendFactory,
    ) -> Font {
        Font {
            brush: Rc::new(RefCell::new(
                g::GlyphBrushBuilder::using_font_bytes(buf).build(factory),
            )),
            id: id,
        }
    }

    pub(crate) fn queue(
        &self,
        section: &g::OwnedVariedSection,
    ) {
        let mut brush = self.brush.borrow_mut();
        brush.queue(section);
    }

    pub(crate) fn draw(
        &self,
        encoder: &mut Encoder<BackendResources, BackendCommandBuffer>,
        out: &RenderTargetView<BackendResources, ColorFormat>,
        depth: &DepthStencilView<BackendResources, DepthFormat>,
    ) {
        let mut brush = self.brush.borrow_mut();
        brush
            .draw_queued(encoder, out, depth)
            .expect("Error while drawing text");
    }
}

impl fmt::Debug for Font {
    fn fmt(
        &self,
        f: &mut fmt::Formatter,
    ) -> fmt::Result {
        write!(f, "Font {{ {} }}", self.id)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct TextData {
    pub(crate) section: g::OwnedVariedSection,
    pub(crate) font: Font,
}

impl TextData {
    pub(crate) fn new<S: Into<String>>(
        font: &Font,
        text: S,
    ) -> Self {
        TextData {
            section: g::OwnedVariedSection {
                text: vec![
                    g::OwnedSectionText {
                        color: [1.0, 1.0, 1.0, 1.0],
                        text: text.into(),
                        ..g::OwnedSectionText::default()
                    },
                ],
                ..Default::default()
            },
            font: font.clone(),
        }
    }
}

/// UI (on-screen) text.
/// To use, create the new one using [`Factory::ui_text`](struct.Factory.html#method.ui_text)
/// and add it to the scene using [`Scene::add`](struct.Scene.html#method.add).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Text {
    pub(crate) object: object::Base,
}
three_object!(Text::object);
derive_DowncastObject!(Text => object::ObjectType::Text);

impl Text {
    pub(crate) fn with_object(object: object::Base) -> Self {
        Text { object }
    }

    /// Change text.
    pub fn set_text<S: Into<String>>(
        &mut self,
        text: S,
    ) {
        let msg = HubOperation::SetText(Operation::Text(text.into()));
        let _ = self.object.tx.send((self.object.node.downgrade(), msg));
    }

    /// Change font.
    pub fn set_font(
        &mut self,
        font: &Font,
    ) {
        let msg = HubOperation::SetText(Operation::Font(font.clone()));
        let _ = self.object.tx.send((self.object.node.downgrade(), msg));
    }

    /// Change text position.
    /// Coordinates in pixels from top-left.
    /// Defaults to (0, 0).
    pub fn set_pos<P: Into<mint::Point2<f32>>>(
        &mut self,
        point: P,
    ) {
        let msg = HubOperation::SetText(Operation::Pos(point.into()));
        let _ = self.object.tx.send((self.object.node.downgrade(), msg));
    }

    /// Change maximum bounds size, in pixels from top-left.
    /// Defaults to unbound.
    pub fn set_size<V: Into<mint::Vector2<f32>>>(
        &mut self,
        dimensions: V,
    ) {
        let msg = HubOperation::SetText(Operation::Size(dimensions.into()));
        let _ = self.object.tx.send((self.object.node.downgrade(), msg));
    }

    /// Change text color.
    /// Defaults to white (`0xFFFFFF`).
    pub fn set_color(
        &mut self,
        color: Color,
    ) {
        let msg = HubOperation::SetText(Operation::Color(color));
        let _ = self.object.tx.send((self.object.node.downgrade(), msg));
    }

    /// Change text opacity.
    /// From `0.0` to `1.0`.
    /// Defaults to `1.0`.
    pub fn set_opacity(
        &mut self,
        opacity: f32,
    ) {
        let msg = HubOperation::SetText(Operation::Opacity(opacity));
        let _ = self.object.tx.send((self.object.node.downgrade(), msg));
    }

    /// Change font size (scale).
    /// Defaults to 16.
    pub fn set_font_size(
        &mut self,
        size: f32,
    ) {
        let msg = HubOperation::SetText(Operation::Scale(size));
        let _ = self.object.tx.send((self.object.node.downgrade(), msg));
    }

    /// Change text layout.
    /// Defaults to `Layout::SingleLine(Align::Left)`.
    pub fn set_layout(
        &mut self,
        layout: Layout,
    ) {
        let msg = HubOperation::SetText(Operation::Layout(layout));
        let _ = self.object.tx.send((self.object.node.downgrade(), msg));
    }
}
