pub const CODER_SYSTEM_PROMPT: &str = include_str!("prompts/coder_system.txt");

const EDIT_TEMPLATE: &str = include_str!("prompts/edit_template.txt");
const EXPLAIN_TEMPLATE: &str = include_str!("prompts/explain_template.txt");
const FIX_TEMPLATE: &str = include_str!("prompts/fix_template.txt");
const PLAN_TEMPLATE: &str = include_str!("prompts/plan_template.txt");
const REVIEW_TEMPLATE: &str = include_str!("prompts/review_template.txt");
const GENERATE_TESTS_TEMPLATE: &str = include_str!("prompts/generate_tests_template.txt");
const PR_SUMMARY_TEMPLATE: &str = include_str!("prompts/pr_summary_template.txt");

fn render_template(template: &str, replacements: &[(&str, &str)]) -> String {
    let mut rendered = String::with_capacity(template.len());
    let mut cursor = 0;

    while cursor < template.len() {
        if let Some((placeholder, value)) = replacements
            .iter()
            .find(|(placeholder, _)| template[cursor..].starts_with(*placeholder))
        {
            rendered.push_str(value);
            cursor += placeholder.len();
            continue;
        }

        let next_char = template[cursor..]
            .chars()
            .next()
            .expect("cursor must remain on a char boundary");
        rendered.push(next_char);
        cursor += next_char.len_utf8();
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

pub fn render_custom_command_instruction(template: &str, context: &str, input: &str) -> String {
    render_template(template, &[("{{context}}", context), ("{{input}}", input)])
}

pub fn render_generate_tests_prompt(instruction: &str, context: &str, framework: &str) -> String {
    render_template(
        GENERATE_TESTS_TEMPLATE,
        &[
            ("{{instruction}}", instruction),
            ("{{context}}", context),
            ("{{framework}}", framework),
        ],
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

        let generate_tests = render_generate_tests_prompt("generate tests", "ctx", "cargo-test");
        assert!(generate_tests.contains("generate tests"));
        assert!(generate_tests.contains("cargo-test"));
        assert!(!generate_tests.contains("{{framework}}"));
    }

    #[test]
    fn test_prompt_templates_preserve_literal_placeholder_text_in_values() {
        let instruction = "plan around literal {{context}} text";
        let context = "context keeps {{scope}} and {{instruction}} unchanged";
        let scope = "scope keeps {{context}} unchanged";

        let rendered = render_plan_prompt(instruction, context, scope);

        assert!(rendered.contains(instruction));
        assert!(rendered.contains(context));
        assert!(rendered.contains(scope));
    }

    #[test]
    fn test_custom_command_template_replaces_context_and_input() {
        let rendered = render_custom_command_instruction(
            "Input={{input}}\nContext={{context}}",
            "ctx",
            "rest of prompt",
        );
        assert!(rendered.contains("Input=rest of prompt"));
        assert!(rendered.contains("Context=ctx"));
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
            "/tools [desc]",
            "/generate-tests [path] [--framework <name>]",
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
