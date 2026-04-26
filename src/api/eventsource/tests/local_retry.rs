use super::*;

#[tokio::test]
async fn retry_retries_on_initial_local_timeout_and_does_not_retry_remote_errors() {
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
                    let h = Arc::clone(&handler_attempts);
                    async move {
                        if h.fetch_add(1, Ordering::SeqCst) == 0 {
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
        "req-retry",
        "test_retry",
        || {
            let c = client.clone();
            let ca = Arc::clone(&client_attempts);
            async move {
                ca.fetch_add(1, Ordering::SeqCst);
                c.get(format!("http://{server_addr}/health")).send().await
            }
        },
    )
    .await;

    assert!(result.is_ok(), "retry must eventually succeed: {result:?}");
    assert_eq!(client_attempts.load(Ordering::SeqCst), 2);
    server.abort();

    let attempts = Arc::new(AtomicUsize::new(0));
    let nc = reqwest::Client::builder()
        .timeout(Duration::from_millis(20))
        .build()
        .unwrap();
    let r2 = retry_local_connect_errors(
        "https://model.example.internal/v1/messages",
        "req-remote",
        "test_no_retry",
        || {
            let c = nc.clone();
            let a = Arc::clone(&attempts);
            async move {
                a.fetch_add(1, Ordering::SeqCst);
                c.get("http://127.0.0.1:9/health").send().await
            }
        },
    )
    .await;
    assert!(r2.is_err());
    assert_eq!(
        attempts.load(Ordering::SeqCst),
        1,
        "non-local errors must not retry"
    );
}

#[tokio::test]
async fn retry_stops_after_max_elapsed_time() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let start = Instant::now();
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(20))
        .build()
        .unwrap();
    let result = retry_local_connect_errors(
        "http://127.0.0.1:9/v1/messages",
        "req-elapsed",
        "test_max_elapsed",
        || {
            let c = client.clone();
            let a = Arc::clone(&attempts);
            async move {
                a.fetch_add(1, Ordering::SeqCst);
                c.get("http://127.0.0.1:9/health").send().await
            }
        },
    )
    .await;
    assert!(result.is_err());
    assert!(attempts.load(Ordering::SeqCst) >= 2);
    assert!(start.elapsed() >= LOCAL_CONNECT_RETRY_INITIAL_INTERVAL);
}
