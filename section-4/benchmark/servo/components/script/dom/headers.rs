/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use crate::dom::bindings::cell::DomRefCell;
use crate::dom::bindings::codegen::Bindings::HeadersBinding::{
    HeadersInit, HeadersMethods, HeadersWrap,
};
use crate::dom::bindings::error::{Error, ErrorResult, Fallible};
use crate::dom::bindings::iterable::Iterable;
use crate::dom::bindings::reflector::{reflect_dom_object, Reflector};
use crate::dom::bindings::root::DomRoot;
use crate::dom::bindings::str::{is_token, ByteString};
use crate::dom::globalscope::GlobalScope;
use dom_struct::dom_struct;
use http::header::{self, HeaderMap as HyperHeaders, HeaderName, HeaderValue};
use net_traits::request::is_cors_safelisted_request_header;
use std::cell::Cell;
use std::str::{self, FromStr};

#[dom_struct]
pub struct Headers {
    reflector_: Reflector,
    guard: Cell<Guard>,
    #[ignore_malloc_size_of = "Defined in hyper"]
    header_list: DomRefCell<HyperHeaders>,
}

// https://fetch.spec.whatwg.org/#concept-headers-guard
#[derive(Clone, Copy, Debug, JSTraceable, MallocSizeOf, PartialEq)]
pub enum Guard {
    Immutable,
    Request,
    RequestNoCors,
    Response,
    None,
}

impl Headers {
    pub fn new_inherited() -> Headers {
        Headers {
            reflector_: Reflector::new(),
            guard: Cell::new(Guard::None),
            header_list: DomRefCell::new(HyperHeaders::new()),
        }
    }

    pub fn new(global: &GlobalScope) -> DomRoot<Headers> {
        reflect_dom_object(Box::new(Headers::new_inherited()), global, HeadersWrap)
    }

    // https://fetch.spec.whatwg.org/#dom-headers
    #[allow(non_snake_case)]
    pub fn Constructor(
        global: &GlobalScope,
        init: Option<HeadersInit>,
    ) -> Fallible<DomRoot<Headers>> {
        let dom_headers_new = Headers::new(global);
        dom_headers_new.fill(init)?;
        Ok(dom_headers_new)
    }
}

impl HeadersMethods for Headers {
    // https://fetch.spec.whatwg.org/#concept-headers-append
    fn Append(&self, name: ByteString, value: ByteString) -> ErrorResult {
        // Step 1
        let value = normalize_value(value);
        // Step 2
        let (mut valid_name, valid_value) = validate_name_and_value(name, value)?;
        valid_name = valid_name.to_lowercase();
        // Step 3
        if self.guard.get() == Guard::Immutable {
            return Err(Error::Type("Guard is immutable".to_string()));
        }
        // Step 4
        if self.guard.get() == Guard::Request && is_forbidden_header_name(&valid_name) {
            return Ok(());
        }
        // Step 5
        if self.guard.get() == Guard::RequestNoCors &&
            !is_cors_safelisted_request_header(&valid_name, &valid_value)
        {
            return Ok(());
        }
        // Step 6
        if self.guard.get() == Guard::Response && is_forbidden_response_header(&valid_name) {
            return Ok(());
        }
        // Step 7
        // FIXME: this is NOT what WHATWG says to do when appending
        // another copy of an existing header. HyperHeaders
        // might not expose the information we need to do it right.
        let mut combined_value: Vec<u8> = vec![];
        if let Some(v) = self
            .header_list
            .borrow()
            .get(HeaderName::from_str(&valid_name).unwrap())
        {
            combined_value = v.as_bytes().to_vec();
            combined_value.push(b',');
        }
        combined_value.extend(valid_value.iter().cloned());
        match HeaderValue::from_bytes(&combined_value) {
            Ok(value) => {
                self.header_list
                    .borrow_mut()
                    .insert(HeaderName::from_str(&valid_name).unwrap(), value);
            },
            Err(_) => {
                // can't add the header, but we don't need to panic the browser over it
                warn!(
                    "Servo thinks \"{:?}\" is a valid HTTP header value but HeaderValue doesn't.",
                    combined_value
                );
            },
        };
        Ok(())
    }

    // https://fetch.spec.whatwg.org/#dom-headers-delete
    fn Delete(&self, name: ByteString) -> ErrorResult {
        // Step 1
        let valid_name = validate_name(name)?;
        // Step 2
        if self.guard.get() == Guard::Immutable {
            return Err(Error::Type("Guard is immutable".to_string()));
        }
        // Step 3
        if self.guard.get() == Guard::Request && is_forbidden_header_name(&valid_name) {
            return Ok(());
        }
        // Step 4
        if self.guard.get() == Guard::RequestNoCors &&
            !is_cors_safelisted_request_header(&valid_name, &b"invalid".to_vec())
        {
            return Ok(());
        }
        // Step 5
        if self.guard.get() == Guard::Response && is_forbidden_response_header(&valid_name) {
            return Ok(());
        }
        // Step 6
        self.header_list.borrow_mut().remove(&valid_name);
        Ok(())
    }

    // https://fetch.spec.whatwg.org/#dom-headers-get
    fn Get(&self, name: ByteString) -> Fallible<Option<ByteString>> {
        // Step 1
        let valid_name = validate_name(name)?;
        Ok(self
            .header_list
            .borrow()
            .get(HeaderName::from_str(&valid_name).unwrap())
            .map(|v| ByteString::new(v.as_bytes().to_vec())))
    }

    // https://fetch.spec.whatwg.org/#dom-headers-has
    fn Has(&self, name: ByteString) -> Fallible<bool> {
        // Step 1
        let valid_name = validate_name(name)?;
        // Step 2
        Ok(self.header_list.borrow_mut().get(&valid_name).is_some())
    }

    // https://fetch.spec.whatwg.org/#dom-headers-set
    fn Set(&self, name: ByteString, value: ByteString) -> Fallible<()> {
        // Step 1
        let value = normalize_value(value);
        // Step 2
        let (mut valid_name, valid_value) = validate_name_and_value(name, value)?;
        valid_name = valid_name.to_lowercase();
        // Step 3
        if self.guard.get() == Guard::Immutable {
            return Err(Error::Type("Guard is immutable".to_string()));
        }
        // Step 4
        if self.guard.get() == Guard::Request && is_forbidden_header_name(&valid_name) {
            return Ok(());
        }
        // Step 5
        if self.guard.get() == Guard::RequestNoCors &&
            !is_cors_safelisted_request_header(&valid_name, &valid_value)
        {
            return Ok(());
        }
        // Step 6
        if self.guard.get() == Guard::Response && is_forbidden_response_header(&valid_name) {
            return Ok(());
        }
        // Step 7
        // https://fetch.spec.whatwg.org/#concept-header-list-set
        self.header_list.borrow_mut().insert(
            HeaderName::from_str(&valid_name).unwrap(),
            HeaderValue::from_bytes(&valid_value).unwrap(),
        );
        Ok(())
    }
}

impl Headers {
    // https://fetch.spec.whatwg.org/#concept-headers-fill
    pub fn fill(&self, filler: Option<HeadersInit>) -> ErrorResult {
        match filler {
            // Step 1
            Some(HeadersInit::Headers(h)) => {
                for (name, value) in h.header_list.borrow().iter() {
                    self.Append(
                        ByteString::new(Vec::from(name.as_str())),
                        ByteString::new(Vec::from(value.as_bytes())),
                    )?;
                }
                Ok(())
            },
            // Step 2
            Some(HeadersInit::ByteStringSequenceSequence(v)) => {
                for mut seq in v {
                    if seq.len() == 2 {
                        let val = seq.pop().unwrap();
                        let name = seq.pop().unwrap();
                        self.Append(name, val)?;
                    } else {
                        return Err(Error::Type(
                            format!("Each header object must be a sequence of length 2 - found one with length {}",
                                    seq.len())));
                    }
                }
                Ok(())
            },
            Some(HeadersInit::ByteStringByteStringRecord(m)) => {
                for (key, value) in m.iter() {
                    self.Append(key.clone(), value.clone())?;
                }
                Ok(())
            },
            None => Ok(()),
        }
    }

    pub fn for_request(global: &GlobalScope) -> DomRoot<Headers> {
        let headers_for_request = Headers::new(global);
        headers_for_request.guard.set(Guard::Request);
        headers_for_request
    }

    pub fn for_response(global: &GlobalScope) -> DomRoot<Headers> {
        let headers_for_response = Headers::new(global);
        headers_for_response.guard.set(Guard::Response);
        headers_for_response
    }

    pub fn set_guard(&self, new_guard: Guard) {
        self.guard.set(new_guard)
    }

    pub fn get_guard(&self) -> Guard {
        self.guard.get()
    }

    pub fn empty_header_list(&self) {
        *self.header_list.borrow_mut() = HyperHeaders::new();
    }

    pub fn set_headers(&self, hyper_headers: HyperHeaders) {
        *self.header_list.borrow_mut() = hyper_headers;
    }

    pub fn get_headers_list(&self) -> HyperHeaders {
        self.header_list.borrow_mut().clone()
    }

    // https://fetch.spec.whatwg.org/#concept-header-extract-mime-type
    pub fn extract_mime_type(&self) -> Vec<u8> {
        self.header_list
            .borrow()
            .get(header::CONTENT_TYPE)
            .map_or(vec![], |v| v.as_bytes().to_owned())
    }

    pub fn sort_header_list(&self) -> Vec<(String, Vec<u8>)> {
        let borrowed_header_list = self.header_list.borrow();
        let headers_iter = borrowed_header_list.iter();
        let mut header_vec = vec![];
        for (name, value) in headers_iter {
            let name = name.as_str().to_owned();
            let value = value.as_bytes().to_vec();
            let name_value = (name, value);
            header_vec.push(name_value);
        }
        header_vec.sort();
        header_vec
    }
}

impl Iterable for Headers {
    type Key = ByteString;
    type Value = ByteString;

    fn get_iterable_length(&self) -> u32 {
        self.header_list.borrow().iter().count() as u32
    }

    fn get_value_at_index(&self, n: u32) -> ByteString {
        let sorted_header_vec = self.sort_header_list();
        let value = sorted_header_vec[n as usize].1.clone();
        ByteString::new(value)
    }

    fn get_key_at_index(&self, n: u32) -> ByteString {
        let sorted_header_vec = self.sort_header_list();
        let key = sorted_header_vec[n as usize].0.clone();
        ByteString::new(key.into_bytes().to_vec())
    }
}

// https://fetch.spec.whatwg.org/#forbidden-response-header-name
fn is_forbidden_response_header(name: &str) -> bool {
    match name {
        "set-cookie" | "set-cookie2" => true,
        _ => false,
    }
}

// https://fetch.spec.whatwg.org/#forbidden-header-name
pub fn is_forbidden_header_name(name: &str) -> bool {
    let disallowed_headers = [
        "accept-charset",
        "accept-encoding",
        "access-control-request-headers",
        "access-control-request-method",
        "connection",
        "content-length",
        "cookie",
        "cookie2",
        "date",
        "dnt",
        "expect",
        "host",
        "keep-alive",
        "origin",
        "referer",
        "te",
        "trailer",
        "transfer-encoding",
        "upgrade",
        "via",
    ];

    let disallowed_header_prefixes = ["sec-", "proxy-"];

    disallowed_headers.iter().any(|header| *header == name) ||
        disallowed_header_prefixes
            .iter()
            .any(|prefix| name.starts_with(prefix))
}

// There is some unresolved confusion over the definition of a name and a value.
//
// As of December 2019, WHATWG has no formal grammar production for value;
// https://fetch.spec.whatg.org/#concept-header-value just says not to have
// newlines, nulls, or leading/trailing whitespace. It even allows
// octets that aren't a valid UTF-8 encoding, and WPT tests reflect this.
// The HeaderValue class does not fully reflect this, so headers
// containing bytes with values 1..31 or 127 can't be created, failing
// WPT tests but probably not affecting anything important on the real Internet.
fn validate_name_and_value(name: ByteString, value: ByteString) -> Fallible<(String, Vec<u8>)> {
    let valid_name = validate_name(name)?;
    if !is_legal_header_value(&value) {
        return Err(Error::Type("Header value is not valid".to_string()));
    }
    Ok((valid_name, value.into()))
}

fn validate_name(name: ByteString) -> Fallible<String> {
    if !is_field_name(&name) {
        return Err(Error::Type("Name is not valid".to_string()));
    }
    match String::from_utf8(name.into()) {
        Ok(ns) => Ok(ns),
        _ => Err(Error::Type("Non-UTF8 header name found".to_string())),
    }
}

// Removes trailing and leading HTTP whitespace bytes.
// https://fetch.spec.whatwg.org/#concept-header-value-normalize
pub fn normalize_value(value: ByteString) -> ByteString {
    match (
        index_of_first_non_whitespace(&value),
        index_of_last_non_whitespace(&value),
    ) {
        (Some(begin), Some(end)) => ByteString::new(value[begin..end + 1].to_owned()),
        _ => ByteString::new(vec![]),
    }
}

fn is_http_whitespace(byte: u8) -> bool {
    byte == b'\t' || byte == b'\n' || byte == b'\r' || byte == b' '
}

fn index_of_first_non_whitespace(value: &ByteString) -> Option<usize> {
    for (index, &byte) in value.iter().enumerate() {
        if !is_http_whitespace(byte) {
            return Some(index);
        }
    }
    None
}

fn index_of_last_non_whitespace(value: &ByteString) -> Option<usize> {
    for (index, &byte) in value.iter().enumerate().rev() {
        if !is_http_whitespace(byte) {
            return Some(index);
        }
    }
    None
}

// http://tools.ietf.org/html/rfc7230#section-3.2
fn is_field_name(name: &ByteString) -> bool {
    is_token(&*name)
}

// https://fetch.spec.whatg.org/#concept-header-value
fn is_legal_header_value(value: &ByteString) -> bool {
    let value_len = value.len();
    if value_len == 0 {
        return true;
    }
    match value[0] {
        b' ' | b'\t' => return false,
        _ => {},
    };
    match value[value_len - 1] {
        b' ' | b'\t' => return false,
        _ => {},
    };
    for &ch in &value[..] {
        match ch {
            b'\0' | b'\n' | b'\r' => return false,
            _ => {},
        }
    }
    true
    // If accepting non-UTF8 header values causes breakage,
    // removing the above "true" and uncommenting the below code
    // would ameliorate it while still accepting most reasonable headers:
    //match str::from_utf8(value) {
    //    Ok(_) => true,
    //    Err(_) => {
    //        warn!(
    //            "Rejecting spec-legal but non-UTF8 header value: {:?}",
    //            value
    //        );
    //        false
    //    },
    // }
}

// https://tools.ietf.org/html/rfc5234#appendix-B.1
pub fn is_vchar(x: u8) -> bool {
    match x {
        0x21..=0x7E => true,
        _ => false,
    }
}

// http://tools.ietf.org/html/rfc7230#section-3.2.6
pub fn is_obs_text(x: u8) -> bool {
    match x {
        0x80..=0xFF => true,
        _ => false,
    }
}
