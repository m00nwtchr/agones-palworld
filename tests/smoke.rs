#[test]
fn build_only_runs() {
    let path = std::path::Path::new(env!("CARGO_BIN_EXE_agones-palworld"));
    assert!(path.exists());
}
