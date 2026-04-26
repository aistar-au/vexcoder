use super::*;

#[tokio::test]
async fn rename_file_refreshes_codebase_search_index() -> Result<()> {
    let _env_lock = crate::test_support::ENV_LOCK.lock().await;
    let temp = TempDir::new()?;
    let src_dir = temp.path().join("src");
    std::fs::create_dir_all(&src_dir)?;
    std::fs::write(src_dir.join("old_name.rs"), "pub fn renamed_symbol() {}\n")?;
    rebuild_codebase_index_for_tests(temp.path());
    let operator = ToolOperator::new(temp.path().to_path_buf());

    let before = call_tool_routing(
        &operator,
        "codebase_search",
        &json!({"query":"renamed_symbol","max_results":5}),
    )?;
    assert!(before.contains("src/old_name.rs:1-1"));

    call_tool_routing(
        &operator,
        "rename_file",
        &json!({"old_path":"src/old_name.rs","new_path":"src/new_name.rs"}),
    )?;

    let after = call_tool_routing(
        &operator,
        "codebase_search",
        &json!({"query":"renamed_symbol","max_results":5}),
    )?;
    assert!(after.contains("src/new_name.rs:1-1"));
    assert!(!after.contains("src/old_name.rs:1-1"));
    Ok(())
}

#[tokio::test]
async fn codebase_search_works_without_embeddings() -> Result<()> {
    let _env_lock = crate::test_support::ENV_LOCK.lock().await;
    crate::test_support::test_remove_var(&_env_lock, "VEX_EMBEDDING_PROVIDER");
    let temp = TempDir::new()?;
    std::fs::create_dir_all(temp.path().join("src"))?;
    std::fs::write(
        temp.path().join("src/searchable.rs"),
        "pub fn semantic_fallback() {}\n",
    )?;
    rebuild_codebase_index_for_tests(temp.path());
    let operator = ToolOperator::new(temp.path().to_path_buf());
    let result = call_tool_routing(
        &operator,
        "codebase_search",
        &json!({"query":"semantic_fallback","max_results":5}),
    )?;
    assert!(
        result.contains("semantic_fallback") || result.contains("searchable.rs"),
        "search must find function without embeddings; result: {result}"
    );
    Ok(())
}

#[test]
fn mutating_tool_prompts_approval_when_env_is_off() {
    let _env_lock = crate::test_support::ENV_LOCK.blocking_lock();
    crate::test_support::test_set_var(&_env_lock, "VEX_TOOL_CONFIRM", "off");
    assert!(!tool_approval_enabled(false));
    assert!(tool_requires_confirmation("write_file"));
    let requires_approval = tool_approval_enabled(false) || tool_requires_confirmation("write_file");
    assert!(requires_approval, "write_file must request approval when VEX_TOOL_CONFIRM=off");
}
