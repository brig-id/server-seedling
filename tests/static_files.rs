// The interface as the server serves it: a minimal router and a build of
// the interface made by the test, read from a directory. The embedded
// interface of a released executable goes through the same code.
//
//   - a page is a file: `/signup/` is `signup/index.html`
//   - a path that names nothing is `404`: no page answers for the others
//   - files named after their content are cached for good, pages are checked
//   - API routes take priority
//   - every response carries the security headers, and the interface's are
//     served under the policy made from the build's list of script hashes
//   - without that list, there is no interface

use std::{fs, sync::Arc};

use axum::{Router, body::Body, http::Request, routing::get};
use http_body_util::BodyExt;
use seedling::{Ui, UiError, apply_ui};
use tempfile::TempDir;
use tower::ServiceExt;

const HASHES: &str = r#"{"script":["'sha256-AAAA'"],"style":[],"pages":[]}"#;

fn make_dist() -> TempDir {
    let dir = TempDir::new().expect("tempdir");
    let write = |path: &str, content: &[u8]| {
        let path = dir.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).expect("mkdir");
        fs::write(path, content).expect("write");
    };
    write(
        "index.html",
        b"<!DOCTYPE html><html><body>SIGN IN</body></html>",
    );
    write(
        "signup/index.html",
        b"<!DOCTYPE html><html><body>SIGN UP</body></html>",
    );
    write("build/q-chunk.js", b"console.log('qwik');");
    write("assets/Abc123-font.woff2", b"\x00font");
    write("favicon.svg", b"<svg/>");
    write("q-manifest.json", b"{\"the bundler's report\":true}");
    write("csp-hashes.json", HASHES.as_bytes());
    dir
}

fn app(dist: &TempDir, base: Router) -> Router {
    apply_ui(base, Some(Arc::new(Ui::from_dir(dist.path()).unwrap())))
}

async fn get_path(app: &Router, path: &str) -> axum::response::Response {
    app.clone()
        .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
        .await
        .unwrap()
}

async fn body_string(resp: axum::response::Response) -> String {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

#[tokio::test]
async fn a_page_is_a_file_of_the_build() {
    let dist = make_dist();
    let app = app(&dist, Router::new());

    let resp = get_path(&app, "/").await;
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.headers()["content-type"], "text/html; charset=utf-8");
    // A page is checked again at each visit, and has a tag to check with.
    assert_eq!(resp.headers()["cache-control"], "no-cache");
    assert!(resp.headers().contains_key("etag"));
    assert!(body_string(resp).await.contains("SIGN IN"));

    let resp = get_path(&app, "/signup/").await;
    assert_eq!(resp.status(), 200);
    assert!(body_string(resp).await.contains("SIGN UP"));

    // The address of the request the linking page reads stays with it.
    let resp = get_path(&app, "/signup/?request=abc").await;
    assert_eq!(resp.status(), 200);

    // Without its final slash, a directory says where it is.
    let resp = get_path(&app, "/signup?x=1").await;
    assert_eq!(resp.status(), 308);
    assert_eq!(resp.headers()["location"], "/signup/?x=1");
}

#[tokio::test]
async fn a_path_that_names_nothing_is_not_found() {
    let dist = make_dist();
    let app = app(&dist, Router::new());
    for path in [
        "/login",
        "/login/",
        "/signup/other/",
        "/build/missing.js",
        "/../etc/passwd",
        "/build/%2e%2e/index.html",
        "//signup/",
        // Read by the server, or the bundler's own: not served.
        "/csp-hashes.json",
        "/q-manifest.json",
    ] {
        let resp = get_path(&app, path).await;
        assert_eq!(resp.status(), 404, "{path}");
        assert_eq!(body_string(resp).await, "Not found\n", "{path}");
    }
}

#[tokio::test]
async fn files_named_after_their_content_are_cached_for_good() {
    let dist = make_dist();
    let app = app(&dist, Router::new());

    let resp = get_path(&app, "/build/q-chunk.js").await;
    assert_eq!(resp.status(), 200);
    assert_eq!(
        resp.headers()["content-type"],
        "text/javascript; charset=utf-8"
    );
    assert_eq!(
        resp.headers()["cache-control"],
        "public, max-age=31536000, immutable"
    );
    assert!(body_string(resp).await.contains("qwik"));

    let resp = get_path(&app, "/assets/Abc123-font.woff2").await;
    assert_eq!(resp.headers()["content-type"], "font/woff2");
    assert!(
        resp.headers()["cache-control"]
            .to_str()
            .unwrap()
            .contains("immutable")
    );

    let resp = get_path(&app, "/favicon.svg").await;
    assert_eq!(resp.headers()["content-type"], "image/svg+xml");
    assert_eq!(resp.headers()["cache-control"], "no-cache");
}

#[tokio::test]
async fn a_page_already_held_is_not_sent_again() {
    let dist = make_dist();
    let app = app(&dist, Router::new());
    let etag = get_path(&app, "/").await.headers()["etag"].clone();

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/")
                .header("if-none-match", &etag)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 304);
    assert_eq!(resp.headers()["etag"], etag);
    assert_eq!(body_string(resp).await, "");

    // Another content has another tag: the page is sent.
    fs::write(dist.path().join("index.html"), b"<html>CHANGED</html>").unwrap();
    let changed = apply_ui(
        Router::new(),
        Some(Arc::new(Ui::from_dir(dist.path()).unwrap())),
    );
    let resp = changed
        .oneshot(
            Request::builder()
                .uri("/")
                .header("if-none-match", &etag)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    assert!(body_string(resp).await.contains("CHANGED"));
}

#[tokio::test]
async fn only_reading_is_offered() {
    let dist = make_dist();
    let app = app(&dist, Router::new());
    let send = async |method: &str, path: &str| {
        app.clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
    };
    let resp = send("HEAD", "/").await;
    assert_eq!(resp.status(), 200);
    assert!(resp.headers().contains_key("content-length"));
    assert_eq!(body_string(resp).await, "");
    let resp = send("POST", "/").await;
    assert_eq!(resp.status(), 405);
    assert_eq!(resp.headers()["allow"], "GET, HEAD");
    assert_eq!(send("POST", "/nothing/").await.status(), 404);
}

#[tokio::test]
async fn api_routes_take_priority_over_the_interface() {
    let dist = make_dist();
    let base = Router::new().route(
        "/signup/",
        get(|| async { axum::http::StatusCode::NO_CONTENT }),
    );
    let app = app(&dist, base);
    assert_eq!(get_path(&app, "/signup/").await.status(), 204);
}

#[tokio::test]
async fn without_an_interface_unknown_paths_are_not_found() {
    let app = apply_ui(Router::new(), None);
    let resp = get_path(&app, "/").await;
    assert_eq!(resp.status(), 404);
    // Nothing is served from here: the policy loads nothing.
    assert_eq!(
        resp.headers()["content-security-policy"],
        "default-src 'none'; frame-ancestors 'none'"
    );
}

/// The headers reach every response: pages, files, refusals, and the routes
/// defined before the interface was attached. The interface's own policy is
/// made of the build's hashes, never of `'unsafe-inline'`.
#[tokio::test]
async fn every_response_carries_the_security_headers() {
    let dist = make_dist();
    let base = Router::new().route("/api/ok", get(|| async { "ok" }));
    let app = app(&dist, base);
    for path in ["/", "/signup/", "/build/q-chunk.js", "/nothing/", "/api/ok"] {
        let resp = get_path(&app, path).await;
        let headers = resp.headers();
        assert_eq!(headers["x-content-type-options"], "nosniff", "{path}");
        assert_eq!(headers["x-frame-options"], "DENY", "{path}");
        assert!(headers.contains_key("strict-transport-security"), "{path}");
        let csp = headers["content-security-policy"].to_str().unwrap();
        assert!(
            csp.contains("script-src 'self' 'sha256-AAAA';"),
            "{path}: {csp}"
        );
        assert!(!csp.contains("unsafe-inline"), "{path}: {csp}");
    }
}

#[test]
fn a_directory_that_is_not_a_build_is_refused() {
    // No list of hashes: no policy to serve the pages under, so no interface.
    let dist = make_dist();
    fs::remove_file(dist.path().join("csp-hashes.json")).unwrap();
    assert!(matches!(
        Ui::from_dir(dist.path()),
        Err(UiError::NoPolicy(_))
    ));
    fs::write(dist.path().join("csp-hashes.json"), b"{}").unwrap();
    assert!(matches!(
        Ui::from_dir(dist.path()),
        Err(UiError::NoPolicy(_))
    ));

    // No index.html.
    let dist = make_dist();
    fs::remove_file(dist.path().join("index.html")).unwrap();
    assert!(matches!(
        Ui::from_dir(dist.path()),
        Err(UiError::NoIndex(_))
    ));

    // No directory at all.
    let gone = dist.path().join("nowhere");
    let error = Ui::from_dir(&gone).err().expect("refused");
    assert!(error.to_string().contains("pnpm build"), "{error}");
}
