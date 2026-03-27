use crate::parse_session_id;

#[test]
fn test_parse_session_id_bare() {
    assert_eq!(parse_session_id("my-track"), "my-track");
}

#[test]
fn test_parse_session_id_with_host_prefix() {
    assert_eq!(parse_session_id("db.layers.audio/my-track"), "my-track");
}

#[test]
fn test_parse_session_id_with_wss_prefix() {
    assert_eq!(parse_session_id("wss://db.layers.audio:8000/my-track"), "my-track");
}

#[test]
fn test_parse_session_id_trims_and_lowercases() {
    assert_eq!(parse_session_id("  MY-TRACK  "), "my-track");
}

#[test]
fn test_parse_session_id_empty() {
    assert_eq!(parse_session_id(""), "");
}
