use xandsuite_lib::engine::local::LocalEngine;

#[test]
fn test_local_engine_rejects_missing_file() {
    let result = LocalEngine::new("/nonexistent/path/model.gguf".to_string());
    assert!(result.is_err(), "Should fail for missing model file");
}
