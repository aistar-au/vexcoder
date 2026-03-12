pub const CODER_SYSTEM_PROMPT: &str = include_str!("prompts/coder_system.txt");

const EDIT_TEMPLATE: &str = include_str!("prompts/edit_template.txt");
const EXPLAIN_TEMPLATE: &str = include_str!("prompts/explain_template.txt");
const FIX_TEMPLATE: &str = include_str!("prompts/fix_template.txt");
const PLAN_TEMPLATE: &str = include_str!("prompts/plan_template.txt");
const REVIEW_TEMPLATE: &str = include_str!("prompts/review_template.txt");
const GENERATE_TESTS_TEMPLATE: &str = include_str!("prompts/generate_tests_template.txt");
const PR_SUMMARY_TEMPLATE: &str = include_str!("prompts/pr_summary_template.txt");

fn render_template(template: &str, replacements: &[(&str, &str)]) -> String {
    let mut rendered = template.to_string();
    for (placeholder, value) in replacements {
        rendered = rendered.replace(placeholder, value);
    }
    rendered
}

pub fn render_edit_prompt(instruction: &str, context: &str) -> String {
    render_template(
        EDIT_TEMPLATE,
        &[("{{instruction}}", instruction), ("{{context}}", context)],
    )
}

pub fn render_explain_prompt(instruction: &str, context: &str) -> String {
    render_template(
        EXPLAIN_TEMPLATE,
        &[("{{instruction}}", instruction), ("{{context}}", context)],
    )
}

pub fn render_fix_prompt(instruction: &str, context: &str) -> String {
    render_template(
        FIX_TEMPLATE,
        &[("{{instruction}}", instruction), ("{{context}}", context)],
    )
}

pub fn render_plan_prompt(instruction: &str, context: &str, scope: &str) -> String {
    render_template(
        PLAN_TEMPLATE,
        &[
            ("{{instruction}}", instruction),
            ("{{context}}", context),
            ("{{scope}}", scope),
        ],
    )
}

pub fn render_review_prompt(instruction: &str, context: &str, diff_context: &str) -> String {
    render_template(
        REVIEW_TEMPLATE,
        &[
            ("{{instruction}}", instruction),
            ("{{context}}", context),
            ("{{diff_context}}", diff_context),
        ],
    )
}

pub fn render_generate_tests_prompt(instruction: &str, context: &str) -> String {
    render_template(
        GENERATE_TESTS_TEMPLATE,
        &[("{{instruction}}", instruction), ("{{context}}", context)],
    )
}

pub fn render_pr_summary_prompt(instruction: &str, context: &str, diff_context: &str) -> String {
    render_template(
        PR_SUMMARY_TEMPLATE,
        &[
            ("{{instruction}}", instruction),
            ("{{context}}", context),
            ("{{diff_context}}", diff_context),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prompt_templates_replace_placeholders() {
        let explain = render_explain_prompt("explain src/app.rs", "context block");
        assert!(explain.contains("explain src/app.rs"));
        assert!(explain.contains("context block"));
        assert!(!explain.contains("{{instruction}}"));
        assert!(!explain.contains("{{context}}"));

        let plan = render_plan_prompt("plan the change", "ctx", "src/app.rs");
        assert!(plan.contains("plan the change"));
        assert!(plan.contains("ctx"));
        assert!(plan.contains("src/app.rs"));
        assert!(!plan.contains("{{scope}}"));
    }

    #[test]
    fn test_docs_tools_md_lists_slash_commands() {
        let docs =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/docs/src/commands.md"))
                .expect("commands docs must be readable");

        for command in [
            "/edit <instruction>",
            "/fix",
            "/explain [path]",
            "/run [command]",
            "/test",
            "/context",
            "/commands",
            "/help",
        ] {
            assert!(
                docs.contains(command),
                "commands docs must mention {command}"
            );
        }
    }
}
