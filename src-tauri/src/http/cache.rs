//! An in-memory cache for GET requests. Fresh answers (`Cache-Control: max-age`) are reused
//! without asking again; stale ones are revalidated with `If-None-Match` or
//! `If-Modified-Since`, and a `304 Not Modified` is answered from the cache.
//!
//! Requests for the same URL and headers wait for each other, so a repository checked for
//! several game versions at once is fetched once.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::{Http, HttpError, Response};

pub struct CachedHttp<H> {
    inner: H,
    /// Keyed by URL and request headers, since GitHub answers differently with a token.
    entries: Mutex<HashMap<String, Arc<Mutex<Option<Entry>>>>>,
}

struct Entry {
    /// Always a 200.
    response: Response,
    etag: Option<String>,
    last_modified: Option<String>,
    fresh_until: Option<Instant>,
}

impl<H: Http> CachedHttp<H> {
    pub fn new(inner: H) -> Self {
        Self {
            inner,
            entries: Mutex::default(),
        }
    }

    fn slot(&self, url: &str, headers: &[(&str, &str)]) -> Arc<Mutex<Option<Entry>>> {
        let mut key = url.to_string();
        for (name, value) in headers {
            key.push_str(&format!("\n{}: {value}", name.to_ascii_lowercase()));
        }
        self.entries.lock().unwrap().entry(key).or_default().clone()
    }

    /// Makes every cached answer stale, as if its `max-age` had passed.
    #[cfg(test)]
    pub fn expire(&self) {
        for slot in self.entries.lock().unwrap().values() {
            if let Some(entry) = slot.lock().unwrap().as_mut() {
                entry.fresh_until = None;
            }
        }
    }
}

impl<H: Http> Http for CachedHttp<H> {
    fn get(&self, url: &str, headers: &[(&str, &str)]) -> Result<Response, HttpError> {
        let slot = self.slot(url, headers);
        // Held during the request: others asking for the same thing wait for this answer.
        let mut cached = slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let now = Instant::now();
        if let Some(entry) = cached.as_ref()
            && entry.fresh_until.is_some_and(|until| now < until)
        {
            return Ok(entry.response.clone());
        }

        let validator = cached.as_ref().and_then(|entry| {
            entry
                .etag
                .clone()
                .map(|etag| ("If-None-Match", etag))
                .or_else(|| {
                    entry
                        .last_modified
                        .clone()
                        .map(|date| ("If-Modified-Since", date))
                })
        });
        let mut request: Vec<(&str, &str)> = headers.to_vec();
        if let Some((name, value)) = &validator {
            request.push((*name, value.as_str()));
        }
        let response = self.inner.get(url, &request)?;

        match response.status {
            304 => match cached.as_mut() {
                Some(entry) => {
                    entry.revalidated(&response, now);
                    Ok(entry.response.clone())
                }
                None => Ok(response),
            },
            200 => {
                *cached = Entry::store(&response, now);
                Ok(response)
            }
            // Errors such as a rate limit don't replace what was cached.
            _ => Ok(response),
        }
    }

    fn download(&self, url: &str, dest: &Path) -> Result<(), HttpError> {
        self.inner.download(url, dest)
    }
}

impl Entry {
    /// `None` if the response can't be reused or revalidated.
    fn store(response: &Response, now: Instant) -> Option<Self> {
        let control = CacheControl::parse(response);
        if control.no_store {
            return None;
        }
        let entry = Self {
            response: response.clone(),
            etag: response.header("etag").map(str::to_string),
            last_modified: response.header("last-modified").map(str::to_string),
            fresh_until: control.fresh_until(now),
        };
        (entry.etag.is_some() || entry.last_modified.is_some() || entry.fresh_until.is_some())
            .then_some(entry)
    }

    /// Takes the newer headers of a 304, such as the remaining rate limit, and its freshness.
    fn revalidated(&mut self, not_modified: &Response, now: Instant) {
        for (name, value) in &not_modified.headers {
            match self.response.headers.iter_mut().find(|(n, _)| n == name) {
                Some(header) => header.1 = value.clone(),
                None => self.response.headers.push((name.clone(), value.clone())),
            }
        }
        if let Some(etag) = not_modified.header("etag") {
            self.etag = Some(etag.to_string());
        }
        self.fresh_until = CacheControl::parse(not_modified).fresh_until(now);
    }
}

#[derive(Default)]
struct CacheControl {
    max_age: Option<u64>,
    no_store: bool,
    no_cache: bool,
}

impl CacheControl {
    fn parse(response: &Response) -> Self {
        let mut control = Self::default();
        let Some(value) = response.header("cache-control") else {
            return control;
        };
        for directive in value.split(',') {
            let directive = directive.trim().to_ascii_lowercase();
            match directive.split_once('=') {
                Some(("max-age", seconds)) => control.max_age = seconds.trim().parse().ok(),
                None if directive == "no-store" => control.no_store = true,
                None if directive == "no-cache" => control.no_cache = true,
                _ => {}
            }
        }
        control
    }

    fn fresh_until(&self, now: Instant) -> Option<Instant> {
        match self.max_age {
            Some(seconds) if seconds > 0 && !self.no_cache => {
                Some(now + Duration::from_secs(seconds))
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::recorded::RecordedHttp;

    const URL: &str = "https://api.github.com/repos/o/a/releases";

    fn cached(response: Response) -> CachedHttp<RecordedHttp> {
        CachedHttp::new(RecordedHttp::new().respond_with(URL, response))
    }

    fn requests(http: &CachedHttp<RecordedHttp>) -> usize {
        http.inner.requests().len()
    }

    #[test]
    fn reuses_fresh_answers_without_asking() {
        let http =
            cached(Response::new(200, "list").with_header("Cache-Control", "public, max-age=60"));
        assert_eq!(http.get(URL, &[]).unwrap().body, b"list");
        assert_eq!(http.get(URL, &[]).unwrap().body, b"list");
        assert_eq!(requests(&http), 1);
    }

    #[test]
    fn revalidates_stale_answers_with_the_etag() {
        let http = cached(
            Response::new(200, "list")
                .with_header("ETag", "W/\"abc\"")
                .with_header("Cache-Control", "max-age=60")
                .with_header("X-RateLimit-Remaining", "59"),
        );
        http.get(URL, &[("Accept", "json")]).unwrap();
        http.expire();
        http.inner.replace(
            URL,
            Response::new(304, "").with_header("X-RateLimit-Remaining", "58"),
        );

        let response = http.get(URL, &[("Accept", "json")]).unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(response.body, b"list");
        assert_eq!(response.header("x-ratelimit-remaining"), Some("58"));
        assert_eq!(
            http.inner.request_headers(1),
            [
                ("accept".to_string(), "json".to_string()),
                ("if-none-match".to_string(), "W/\"abc\"".to_string())
            ]
        );
        // The 304 had no max-age, so the next request asks again.
        http.get(URL, &[("Accept", "json")]).unwrap();
        assert_eq!(requests(&http), 3);
    }

    #[test]
    fn falls_back_to_last_modified() {
        let date = "Sun, 08 Nov 2026 10:00:00 GMT";
        let http = cached(Response::new(200, "file").with_header("Last-Modified", date));
        http.get(URL, &[]).unwrap();
        http.get(URL, &[]).unwrap();
        assert_eq!(
            http.inner.request_headers(1),
            [("if-modified-since".to_string(), date.to_string())]
        );
    }

    #[test]
    fn does_not_keep_what_it_may_not() {
        for response in [
            Response::new(200, "plain"),
            Response::new(200, "secret")
                .with_header("ETag", "\"x\"")
                .with_header("Cache-Control", "no-store"),
        ] {
            let http = cached(response);
            http.get(URL, &[]).unwrap();
            http.get(URL, &[]).unwrap();
            assert_eq!(requests(&http), 2);
            assert!(http.inner.request_headers(1).is_empty());
        }
    }

    #[test]
    fn errors_keep_the_cached_answer() {
        let http = cached(Response::new(200, "list").with_header("ETag", "\"v1\""));
        http.get(URL, &[]).unwrap();
        http.inner.replace(URL, Response::new(403, "rate limited"));
        assert_eq!(http.get(URL, &[]).unwrap().status, 403);
        http.inner.replace(URL, Response::new(304, ""));
        assert_eq!(http.get(URL, &[]).unwrap().body, b"list");
    }

    #[test]
    fn different_headers_are_cached_apart() {
        let http = cached(Response::new(200, "list").with_header("Cache-Control", "max-age=60"));
        http.get(URL, &[]).unwrap();
        http.get(URL, &[("Authorization", "Bearer t")]).unwrap();
        http.get(URL, &[("Authorization", "Bearer t")]).unwrap();
        assert_eq!(requests(&http), 2);
    }

    /// Talks to the real GitHub API: `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn live_github_revalidates_with_304() {
        struct Statuses(crate::http::UreqHttp, Mutex<Vec<u16>>);
        impl Http for Statuses {
            fn get(&self, url: &str, headers: &[(&str, &str)]) -> Result<Response, HttpError> {
                let response = self.0.get(url, headers)?;
                self.1.lock().unwrap().push(response.status);
                Ok(response)
            }
            fn download(&self, _: &str, _: &Path) -> Result<(), HttpError> {
                unreachable!()
            }
        }

        let http = CachedHttp::new(Statuses(crate::http::UreqHttp::new(), Mutex::default()));
        let url = "https://api.github.com/repos/BigWigsMods/BigWigs/releases?per_page=1";
        let headers = [("Accept", "application/vnd.github+json")];
        let first = http.get(url, &headers).unwrap();
        println!("cache-control: {:?}", first.header("cache-control"));
        assert_eq!(http.get(url, &headers).unwrap().body, first.body);
        http.expire();
        assert_eq!(http.get(url, &headers).unwrap().body, first.body);
        assert_eq!(*http.inner.1.lock().unwrap(), [200, 304]);
    }

    #[test]
    fn concurrent_requests_for_one_url_are_sent_once() {
        let http = Arc::new(cached(
            Response::new(200, "list").with_header("Cache-Control", "max-age=60"),
        ));
        let threads: Vec<_> = (0..8)
            .map(|_| {
                let http = http.clone();
                std::thread::spawn(move || http.get(URL, &[]).unwrap().body)
            })
            .collect();
        for thread in threads {
            assert_eq!(thread.join().unwrap(), b"list");
        }
        assert_eq!(requests(&http), 1);
    }
}
