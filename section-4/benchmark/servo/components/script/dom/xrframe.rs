/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use crate::dom::bindings::codegen::Bindings::XRFrameBinding;
use crate::dom::bindings::codegen::Bindings::XRFrameBinding::XRFrameMethods;
use crate::dom::bindings::error::Error;
use crate::dom::bindings::inheritance::Castable;
use crate::dom::bindings::reflector::{reflect_dom_object, DomObject, Reflector};
use crate::dom::bindings::root::{Dom, DomRoot};
use crate::dom::globalscope::GlobalScope;
use crate::dom::xrpose::XRPose;
use crate::dom::xrreferencespace::XRReferenceSpace;
use crate::dom::xrsession::XRSession;
use crate::dom::xrspace::XRSpace;
use crate::dom::xrviewerpose::XRViewerPose;
use dom_struct::dom_struct;
use std::cell::Cell;
use webxr_api::Frame;

#[dom_struct]
pub struct XRFrame {
    reflector_: Reflector,
    session: Dom<XRSession>,
    #[ignore_malloc_size_of = "defined in rust-webvr"]
    data: Frame,
    active: Cell<bool>,
    animation_frame: Cell<bool>,
}

impl XRFrame {
    fn new_inherited(session: &XRSession, data: Frame) -> XRFrame {
        XRFrame {
            reflector_: Reflector::new(),
            session: Dom::from_ref(session),
            data,
            active: Cell::new(false),
            animation_frame: Cell::new(false),
        }
    }

    pub fn new(global: &GlobalScope, session: &XRSession, data: Frame) -> DomRoot<XRFrame> {
        reflect_dom_object(
            Box::new(XRFrame::new_inherited(session, data)),
            global,
            XRFrameBinding::Wrap,
        )
    }

    /// https://immersive-web.github.io/webxr/#xrframe-active
    pub fn set_active(&self, active: bool) {
        self.active.set(active);
    }

    /// https://immersive-web.github.io/webxr/#xrframe-animationframe
    pub fn set_animation_frame(&self, animation_frame: bool) {
        self.animation_frame.set(animation_frame);
    }
}

impl XRFrameMethods for XRFrame {
    /// https://immersive-web.github.io/webxr/#dom-xrframe-session
    fn Session(&self) -> DomRoot<XRSession> {
        DomRoot::from_ref(&self.session)
    }

    /// https://immersive-web.github.io/webxr/#dom-xrframe-getviewerpose
    fn GetViewerPose(
        &self,
        reference: &XRReferenceSpace,
    ) -> Result<Option<DomRoot<XRViewerPose>>, Error> {
        if self.session != reference.upcast::<XRSpace>().session() {
            return Err(Error::InvalidState);
        }

        if !self.active.get() || !self.animation_frame.get() {
            return Err(Error::InvalidState);
        }

        let pose = if let Some(pose) = reference.get_viewer_pose(&self.data) {
            pose
        } else {
            return Ok(None);
        };
        Ok(Some(XRViewerPose::new(&self.global(), &self.session, pose)))
    }

    /// https://immersive-web.github.io/webxr/#dom-xrframe-getpose
    fn GetPose(
        &self,
        space: &XRSpace,
        relative_to: &XRSpace,
    ) -> Result<Option<DomRoot<XRPose>>, Error> {
        if self.session != space.session() || self.session != relative_to.session() {
            return Err(Error::InvalidState);
        }
        if !self.active.get() {
            return Err(Error::InvalidState);
        }
        let space = if let Some(space) = space.get_pose(&self.data) {
            space
        } else {
            return Ok(None);
        };
        let relative_to = if let Some(r) = relative_to.get_pose(&self.data) {
            r
        } else {
            return Ok(None);
        };
        let pose = relative_to.inverse().pre_transform(&space);
        Ok(Some(XRPose::new(&self.global(), pose)))
    }
}
