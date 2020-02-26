/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use crate::dom::bindings::codegen::Bindings::HTMLMenuElementBinding;
use crate::dom::bindings::codegen::Bindings::HTMLMenuElementBinding::HTMLMenuElementMethods;
use crate::dom::bindings::root::DomRoot;
use crate::dom::document::Document;
use crate::dom::htmlelement::HTMLElement;
use crate::dom::node::Node;
use dom_struct::dom_struct;
use html5ever::{LocalName, Prefix};

#[dom_struct]
pub struct HTMLMenuElement {
    htmlelement: HTMLElement,
}

impl HTMLMenuElement {
    fn new_inherited(
        local_name: LocalName,
        prefix: Option<Prefix>,
        document: &Document,
    ) -> HTMLMenuElement {
        HTMLMenuElement {
            htmlelement: HTMLElement::new_inherited(local_name, prefix, document),
        }
    }

    #[allow(unrooted_must_root)]
    pub fn new(
        local_name: LocalName,
        prefix: Option<Prefix>,
        document: &Document,
    ) -> DomRoot<HTMLMenuElement> {
        Node::reflect_node(
            Box::new(HTMLMenuElement::new_inherited(local_name, prefix, document)),
            document,
            HTMLMenuElementBinding::Wrap,
        )
    }
}

impl HTMLMenuElementMethods for HTMLMenuElement {
    // spec just mandates that compact reflects the content attribute,
    // with no other semantics. Layout could use it to
    // change line spacing, but nothing requires it to do so.

    // https://html.spec.whatwg.org/multipage/#dom-menu-compact
    make_bool_setter!(SetCompact, "compact");

    // https://html.spec.whatwg.org/multipage/#dom-menu-compact
    make_bool_getter!(Compact, "compact");
}
