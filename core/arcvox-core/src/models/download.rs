//! Catalogue model downloads: resumable, checksummed, cancellable.

use super::ModelError;
use serde::Serialize;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DownloadProgress {
    pub id: String,
    pub received: u64,
    pub total: u64,
    pub done: bool,
    pub error: Option<String>,
}

/// Streams `url` into `part`, continuing from its current length when the
/// server honours the range request. Progress is reported after every chunk.
pub(crate) async fn fetch_to(
    client: &reqwest::Client,
    url: &str,
    size_bytes: u64,
    part: &Path,
    cancel: Arc<AtomicBool>,
    mut on_progress: impl FnMut(u64, u64),
) -> Result<(), ModelError> {
    use futures_util::StreamExt;
    use tokio::io::AsyncWriteExt;
    let existing = tokio::fs::metadata(part)
        .await
        .map(|m| m.len())
        .unwrap_or(0);
    if existing > 0 && existing >= size_bytes {
        // Nothing left to fetch: the caller verifies the checksum, and a wrong file is dropped there.
        return Ok(());
    }
    let mut req = client.get(url);
    if existing > 0 {
        req = req.header(reqwest::header::RANGE, format!("bytes={existing}-"));
    }
    let resp = req
        .send()
        .await
        .map_err(|e| ModelError::Http(e.to_string()))?;
    if cancel.load(Ordering::SeqCst) {
        return Err(ModelError::Cancelled);
    }
    let status = resp.status();
    let (mut file, mut received) = if status == reqwest::StatusCode::PARTIAL_CONTENT {
        (
            tokio::fs::OpenOptions::new()
                .append(true)
                .open(part)
                .await?,
            existing,
        )
    } else if status == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
        // The partial file does not fit the remote file; the next attempt starts from zero.
        let _ = tokio::fs::remove_file(part).await;
        return Err(ModelError::Http(format!("status {status}")));
    } else if status.is_success() {
        // The server ignored the range: start over.
        (tokio::fs::File::create(part).await?, 0)
    } else {
        return Err(ModelError::Http(format!("status {status}")));
    };
    let total = size_bytes;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        if cancel.load(Ordering::SeqCst) {
            file.flush().await?;
            return Err(ModelError::Cancelled);
        }
        let chunk = match chunk {
            Ok(c) => c,
            Err(e) => {
                // Keep what arrived so a retry resumes from it.
                let _ = file.flush().await;
                return Err(ModelError::Http(e.to_string()));
            }
        };
        file.write_all(&chunk).await?;
        received += chunk.len() as u64;
        on_progress(received, total);
    }
    file.flush().await?;
    Ok(())
}

pub(crate) fn sha256_file(path: &Path) -> std::io::Result<String> {
    use sha2::Digest;
    let mut f = std::fs::File::open(path)?;
    let mut h = sha2::Sha256::new();
    std::io::copy(&mut f, &mut h)?;
    Ok(format!("{:x}", h.finalize()))
}

#[cfg(test)]
mod tests {
    use crate::models::{ModelError, ModelStore};
    use crate::paths::DataDir;
    use std::sync::Arc;

    fn body() -> Vec<u8> {
        (0..100_000u32).map(|i| (i % 251) as u8).collect()
    }

    fn sha(b: &[u8]) -> String {
        use sha2::Digest;
        format!("{:x}", sha2::Sha256::digest(b))
    }

    async fn server_with_range() -> wiremock::MockServer {
        use wiremock::{Mock, MockServer, Request, ResponseTemplate};
        let s = MockServer::start().await;
        let data = body();
        Mock::given(wiremock::matchers::path("/m.bin"))
            .respond_with(move |req: &Request| {
                let range = req
                    .headers
                    .get("range")
                    .and_then(|v| v.to_str().ok())
                    .map(|s| s.to_string());
                match range {
                    Some(r) => {
                        let start: usize = r
                            .trim_start_matches("bytes=")
                            .trim_end_matches('-')
                            .parse()
                            .unwrap();
                        ResponseTemplate::new(206)
                            .insert_header(
                                "content-range",
                                format!("bytes {}-{}/{}", start, data.len() - 1, data.len())
                                    .as_str(),
                            )
                            .set_body_bytes(data[start..].to_vec())
                    }
                    None => ResponseTemplate::new(200).set_body_bytes(data.clone()),
                }
            })
            .mount(&s)
            .await;
        s
    }

    fn manifest_for(uri: &str, sha256: &str, size: u64) -> String {
        format!(
            r#"{{"schema_version":1,"models":[{{"id":"m","runtime":"whisper","role":"asr","label":"M","url":"{uri}/m.bin","sha256":"{sha256}","size_bytes":{size},"license":"MIT","recommended":true}}]}}"#
        )
    }

    fn store_for(tmp: &std::path::Path, manifest: String) -> ModelStore {
        let dirs = DataDir::custom(tmp);
        dirs.ensure().unwrap();
        ModelStore::new(
            &dirs,
            reqwest::Client::new(),
            Box::leak(manifest.into_boxed_str()),
            None,
        )
    }

    #[tokio::test]
    async fn downloads_verifies_and_reports_progress() {
        let s = server_with_range().await;
        let tmp = tempfile::tempdir().unwrap();
        let m = manifest_for(&s.uri(), &sha(&body()), body().len() as u64);
        let store = store_for(tmp.path(), m);
        let mut rx = store.subscribe();
        let installed = store.download("m").await.unwrap();
        assert_eq!(std::fs::read(&installed.path).unwrap(), body());
        let mut events = Vec::new();
        while let Ok(p) = rx.try_recv() {
            events.push(p);
        }
        let last = events.last().unwrap();
        assert!(last.done && last.error.is_none() && last.received == last.total);
        assert_eq!(events.iter().filter(|p| p.done).count(), 1);
        assert!(events.iter().any(|p| !p.done && p.received > 0));
    }

    #[tokio::test]
    async fn resume_continues_from_offset() {
        let s = server_with_range().await;
        let tmp = tempfile::tempdir().unwrap();
        let m = manifest_for(&s.uri(), &sha(&body()), body().len() as u64);
        let store = store_for(tmp.path(), m);
        let part = store.model_path("m").with_extension("bin.part");
        std::fs::create_dir_all(part.parent().unwrap()).unwrap();
        std::fs::write(&part, &body()[..60_000]).unwrap();
        store.download("m").await.unwrap();
        assert_eq!(std::fs::read(store.model_path("m")).unwrap(), body());
        let reqs = s.received_requests().await.unwrap();
        assert_eq!(
            reqs[0].headers.get("range").unwrap().to_str().unwrap(),
            "bytes=60000-"
        );
    }

    #[tokio::test]
    async fn ignored_range_restarts_from_zero() {
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let s = MockServer::start().await;
        Mock::given(wiremock::matchers::path("/m.bin"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(body()))
            .mount(&s)
            .await;
        let tmp = tempfile::tempdir().unwrap();
        let m = manifest_for(&s.uri(), &sha(&body()), body().len() as u64);
        let store = store_for(tmp.path(), m);
        let part = store.model_path("m").with_extension("bin.part");
        std::fs::create_dir_all(part.parent().unwrap()).unwrap();
        std::fs::write(&part, b"stale partial bytes").unwrap();
        store.download("m").await.unwrap();
        assert_eq!(std::fs::read(store.model_path("m")).unwrap(), body());
    }

    #[tokio::test]
    async fn sha_mismatch_removes_temp() {
        let s = server_with_range().await;
        let tmp = tempfile::tempdir().unwrap();
        let m = manifest_for(&s.uri(), "00", body().len() as u64);
        let store = store_for(tmp.path(), m);
        let err = store.download("m").await.unwrap_err();
        assert!(matches!(err, ModelError::Sha256Mismatch { .. }));
        assert!(!store.model_path("m").exists());
        assert!(!store.model_path("m").with_extension("bin.part").exists());
    }

    #[tokio::test]
    async fn cancel_stops_and_never_installs() {
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let s = MockServer::start().await;
        Mock::given(wiremock::matchers::path("/m.bin"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_bytes(body())
                    .set_delay(std::time::Duration::from_millis(500)),
            )
            .mount(&s)
            .await;
        let tmp = tempfile::tempdir().unwrap();
        let m = manifest_for(&s.uri(), &sha(&body()), body().len() as u64);
        let store = Arc::new(store_for(tmp.path(), m));
        let st = store.clone();
        let task = tokio::spawn(async move { st.download("m").await });
        // Wait until the request reached the server, then cancel while it is delayed.
        for _ in 0..1000 {
            if !s.received_requests().await.unwrap().is_empty() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert!(!s.received_requests().await.unwrap().is_empty());
        store.cancel("m");
        assert!(matches!(task.await.unwrap(), Err(ModelError::Cancelled)));
        assert!(!store.model_path("m").exists());
        // A finished (cancelled) download frees the id for a new attempt.
        assert!(store.downloads.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn second_download_of_a_running_id_is_refused() {
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let s = MockServer::start().await;
        Mock::given(wiremock::matchers::path("/m.bin"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_bytes(body())
                    .set_delay(std::time::Duration::from_millis(300)),
            )
            .mount(&s)
            .await;
        let tmp = tempfile::tempdir().unwrap();
        let m = manifest_for(&s.uri(), &sha(&body()), body().len() as u64);
        let store = Arc::new(store_for(tmp.path(), m));
        let st = store.clone();
        let first = tokio::spawn(async move { st.download("m").await });
        for _ in 0..1000 {
            if !s.received_requests().await.unwrap().is_empty() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert!(!s.received_requests().await.unwrap().is_empty());
        assert!(matches!(
            store.download("m").await,
            Err(ModelError::Http(m)) if m == "already downloading"
        ));
        first.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn refuses_when_disk_is_too_small() {
        let tmp = tempfile::tempdir().unwrap();
        let m = manifest_for("http://127.0.0.1:9", "00", u64::MAX / 4);
        let store = store_for(tmp.path(), m);
        assert!(matches!(
            store.download("m").await,
            Err(ModelError::InsufficientDisk { .. })
        ));
    }

    #[tokio::test]
    async fn complete_part_verifies_without_a_request() {
        let s = server_with_range().await;
        let tmp = tempfile::tempdir().unwrap();
        let m = manifest_for(&s.uri(), &sha(&body()), body().len() as u64);
        let store = store_for(tmp.path(), m);
        let part = store.model_path("m").with_extension("bin.part");
        std::fs::create_dir_all(part.parent().unwrap()).unwrap();
        std::fs::write(&part, body()).unwrap();
        store.download("m").await.unwrap();
        assert_eq!(std::fs::read(store.model_path("m")).unwrap(), body());
        assert!(s.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn oversized_part_is_dropped_and_the_next_attempt_succeeds() {
        let s = server_with_range().await;
        let tmp = tempfile::tempdir().unwrap();
        let m = manifest_for(&s.uri(), &sha(&body()), body().len() as u64);
        let store = store_for(tmp.path(), m);
        let part = store.model_path("m").with_extension("bin.part");
        std::fs::create_dir_all(part.parent().unwrap()).unwrap();
        let mut big = body();
        big.extend_from_slice(b"extra");
        std::fs::write(&part, big).unwrap();
        assert!(matches!(
            store.download("m").await,
            Err(ModelError::Sha256Mismatch { .. })
        ));
        assert!(!part.exists());
        store.download("m").await.unwrap();
        assert_eq!(std::fs::read(store.model_path("m")).unwrap(), body());
    }

    #[tokio::test]
    async fn range_not_satisfiable_drops_the_part() {
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let s = MockServer::start().await;
        Mock::given(wiremock::matchers::path("/m.bin"))
            .respond_with(ResponseTemplate::new(416))
            .mount(&s)
            .await;
        let tmp = tempfile::tempdir().unwrap();
        let m = manifest_for(&s.uri(), &sha(&body()), body().len() as u64);
        let store = store_for(tmp.path(), m);
        let part = store.model_path("m").with_extension("bin.part");
        std::fs::create_dir_all(part.parent().unwrap()).unwrap();
        std::fs::write(&part, &body()[..10]).unwrap();
        assert!(matches!(
            store.download("m").await,
            Err(ModelError::Http(m)) if m == "status 416 Range Not Satisfiable"
        ));
        assert!(!part.exists());
    }

    #[tokio::test]
    async fn http_error_keeps_the_part() {
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let s = MockServer::start().await;
        Mock::given(wiremock::matchers::path("/m.bin"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&s)
            .await;
        let tmp = tempfile::tempdir().unwrap();
        let m = manifest_for(&s.uri(), &sha(&body()), body().len() as u64);
        let store = store_for(tmp.path(), m);
        let part = store.model_path("m").with_extension("bin.part");
        std::fs::create_dir_all(part.parent().unwrap()).unwrap();
        std::fs::write(&part, &body()[..10]).unwrap();
        assert!(matches!(
            store.download("m").await,
            Err(ModelError::Http(_))
        ));
        assert_eq!(std::fs::read(&part).unwrap(), &body()[..10]);
        assert!(!store.model_path("m").exists());
    }

    /// Serves the body in two writes with a pause between them.
    fn two_chunk_server(data: Vec<u8>) -> String {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let Ok((mut sock, _)) = listener.accept() else {
                return;
            };
            let mut buf = [0u8; 4096];
            let mut seen = Vec::new();
            while !seen.windows(4).any(|w| w == b"\r\n\r\n") {
                match sock.read(&mut buf) {
                    Ok(0) | Err(_) => return,
                    Ok(n) => seen.extend_from_slice(&buf[..n]),
                }
            }
            let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", data.len());
            let half = data.len() / 2;
            let _ = sock.write_all(head.as_bytes());
            let _ = sock.write_all(&data[..half]);
            let _ = sock.flush();
            std::thread::sleep(std::time::Duration::from_millis(500));
            let _ = sock.write_all(&data[half..]);
        });
        format!("http://{addr}")
    }

    #[tokio::test]
    async fn cancel_between_chunks_keeps_the_part() {
        let uri = two_chunk_server(body());
        let tmp = tempfile::tempdir().unwrap();
        let m = manifest_for(&uri, &sha(&body()), body().len() as u64);
        let store = Arc::new(store_for(tmp.path(), m));
        let mut rx = store.subscribe();
        let st = store.clone();
        let task = tokio::spawn(async move { st.download("m").await });
        let first = tokio::time::timeout(std::time::Duration::from_secs(10), rx.recv())
            .await
            .expect("first chunk arrives")
            .unwrap();
        assert!(!first.done && first.received > 0);
        store.cancel("m");
        assert!(matches!(task.await.unwrap(), Err(ModelError::Cancelled)));
        let part = store.model_path("m").with_extension("bin.part");
        assert!(std::fs::metadata(&part).unwrap().len() > 0);
        assert!(!store.model_path("m").exists());
    }

    #[tokio::test]
    async fn dropped_download_frees_the_id() {
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let s = MockServer::start().await;
        Mock::given(wiremock::matchers::path("/m.bin"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_bytes(body())
                    .set_delay(std::time::Duration::from_secs(5)),
            )
            .mount(&s)
            .await;
        let tmp = tempfile::tempdir().unwrap();
        let m = manifest_for(&s.uri(), &sha(&body()), body().len() as u64);
        let store = store_for(tmp.path(), m);
        let _ =
            tokio::time::timeout(std::time::Duration::from_millis(200), store.download("m")).await;
        assert!(store.downloads.lock().unwrap().is_empty());
    }
}
