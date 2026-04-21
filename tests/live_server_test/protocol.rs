use super::*;

#[tokio::test]
async fn test_chat_compat_config_builds_valid_api_client() {
    let base_url = live_server_url();
    let model = require_live_server!(&base_url);

    let config = build_chat_compat_config(&base_url, &model);
    let result = vexcoder::api::ApiClient::new(&config);
    assert!(
        result.is_ok(),
        "ApiClient::new must succeed for chat-compat config: {:?}",
        result.err()
    );

    let client = result.unwrap();
    assert!(
        client.is_local_endpoint(),
        "live server URL must be detected as local endpoint"
    );
    assert!(
        client.https_local_startup_warning().is_none(),
        "plain HTTP local server must not trigger HTTPS warning"
    );
    assert_eq!(
        client.protocol(),
        ModelProtocol::ChatCompat,
        "client must use ChatCompat protocol"
    );
}

#[tokio::test]
async fn test_messages_v1_config_builds_valid_api_client() {
    let base_url = live_server_url();
    let model = require_live_server!(&base_url);

    let config = build_messages_v1_config(&base_url, &model);
    let result = vexcoder::api::ApiClient::new(&config);
    assert!(
        result.is_ok(),
        "ApiClient::new must succeed for messages-v1 config: {:?}",
        result.err()
    );

    let client = result.unwrap();
    assert!(
        client.is_local_endpoint(),
        "messages-v1 live server URL must be detected as local endpoint"
    );
    assert!(
        client.https_local_startup_warning().is_none(),
        "plain HTTP messages-v1 server must not trigger HTTPS warning"
    );
    assert_eq!(
        client.protocol(),
        ModelProtocol::MessagesV1,
        "client must use MessagesV1 protocol"
    );
}

#[tokio::test]
async fn test_chat_compat_and_messages_v1_use_different_protocols() {
    let base_url = live_server_url();
    let model = require_live_server!(&base_url);

    let chat_config = build_chat_compat_config(&base_url, &model);
    let msg_config = build_messages_v1_config(&base_url, &model);

    let chat_client = vexcoder::api::ApiClient::new(&chat_config).unwrap();
    let msg_client = vexcoder::api::ApiClient::new(&msg_config).unwrap();

    assert_eq!(chat_client.protocol(), ModelProtocol::ChatCompat);
    assert_eq!(msg_client.protocol(), ModelProtocol::MessagesV1);
}

#[tokio::test]
async fn test_messages_v1_url_resolves_correctly() {
    let base_url = live_server_url();
    let model = require_live_server!(&base_url);

    let config = build_messages_v1_config(&base_url, &model);
    let expected_url = format!("{}/v1/messages", base_url.trim_end_matches('/'));
    assert_eq!(config.model_url, expected_url);
    assert_eq!(config.model_protocol, ModelProtocol::MessagesV1);
}

#[tokio::test]
async fn test_chat_compat_url_resolves_correctly() {
    let base_url = live_server_url();
    let model = require_live_server!(&base_url);

    let config = build_chat_compat_config(&base_url, &model);
    let expected_url = format!("{}/v1/chat/completions", base_url.trim_end_matches('/'));
    assert_eq!(config.model_url, expected_url);
    assert_eq!(config.model_protocol, ModelProtocol::ChatCompat);
}
