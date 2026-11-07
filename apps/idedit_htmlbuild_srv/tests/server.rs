//! The actual process serves saved redb revisions without a published folder.
#![cfg(not(target_os = "trueos"))]
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use trueos_ide::{
    SourceFileInput,
    html_preview::{CSS, Config, HTML, JS, PROTOCOL, READY_PREFIX, Ready},
    source_image::SourceImage,
};

struct Server(Child);
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn request(port: u16, method: &str, path: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut result = String::new();
    stream.read_to_string(&mut result).unwrap();
    result
}
#[test]
fn source_store_save_refreshes_the_same_development_server() {
    let directory = tempfile::tempdir().unwrap();
    let database = directory.path().join("sources.redb");
    let mut image = SourceImage::open(&[]).unwrap();
    image
        .database
        .import_local(
            "web",
            None,
            vec![
                SourceFileInput::new("index.html", HTML.as_bytes().to_vec()),
                SourceFileInput::new("main.js", JS.as_bytes().to_vec()),
                SourceFileInput::new("style.css", CSS.as_bytes().to_vec()),
            ],
        )
        .unwrap();
    std::fs::write(&database, image.into_bytes().unwrap()).unwrap();
    let config = Config {
        endpoint: 7,
        database: database.to_str().unwrap().into(),
        project: "web".into(),
        entry: "index.html".into(),
    };
    let script = directory.path().join("startup");
    std::fs::write(&script, config.script().unwrap()).unwrap();
    let mut child = Server(
        Command::new(env!("CARGO_BIN_EXE_idedit_htmlbuild_srv"))
            .arg(script)
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    );
    let stdout = child.0.stdout.take().unwrap();
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Some(record) = line.strip_prefix(READY_PREFIX) {
                sender
                    .send(serde_json::from_str::<Ready>(record).unwrap())
                    .unwrap();
                break;
            }
        }
    });
    let ready = receiver.recv_timeout(Duration::from_secs(8)).unwrap();
    assert_eq!(ready.protocol, PROTOCOL);
    assert_eq!(ready.endpoint, 7);
    let port = ready
        .url
        .strip_prefix("http://127.0.0.1:")
        .unwrap()
        .split('/')
        .next()
        .unwrap()
        .parse()
        .unwrap();
    let first_revision = request(port, "GET", "/__idedit/revision")
        .split_once("\r\n\r\n")
        .unwrap()
        .1
        .to_owned();
    for (name, text, mime) in [
        ("index.html", HTML, "text/html"),
        ("main.js", JS, "text/javascript"),
        ("style.css", CSS, "text/css"),
    ] {
        let response = request(port, "GET", &format!("/{name}"));
        assert!(response.starts_with("HTTP/1.1 200"));
        assert!(
            response
                .to_ascii_lowercase()
                .contains(&format!("content-type: {mime}"))
        );
        assert!(
            response
                .to_ascii_lowercase()
                .contains("cache-control: no-store")
        );
        assert!(response.ends_with(text));
    }
    let bytes = std::fs::read(&database).unwrap();
    let mut image = SourceImage::open(&bytes).unwrap();
    let source = image.database.get("web", None).unwrap().unwrap();
    image
        .database
        .save_files(
            "web",
            source.revision.id,
            vec![
                SourceFileInput::new("index.html", HTML.as_bytes().to_vec()),
                SourceFileInput::new("style.css", b"body { background: red; }".to_vec()),
            ],
        )
        .unwrap();
    let updated = database.with_extension("new");
    std::fs::write(&updated, image.into_bytes().unwrap()).unwrap();
    std::fs::rename(updated, &database).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if request(port, "GET", "/style.css").ends_with("body { background: red; }") {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "saved source revision was not loaded"
        );
        std::thread::sleep(Duration::from_millis(30));
    }
    assert_ne!(
        request(port, "GET", "/__idedit/revision")
            .split_once("\r\n\r\n")
            .unwrap()
            .1,
        first_revision
    );
    assert!(request(port, "GET", "/main.js").starts_with("HTTP/1.1 404"));
    assert!(request(port, "GET", "/index.html").contains("<!-- idedit_htmlbuild_srv revision="));
    assert!(request(port, "HEAD", "/index.html").ends_with("\r\n\r\n"));
    assert!(request(port, "GET", "/%2e%2e/sources.redb").starts_with("HTTP/1.1 400"));
    assert!(request(port, "POST", "/index.html").starts_with("HTTP/1.1 405"));
    assert!(!directory.path().join("public").exists());
}
