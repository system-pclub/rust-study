/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use crate::canvas_state::{CanvasContextState, CanvasState};
use crate::dom::bindings::cell::DomRefCell;
use crate::dom::bindings::codegen::Bindings::CanvasRenderingContext2DBinding;
use crate::dom::bindings::codegen::Bindings::CanvasRenderingContext2DBinding::CanvasFillRule;
use crate::dom::bindings::codegen::Bindings::CanvasRenderingContext2DBinding::CanvasImageSource;
use crate::dom::bindings::codegen::Bindings::CanvasRenderingContext2DBinding::CanvasLineCap;
use crate::dom::bindings::codegen::Bindings::CanvasRenderingContext2DBinding::CanvasLineJoin;
use crate::dom::bindings::codegen::Bindings::CanvasRenderingContext2DBinding::CanvasRenderingContext2DMethods;
use crate::dom::bindings::codegen::UnionTypes::StringOrCanvasGradientOrCanvasPattern;
use crate::dom::bindings::error::{ErrorResult, Fallible};
use crate::dom::bindings::num::Finite;
use crate::dom::bindings::reflector::{reflect_dom_object, DomObject, Reflector};
use crate::dom::bindings::root::{Dom, DomRoot, LayoutDom};
use crate::dom::bindings::str::DOMString;
use crate::dom::canvasgradient::CanvasGradient;
use crate::dom::canvaspattern::CanvasPattern;
use crate::dom::dommatrix::DOMMatrix;
use crate::dom::globalscope::GlobalScope;
use crate::dom::htmlcanvaselement::HTMLCanvasElement;
use crate::dom::imagedata::ImageData;
use crate::dom::textmetrics::TextMetrics;
use crate::euclidext::Size2DExt;
use canvas_traits::canvas::{Canvas2dMsg, CanvasId, CanvasMsg};
use dom_struct::dom_struct;
use euclid::default::{Point2D, Rect, Size2D};
use ipc_channel::ipc::IpcSender;
use servo_url::ServoUrl;
use std::mem;

// https://html.spec.whatwg.org/multipage/#canvasrenderingcontext2d
#[dom_struct]
pub struct CanvasRenderingContext2D {
    reflector_: Reflector,
    /// For rendering contexts created by an HTML canvas element, this is Some,
    /// for ones created by a paint worklet, this is None.
    canvas: Option<Dom<HTMLCanvasElement>>,
    canvas_state: DomRefCell<CanvasState>,
}

impl CanvasRenderingContext2D {
    pub fn new_inherited(
        global: &GlobalScope,
        canvas: Option<&HTMLCanvasElement>,
        size: Size2D<u32>,
    ) -> CanvasRenderingContext2D {
        CanvasRenderingContext2D {
            reflector_: Reflector::new(),
            canvas: canvas.map(Dom::from_ref),
            canvas_state: DomRefCell::new(CanvasState::new(
                global,
                Size2D::new(size.width as u64, size.height as u64),
            )),
        }
    }

    pub fn new(
        global: &GlobalScope,
        canvas: &HTMLCanvasElement,
        size: Size2D<u32>,
    ) -> DomRoot<CanvasRenderingContext2D> {
        let boxed = Box::new(CanvasRenderingContext2D::new_inherited(
            global,
            Some(canvas),
            size,
        ));
        reflect_dom_object(boxed, global, CanvasRenderingContext2DBinding::Wrap)
    }

    // https://html.spec.whatwg.org/multipage/#concept-canvas-set-bitmap-dimensions
    pub fn set_bitmap_dimensions(&self, size: Size2D<u32>) {
        self.reset_to_initial_state();
        self.canvas_state
            .borrow()
            .get_ipc_renderer()
            .send(CanvasMsg::Recreate(
                size.to_u64(),
                self.canvas_state.borrow().get_canvas_id(),
            ))
            .unwrap();
    }

    //  TODO: This duplicates functionality in canvas state
    // https://html.spec.whatwg.org/multipage/#reset-the-rendering-context-to-its-default-state
    fn reset_to_initial_state(&self) {
        self.canvas_state
            .borrow()
            .get_saved_state()
            .borrow_mut()
            .clear();
        *self.canvas_state.borrow().get_state().borrow_mut() = CanvasContextState::new();
    }
    /*
        pub fn get_canvas_state(&self) -> Ref<CanvasState> {
            self.canvas_state.borrow()
        }
    */

    pub fn set_canvas_bitmap_dimensions(&self, size: Size2D<u64>) {
        self.canvas_state.borrow().set_bitmap_dimensions(size);
    }

    pub fn mark_as_dirty(&self) {
        self.canvas_state
            .borrow()
            .mark_as_dirty(self.canvas.as_ref().map(|c| &**c))
    }

    pub fn take_missing_image_urls(&self) -> Vec<ServoUrl> {
        mem::replace(
            &mut self
                .canvas_state
                .borrow()
                .get_missing_image_urls()
                .borrow_mut(),
            vec![],
        )
    }

    pub fn get_canvas_id(&self) -> CanvasId {
        self.canvas_state.borrow().get_canvas_id()
    }

    pub fn send_canvas_2d_msg(&self, msg: Canvas2dMsg) {
        self.canvas_state.borrow().send_canvas_2d_msg(msg)
    }

    // TODO: Remove this
    pub fn get_ipc_renderer(&self) -> IpcSender<CanvasMsg> {
        self.canvas_state.borrow().get_ipc_renderer().clone()
    }

    pub fn origin_is_clean(&self) -> bool {
        self.canvas_state.borrow().origin_is_clean()
    }

    pub fn get_rect(&self, rect: Rect<u32>) -> Vec<u8> {
        let rect = Rect::new(
            Point2D::new(rect.origin.x as u64, rect.origin.y as u64),
            Size2D::new(rect.size.width as u64, rect.size.height as u64),
        );
        self.canvas_state.borrow().get_rect(
            self.canvas
                .as_ref()
                .map_or(Size2D::zero(), |c| c.get_size().to_u64()),
            rect,
        )
    }
}

pub trait LayoutCanvasRenderingContext2DHelpers {
    #[allow(unsafe_code)]
    unsafe fn get_ipc_renderer(&self) -> IpcSender<CanvasMsg>;
    #[allow(unsafe_code)]
    unsafe fn get_canvas_id(&self) -> CanvasId;
}

impl LayoutCanvasRenderingContext2DHelpers for LayoutDom<CanvasRenderingContext2D> {
    #[allow(unsafe_code)]
    unsafe fn get_ipc_renderer(&self) -> IpcSender<CanvasMsg> {
        (*self.unsafe_get())
            .canvas_state
            .borrow_for_layout()
            .get_ipc_renderer()
            .clone()
    }

    #[allow(unsafe_code)]
    unsafe fn get_canvas_id(&self) -> CanvasId {
        (*self.unsafe_get())
            .canvas_state
            .borrow_for_layout()
            .get_canvas_id()
    }
}

// We add a guard to each of methods by the spec:
// http://www.w3.org/html/wg/drafts/2dcontext/html5_canvas_CR/
//
// > Except where otherwise specified, for the 2D context interface,
// > any method call with a numeric argument whose value is infinite or a NaN value must be ignored.
//
//  Restricted values are guarded in glue code. Therefore we need not add a guard.
//
// FIXME: this behavior should might be generated by some annotattions to idl.
impl CanvasRenderingContext2DMethods for CanvasRenderingContext2D {
    // https://html.spec.whatwg.org/multipage/#dom-context-2d-canvas
    fn Canvas(&self) -> DomRoot<HTMLCanvasElement> {
        // This method is not called from a paint worklet rendering context,
        // so it's OK to panic if self.canvas is None.
        DomRoot::from_ref(self.canvas.as_ref().expect("No canvas."))
    }

    // https://html.spec.whatwg.org/multipage/#dom-context-2d-save
    fn Save(&self) {
        self.canvas_state.borrow().save()
    }

    #[allow(unrooted_must_root)]
    // https://html.spec.whatwg.org/multipage/#dom-context-2d-restore
    fn Restore(&self) {
        self.canvas_state.borrow().restore()
    }

    // https://html.spec.whatwg.org/multipage/#dom-context-2d-scale
    fn Scale(&self, x: f64, y: f64) {
        self.canvas_state.borrow().scale(x, y)
    }

    // https://html.spec.whatwg.org/multipage/#dom-context-2d-rotate
    fn Rotate(&self, angle: f64) {
        self.canvas_state.borrow().rotate(angle)
    }

    // https://html.spec.whatwg.org/multipage/#dom-context-2d-translate
    fn Translate(&self, x: f64, y: f64) {
        self.canvas_state.borrow().translate(x, y)
    }

    // https://html.spec.whatwg.org/multipage/#dom-context-2d-transform
    fn Transform(&self, a: f64, b: f64, c: f64, d: f64, e: f64, f: f64) {
        self.canvas_state.borrow().transform(a, b, c, d, e, f)
    }

    // https://html.spec.whatwg.org/multipage/#dom-context-2d-gettransform
    fn GetTransform(&self) -> DomRoot<DOMMatrix> {
        self.canvas_state.borrow().get_transform(&self.global())
    }

    // https://html.spec.whatwg.org/multipage/#dom-context-2d-settransform
    fn SetTransform(&self, a: f64, b: f64, c: f64, d: f64, e: f64, f: f64) {
        self.canvas_state.borrow().set_transform(a, b, c, d, e, f)
    }

    // https://html.spec.whatwg.org/multipage/#dom-context-2d-resettransform
    fn ResetTransform(&self) {
        self.canvas_state.borrow().reset_transform()
    }

    // https://html.spec.whatwg.org/multipage/#dom-context-2d-globalalpha
    fn GlobalAlpha(&self) -> f64 {
        self.canvas_state.borrow().global_alpha()
    }

    // https://html.spec.whatwg.org/multipage/#dom-context-2d-globalalpha
    fn SetGlobalAlpha(&self, alpha: f64) {
        self.canvas_state.borrow().set_global_alpha(alpha)
    }

    // https://html.spec.whatwg.org/multipage/#dom-context-2d-globalcompositeoperation
    fn GlobalCompositeOperation(&self) -> DOMString {
        self.canvas_state.borrow().global_composite_operation()
    }

    // https://html.spec.whatwg.org/multipage/#dom-context-2d-globalcompositeoperation
    fn SetGlobalCompositeOperation(&self, op_str: DOMString) {
        self.canvas_state
            .borrow()
            .set_global_composite_operation(op_str)
    }

    // https://html.spec.whatwg.org/multipage/#dom-context-2d-fillrect
    fn FillRect(&self, x: f64, y: f64, width: f64, height: f64) {
        self.canvas_state.borrow().fill_rect(x, y, width, height);
        self.mark_as_dirty();
    }

    // https://html.spec.whatwg.org/multipage/#dom-context-2d-clearrect
    fn ClearRect(&self, x: f64, y: f64, width: f64, height: f64) {
        self.canvas_state.borrow().clear_rect(x, y, width, height);
        self.mark_as_dirty();
    }

    // https://html.spec.whatwg.org/multipage/#dom-context-2d-strokerect
    fn StrokeRect(&self, x: f64, y: f64, width: f64, height: f64) {
        self.canvas_state.borrow().stroke_rect(x, y, width, height);
        self.mark_as_dirty();
    }

    // https://html.spec.whatwg.org/multipage/#dom-context-2d-beginpath
    fn BeginPath(&self) {
        self.canvas_state.borrow().begin_path()
    }

    // https://html.spec.whatwg.org/multipage/#dom-context-2d-closepath
    fn ClosePath(&self) {
        self.canvas_state.borrow().close_path()
    }

    // https://html.spec.whatwg.org/multipage/#dom-context-2d-fill
    fn Fill(&self, fill_rule: CanvasFillRule) {
        self.canvas_state.borrow().fill(fill_rule);
        self.mark_as_dirty();
    }

    // https://html.spec.whatwg.org/multipage/#dom-context-2d-stroke
    fn Stroke(&self) {
        self.canvas_state.borrow().stroke();
        self.mark_as_dirty();
    }

    // https://html.spec.whatwg.org/multipage/#dom-context-2d-clip
    fn Clip(&self, fill_rule: CanvasFillRule) {
        self.canvas_state.borrow().clip(fill_rule)
    }

    // https://html.spec.whatwg.org/multipage/#dom-context-2d-ispointinpath
    fn IsPointInPath(&self, x: f64, y: f64, fill_rule: CanvasFillRule) -> bool {
        self.canvas_state
            .borrow()
            .is_point_in_path(&self.global(), x, y, fill_rule)
    }

    // https://html.spec.whatwg.org/multipage/#dom-context-2d-filltext
    fn FillText(&self, text: DOMString, x: f64, y: f64, max_width: Option<f64>) {
        self.canvas_state.borrow().fill_text(text, x, y, max_width);
        self.mark_as_dirty();
    }

    // https://html.spec.whatwg.org/multipage/#textmetrics
    fn MeasureText(&self, text: DOMString) -> DomRoot<TextMetrics> {
        self.canvas_state
            .borrow()
            .measure_text(&self.global(), text)
    }

    // https://html.spec.whatwg.org/multipage/#dom-context-2d-drawimage
    fn DrawImage(&self, image: CanvasImageSource, dx: f64, dy: f64) -> ErrorResult {
        self.canvas_state
            .borrow()
            .draw_image(self.canvas.as_ref().map(|c| &**c), image, dx, dy)
    }

    // https://html.spec.whatwg.org/multipage/#dom-context-2d-drawimage
    fn DrawImage_(
        &self,
        image: CanvasImageSource,
        dx: f64,
        dy: f64,
        dw: f64,
        dh: f64,
    ) -> ErrorResult {
        self.canvas_state.borrow().draw_image_(
            self.canvas.as_ref().map(|c| &**c),
            image,
            dx,
            dy,
            dw,
            dh,
        )
    }

    // https://html.spec.whatwg.org/multipage/#dom-context-2d-drawimage
    fn DrawImage__(
        &self,
        image: CanvasImageSource,
        sx: f64,
        sy: f64,
        sw: f64,
        sh: f64,
        dx: f64,
        dy: f64,
        dw: f64,
        dh: f64,
    ) -> ErrorResult {
        self.canvas_state.borrow().draw_image__(
            self.canvas.as_ref().map(|c| &**c),
            image,
            sx,
            sy,
            sw,
            sh,
            dx,
            dy,
            dw,
            dh,
        )
    }

    // https://html.spec.whatwg.org/multipage/#dom-context-2d-moveto
    fn MoveTo(&self, x: f64, y: f64) {
        self.canvas_state.borrow().move_to(x, y)
    }

    // https://html.spec.whatwg.org/multipage/#dom-context-2d-lineto
    fn LineTo(&self, x: f64, y: f64) {
        self.canvas_state.borrow().line_to(x, y)
    }

    // https://html.spec.whatwg.org/multipage/#dom-context-2d-rect
    fn Rect(&self, x: f64, y: f64, width: f64, height: f64) {
        self.canvas_state.borrow().rect(x, y, width, height)
    }

    // https://html.spec.whatwg.org/multipage/#dom-context-2d-quadraticcurveto
    fn QuadraticCurveTo(&self, cpx: f64, cpy: f64, x: f64, y: f64) {
        self.canvas_state
            .borrow()
            .quadratic_curve_to(cpx, cpy, x, y)
    }

    // https://html.spec.whatwg.org/multipage/#dom-context-2d-beziercurveto
    fn BezierCurveTo(&self, cp1x: f64, cp1y: f64, cp2x: f64, cp2y: f64, x: f64, y: f64) {
        self.canvas_state
            .borrow()
            .bezier_curve_to(cp1x, cp1y, cp2x, cp2y, x, y)
    }

    // https://html.spec.whatwg.org/multipage/#dom-context-2d-arc
    fn Arc(&self, x: f64, y: f64, r: f64, start: f64, end: f64, ccw: bool) -> ErrorResult {
        self.canvas_state.borrow().arc(x, y, r, start, end, ccw)
    }

    // https://html.spec.whatwg.org/multipage/#dom-context-2d-arcto
    fn ArcTo(&self, cp1x: f64, cp1y: f64, cp2x: f64, cp2y: f64, r: f64) -> ErrorResult {
        self.canvas_state.borrow().arc_to(cp1x, cp1y, cp2x, cp2y, r)
    }

    // https://html.spec.whatwg.org/multipage/#dom-context-2d-ellipse
    fn Ellipse(
        &self,
        x: f64,
        y: f64,
        rx: f64,
        ry: f64,
        rotation: f64,
        start: f64,
        end: f64,
        ccw: bool,
    ) -> ErrorResult {
        self.canvas_state
            .borrow()
            .ellipse(x, y, rx, ry, rotation, start, end, ccw)
    }

    // https://html.spec.whatwg.org/multipage/#dom-context-2d-imagesmoothingenabled
    fn ImageSmoothingEnabled(&self) -> bool {
        self.canvas_state.borrow().image_smoothing_enabled()
    }

    // https://html.spec.whatwg.org/multipage/#dom-context-2d-imagesmoothingenabled
    fn SetImageSmoothingEnabled(&self, value: bool) {
        self.canvas_state
            .borrow()
            .set_image_smoothing_enabled(value)
    }

    // https://html.spec.whatwg.org/multipage/#dom-context-2d-strokestyle
    fn StrokeStyle(&self) -> StringOrCanvasGradientOrCanvasPattern {
        self.canvas_state.borrow().stroke_style()
    }

    // https://html.spec.whatwg.org/multipage/#dom-context-2d-strokestyle
    fn SetStrokeStyle(&self, value: StringOrCanvasGradientOrCanvasPattern) {
        self.canvas_state
            .borrow()
            .set_stroke_style(self.canvas.as_ref().map(|c| &**c), value)
    }

    // https://html.spec.whatwg.org/multipage/#dom-context-2d-strokestyle
    fn FillStyle(&self) -> StringOrCanvasGradientOrCanvasPattern {
        self.canvas_state.borrow().fill_style()
    }

    // https://html.spec.whatwg.org/multipage/#dom-context-2d-strokestyle
    fn SetFillStyle(&self, value: StringOrCanvasGradientOrCanvasPattern) {
        self.canvas_state
            .borrow()
            .set_fill_style(self.canvas.as_ref().map(|c| &**c), value)
    }

    // https://html.spec.whatwg.org/multipage/#dom-context-2d-createimagedata
    fn CreateImageData(&self, sw: i32, sh: i32) -> Fallible<DomRoot<ImageData>> {
        self.canvas_state
            .borrow()
            .create_image_data(&self.global(), sw, sh)
    }

    // https://html.spec.whatwg.org/multipage/#dom-context-2d-createimagedata
    fn CreateImageData_(&self, imagedata: &ImageData) -> Fallible<DomRoot<ImageData>> {
        self.canvas_state
            .borrow()
            .create_image_data_(&self.global(), imagedata)
    }

    // https://html.spec.whatwg.org/multipage/#dom-context-2d-getimagedata
    fn GetImageData(&self, sx: i32, sy: i32, sw: i32, sh: i32) -> Fallible<DomRoot<ImageData>> {
        self.canvas_state.borrow().get_image_data(
            self.canvas
                .as_ref()
                .map_or(Size2D::zero(), |c| c.get_size().to_u64()),
            &self.global(),
            sx,
            sy,
            sw,
            sh,
        )
    }

    // https://html.spec.whatwg.org/multipage/#dom-context-2d-putimagedata
    fn PutImageData(&self, imagedata: &ImageData, dx: i32, dy: i32) {
        self.canvas_state.borrow().put_image_data(
            self.canvas
                .as_ref()
                .map_or(Size2D::zero(), |c| c.get_size().to_u64()),
            imagedata,
            dx,
            dy,
        )
    }

    // https://html.spec.whatwg.org/multipage/#dom-context-2d-putimagedata
    #[allow(unsafe_code)]
    fn PutImageData_(
        &self,
        imagedata: &ImageData,
        dx: i32,
        dy: i32,
        dirty_x: i32,
        dirty_y: i32,
        dirty_width: i32,
        dirty_height: i32,
    ) {
        self.canvas_state.borrow().put_image_data_(
            self.canvas
                .as_ref()
                .map_or(Size2D::zero(), |c| c.get_size().to_u64()),
            imagedata,
            dx,
            dy,
            dirty_x,
            dirty_y,
            dirty_width,
            dirty_height,
        );
        self.mark_as_dirty();
    }

    // https://html.spec.whatwg.org/multipage/#dom-context-2d-createlineargradient
    fn CreateLinearGradient(
        &self,
        x0: Finite<f64>,
        y0: Finite<f64>,
        x1: Finite<f64>,
        y1: Finite<f64>,
    ) -> DomRoot<CanvasGradient> {
        self.canvas_state
            .borrow()
            .create_linear_gradient(&self.global(), x0, y0, x1, y1)
    }

    // https://html.spec.whatwg.org/multipage/#dom-context-2d-createradialgradient
    fn CreateRadialGradient(
        &self,
        x0: Finite<f64>,
        y0: Finite<f64>,
        r0: Finite<f64>,
        x1: Finite<f64>,
        y1: Finite<f64>,
        r1: Finite<f64>,
    ) -> Fallible<DomRoot<CanvasGradient>> {
        self.canvas_state
            .borrow()
            .create_radial_gradient(&self.global(), x0, y0, r0, x1, y1, r1)
    }

    // https://html.spec.whatwg.org/multipage/#dom-context-2d-createpattern
    fn CreatePattern(
        &self,
        image: CanvasImageSource,
        repetition: DOMString,
    ) -> Fallible<Option<DomRoot<CanvasPattern>>> {
        self.canvas_state
            .borrow()
            .create_pattern(&self.global(), image, repetition)
    }

    // https://html.spec.whatwg.org/multipage/#dom-context-2d-linewidth
    fn LineWidth(&self) -> f64 {
        self.canvas_state.borrow().line_width()
    }

    // https://html.spec.whatwg.org/multipage/#dom-context-2d-linewidth
    fn SetLineWidth(&self, width: f64) {
        self.canvas_state.borrow().set_line_width(width)
    }

    // https://html.spec.whatwg.org/multipage/#dom-context-2d-linecap
    fn LineCap(&self) -> CanvasLineCap {
        self.canvas_state.borrow().line_cap()
    }

    // https://html.spec.whatwg.org/multipage/#dom-context-2d-linecap
    fn SetLineCap(&self, cap: CanvasLineCap) {
        self.canvas_state.borrow().set_line_cap(cap)
    }

    // https://html.spec.whatwg.org/multipage/#dom-context-2d-linejoin
    fn LineJoin(&self) -> CanvasLineJoin {
        self.canvas_state.borrow().line_join()
    }

    // https://html.spec.whatwg.org/multipage/#dom-context-2d-linejoin
    fn SetLineJoin(&self, join: CanvasLineJoin) {
        self.canvas_state.borrow().set_line_join(join)
    }

    // https://html.spec.whatwg.org/multipage/#dom-context-2d-miterlimit
    fn MiterLimit(&self) -> f64 {
        self.canvas_state.borrow().miter_limit()
    }

    // https://html.spec.whatwg.org/multipage/#dom-context-2d-miterlimit
    fn SetMiterLimit(&self, limit: f64) {
        self.canvas_state.borrow().set_miter_limit(limit)
    }

    // https://html.spec.whatwg.org/multipage/#dom-context-2d-shadowoffsetx
    fn ShadowOffsetX(&self) -> f64 {
        self.canvas_state.borrow().shadow_offset_x()
    }

    // https://html.spec.whatwg.org/multipage/#dom-context-2d-shadowoffsetx
    fn SetShadowOffsetX(&self, value: f64) {
        self.canvas_state.borrow().set_shadow_offset_x(value)
    }

    // https://html.spec.whatwg.org/multipage/#dom-context-2d-shadowoffsety
    fn ShadowOffsetY(&self) -> f64 {
        self.canvas_state.borrow().shadow_offset_y()
    }

    // https://html.spec.whatwg.org/multipage/#dom-context-2d-shadowoffsety
    fn SetShadowOffsetY(&self, value: f64) {
        self.canvas_state.borrow().set_shadow_offset_y(value)
    }

    // https://html.spec.whatwg.org/multipage/#dom-context-2d-shadowblur
    fn ShadowBlur(&self) -> f64 {
        self.canvas_state.borrow().shadow_blur()
    }

    // https://html.spec.whatwg.org/multipage/#dom-context-2d-shadowblur
    fn SetShadowBlur(&self, value: f64) {
        self.canvas_state.borrow().set_shadow_blur(value)
    }

    // https://html.spec.whatwg.org/multipage/#dom-context-2d-shadowcolor
    fn ShadowColor(&self) -> DOMString {
        self.canvas_state.borrow().shadow_color()
    }

    // https://html.spec.whatwg.org/multipage/#dom-context-2d-shadowcolor
    fn SetShadowColor(&self, value: DOMString) {
        self.canvas_state.borrow().set_shadow_color(value)
    }
}

impl Drop for CanvasRenderingContext2D {
    fn drop(&mut self) {
        if let Err(err) = self
            .canvas_state
            .borrow()
            .get_ipc_renderer()
            .send(CanvasMsg::Close(self.canvas_state.borrow().get_canvas_id()))
        {
            warn!("Could not close canvas: {}", err)
        }
    }
}
