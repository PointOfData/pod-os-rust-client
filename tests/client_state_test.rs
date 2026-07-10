use pod_os_client::ConnectionState;

#[test]
fn connection_state_display() {
    assert_eq!(ConnectionState::Connected.to_string(), "connected");
    assert_eq!(ConnectionState::Disconnected.to_string(), "disconnected");
    assert_eq!(ConnectionState::Reconnecting.to_string(), "reconnecting");
    assert_eq!(
        ConnectionState::ReconnectFailed.to_string(),
        "reconnect_failed"
    );
}

#[test]
fn connection_state_equality() {
    assert_eq!(ConnectionState::Connected, ConnectionState::Connected);
    assert_ne!(ConnectionState::Connected, ConnectionState::Disconnected);
}

#[test]
fn connection_state_debug() {
    let s = format!("{:?}", ConnectionState::Connected);
    assert_eq!(s, "Connected");
}
