use super::*;

#[tokio::test]
async fn retry_local_connect_errors_retries_connect_failures_for_local_endpoints() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let failing_listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    failing_listener.set_nonblocking(true).unwrap();
    let failing_addr = failing_listener.local_addr().unwrap();
    drop(failing_listener);

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let server_addr = listener.local_addr().unwrap();

    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new().route("/health", get(|| async { "ok" })),
        )
        .await
        .unwrap();
    });

    let readiness_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(1))
        .build()
        .unwrap();

    let mut server_ready = false;
    for _ in 0..20 {
        match readiness_client
            .get(format!("http://{server_addr}/health"))
            .send()
            .await
        {
            Ok(response) if response.status().is_success() => {
                let body = response.text().await.unwrap_or_default();
                if body == "ok" {
                    server_ready = true;
                    break;
                }
            }
            Ok(_) | Err(_) => {}
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(server_ready, "expected local test server to become ready");

    let result = retry_local_connect_errors(
        &format!("http://{server_addr}/v1/messages"),
        "req-local-retry",
        "test_connect_retry",
        || {
            let attempts = Arc::clone(&attempts);
            async move {
                let target_addr = if attempts.fetch_add(1, Ordering::SeqCst) == 0 {
                    failing_addr
                } else {
                    server_addr
                };
                reqwest::Client::builder()
                    .timeout(Duration::from_secs(1))
                    .build()
                    .unwrap()
                    .get(format!("http://{target_addr}/health"))
                    .send()
                    .await
            }
        },
    )
    .await;

    assert!(
        result.is_ok(),
        "expected retry to eventually succeed: {result:?}"
    );
    assert!(attempts.load(Ordering::SeqCst) > 1);

    server.abort();
}

#[tokio::test]
async fn retry_local_connect_errors_does_not_retry_non_local_connect_errors() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(20))
        .build()
        .unwrap();

    let result = retry_local_connect_errors(
        "https://model.example.internal/v1/messages",
        "req-remote-connect",
        "test_remote_no_retry",
        || {
            let client = client.clone();
            let attempts = Arc::clone(&attempts);
            async move {
                attempts.fetch_add(1, Ordering::SeqCst);
                client.get("http://127.0.0.1:9/health").send().await
            }
        },
    )
    .await;

    assert!(result.is_err());
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn retry_local_connect_errors_does_not_retry_non_connect_errors() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let client = reqwest::Client::new();

    let result = retry_local_connect_errors(
        "http://127.0.0.1:8000/v1/messages",
        "req-local-non-connect",
        "test_builder_error",
        || {
            let client = client.clone();
            let attempts = Arc::clone(&attempts);
            async move {
                attempts.fetch_add(1, Ordering::SeqCst);
                client.get("http://[").send().await
            }
        },
    )
    .await;

    assert!(result.is_err());
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn retry_local_connect_errors_stops_after_max_elapsed_time() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let start = Instant::now();
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(20))
        .build()
        .unwrap();

    let result = retry_local_connect_errors(
        "http://127.0.0.1:9/v1/messages",
        "req-local-timeout",
        "test_max_elapsed",
        || {
            let client = client.clone();
            let attempts = Arc::clone(&attempts);
            async move {
                attempts.fetch_add(1, Ordering::SeqCst);
                client.get("http://127.0.0.1:9/health").send().await
            }
        },
    )
    .await;

    assert!(result.is_err());
    assert_eq!(attempts.load(Ordering::SeqCst), 2);
    assert!(start.elapsed() >= LOCAL_CONNECT_RETRY_INITIAL_INTERVAL);
    assert!(start.elapsed() < LOCAL_CONNECT_RETRY_MAX_ELAPSED);
}
