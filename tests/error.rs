use agones_palworld::error::AppError;

#[test]
fn config_error_carries_message() {
    let err = AppError::Config("missing PALWORLD_API_URL".into());
    assert_eq!(err.to_string(), "config: missing PALWORLD_API_URL");
}

#[test]
fn palworld_http_includes_status() {
    let err = AppError::PalworldHttp(reqwest::StatusCode::UNAUTHORIZED, "bad password".into());
    let s = err.to_string();
    assert!(s.contains("401"), "got: {s}");
    assert!(s.contains("bad password"));
}
