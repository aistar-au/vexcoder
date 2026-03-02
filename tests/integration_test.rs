use vexcoder::config::Config;

#[test]
fn test_config_validation_rejects_local_model_for_remote_endpoint() {
    let config = Config {
        model_token: Some("test-key".to_string()),
        model_name: "local/mock-model".to_string(),
        model_url: "https://model.example.internal/v1/messages".to_string(),
        working_dir: std::env::current_dir().expect("cwd"),
    };
    assert!(config.validate().is_err());
}

#[test]
fn test_config_validation_allows_local_endpoint_without_token() {
    let config = Config {
        model_token: None,
        model_name: "local/llama3.3".to_string(),
        model_url: "http://localhost:8000/v1/messages".to_string(),
        working_dir: std::env::current_dir().expect("cwd"),
    };
    assert!(config.validate().is_ok());
}
