use super::super::stream_block::{StreamBlock, ToolStatus};
use super::core::{emit_server_metadata_update, CompletedToolCall};
use super::tool_call_parser::{parser_for_mode, ToolCallParser, ToolParserMode};
use super::{
    history::*, streaming::*, tools::*, ConversationManager, ConversationStreamUpdate,
    TurnToolPolicy,
};
use crate::api::stream::StreamParser;
use crate::runtime::policy::{default_runtime_policy, RuntimeCorePolicy};
use crate::types::{ApiMessage, ApiUsage, Content, ContentBlock, StreamEvent};
use crate::usage::TurnTokens;
use anyhow::Result;
use futures::StreamExt;
use std::collections::BTreeSet;
use tokio::sync::mpsc;

impl ConversationManager {
    pub async fn send_message_with_policy(
        &mut self,
        content: String,
        stream_delta_tx: Option<&mpsc::UnboundedSender<ConversationStreamUpdate>>,
        turn_tool_policy: TurnToolPolicy,
    ) -> Result<String> {
        self.current_turn_blocks.clear();
        self.current_turn_applied_mutation = false;
        self.last_turn_tokens = TurnTokens::default();
        let original_user_input = content.clone();
        self.push_user_message(content);
        if let Some(response) = builtin_supported_git_tools_response(&original_user_input) {
            self.api_messages.push(ApiMessage {
                role: "assistant".to_string(),
                content: Content::Text(response.clone()),
            });
            emit_text_update(stream_delta_tx, response.clone());
            return Ok(response);
        }
        let mut turn_user_anchor_index = self.api_messages.len().saturating_sub(1);

        let core_policy = default_runtime_policy();
        let use_structured_tool_protocol = self.client.supports_structured_tool_protocol();
        let requires_tool_evidence =
            core_policy.request_requires_tool_evidence(&original_user_input);
        let limits = resolve_history_limits(self.client.is_local_endpoint());
        let tool_timeout = resolve_tool_timeout(self.client.is_local_endpoint());
        let max_tool_rounds = resolve_max_tool_rounds(self.client.is_local_endpoint());
        let require_tool_approval = tool_approval_enabled(self.client.is_local_endpoint());
        let history_keep_turns = resolve_history_keep_turns();
        let mut rounds = 0usize;
        let mut forced_tool_retry_count = 0usize;
        let mut saw_any_tool_round = false;
        let mut previous_round_signature: Option<Vec<String>> = None;
        let mut repeated_read_only_rounds = 0usize;
        let mut repeated_mutating_rounds = 0usize;
        let mut repeated_round_nudge_used = false;
        let mut last_assistant_text_for_history = String::new();
        let mut turn_tokens = TurnTokens::default();
        let mut compacted_this_turn = false;
        let default_tool_parser_mode = if self.client.is_local_endpoint() {
            ToolParserMode::Hybrid
        } else {
            ToolParserMode::Tagged
        };
        let tool_parser: Box<dyn ToolCallParser> =
            parser_for_mode(ToolParserMode::from_env_or(None, default_tool_parser_mode));
        // Condense once per user turn, not per API round, to stay idempotent.
        self.condense_old_tool_results(history_keep_turns);
        loop {
            self.current_turn_blocks.clear();
            turn_user_anchor_index = self
                .prune_message_history_preserving(limits.max_api_messages, turn_user_anchor_index);
            rounds += 1;
            if rounds > max_tool_rounds {
                return Ok(render_loop_limit_guard_message(
                    &last_assistant_text_for_history,
                    max_tool_rounds,
                ));
            }

            let mut stream = match self.client.create_stream(&self.api_messages).await {
                Ok(s) => s,
                Err(e)
                    if !compacted_this_turn
                        && crate::api::client::is_context_overflow(&e.to_string()) =>
                {
                    let before = self.api_messages.len();
                    self.compact_for_context_overflow();
                    let after = self.api_messages.len();
                    compacted_this_turn = true;
                    let summary = format!(
                        "compacted {} → {} messages to fit server window",
                        before, after
                    );
                    emit_text_update(
                        stream_delta_tx,
                        format!("\n[context: {summary}, retrying]\n"),
                    );
                    emit_stream_update(
                        stream_delta_tx,
                        ConversationStreamUpdate::ContextCompacted {
                            messages_before: before,
                            messages_after: after,
                            summary,
                        },
                    );
                    // Do not increment rounds — this is recovery, not a tool round.
                    rounds -= 1;
                    continue;
                }
                Err(e) => return Err(e),
            };
            let mut parser = StreamParser::new();
            let mut assistant_text = String::new();
            let mut tool_use_blocks = Vec::new();
            let mut tool_input_buffers: Vec<Option<String>> = Vec::new();
            let mut deferred_text_block_indices = BTreeSet::new();

            while let Some(chunk_result) = stream.next().await {
                let chunk = chunk_result?;
                let events = parser.process(&chunk)?;

                for event in events {
                    match event {
                        StreamEvent::MessageStart { message } => {
                            accumulate_usage(&mut turn_tokens, message.usage.as_ref());
                            emit_server_metadata_update(message.metadata.as_ref(), stream_delta_tx);
                        }
                        StreamEvent::ContentBlockStart {
                            index,
                            content_block,
                        } => {
                            // Structured block pipeline: all SSE events are
                            // normalised into typed StreamBlock variants that
                            // flow through the single runtime core engine.
                            match &content_block {
                                ContentBlock::Text { .. } => {
                                    self.upsert_turn_block(
                                        index,
                                        StreamBlock::Thinking {
                                            content: String::new(),
                                            collapsed: false,
                                        },
                                        None,
                                    );
                                    deferred_text_block_indices.insert(index);
                                }
                                ContentBlock::ToolUse {
                                    id, name, input, ..
                                } => {
                                    self.flush_deferred_thinking_blocks(
                                        &mut deferred_text_block_indices,
                                        stream_delta_tx,
                                    );
                                    self.upsert_turn_block(
                                        index,
                                        StreamBlock::ToolCall {
                                            id: id.clone(),
                                            name: name.clone(),
                                            input: input.clone(),
                                            status: ToolStatus::Pending,
                                        },
                                        stream_delta_tx,
                                    );
                                }
                                ContentBlock::ToolResult { .. } => {}
                                ContentBlock::Thinking { .. }
                                | ContentBlock::RedactedThinking { .. } => {}
                                ContentBlock::ServerToolUse { .. }
                                | ContentBlock::WebSearchToolResult { .. } => {}
                            }

                            let tool_name =
                                if let ContentBlock::ToolUse { name, .. } = &content_block {
                                    Some(name.clone())
                                } else {
                                    None
                                };
                            if tool_name.is_some() {
                                while tool_use_blocks.len() <= index {
                                    tool_use_blocks.push(None);
                                    tool_input_buffers.push(None);
                                }
                                tool_use_blocks[index] = Some(content_block);
                                tool_input_buffers[index] = Some(String::new());
                            }
                        }
                        StreamEvent::ContentBlockDelta { index, delta } => {
                            if let Some(text) = delta.text {
                                let delta_tx = if deferred_text_block_indices.contains(&index) {
                                    None
                                } else {
                                    stream_delta_tx
                                };
                                let appended = self.append_text_delta(index, &text, delta_tx);
                                assistant_text.push_str(&appended);
                            }

                            if let Some(partial_json) = delta.partial_json {
                                let maybe_buffer = tool_input_buffers.get_mut(index);
                                if let Some(Some(buffer)) = maybe_buffer {
                                    buffer.push_str(&partial_json);

                                    if let Ok(parsed_input) =
                                        serde_json::from_str::<serde_json::Value>(buffer)
                                    {
                                        if let Some(StreamBlock::ToolCall { input, .. }) =
                                            self.current_turn_blocks.get_mut(index)
                                        {
                                            *input = parsed_input;
                                        }
                                    }
                                    emit_stream_update(
                                        stream_delta_tx,
                                        ConversationStreamUpdate::BlockDelta {
                                            index,
                                            delta: partial_json.clone(),
                                        },
                                    );
                                }
                            }
                        }
                        StreamEvent::ContentBlockStop { index } => {
                            let maybe_json = tool_input_buffers.get_mut(index);
                            let maybe_tool = tool_use_blocks.get_mut(index);

                            if let (
                                Some(Some(json_str)),
                                Some(Some(ContentBlock::ToolUse { input, .. })),
                            ) = (maybe_json, maybe_tool)
                            {
                                if !json_str.is_empty() {
                                    let parse_result: Result<serde_json::Value, _> =
                                        serde_json::from_str(json_str)
                                            .or_else(|_| serde_json::from_str(json_str.trim()));
                                    match parse_result {
                                        Ok(parsed_input) => {
                                            *input = parsed_input;

                                            if let Some(StreamBlock::ToolCall {
                                                input: block_input,
                                                ..
                                            }) = self.current_turn_blocks.get_mut(index)
                                            {
                                                *block_input = input.clone();
                                            }
                                        }
                                        Err(err) => {
                                            tracing::warn!(
                                                index,
                                                json_len = json_str.len(),
                                                err = %err,
                                                "tool input JSON parse failed; tool will run with empty input"
                                            );
                                        }
                                    }
                                }
                            }
                            emit_stream_update(
                                stream_delta_tx,
                                ConversationStreamUpdate::BlockComplete { index },
                            );
                        }
                        StreamEvent::MessageDelta { delta, usage } => {
                            accumulate_usage(&mut turn_tokens, usage.as_ref());
                            emit_server_metadata_update(delta.metadata.as_ref(), stream_delta_tx);
                        }
                        StreamEvent::MessageStop => {}
                        StreamEvent::Ping => {}
                        StreamEvent::Error { error } => {
                            // Surface all stream errors (API errors and SSE
                            // parse failures) to the UI so the user observes
                            // the failure rather than a silently stalled turn.
                            // ADR-021 Item 19.
                            emit_stream_update(
                                stream_delta_tx,
                                ConversationStreamUpdate::StreamError(format!(
                                    "stream error ({}): {}",
                                    error.error_type, error.message
                                )),
                            );
                        }
                        StreamEvent::Unknown => {}
                    }
                }
            }

            let mut assistant_text_for_history = assistant_text.clone();
            let mut used_tagged_fallback = false;
            let mut tool_use_blocks: Vec<ContentBlock> =
                tool_use_blocks.into_iter().flatten().collect();
            if tool_use_blocks.is_empty() && self.client.is_local_endpoint() {
                let tagged_calls = dedupe_tagged_tool_calls(tool_parser.parse(&assistant_text));
                if !tagged_calls.is_empty() {
                    used_tagged_fallback = true;
                    assistant_text_for_history =
                        core_policy.sanitize_assistant_text(&assistant_text);
                    tool_use_blocks = tagged_calls
                        .into_iter()
                        .enumerate()
                        .map(|(index, call)| ContentBlock::ToolUse {
                            id: format!("toolu_tagged_{rounds}_{index}"),
                            name: call.name,
                            input: call.input,
                            metadata: None,
                        })
                        .collect();
                    {
                        let fallback_start_index = self.current_turn_blocks.len();
                        for (offset, block) in tool_use_blocks.iter().enumerate() {
                            if let ContentBlock::ToolUse {
                                id, name, input, ..
                            } = block
                            {
                                self.upsert_turn_block(
                                    fallback_start_index + offset,
                                    StreamBlock::ToolCall {
                                        id: id.clone(),
                                        name: name.clone(),
                                        input: input.clone(),
                                        status: ToolStatus::Pending,
                                    },
                                    stream_delta_tx,
                                );
                            }
                        }
                    }
                }
            }

            let use_structured_round = use_structured_tool_protocol && !used_tagged_fallback;

            let assistant_history_source = if !tool_use_blocks.is_empty() && !use_structured_round {
                let rendered_tool_calls = render_tool_calls_for_text_protocol(&tool_use_blocks);
                if assistant_text_for_history.is_empty() {
                    rendered_tool_calls
                } else {
                    format!("{assistant_text_for_history}\n{rendered_tool_calls}")
                }
            } else if assistant_text_for_history.is_empty() && !tool_use_blocks.is_empty() {
                render_tool_calls_for_text_protocol(&tool_use_blocks)
            } else {
                assistant_text_for_history.clone()
            };

            let mut inject_repeated_round_nudge = false;
            if !tool_use_blocks.is_empty() {
                saw_any_tool_round = true;
                let current_signature = tool_round_signature(&tool_use_blocks);
                let repeated_signature = previous_round_signature
                    .as_ref()
                    .is_some_and(|previous| previous == &current_signature);
                if is_read_only_tool_round(&tool_use_blocks) && repeated_signature {
                    repeated_read_only_rounds += 1;
                } else {
                    repeated_read_only_rounds = 0;
                }

                if is_mutating_tool_round(&tool_use_blocks) && repeated_signature {
                    repeated_mutating_rounds += 1;
                } else {
                    repeated_mutating_rounds = 0;
                }
                previous_round_signature = Some(current_signature);

                if repeated_mutating_rounds >= 1 {
                    return Ok(render_repeated_mutating_tool_guard_message(
                        &assistant_text_for_history,
                    ));
                }

                // Stop faster when the round contains empty-path tool calls
                // (e.g. read_file with no path) — one repeat is enough.
                let has_empty_path_call = tool_use_blocks.iter().any(|block| {
                    if let ContentBlock::ToolUse { name, input, .. } = block {
                        matches!(name.as_str(), "read_file" | "list_files")
                            && missing_read_only_location_prompt(name, input).is_some()
                    } else {
                        false
                    }
                });
                // Local models often loop on the same read-only call because
                // they fail to incorporate the prior tool result into their
                // context. Use a tighter threshold for local endpoints.
                let repeat_threshold = if has_empty_path_call || self.client.is_local_endpoint() {
                    1
                } else {
                    2
                };

                if repeated_read_only_rounds >= repeat_threshold {
                    if !repeated_round_nudge_used
                        && rounds < max_tool_rounds
                        && !has_empty_path_call
                    {
                        repeated_round_nudge_used = true;
                        inject_repeated_round_nudge = true;
                    } else {
                        return Ok(render_repeated_tool_guard_message(
                            &assistant_text_for_history,
                        ));
                    }
                }
            } else {
                previous_round_signature = None;
                repeated_read_only_rounds = 0;
                repeated_mutating_rounds = 0;
            }

            let assistant_history_text = assistant_history_source;
            let assistant_history_text =
                truncate_for_history(&assistant_history_text, limits.max_assistant_history_chars);

            if use_structured_round {
                let mut assistant_content_blocks = Vec::new();
                if !assistant_text_for_history.is_empty() {
                    assistant_content_blocks.push(ContentBlock::Text {
                        text: truncate_for_history(
                            &assistant_text_for_history,
                            limits.max_assistant_history_chars,
                        ),
                        citations: None,
                    });
                }
                assistant_content_blocks.extend(tool_use_blocks.clone());

                self.api_messages.push(ApiMessage {
                    role: "assistant".to_string(),
                    content: Content::Blocks(assistant_content_blocks),
                });
            } else {
                self.api_messages.push(ApiMessage {
                    role: "assistant".to_string(),
                    content: Content::Text(assistant_history_text),
                });
            }
            last_assistant_text_for_history = assistant_text_for_history.clone();

            if inject_repeated_round_nudge {
                self.api_messages.push(ApiMessage {
                    role: "user".to_string(),
                    content: Content::Text(
                        core_policy.repeated_tool_round_instruction().to_string(),
                    ),
                });
                continue;
            }

            if tool_use_blocks.is_empty() {
                if self.client.is_local_endpoint()
                    && requires_tool_evidence
                    && !saw_any_tool_round
                    && forced_tool_retry_count < 2
                    && rounds < max_tool_rounds
                {
                    forced_tool_retry_count += 1;
                    self.api_messages.push(ApiMessage {
                        role: "user".to_string(),
                        content: Content::Text(core_policy.tool_retry_instruction().to_string()),
                    });
                    continue;
                }
                if self.client.is_local_endpoint() && requires_tool_evidence && !saw_any_tool_round
                {
                    return Ok(render_missing_tool_evidence_guard_message(
                        &assistant_text_for_history,
                    ));
                }
                self.promote_thinking_blocks_to_final_text(
                    &deferred_text_block_indices,
                    stream_delta_tx,
                );
                self.last_turn_tokens = turn_tokens;
                return Ok(assistant_text_for_history);
            }

            let mut tool_result_blocks = Vec::new();
            let mut text_protocol_tool_results = Vec::new();
            if should_parallelize_tool_round(&tool_use_blocks, require_tool_approval) {
                let completed_calls = self
                    .execute_parallel_tool_round(
                        &tool_use_blocks,
                        &original_user_input,
                        tool_timeout,
                        true,
                        stream_delta_tx,
                    )
                    .await;

                for completed in completed_calls {
                    let CompletedToolCall {
                        id,
                        name,
                        input,
                        result,
                    } = completed;

                    let final_status = if result.is_err() {
                        ToolStatus::Error
                    } else {
                        ToolStatus::Complete
                    };
                    self.set_tool_call_status(&id, final_status, stream_delta_tx);

                    let output_for_stream = result
                        .as_ref()
                        .map_or_else(|e| e.to_string(), ToString::to_string);
                    self.push_tool_result_block(
                        StreamBlock::ToolResult {
                            tool_call_id: id.clone(),
                            output: output_for_stream,
                            is_error: result.is_err(),
                        },
                        stream_delta_tx,
                    );

                    let history_content = truncate_for_history(
                        &self.format_tool_result_for_history(&name, &input, &result),
                        limits.max_tool_result_history_chars,
                    );
                    if result.is_ok()
                        && matches!(
                            name.as_str(),
                            "write_file" | "apply_patch" | "edit_file" | "rename_file"
                        )
                    {
                        self.current_turn_applied_mutation = true;
                    }
                    if use_structured_round {
                        tool_result_blocks.push(ContentBlock::ToolResult {
                            tool_use_id: id,
                            content: history_content,
                            is_error: result.is_err(),
                        });
                    } else {
                        let rendered = result.as_ref().map_or_else(
                            |_| format!("tool_error {name}:\n{history_content}"),
                            |_| format!("tool_result {name}:\n{history_content}"),
                        );
                        text_protocol_tool_results.push(truncate_for_history(
                            &rendered,
                            limits.max_tool_result_history_chars,
                        ));
                    }
                }
            } else {
                for block in tool_use_blocks {
                    if let ContentBlock::ToolUse {
                        id, name, input, ..
                    } = block
                    {
                        if let Some(clarification) =
                            missing_read_only_location_prompt(&name, &input)
                        {
                            self.set_tool_call_status(&id, ToolStatus::Cancelled, stream_delta_tx);
                            self.push_tool_result_block(
                                StreamBlock::ToolResult {
                                    tool_call_id: id.clone(),
                                    output: clarification.clone(),
                                    is_error: true,
                                },
                                stream_delta_tx,
                            );
                            emit_tool_error(
                                stream_delta_tx,
                                &name,
                                &clarification,
                                id,
                                use_structured_round,
                                limits.max_tool_result_history_chars,
                                &mut tool_result_blocks,
                                &mut text_protocol_tool_results,
                            );
                            continue;
                        }

                        if let Some(clarification) = missing_mutating_location_prompt(&name, &input)
                        {
                            self.set_tool_call_status(&id, ToolStatus::Cancelled, stream_delta_tx);
                            self.push_tool_result_block(
                                StreamBlock::ToolResult {
                                    tool_call_id: id.clone(),
                                    output: clarification.clone(),
                                    is_error: true,
                                },
                                stream_delta_tx,
                            );
                            emit_tool_error(
                                stream_delta_tx,
                                &name,
                                &clarification,
                                id,
                                use_structured_round,
                                limits.max_tool_result_history_chars,
                                &mut tool_result_blocks,
                                &mut text_protocol_tool_results,
                            );
                            continue;
                        }

                        if let Some(read_only_guard) =
                            mutating_tool_read_only_conflict_prompt(&original_user_input, &name)
                        {
                            self.set_tool_call_status(&id, ToolStatus::Cancelled, stream_delta_tx);
                            self.push_tool_result_block(
                                StreamBlock::ToolResult {
                                    tool_call_id: id.clone(),
                                    output: read_only_guard.clone(),
                                    is_error: true,
                                },
                                stream_delta_tx,
                            );
                            emit_tool_error(
                                stream_delta_tx,
                                &name,
                                &read_only_guard,
                                id,
                                use_structured_round,
                                limits.max_tool_result_history_chars,
                                &mut tool_result_blocks,
                                &mut text_protocol_tool_results,
                            );
                            continue;
                        }

                        if let Some(test_only_guard) =
                            tests_only_mutation_conflict_prompt(turn_tool_policy, &name, &input)
                        {
                            self.set_tool_call_status(&id, ToolStatus::Cancelled, stream_delta_tx);
                            self.push_tool_result_block(
                                StreamBlock::ToolResult {
                                    tool_call_id: id.clone(),
                                    output: test_only_guard.clone(),
                                    is_error: true,
                                },
                                stream_delta_tx,
                            );
                            emit_tool_error(
                                stream_delta_tx,
                                &name,
                                &test_only_guard,
                                id,
                                use_structured_round,
                                limits.max_tool_result_history_chars,
                                &mut tool_result_blocks,
                                &mut text_protocol_tool_results,
                            );
                            continue;
                        }

                        let tool_requires_approval =
                            require_tool_approval || tool_requires_confirmation(&name);

                        if tool_requires_approval {
                            self.set_tool_call_status(
                                &id,
                                ToolStatus::WaitingApproval,
                                stream_delta_tx,
                            );
                        }
                        let approved = if tool_requires_approval {
                            self.request_tool_approval(&name, &input, stream_delta_tx)
                                .await
                        } else {
                            true
                        };

                        if approved {
                            self.set_tool_call_status(&id, ToolStatus::Executing, stream_delta_tx);
                        } else {
                            self.set_tool_call_status(&id, ToolStatus::Cancelled, stream_delta_tx);
                        }

                        if !approved {
                            let denial = render_tool_denied_message(&name);
                            self.push_tool_result_block(
                                StreamBlock::ToolResult {
                                    tool_call_id: id.clone(),
                                    output: denial.clone(),
                                    is_error: true,
                                },
                                stream_delta_tx,
                            );
                            emit_tool_error(
                                stream_delta_tx,
                                &name,
                                &denial,
                                id,
                                use_structured_round,
                                limits.max_tool_result_history_chars,
                                &mut tool_result_blocks,
                                &mut text_protocol_tool_results,
                            );
                            continue;
                        }

                        // Capture undo snapshot before executing the tool.
                        let undo_snapshot = self.capture_undo_snapshot(&name, &input);

                        let result = self
                            .execute_tool_with_timeout_with_updates(
                                &name,
                                &input,
                                tool_timeout,
                                stream_delta_tx,
                            )
                            .await;

                        // Push checkpoint only on success.
                        if result.is_ok() {
                            if let Some(cp) = undo_snapshot {
                                self.push_undo_checkpoint(cp);
                            }
                        }

                        let final_status = if result.is_err() {
                            ToolStatus::Error
                        } else {
                            ToolStatus::Complete
                        };
                        self.set_tool_call_status(&id, final_status, stream_delta_tx);

                        let output_for_stream = result
                            .as_ref()
                            .map_or_else(|e| e.to_string(), ToString::to_string);
                        self.push_tool_result_block(
                            StreamBlock::ToolResult {
                                tool_call_id: id.clone(),
                                output: output_for_stream,
                                is_error: result.is_err(),
                            },
                            stream_delta_tx,
                        );

                        let history_content = truncate_for_history(
                            &self.format_tool_result_for_history(&name, &input, &result),
                            limits.max_tool_result_history_chars,
                        );
                        if result.is_ok()
                            && matches!(
                                name.as_str(),
                                "write_file" | "apply_patch" | "edit_file" | "rename_file"
                            )
                        {
                            self.current_turn_applied_mutation = true;
                        }
                        if use_structured_round {
                            tool_result_blocks.push(ContentBlock::ToolResult {
                                tool_use_id: id,
                                content: history_content,
                                is_error: result.is_err(),
                            });
                        } else {
                            let rendered = result.as_ref().map_or_else(
                                |_| format!("tool_error {name}:\n{history_content}"),
                                |_| format!("tool_result {name}:\n{history_content}"),
                            );
                            text_protocol_tool_results.push(truncate_for_history(
                                &rendered,
                                limits.max_tool_result_history_chars,
                            ));
                        }
                    }
                }
            }

            if use_structured_round {
                self.api_messages.push(ApiMessage {
                    role: "user".to_string(),
                    content: Content::Blocks(tool_result_blocks),
                });
            } else {
                self.api_messages.push(ApiMessage {
                    role: "user".to_string(),
                    content: Content::Text(text_protocol_tool_results.join("\n\n")),
                });
            }
        }
    }
}

/// Centralizes the repeated guard-error tail pattern (ADR-021 Item 9).
///
/// After a tool-guard fires, this helper emits the clarification text to the
/// stream and appends the history payload for the active protocol. The caller
/// still handles the structured-round streaming update (`ToolStatus::Error` +
/// `push_tool_result_block`) before calling this function.
#[allow(clippy::too_many_arguments)]
pub(super) fn emit_tool_error(
    stream_delta_tx: Option<&mpsc::UnboundedSender<ConversationStreamUpdate>>,
    tool_name: &str,
    clarification: &str,
    tool_use_id: String,
    use_structured_round: bool,
    max_tool_result_history_chars: usize,
    tool_result_blocks: &mut Vec<ContentBlock>,
    text_protocol_tool_results: &mut Vec<String>,
) {
    emit_text_update(stream_delta_tx, clarification.to_string());
    if use_structured_round {
        let history_content = truncate_for_history(clarification, max_tool_result_history_chars);
        tool_result_blocks.push(ContentBlock::ToolResult {
            tool_use_id,
            content: history_content,
            is_error: true,
        });
    } else {
        let rendered = format!("tool_error {tool_name}:\n{clarification}");
        text_protocol_tool_results.push(truncate_for_history(
            &rendered,
            max_tool_result_history_chars,
        ));
    }
}

fn accumulate_usage(turn_tokens: &mut TurnTokens, usage: Option<&ApiUsage>) {
    let Some(usage) = usage else {
        return;
    };

    if let Some(input) = usage.input_tokens {
        turn_tokens.input = turn_tokens.input.saturating_add(input);
    }
    if let Some(output) = usage.output_tokens {
        turn_tokens.output = turn_tokens.output.saturating_add(output);
    }
    if let Some(cache_creation) = usage.cache_creation_input_tokens {
        turn_tokens.cache_creation_input_tokens = turn_tokens
            .cache_creation_input_tokens
            .saturating_add(cache_creation);
    }
    if let Some(cache_read) = usage.cache_read_input_tokens {
        turn_tokens.cache_read_input_tokens = turn_tokens
            .cache_read_input_tokens
            .saturating_add(cache_read);
    }
}
