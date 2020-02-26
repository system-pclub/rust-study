/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use super::{
    constants as webgl, ext_constants as gl, WebGLExtension, WebGLExtensionSpec, WebGLExtensions,
};
use crate::dom::bindings::codegen::Bindings::OESTextureFloatBinding;
use crate::dom::bindings::reflector::{reflect_dom_object, DomObject, Reflector};
use crate::dom::bindings::root::DomRoot;
use crate::dom::webglrenderingcontext::WebGLRenderingContext;
use canvas_traits::webgl::WebGLVersion;
use dom_struct::dom_struct;

#[dom_struct]
pub struct OESTextureFloat {
    reflector_: Reflector,
}

impl OESTextureFloat {
    fn new_inherited() -> OESTextureFloat {
        Self {
            reflector_: Reflector::new(),
        }
    }
}

impl WebGLExtension for OESTextureFloat {
    type Extension = OESTextureFloat;
    fn new(ctx: &WebGLRenderingContext) -> DomRoot<OESTextureFloat> {
        reflect_dom_object(
            Box::new(OESTextureFloat::new_inherited()),
            &*ctx.global(),
            OESTextureFloatBinding::Wrap,
        )
    }

    fn spec() -> WebGLExtensionSpec {
        WebGLExtensionSpec::Specific(WebGLVersion::WebGL1)
    }

    fn is_supported(ext: &WebGLExtensions) -> bool {
        ext.supports_any_gl_extension(&[
            "GL_OES_texture_float",
            "GL_ARB_texture_float",
            "GL_EXT_color_buffer_float",
        ])
    }

    fn enable(ext: &WebGLExtensions) {
        ext.enable_tex_type(webgl::FLOAT);
        ext.add_effective_tex_internal_format(webgl::RGBA, webgl::FLOAT, gl::RGBA32F);
        ext.add_effective_tex_internal_format(webgl::RGB, webgl::FLOAT, gl::RGB32F);
        ext.add_effective_tex_internal_format(webgl::LUMINANCE, webgl::FLOAT, gl::LUMINANCE32F_ARB);
        ext.add_effective_tex_internal_format(webgl::ALPHA, webgl::FLOAT, gl::ALPHA32F_ARB);
        ext.add_effective_tex_internal_format(
            webgl::LUMINANCE_ALPHA,
            webgl::FLOAT,
            gl::LUMINANCE_ALPHA32F_ARB,
        );
    }

    fn name() -> &'static str {
        "OES_texture_float"
    }
}
