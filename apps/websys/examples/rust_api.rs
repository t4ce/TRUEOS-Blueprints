use std::error::Error;
use std::time::Duration;

use websys::{JobQueue, JobRequest, JobStatus};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let root = std::env::temp_dir().join("trueos-websys-rust-api");

    if tokio::fs::try_exists(&root).await? {
        tokio::fs::remove_dir_all(&root).await?;
    }
    tokio::fs::create_dir_all(&root).await?;

    let queue = JobQueue::new(root.clone());

    let upload = queue
        .enqueue(JobRequest::upload(
            "docs",
            "hello.txt",
            b"hello from rust api".to_vec(),
        )?)
        .await?;
    let upload = queue
        .wait_for_terminal(upload.id, Duration::from_millis(25))
        .await
        .expect("upload job should exist");
    assert_eq!(upload.status, JobStatus::Succeeded);

    let download = queue.enqueue_download("docs/hello.txt").await?;
    let download = queue
        .wait_for_terminal(download.id, Duration::from_millis(25))
        .await
        .expect("download job should exist");
    assert_eq!(download.status, JobStatus::Succeeded);

    println!("upload job id: {}", upload.id);
    println!("download artifact: {:?}", download.result_path);

    Ok(())
}
