#[cfg(test)]
mod tool_tests {
    use super::super::PulseToolPolicy;
    use crate::state::conversation::tools::formatting::parse_tagged_tool_calls;
    use crate::state::conversation::tools::routing::call_tool_routing;
    use crate::state::conversation::tools::validation::{
        is_read_only_user_request, is_test_target_path, tests_only_mutation_conflict_prompt,
    };
    use crate::tools::ToolOperator;
    use serde_json::json;

    #[test]
    fn test_tests_only_policy_allows_rust_tests_module_paths() {
        let input = json!({
            "path": "src/state/conversation/tests/mod.rs",
            "content": "#[test]\nfn keeps_guard_happy() {}\n",
        });

        assert_eq!(
            tests_only_mutation_conflict_prompt(
                PulseToolPolicy::TestsOnlyMutations,
                "write_file",
                &input,
            ),
            None
        );
        assert!(is_test_target_path("src/state/conversation/tests/mod.rs"));
        assert!(is_test_target_path("src/runtime/test.rs"));
    }

    #[test]
    fn test_tests_only_policy_blocks_apply_patch_source_paths() {
        let input = json!({
            "path": "src/lib.rs",
            "content": "pub fn answer() -> i32 { 42 }\n",
        });

        let message = tests_only_mutation_conflict_prompt(
            PulseToolPolicy::TestsOnlyMutations,
            "apply_patch",
            &input,
        )
        .expect("apply_patch against src/ should be rejected");
        assert!(message.contains("src/lib.rs"));
    }

    #[test]
    fn test_tests_only_policy_parses_apply_patch_diff_headers_when_path_missing() {
        let input = json!({
            "content": "\
        diff --git a/src/lib.rs b/src/lib.rs\n\
        --- a/src/lib.rs\n\
        +++ b/src/lib.rs\n\
        @@ -1 +1 @@\n\
        -old\n\
        +new\n"
        });

        let message = tests_only_mutation_conflict_prompt(
            PulseToolPolicy::TestsOnlyMutations,
            "apply_patch",
            &input,
        )
        .expect("diff headers should still identify the blocked source file");
        assert!(message.contains("src/lib.rs"));
    }

    #[test]
    fn test_vex_force_mutating_turn_overrides_heuristic() {
        let _lock = crate::test_support::ENV_LOCK.blocking_lock();

        assert!(
            is_read_only_user_request("show me the files"),
            "expected read-only classification without override"
        );

        crate::test_support::test_set_var(&_lock, "VEX_FORCE_MUTATING_TURN", "1");
        let result = is_read_only_user_request("show me the files");
        crate::test_support::test_remove_var(&_lock, "VEX_FORCE_MUTATING_TURN");
        assert!(
            !result,
            "VEX_FORCE_MUTATING_TURN=1 should force mutating classification"
        );
    }

    #[test]
    fn tagged_numeric_read_file_parameters_preserve_requested_range() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("file.txt"), "one\ntwo\nthree\nfour\nfive\n").unwrap();
        let operator = ToolOperator::new(dir.path().to_path_buf());
        let calls = parse_tagged_tool_calls(
            "<function=read_file>\n<parameter=path>file.txt</parameter>\n<parameter=offset>3</parameter>\n<parameter=limit>2</parameter>\n</function>",
        );

        let output = call_tool_routing(&operator, "read_file", &calls[0].input).unwrap();

        assert!(
            output.contains("(showing lines 3-4 of 5)"),
            "unexpected output: {output}"
        );
        assert!(output.contains("\tthree"), "unexpected output: {output}");
        assert!(output.contains("\tfour"), "unexpected output: {output}");
        assert!(!output.contains("\tone\n"), "unexpected output: {output}");
    }
}
