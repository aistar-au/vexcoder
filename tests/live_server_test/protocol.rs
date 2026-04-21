use super::*;

#[tokio::test]
async fn test_live_server_protocol_specific_configs_build_valid_clients() {
    let base_url = live_server_url();
    let model = require_live_server!(&base_url);
    let trimmed_base_url = base_url.trim_end_matches('/');

    let cases = vec![
        (
            "chat-compat",
            build_chat_compat_config(&base_url, &model),
            format!("{trimmed_base_url}/v1/chat/completions"),
            ModelProtocol::ChatCompat,
        ),
        (
            "messages-v1",
            build_messages_v1_config(&base_url, &model),
            format!("{trimmed_base_url}/v1/messages"),
            ModelProtocol::MessagesV1,
        ),
    ];

    let mut observed_protocols = Vec::new();
    for (label, config, expected_url, expected_protocol) in cases {
        assert_eq!(
            config.model_url, expected_url,
            "{label} config must resolve the expected URL"
        );
        assert_eq!(
            config.model_protocol, expected_protocol,
            "{label} config must preserve its declared protocol"
        );

        let client = vexcoder::api::ApiClient::new(&config).unwrap_or_else(|error| {
            panic!("ApiClient::new must succeed for {label} config: {error:#}")
        });

        assert!(
            client.is_local_endpoint(),
            "{label} live server URL must be detected as local"
        );
        assert!(
            client.https_local_startup_warning().is_none(),
            "{label} plain HTTP live server must not trigger the HTTPS warning"
        );
        assert_eq!(
            client.protocol(),
            expected_protocol,
            "{label} client must use the expected wire protocol"
        );
        observed_protocols.push(client.protocol());
    }

    assert_eq!(
        observed_protocols,
        vec![ModelProtocol::ChatCompat, ModelProtocol::MessagesV1]
    );
}
