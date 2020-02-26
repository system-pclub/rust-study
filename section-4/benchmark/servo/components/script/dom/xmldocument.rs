/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use crate::document_loader::DocumentLoader;
use crate::dom::bindings::codegen::Bindings::DocumentBinding::DocumentMethods;
use crate::dom::bindings::codegen::Bindings::XMLDocumentBinding::{self, XMLDocumentMethods};
use crate::dom::bindings::inheritance::Castable;
use crate::dom::bindings::reflector::reflect_dom_object;
use crate::dom::bindings::root::DomRoot;
use crate::dom::bindings::str::DOMString;
use crate::dom::document::{Document, DocumentSource, HasBrowsingContext, IsHTMLDocument};
use crate::dom::location::Location;
use crate::dom::node::Node;
use crate::dom::window::Window;
use crate::script_runtime::JSContext;
use dom_struct::dom_struct;
use js::jsapi::JSObject;
use mime::Mime;
use script_traits::DocumentActivity;
use servo_url::{MutableOrigin, ServoUrl};
use std::ptr::NonNull;

// https://dom.spec.whatwg.org/#xmldocument
#[dom_struct]
pub struct XMLDocument {
    document: Document,
}

impl XMLDocument {
    fn new_inherited(
        window: &Window,
        has_browsing_context: HasBrowsingContext,
        url: Option<ServoUrl>,
        origin: MutableOrigin,
        is_html_document: IsHTMLDocument,
        content_type: Option<Mime>,
        last_modified: Option<String>,
        activity: DocumentActivity,
        source: DocumentSource,
        doc_loader: DocumentLoader,
    ) -> XMLDocument {
        XMLDocument {
            document: Document::new_inherited(
                window,
                has_browsing_context,
                url,
                origin,
                is_html_document,
                content_type,
                last_modified,
                activity,
                source,
                doc_loader,
                None,
                None,
                Default::default(),
            ),
        }
    }

    pub fn new(
        window: &Window,
        has_browsing_context: HasBrowsingContext,
        url: Option<ServoUrl>,
        origin: MutableOrigin,
        doctype: IsHTMLDocument,
        content_type: Option<Mime>,
        last_modified: Option<String>,
        activity: DocumentActivity,
        source: DocumentSource,
        doc_loader: DocumentLoader,
    ) -> DomRoot<XMLDocument> {
        let doc = reflect_dom_object(
            Box::new(XMLDocument::new_inherited(
                window,
                has_browsing_context,
                url,
                origin,
                doctype,
                content_type,
                last_modified,
                activity,
                source,
                doc_loader,
            )),
            window,
            XMLDocumentBinding::Wrap,
        );
        {
            let node = doc.upcast::<Node>();
            node.set_owner_doc(&doc.document);
        }
        doc
    }
}

impl XMLDocumentMethods for XMLDocument {
    // https://html.spec.whatwg.org/multipage/#dom-document-location
    fn GetLocation(&self) -> Option<DomRoot<Location>> {
        self.upcast::<Document>().GetLocation()
    }

    // https://html.spec.whatwg.org/multipage/#dom-tree-accessors:supported-property-names
    fn SupportedPropertyNames(&self) -> Vec<DOMString> {
        self.upcast::<Document>().SupportedPropertyNames()
    }

    // https://html.spec.whatwg.org/multipage/#dom-tree-accessors:dom-document-nameditem-filter
    fn NamedGetter(&self, _cx: JSContext, name: DOMString) -> Option<NonNull<JSObject>> {
        self.upcast::<Document>().NamedGetter(_cx, name)
    }
}
