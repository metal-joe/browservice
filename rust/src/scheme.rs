//! Port of `src/scheme.{hpp,cpp}`: the handler factory for the
//! `browservice:` scheme. `browservice://bookmarks/` serves the bookmark page,
//! everything else a 404 page. Runs on the CEF IO thread.

use std::sync::{Arc, Mutex};

use cef::*;

struct StaticResponse {
    status: i32,
    status_text: String,
    response: Vec<u8>,
    pos: usize,
}

wrap_resource_handler! {
    struct StaticResponseResourceHandler {
        state: Arc<Mutex<StaticResponse>>,
    }

    impl ResourceHandler {
        fn open(
            &self,
            _request: Option<&mut Request>,
            handle_request: Option<&mut ::std::os::raw::c_int>,
            _callback: Option<&mut Callback>,
        ) -> ::std::os::raw::c_int {
            if let Some(handle_request) = handle_request {
                *handle_request = 1;
            }
            1
        }

        fn response_headers(
            &self,
            response: Option<&mut Response>,
            response_length: Option<&mut i64>,
            _redirect_url: Option<&mut CefString>,
        ) {
            let state = self.state.lock().unwrap();
            if let Some(response_length) = response_length {
                *response_length = state.response.len() as i64;
            }
            if let Some(response) = response {
                response.set_status(state.status);
                response.set_status_text(Some(&CefString::from(state.status_text.as_str())));
                response.set_mime_type(Some(&CefString::from("text/html")));
                response.set_charset(Some(&CefString::from("UTF-8")));
            }
        }

        fn skip(
            &self,
            bytes_to_skip: i64,
            bytes_skipped: Option<&mut i64>,
            _callback: Option<&mut ResourceSkipCallback>,
        ) -> ::std::os::raw::c_int {
            let mut state = self.state.lock().unwrap();
            let max_skip = (state.response.len() - state.pos) as i64;
            let skip_count = bytes_to_skip.min(max_skip);
            if skip_count < 0 {
                crate::panic_log!("Requirement 'skipCount >= (int64_t)0' failed");
            }
            if skip_count > 0 {
                if let Some(bytes_skipped) = bytes_skipped {
                    *bytes_skipped = skip_count;
                }
                state.pos += skip_count as usize;
                1
            } else {
                if let Some(bytes_skipped) = bytes_skipped {
                    *bytes_skipped = -2;
                }
                0
            }
        }

        fn read(
            &self,
            data_out: *mut u8,
            bytes_to_read: ::std::os::raw::c_int,
            bytes_read: Option<&mut ::std::os::raw::c_int>,
            _callback: Option<&mut ResourceReadCallback>,
        ) -> ::std::os::raw::c_int {
            let mut state = self.state.lock().unwrap();
            let max_read = (state.response.len() - state.pos) as i64;
            let read_count = (bytes_to_read as i64).min(max_read) as i32;
            if read_count < 0 {
                crate::panic_log!("Requirement 'readCount >= 0' failed");
            }
            if read_count > 0 {
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        state.response.as_ptr().add(state.pos),
                        data_out,
                        read_count as usize,
                    );
                }
                state.pos += read_count as usize;
                if let Some(bytes_read) = bytes_read {
                    *bytes_read = read_count;
                }
                1
            } else {
                if let Some(bytes_read) = bytes_read {
                    *bytes_read = 0;
                }
                0
            }
        }

        fn cancel(&self) {
            let mut state = self.state.lock().unwrap();
            state.response.clear();
            state.pos = 0;
        }
    }
}

const NOT_FOUND_PAGE: &str = concat!(
    "<!DOCTYPE html>\n<html lang=\"en\"><head><meta charset=\"UTF-8\">",
    "<title>404 Not Found</title></head><body><h1>404 Not Found</h1></body></html>\n"
);

/// Status, status text and body of the response for a `browservice:` URL.
pub fn response_for_url(url: &str, request: Option<&mut Request>) -> (i32, String, String) {
    if url == "browservice://bookmarks/"
        && let Some(request) = request
    {
        (200, "OK".to_string(), crate::bookmarks::handle_bookmarks_request(request))
    } else {
        (404, "Not Found".to_string(), NOT_FOUND_PAGE.to_string())
    }
}

wrap_scheme_handler_factory! {
    pub struct BrowserviceSchemeHandlerFactory;

    impl SchemeHandlerFactory {
        fn create(
            &self,
            _browser: Option<&mut Browser>,
            _frame: Option<&mut Frame>,
            _scheme_name: Option<&CefString>,
            request: Option<&mut Request>,
        ) -> Option<ResourceHandler> {
            let Some(request) = request else {
                crate::panic_log!("Requirement 'request' failed");
            };
            let url = crate::data_url::userfree_to_string(&request.url());
            let (status, status_text, response) = response_for_url(&url, Some(request));
            Some(StaticResponseResourceHandler::new(Arc::new(Mutex::new(StaticResponse {
                status,
                status_text,
                response: response.into_bytes(),
                pos: 0,
            }))))
        }
    }
}

/// Registers the factory for the `browservice` scheme (call in the browser
/// process after the CEF context has been initialized).
pub fn register_browservice_scheme_handler_factory() {
    let mut factory = BrowserviceSchemeHandlerFactory::new();
    if register_scheme_handler_factory(
        Some(&CefString::from("browservice")),
        Some(&CefString::from("")),
        Some(&mut factory),
    ) == 0
    {
        crate::warning_log!("Registering the browservice scheme handler factory failed");
    }
}
