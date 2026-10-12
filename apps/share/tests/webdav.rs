use bytes::Bytes;
use dav_server::DavHandler;
use http_body_util::{BodyExt, Full};
use hyper::{Request, Response, StatusCode};
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn request(
    handler: &DavHandler,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: &str,
) -> Response<dav_server::body::Body> {
    let mut request = Request::builder().method(method).uri(path);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    handler
        .handle(
            request
                .body(Full::new(Bytes::copy_from_slice(body.as_bytes())))
                .unwrap(),
        )
        .await
}

async fn body(response: Response<dav_server::body::Body>) -> Bytes {
    response.into_body().collect().await.unwrap().to_bytes()
}

#[tokio::test]
async fn desktop_file_workflow_and_ranges() {
    let directory = TempDir::new().unwrap();
    let handler = share::handler(directory.path()).await.unwrap();
    let options = request(&handler, "OPTIONS", "/", &[], "").await;
    assert_eq!(options.status(), StatusCode::OK);
    assert!(options.headers().contains_key("dav"));
    assert!(!options.headers().contains_key("www-authenticate"));

    assert_eq!(
        request(&handler, "MKCOL", "/folder", &[], "")
            .await
            .status(),
        StatusCode::CREATED
    );
    assert_eq!(
        request(
            &handler,
            "PUT",
            "/folder/hello%20world.txt",
            &[],
            "hello world"
        )
        .await
        .status(),
        StatusCode::CREATED
    );
    // The bytes must already be committed when PUT returns successfully.
    assert_eq!(
        std::fs::read(directory.path().join("folder/hello world.txt")).unwrap(),
        b"hello world"
    );

    let listing = request(&handler, "PROPFIND", "/folder/", &[("Depth", "1")], "").await;
    assert_eq!(listing.status(), StatusCode::MULTI_STATUS);
    let listing = String::from_utf8(body(listing).await.to_vec()).unwrap();
    assert!(listing.contains("hello%20world.txt"));
    assert!(listing.contains("getcontentlength"));

    let get = request(&handler, "GET", "/folder/hello%20world.txt", &[], "").await;
    assert_eq!(get.status(), StatusCode::OK);
    assert_eq!(body(get).await, "hello world");
    let head = request(&handler, "HEAD", "/folder/hello%20world.txt", &[], "").await;
    assert_eq!(head.status(), StatusCode::OK);
    assert_eq!(head.headers()["content-length"], "11");
    assert!(body(head).await.is_empty());
    let range = request(
        &handler,
        "GET",
        "/folder/hello%20world.txt",
        &[("Range", "bytes=6-10")],
        "",
    )
    .await;
    assert_eq!(range.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(body(range).await, "world");

    assert_eq!(
        request(
            &handler,
            "COPY",
            "/folder/hello%20world.txt",
            &[("Destination", "/copy.txt")],
            ""
        )
        .await
        .status(),
        StatusCode::CREATED
    );
    assert_eq!(
        request(
            &handler,
            "MOVE",
            "/copy.txt",
            &[("Destination", "/moved.txt")],
            ""
        )
        .await
        .status(),
        StatusCode::CREATED
    );
    assert_eq!(
        request(&handler, "GET", "/copy.txt", &[], "")
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        body(request(&handler, "GET", "/moved.txt", &[], "").await).await,
        "hello world"
    );

    assert_eq!(
        request(&handler, "PUT", "/moved.txt", &[], "short")
            .await
            .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        std::fs::read(directory.path().join("moved.txt")).unwrap(),
        b"short"
    );
    assert_eq!(
        request(&handler, "DELETE", "/folder/", &[], "")
            .await
            .status(),
        StatusCode::NO_CONTENT
    );
    assert!(!directory.path().join("folder").exists());
    assert_eq!(
        request(&handler, "DELETE", "/moved.txt", &[], "")
            .await
            .status(),
        StatusCode::NO_CONTENT
    );
}

#[tokio::test]
async fn locks_protect_editor_writes() {
    let directory = TempDir::new().unwrap();
    let handler = share::handler(directory.path()).await.unwrap();
    assert_eq!(
        request(&handler, "PUT", "/edit.txt", &[], "before")
            .await
            .status(),
        StatusCode::CREATED
    );
    let lock = request(&handler, "LOCK", "/edit.txt", &[("Content-Type", "application/xml"), ("Depth", "0")],
        "<D:lockinfo xmlns:D=\"DAV:\"><D:lockscope><D:exclusive/></D:lockscope><D:locktype><D:write/></D:locktype><D:owner>test</D:owner></D:lockinfo>").await;
    assert_eq!(lock.status(), StatusCode::OK);
    let token = lock.headers()["lock-token"].to_str().unwrap().to_owned();
    assert_eq!(
        request(&handler, "PUT", "/edit.txt", &[], "blocked")
            .await
            .status(),
        StatusCode::LOCKED
    );
    let condition = format!("({token})");
    assert_eq!(
        request(&handler, "PUT", "/edit.txt", &[("If", &condition)], "after")
            .await
            .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        request(
            &handler,
            "UNLOCK",
            "/edit.txt",
            &[("Lock-Token", &token)],
            ""
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        std::fs::read(directory.path().join("edit.txt")).unwrap(),
        b"after"
    );
}

#[tokio::test]
async fn root_and_file_size_limits() {
    let directory = TempDir::new().unwrap();
    let handler = share::handler(directory.path()).await.unwrap();
    assert_eq!(
        request(&handler, "DELETE", "/", &[], "").await.status(),
        StatusCode::FORBIDDEN
    );
    assert!(directory.path().exists());
    assert_eq!(
        request(
            &handler,
            "PUT",
            "/too-large.bin",
            &[("Content-Length", "4294967297")],
            ""
        )
        .await
        .status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );
    assert!(!directory.path().join("too-large.bin").exists());
    assert_eq!(
        request(&handler, "GET", "/%2e%2e/outside", &[], "")
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(&handler, "PUT", "/missing/child", &[], "data")
            .await
            .status(),
        StatusCode::CONFLICT
    );
}

#[cfg(unix)]
#[tokio::test]
async fn symlinks_cannot_expose_or_modify_other_directories() {
    let directory = TempDir::new().unwrap();
    let outside = TempDir::new().unwrap();
    std::fs::write(outside.path().join("secret.txt"), b"secret").unwrap();
    std::os::unix::fs::symlink(outside.path(), directory.path().join("escape")).unwrap();
    let handler = share::handler(directory.path()).await.unwrap();
    assert_eq!(
        request(&handler, "GET", "/escape/secret.txt", &[], "")
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(&handler, "PUT", "/escape/new.txt", &[], "data")
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(&handler, "DELETE", "/escape/secret.txt", &[], "")
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        std::fs::read(outside.path().join("secret.txt")).unwrap(),
        b"secret"
    );
    assert!(!outside.path().join("new.txt").exists());
}

#[tokio::test]
async fn real_http_driver_accepts_upload_and_download() {
    let directory = TempDir::new().unwrap();
    let handler = share::handler(directory.path()).await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(share::http::serve(listener, handler));
    let mut connection = tokio::net::TcpStream::connect(address).await.unwrap();
    connection.write_all(b"PUT /network.txt HTTP/1.1\r\nHost: localhost\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello").await.unwrap();
    let mut response = Vec::new();
    connection.read_to_end(&mut response).await.unwrap();
    assert!(response.starts_with(b"HTTP/1.1 201"));
    let mut connection = tokio::net::TcpStream::connect(address).await.unwrap();
    connection
        .write_all(b"GET /network.txt HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .await
        .unwrap();
    response.clear();
    connection.read_to_end(&mut response).await.unwrap();
    assert!(response.starts_with(b"HTTP/1.1 200"));
    assert!(response.ends_with(b"hello"));
    server.abort();
}
