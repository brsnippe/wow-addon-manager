//! Blocking HTTP behind a small trait, so sources can be tested against recorded responses.

use std::fs::{self, File};
use std::io;
use std::path::Path;

use ureq::tls::{RootCerts, TlsConfig, TlsProvider};

const USER_AGENT: &str = concat!(
    "wow-addon-manager/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/brsnippe/wow-addon-manager)"
);

pub trait Http: Send + Sync {
    /// Fetches `url` into memory. Any status counts as a response; callers check it.
    fn get(&self, url: &str, headers: &[(&str, &str)]) -> Result<Response, HttpError>;

    /// Streams `url` into the file `dest`, following redirects.
    fn download(&self, url: &str, dest: &Path) -> Result<(), HttpError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub body: Vec<u8>,
}

#[derive(Debug, thiserror::Error)]
pub enum HttpError {
    #[error("the server answered {status} for {url}")]
    Status { url: String, status: u16 },
    #[error("could not reach {url}: {message}")]
    Transport { url: String, message: String },
    #[error(transparent)]
    Io(#[from] io::Error),
}

/// The real client. Uses the Windows certificate store through native TLS.
pub struct UreqHttp {
    agent: ureq::Agent,
}

impl UreqHttp {
    pub fn new() -> Self {
        let tls = TlsConfig::builder()
            .provider(TlsProvider::NativeTls)
            .root_certs(RootCerts::PlatformVerifier)
            .build();
        let agent = ureq::Agent::config_builder()
            .user_agent(USER_AGENT)
            .http_status_as_error(false)
            .tls_config(tls)
            .build()
            .new_agent();
        Self { agent }
    }

    fn call(
        &self,
        url: &str,
        headers: &[(&str, &str)],
    ) -> Result<ureq::http::Response<ureq::Body>, HttpError> {
        let mut request = self.agent.get(url);
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        request.call().map_err(|e| transport(url, e))
    }
}

impl Default for UreqHttp {
    fn default() -> Self {
        Self::new()
    }
}

impl Http for UreqHttp {
    fn get(&self, url: &str, headers: &[(&str, &str)]) -> Result<Response, HttpError> {
        let mut response = self.call(url, headers)?;
        let status = response.status().as_u16();
        let body = response
            .body_mut()
            .read_to_vec()
            .map_err(|e| transport(url, e))?;
        Ok(Response { status, body })
    }

    fn download(&self, url: &str, dest: &Path) -> Result<(), HttpError> {
        let mut response = self.call(url, &[])?;
        let status = response.status().as_u16();
        if !(200..300).contains(&status) {
            return Err(HttpError::Status {
                url: url.into(),
                status,
            });
        }
        let result = File::create(dest).and_then(|mut file| {
            io::copy(&mut response.body_mut().as_reader(), &mut file)?;
            file.sync_all()
        });
        if result.is_err() {
            let _ = fs::remove_file(dest);
        }
        Ok(result?)
    }
}

fn transport(url: &str, error: ureq::Error) -> HttpError {
    HttpError::Transport {
        url: url.into(),
        message: error.to_string(),
    }
}

/// Answers requests from fixture files instead of the network.
#[cfg(test)]
pub mod recorded {
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Mutex;

    use super::*;

    #[derive(Default)]
    pub struct RecordedHttp {
        responses: HashMap<String, Response>,
        requests: Mutex<Vec<String>>,
    }

    impl RecordedHttp {
        pub fn new() -> Self {
            Self::default()
        }

        /// Answers `url` with the contents of `tests/fixtures/<fixture>`.
        pub fn fixture(self, url: &str, fixture: &str) -> Self {
            let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures")
                .join(fixture);
            let body = fs::read(&path)
                .unwrap_or_else(|e| panic!("missing fixture {}: {e}", path.display()));
            self.respond(url, 200, body)
        }

        pub fn respond(mut self, url: &str, status: u16, body: impl Into<Vec<u8>>) -> Self {
            let body = body.into();
            self.responses
                .insert(url.to_string(), Response { status, body });
            self
        }

        /// The URLs requested so far, in order.
        pub fn requests(&self) -> Vec<String> {
            self.requests.lock().unwrap().clone()
        }

        fn lookup(&self, url: &str) -> Response {
            self.requests.lock().unwrap().push(url.to_string());
            self.responses
                .get(url)
                .unwrap_or_else(|| panic!("no recorded response for {url}"))
                .clone()
        }
    }

    impl Http for RecordedHttp {
        fn get(&self, url: &str, _headers: &[(&str, &str)]) -> Result<Response, HttpError> {
            Ok(self.lookup(url))
        }

        fn download(&self, url: &str, dest: &Path) -> Result<(), HttpError> {
            let response = self.lookup(url);
            if !(200..300).contains(&response.status) {
                return Err(HttpError::Status {
                    url: url.into(),
                    status: response.status,
                });
            }
            Ok(fs::write(dest, response.body)?)
        }
    }
}
