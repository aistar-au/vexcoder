use super::*;

impl TuiMode {
    pub(super) fn expand_inline_file_tokens(&self, input: &str) -> String {
        if input.starts_with('/') {
            return input.to_string();
        }

        let assembler = ContextAssembler::default();
        let operator = ToolOperator::new(self.working_dir.clone());
        let mut output = String::new();
        let mut token = String::new();

        for ch in input.chars() {
            if ch.is_whitespace() {
                if !token.is_empty() {
                    output.push_str(&self.expand_inline_token(&token, &operator, &assembler));
                    token.clear();
                }
                output.push(ch);
            } else {
                token.push(ch);
            }
        }

        if !token.is_empty() {
            output.push_str(&self.expand_inline_token(&token, &operator, &assembler));
        }

        output
    }

    pub(super) fn expand_inline_token(
        &self,
        token: &str,
        operator: &ToolOperator,
        assembler: &ContextAssembler,
    ) -> String {
        let Some(path) = token.strip_prefix('@') else {
            return token.to_string();
        };

        if path.is_empty() {
            return token.to_string();
        }

        match operator.existing_path(path) {
            Ok(Some(resolved)) if resolved.is_dir() => {
                match operator.list_files(Some(path), assembler.max_related) {
                    Ok(listing) => format_inline_block("dir", path, &listing, false, None),
                    Err(error) => format!("[dir: {path} \u{2014} {error}]"),
                }
            }
            Ok(Some(_)) => match operator.read_file(path) {
                Ok(content) => {
                    let (content, truncated) =
                        truncate_head_bytes(&content, assembler.max_file_bytes);
                    format_inline_block(
                        "file",
                        path,
                        &content,
                        truncated,
                        Some(assembler.max_file_bytes),
                    )
                }
                Err(error) => format!("[file: {path} \u{2014} {error}]"),
            },
            Ok(None) => format!("[file: {path} \u{2014} not found]"),
            Err(error) => format!("[file: {path} \u{2014} {error}]"),
        }
    }
}
