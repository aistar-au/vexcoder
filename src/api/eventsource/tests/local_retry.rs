use super::*;

#[tokio::test]
async fn retry_local_connect_errors_retries_initial_local_startup_timeout() {
    let client_attempts = Arc::new(AtomicUsize::new(0));
    let server_attempts = Arc::new(AtomicUsize::new(0));
    let request_timeout = LOCAL_CONNECT_RETRY_MAX_ELAPSED + Duration::from_millis(50);
    let stall_duration = request_timeout + Duration::from_millis(20);

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let server_addr = listener.local_addr().unwrap();
    let handler_attempts = Arc::clone(&server_attempts);
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new().route(
                "/health",
                get(move || {
                    let handler_attempts = Arc::clone(&handler_attempts);
                    async move {
                        if handler_attempts.fetch_add(1, Ordering::SeqCst) == 0 {
                            tokio::time::sleep(stall_duration).await;
                        }
                        "ok"
                    }
                }),
            ),
        )
        .await
        .unwrap();
    });

    let client = reqwest::Client::builder()
        .timeout(request_timeout)
        .build()
        .unwrap();

    let result = retry_local_connect_errors(
        &format!("http://{server_addr}/v1/messages"),
        "req-local-timeout-retry",
        "test_timeout_retry",
        || {
            let client = client.clone();
            let client_attempts = Arc::clone(&client_attempts);
            async move {
                client_attempts.fetch_add(1, Ordering::SeqCst);
                client
                    .get(format!("http://{server_addr}/health"))
                    .send()
                    .await
            }
        },
    )
    .await;

    assert!(
        result.is_ok(),
        "expected retry to eventually succeed after the initial timeout: {result:?}"
    );
    assert_eq!(client_attempts.load(Ordering::SeqCst), 2);
    assert_eq!(server_attempts.load(Ordering::SeqCst), 2);

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
    let request_timeout = Duration::from_millis(20);
    let client = reqwest::Client::builder()
        .timeout(request_timeout)
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
    assert!(attempts.load(Ordering::SeqCst) >= 2);
    assert!(start.elapsed() >= LOCAL_CONNECT_RETRY_INITIAL_INTERVAL);
    assert!(
        start.elapsed()
            < LOCAL_CONNECT_RETRY_MAX_ELAPSED + LOCAL_CONNECT_RETRY_MAX_INTERVAL + request_timeout
    );
}
