// Copyright 2026 Salesforce, Inc. All rights reserved.

//! Helpers shared across mcp-apps-policy integration tests.

#![allow(dead_code)]

use std::cell::RefCell;
use std::rc::Rc;

use pdk_unit::{Backend, UnitHttpMessage, UnitHttpRequest, UnitHttpResponse};

/// Configurable upstream: each test sets the response body it wants
/// the gateway to receive. The captured upstream request is exposed
/// so tests can assert that short-circuited requests never made it
/// through.
pub struct ConfigurableBackend {
    response_body: Rc<RefCell<Vec<u8>>>,
    response_content_type: Rc<RefCell<String>>,
    response_status: Rc<RefCell<u32>>,
    captured_request: Rc<RefCell<Option<Vec<u8>>>>,
}

pub struct BackendHandle {
    pub response_body: Rc<RefCell<Vec<u8>>>,
    pub response_content_type: Rc<RefCell<String>>,
    pub response_status: Rc<RefCell<u32>>,
    pub captured_request: Rc<RefCell<Option<Vec<u8>>>>,
}

impl ConfigurableBackend {
    pub fn new() -> (Self, BackendHandle) {
        let response_body = Rc::new(RefCell::new(b"{}".to_vec()));
        let response_content_type = Rc::new(RefCell::new("application/json".to_string()));
        let response_status = Rc::new(RefCell::new(200_u32));
        let captured_request: Rc<RefCell<Option<Vec<u8>>>> = Rc::new(RefCell::new(None));
        let me = Self {
            response_body: response_body.clone(),
            response_content_type: response_content_type.clone(),
            response_status: response_status.clone(),
            captured_request: captured_request.clone(),
        };
        let handle = BackendHandle {
            response_body,
            response_content_type,
            response_status,
            captured_request,
        };
        (me, handle)
    }
}

impl Backend for ConfigurableBackend {
    fn call(&self, req: UnitHttpRequest) -> UnitHttpResponse {
        *self.captured_request.borrow_mut() = Some(req.body().to_vec());
        UnitHttpResponse::new(*self.response_status.borrow())
            .with_header("content-type", self.response_content_type.borrow().as_str())
            .with_body(self.response_body.borrow().clone())
    }
}

impl BackendHandle {
    pub fn set_json(&self, body: &serde_json::Value) {
        *self.response_body.borrow_mut() = body.to_string().into_bytes();
        *self.response_content_type.borrow_mut() = "application/json".into();
    }

    pub fn set_response(&self, status: u32, content_type: &str, body: Vec<u8>) {
        *self.response_status.borrow_mut() = status;
        *self.response_content_type.borrow_mut() = content_type.into();
        *self.response_body.borrow_mut() = body;
    }

    pub fn was_called(&self) -> bool {
        self.captured_request.borrow().is_some()
    }
}

pub fn parse_json(bytes: &[u8]) -> serde_json::Value {
    serde_json::from_slice(bytes).unwrap_or_else(|e| {
        panic!(
            "expected JSON body, got error {e}: {}",
            String::from_utf8_lossy(bytes)
        )
    })
}
