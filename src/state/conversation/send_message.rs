use super::super::stream_block::{StreamBlock, ToolStatus};
use super::core::{CompletedToolCall, emit_server_metadata_update};
use super::{
    ConversationManager, ConversationStreamUpdate, PulseToolPolicy, history::*, streaming::*,
    tools::*,
};
use crate::runtime::policy::{RuntimeCorePolicy, default_runtime_policy};
use crate::runtime::task_document::PulseOutcome;
use crate::runtime::{RuntimeSignal, TokenUsageEnvelope};
use crate::types::{ApiMessage, Content, ContentBlock};
use crate::usage::PulseTokens;
use anyhow::Result;
use chrono::{DateTime, Utc};
use futures::StreamExt;
use std::collections::{HashMap, HashSet, VecDeque};
use tokio::sync::mpsc;

impl ConversationManager {
    pub async fn send_message_with_policy(
        &mut self,
        content: String,
        stream_delta_tx: Option<&mpsc::UnboundedSender<ConversationStreamUpdate>>,
        turn_tool_policy: PulseToolPolicy,
        shared_prefix_context: Option<String>,
    ) -> Result<String> {
        self.last_turn_tokens = PulseTokens::default();
        let original_user_input = content.clone();
        self.ensure_task_doc();
        self.begin_turn_doc(content.clone(), turn_tool_policy);
        let cache_hint = self.shared_prefix_cache_hint(shared_prefix_context.as_deref());
        self.push_user_message(content, cache_hint);
        if let Some(response) = builtin_supported_git_tools_response(&original_user_input) {
            self.api_messages.push(ApiMessage {
                role: "assistant".to_string(),
                content: Content::Text(response.clone()),
                cache_hint: None,
            });
            emit_text_update(stream_delta_tx, response.clone());
            self.finish_turn_doc(PulseOutcome::Completed, PulseTokens::default());
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
        let mut turn_tokens = PulseTokens::default();
        let mut overflow_retry_count = 0usize;
        self.condense_old_tool_results(history_keep_turns);

        let ctx_window = self.client.context_window_tokens();
        if let Some((before, after, _heuristic_content)) = self.run_proactive_compaction(ctx_window)
        {
            let display_summary = format!("{before} → {after} messages (proactive)");
            emit_text_update(
                stream_delta_tx,
                format!("\n[context compacted: {display_summary}]\n"),
            );
            emit_stream_update(
                stream_delta_tx,
                ConversationStreamUpdate::ContextCompacted {
                    messages_before: before,
                    messages_after: after,
                    summary: display_summary,
                },
            );
        }

        loop {
            self.advance_round_start();
            turn_user_anchor_index = self
                .prune_message_history_preserving(limits.max_api_messages, turn_user_anchor_index);
            rounds += 1;
            if rounds > max_tool_rounds {
                let msg = render_loop_limit_guard_message(
                    &last_assistant_text_for_history,
                    max_tool_rounds,
                );
                if let Some(guard_line) = msg.lines().find(|l| l.starts_with("[loop guard]")) {
                    emit_stream_update(
                        stream_delta_tx,
                        ConversationStreamUpdate::TranscriptLine(guard_line.to_string()),
                    );
                }
                self.finish_turn_doc(PulseOutcome::MaxTurnsReached, PulseTokens::default());
                return Ok(msg);
            }

            let mut stream = match self.client.create_stream(&self.api_messages).await {
                Ok(s) => {
                    overflow_retry_count = 0;
                    s
                }
                Err(e)
                    if overflow_retry_count < 3
                        && crate::api::client::is_context_overflow(&e.to_string()) =>
                {
                    let keep_messages = match overflow_retry_count {
                        0 => 4,
                        1 => 2,
                        _ => 1,
                    };
                    let before = self.api_messages.len();
                    if overflow_retry_count == 0 {
                        self.compact_for_context_overflow();
                    } else {
                        self.compact_for_context_overflow_with_keep(keep_messages);
                    }
                    let after = self.api_messages.len();
                    overflow_retry_count += 1;
                    let summary = format!(
                        "compacted {} → {} messages to fit server window (keep {})",
                        before, after, keep_messages
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
                    rounds -= 1;
                    continue;
                }
                Err(e) => {
                    self.finish_turn_doc(
                        PulseOutcome::Failed {
                            message: e.to_string(),
                        },
                        turn_tokens,
                    );
                    return Err(e);
                }
            };
            let mut assistant_text = String::new();
            let mut tool_use_blocks = Vec::new();
            let mut tool_use_positions: HashMap<String, usize> = HashMap::new();
            let mut pending_tool_block_indices = VecDeque::new();
            let mut tool_block_indices: HashMap<String, usize> = HashMap::new();
            let mut block_tool_call_ids: HashMap<usize, String> = HashMap::new();
            let mut completed_tool_call_ids = HashSet::new();
            let mut saw_usage_update = false;
            while let Some(signal_outcome) = stream.next().await {
                let signal = match signal_outcome {
                    Ok(envelope) => envelope.signal,
                    Err(error) => {
                        self.finish_turn_doc(
                            PulseOutcome::Failed {
                                message: error.to_string(),
                            },
                            turn_tokens,
                        );
                        return Err(error);
                    }
                };

                match signal.clone() {
                    RuntimeSignal::PulseStart { .. } => {}
                    RuntimeSignal::TranscriptLine { line } => {
                        self.apply_doc_delta(signal);
                        emit_stream_update(
                            stream_delta_tx,
                            ConversationStreamUpdate::TranscriptLine(line),
                        );
                    }
                    RuntimeSignal::TranscriptBlockStart { index, block } => match &block {
                        StreamBlock::Thinking { .. } | StreamBlock::FinalText { .. } => {
                            self.upsert_turn_block(index, block, stream_delta_tx);
                        }
                        StreamBlock::ToolCall { .. } => {
                            pending_tool_block_indices.push_back(index);
                            self.emit_stream_block_start_update(index, block, stream_delta_tx);
                        }
                        StreamBlock::ToolResult { .. } => {
                            self.emit_stream_block_start_update(index, block, stream_delta_tx);
                        }
                    },
                    RuntimeSignal::TranscriptBlockDelta { index, delta } => {
                        if block_tool_call_ids.contains_key(&index) {
                            continue;
                        }
                        let appended = self.append_text_delta(index, &delta, stream_delta_tx);
                        assistant_text.push_str(&appended);
                    }
                    RuntimeSignal::TranscriptBlockComplete { index } => {
                        if block_tool_call_ids.contains_key(&index) {
                            emit_stream_update(
                                stream_delta_tx,
                                ConversationStreamUpdate::BlockComplete { index },
                            );
                        } else {
                            self.apply_doc_delta(signal);
                            emit_stream_update(
                                stream_delta_tx,
                                ConversationStreamUpdate::BlockComplete { index },
                            );
                        }
                    }
                    RuntimeSignal::TranscriptBlockPhaseUpdated { .. }
                    | RuntimeSignal::ToolCallStatusUpdated { .. }
                    | RuntimeSignal::ApprovalRequest { .. }
                    | RuntimeSignal::ApprovalResolved { .. }
                    | RuntimeSignal::ValidationResult { .. }
                    | RuntimeSignal::MaxTurnsReached { .. } => {
                        self.apply_doc_delta(signal);
                    }
                    RuntimeSignal::ToolCallStarted {
                        tool_call_id,
                        tool_name,
                        arguments,
                        started_at,
                        ..
                    } => {
                        if let Some(started_at) = parse_runtime_timestamp(&started_at) {
                            self.record_tool_call_started_at(&tool_call_id, started_at);
                        }

                        if let Some(index) = pending_tool_block_indices.pop_front() {
                            tool_block_indices.insert(tool_call_id.clone(), index);
                            block_tool_call_ids.insert(index, tool_call_id.clone());
                        }

                        let position = *tool_use_positions
                            .entry(tool_call_id.clone())
                            .or_insert_with(|| {
                                tool_use_blocks.push(ContentBlock::ToolUse {
                                    id: tool_call_id.clone(),
                                    name: tool_name.clone(),
                                    input: arguments.clone(),
                                    metadata: None,
                                });
                                tool_use_blocks.len().saturating_sub(1)
                            });
                        tool_use_blocks[position] = ContentBlock::ToolUse {
                            id: tool_call_id,
                            name: tool_name,
                            input: arguments,
                            metadata: None,
                        };
                        self.apply_doc_delta(signal);
                    }
                    RuntimeSignal::ToolCallArgumentsDelta {
                        tool_call_id,
                        tool_name,
                        delta,
                        arguments,
                        ..
                    } => {
                        let tool_name_for_ui = tool_name.clone();

                        if let Some(position) = tool_use_positions.get(&tool_call_id).copied()
                            && let Some(ContentBlock::ToolUse { name, .. }) =
                                tool_use_blocks.get_mut(position)
                            && let Some(tool_name) = tool_name
                        {
                            *name = tool_name;
                        }

                        if let Some(arguments) = arguments.as_ref()
                            && let Some(position) = tool_use_positions.get(&tool_call_id).copied()
                            && let Some(ContentBlock::ToolUse { input, .. }) =
                                tool_use_blocks.get_mut(position)
                        {
                            *input = arguments.clone();
                        }

                        if let Some(index) = tool_block_indices.get(&tool_call_id).copied() {
                            emit_stream_update(
                                stream_delta_tx,
                                ConversationStreamUpdate::BlockDelta { index, delta },
                            );
                        }
                        if let Some(arguments) = arguments {
                            emit_stream_update(
                                stream_delta_tx,
                                ConversationStreamUpdate::ToolCallArgumentsUpdated {
                                    tool_call_id,
                                    tool_name: tool_name_for_ui,
                                    arguments,
                                },
                            );
                        }
                        self.apply_doc_delta(signal);
                    }
                    RuntimeSignal::ToolCallCompleted {
                        tool_call_id,
                        output,
                        ..
                    } => {
                        completed_tool_call_ids.insert(tool_call_id.clone());
                        self.apply_doc_delta(signal);
                        self.push_tool_result_block(
                            StreamBlock::ToolResult {
                                tool_call_id,
                                output,
                                is_error: false,
                            },
                            stream_delta_tx,
                        );
                    }
                    RuntimeSignal::ToolCallFailed {
                        tool_call_id,
                        output,
                        ..
                    } => {
                        completed_tool_call_ids.insert(tool_call_id.clone());
                        self.apply_doc_delta(signal);
                        self.push_tool_result_block(
                            StreamBlock::ToolResult {
                                tool_call_id,
                                output,
                                is_error: true,
                            },
                            stream_delta_tx,
                        );
                    }
                    RuntimeSignal::ServerMetadata { metadata } => {
                        emit_server_metadata_update(Some(metadata.as_ref()), stream_delta_tx);
                        self.apply_doc_delta(signal);
                    }
                    RuntimeSignal::UsageUpdated { usage } => {
                        saw_usage_update = true;
                        accumulate_usage_envelope(&mut turn_tokens, Some(&usage));
                    }
                    RuntimeSignal::PulseEnd { usage, .. } => {
                        if !saw_usage_update {
                            accumulate_usage_envelope(&mut turn_tokens, usage.as_ref());
                        }
                    }
                    RuntimeSignal::Error { code, message, .. } => {
                        emit_stream_update(
                            stream_delta_tx,
                            ConversationStreamUpdate::StreamError(format!(
                                "stream error ({code}): {message}"
                            )),
                        );
                    }
                }
            }

            tool_use_blocks.retain(|block| {
                if let ContentBlock::ToolUse { id, .. } = block {
                    !completed_tool_call_ids.contains(id)
                } else {
                    true
                }
            });

            let mut assistant_text_for_history = assistant_text.clone();
            let mut used_tagged_fallback = false;
            if tool_use_blocks.is_empty() && self.client.is_local_endpoint() {
                let tagged_calls = dedupe_tagged_tool_calls(parse_tagged_tool_calls(&assistant_text));
                if !tagged_calls.is_empty() {
                    used_tagged_fallback = true;
                    assistant_text_for_history = core_policy.sanitize_assistant_text(&assistant_text);
                    let fallback_start_index = self.current_round_stream_block_count;
                    tool_use_blocks = tagged_calls
                        .into_iter()
                        .enumerate()
                        .map(|(offset, call)| {
                            let block = ContentBlock::ToolUse {
                                id: format!("toolu_tagged_{rounds}_{offset}"),
                                name: call.name,
                                input: call.input,
                                metadata: None,
                            };

                            if let ContentBlock::ToolUse {
                                id, name, input, ..
                            } = &block
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

                            block
                        })
                        .collect();
                }
            }
            let use_structured_round = use_structured_tool_protocol && !used_tagged_fallback;

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
                    let msg =
                        render_repeated_mutating_tool_guard_message(&assistant_text_for_history);
                    if let Some(guard_line) = msg.lines().find(|l| l.starts_with("[loop guard]")) {
                        emit_stream_update(
                            stream_delta_tx,
                            ConversationStreamUpdate::TranscriptLine(guard_line.to_string()),
                        );
                    }
                    self.finish_turn_doc(PulseOutcome::Completed, self.last_turn_tokens);
                    return Ok(msg);
                }

                let has_empty_path_call = tool_use_blocks.iter().any(|block| {
                    if let ContentBlock::ToolUse { name, input, .. } = block {
                        matches!(name.as_str(), "read_file" | "list_files")
                            && missing_read_only_location_prompt(name, input).is_some()
                    } else {
                        false
                    }
                });
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
                        let msg = render_repeated_tool_guard_message(&assistant_text_for_history);
                        if let Some(guard_line) =
                            msg.lines().find(|l| l.starts_with("[loop guard]"))
                        {
                            emit_stream_update(
                                stream_delta_tx,
                                ConversationStreamUpdate::TranscriptLine(guard_line.to_string()),
                            );
                        }
                        self.finish_turn_doc(PulseOutcome::Completed, self.last_turn_tokens);
                        return Ok(msg);
                    }
                }
            } else {
                previous_round_signature = None;
                repeated_read_only_rounds = 0;
                repeated_mutating_rounds = 0;
            }

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
            let assistant_history_text =
                truncate_for_history(&assistant_history_source, limits.max_assistant_history_chars);

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
                    cache_hint: None,
                });
            } else {
                self.api_messages.push(ApiMessage {
                    role: "assistant".to_string(),
                    content: Content::Text(assistant_history_text),
                    cache_hint: None,
                });
            }
            last_assistant_text_for_history = assistant_text_for_history.clone();

            if inject_repeated_round_nudge {
                self.api_messages.push(ApiMessage {
                    role: "user".to_string(),
                    content: Content::Text(
                        core_policy.repeated_tool_round_instruction().to_string(),
                    ),
                    cache_hint: None,
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
                        cache_hint: None,
                    });
                    continue;
                }
                if self.client.is_local_endpoint() && requires_tool_evidence && !saw_any_tool_round
                {
                    let msg =
                        render_missing_tool_evidence_guard_message(&assistant_text_for_history);
                    if let Some(guard_line) = msg.lines().find(|l| l.starts_with("[loop guard]")) {
                        emit_stream_update(
                            stream_delta_tx,
                            ConversationStreamUpdate::TranscriptLine(guard_line.to_string()),
                        );
                    }
                    self.finish_turn_doc(PulseOutcome::Completed, self.last_turn_tokens);
                    return Ok(msg);
                }
                self.promote_thinking_blocks_to_final_text(stream_delta_tx);
                self.last_turn_tokens = turn_tokens;
                self.finish_turn_doc(PulseOutcome::Completed, turn_tokens);
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

                        let undo_snapshot = self.capture_undo_snapshot(&name, &input);

                        let result = self
                            .execute_tool_with_timeout_with_updates(
                                &name,
                                &input,
                                tool_timeout,
                                stream_delta_tx,
                            )
                            .await;

                        if result.is_ok()
                            && let Some(cp) = undo_snapshot
                        {
                            self.push_undo_checkpoint(cp);
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
                    cache_hint: None,
                });
            } else {
                self.api_messages.push(ApiMessage {
                    role: "user".to_string(),
                    content: Content::Text(text_protocol_tool_results.join("\n\n")),
                    cache_hint: None,
                });
            }
        }
    }
}

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

fn accumulate_usage_envelope(turn_tokens: &mut PulseTokens, usage: Option<&TokenUsageEnvelope>) {
    let Some(usage) = usage else {
        return;
    };

    turn_tokens.input = turn_tokens.input.saturating_add(usage.input);
    turn_tokens.output = turn_tokens.output.saturating_add(usage.output);
    turn_tokens.estimated = turn_tokens.estimated || usage.estimated;
    turn_tokens.cache_creation_input_tokens = turn_tokens
        .cache_creation_input_tokens
        .saturating_add(usage.cache_creation_input);
    turn_tokens.cache_read_input_tokens = turn_tokens
        .cache_read_input_tokens
        .saturating_add(usage.cache_read_input);
}

fn parse_runtime_timestamp(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|timestamp| timestamp.with_timezone(&Utc))
}
